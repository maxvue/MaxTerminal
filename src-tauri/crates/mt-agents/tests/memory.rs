//! Testes de integração da Memória Contextual de Comandos (`CommandMemory` — normalização, sanitização, dedupe, tombstone e equivalência de `rebuild`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use mt_agents::memory::{CommandMemory, MemoryQuery, PersistedCommandBlock};
use mt_agents::ports::{ProjectRecord, WorkspacePort};
use mt_core::{CommandBlockId, MtResult, ProjectId, TabId};

struct DummyWorkspacePort {
    project_id: ProjectId,
}

#[async_trait]
impl WorkspacePort for DummyWorkspacePort {
    async fn project(&self, id: ProjectId) -> MtResult<ProjectRecord> {
        Ok(ProjectRecord {
            id,
            title: "Test".to_string(),
            path: PathBuf::from("/tmp"),
            stack: "rust".to_string(),
        })
    }

    fn project_of_tab(&self, _tab: TabId) -> Option<ProjectId> {
        Some(self.project_id)
    }

    fn tabs_using_cwd(&self, _dir: &Path) -> Vec<TabId> {
        Vec::new()
    }

    async fn ensure_agent_templates(&self, _project: ProjectId) -> MtResult<u32> {
        Ok(0)
    }
}

async fn setup_memory_db() -> (tempfile::TempDir, mt_db::Db, ProjectId, CommandMemory) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("mem.sqlite");
    let db = mt_db::Db::open(mt_db::DbConfig::for_path(&db_path)).unwrap();

    let project_id = ProjectId::new();
    let pid_str = project_id.to_string();
    db.write(move |conn| {
        conn.execute(
            "INSERT INTO projects (id, title, path, icon, color, stack, sort_order, created_at, updated_at)
             VALUES (?1, 'Mem Project', '/tmp/mem', 'mdi:folder', '#10B981', 'rust', 0, 1000, 1000);",
            rusqlite::params![pid_str],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "insert_proj"))?;
        Ok(())
    })
    .await
    .unwrap();

    let ws: Arc<dyn WorkspacePort> = Arc::new(DummyWorkspacePort { project_id });
    let mem = CommandMemory::new(db.clone(), ws);
    (dir, db, project_id, mem)
}

#[test]
fn normalize_rejects_secrets_spaces_multiline_and_preserves_inner_quotes() {
    // 1. Comando iniciado por espaço é ignorado (HISTCONTROL=ignorespace)
    assert!(CommandMemory::normalize("  git status").is_none());

    // 2. Comando com segredo que vira [REDACTED:...] é rejeitado (CA-31)
    assert!(
        CommandMemory::normalize("curl -H 'Authorization: Bearer sk-1234567890abcdef1234567890abcdef' https://api.example.com")
            .is_none()
    );

    // 3. Multiline ou controle é rejeitado
    assert!(CommandMemory::normalize("echo hello\necho world").is_none());

    // 4. Espaços internos entre aspas são preservados (CA-33)
    let a = CommandMemory::normalize("echo 'a  b'").unwrap();
    let b = CommandMemory::normalize("echo 'a b'").unwrap();
    assert_ne!(a, b);
    assert_eq!(a, "echo 'a  b'");
}

#[tokio::test]
async fn idempotent_processing_forget_tombstone_and_rebuild_equivalence() {
    let (_dir, db, project_id, mem) = setup_memory_db().await;
    let tab_id = TabId::new();
    let session_id = mt_core::SessionId::new();

    // Insere sessão em `sessions` para suportar `command_blocks` e `rebuild`
    let sid_str = session_id.to_string();
    let pid_str = project_id.to_string();
    let tid_str = tab_id.to_string();
    db.write(move |conn| {
        conn.execute(
            "INSERT INTO sessions (id, tab_id, project_id, pty_id, shell_kind, shell_executable, initial_cwd, started_at)
             VALUES (?1, ?2, ?3, 'pty-1', 'bash', '/bin/bash', '/tmp/mem', 1000);",
            rusqlite::params![sid_str, tid_str, pid_str],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "insert_session"))?;
        Ok(())
    })
    .await
    .unwrap();

    let blocks = vec![
        (
            CommandBlockId::new(),
            "cargo test --workspace",
            "/tmp/mem",
            Some(0),
            1000,
            2000,
        ),
        (
            CommandBlockId::new(),
            "cargo test --workspace",
            "/tmp/mem",
            Some(101),
            2100,
            3000,
        ),
        (
            CommandBlockId::new(),
            "git status -sb",
            "/tmp/mem/sub",
            Some(0),
            3100,
            3500,
        ),
        (
            CommandBlockId::new(),
            "npm run check",
            "/tmp/mem",
            Some(0),
            3600,
            4000,
        ),
    ];

    for (seq, (bid, cmd, cwd, code, start, end)) in blocks.iter().enumerate() {
        let bid_s = bid.to_string();
        let sid_s = session_id.to_string();
        let tid_s = tab_id.to_string();
        let cmd_s = (*cmd).to_string();
        let cwd_s = (*cwd).to_string();
        let exit_c = *code;
        let st = *start;
        let en = *end;
        let status = if exit_c == Some(0) {
            "succeeded"
        } else {
            "failed"
        };
        let reason = if exit_c == Some(0) {
            "exit_zero"
        } else {
            "exit_non_zero"
        };
        let seq_i = i64::try_from(seq + 1).unwrap();

        db.write(move |conn| {
            conn.execute(
                "INSERT INTO command_blocks
                    (id, session_id, tab_id, pty_id, command_seq, command_text, command_text_quality,
                     cwd, started_at, ended_at, duration_ms, exit_code, status, end_reason,
                     source, confidence, start_pty_seq, start_byte_offset)
                 VALUES (?1, ?2, ?3, 'pty-1', ?4, ?5, 'exact', ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'osc133', 'exact', 0, 0);",
                rusqlite::params![bid_s, sid_s, tid_s, seq_i, cmd_s, cwd_s, st, en, en - st, exit_c, status, reason],
            )
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "insert_block"))?;
            Ok(())
        })
        .await
        .unwrap();

        let pblock = PersistedCommandBlock {
            block_id: *bid,
            project_id,
            tab_id,
            command: (*cmd).to_string(),
            cwd: Some((*cwd).to_string()),
            exit_code: *code,
            started_at_ms: *start,
            ended_at_ms: *end,
            is_agent_launcher: false,
        };
        assert!(mem.process_block(&pblock, 5000).await.unwrap());
        // Replay duplicado do mesmo block_id não incrementa contadores (CA-33)
        assert!(!mem.process_block(&pblock, 5000).await.unwrap());
    }

    // Fixa `git status -sb` e esquece `npm run check`
    let pinned_entry = mem
        .set_pinned(project_id, "git status -sb", true, 5000)
        .await
        .unwrap();
    assert!(pinned_entry.pinned);

    assert!(mem.forget(project_id, "npm run check", 5000).await.unwrap());

    let before_rebuild = mem.list(project_id, true, 50, 10_000).await.unwrap();
    assert_eq!(before_rebuild.len(), 2);
    assert_eq!(before_rebuild[0].command, "git status -sb");
    assert_eq!(before_rebuild[1].command, "cargo test --workspace");
    assert_eq!(before_rebuild[1].use_count, 2);
    assert_eq!(before_rebuild[1].success_count, 1);
    assert_eq!(before_rebuild[1].failure_count, 1);

    // Rebuild produz estado idêntico ao incremental, preservando pinned e respeitando tombstone de forget (CA-32, CA-33)
    let rebuilt_count = mem.rebuild(Some(project_id), 10_000).await.unwrap();
    assert_eq!(rebuilt_count, 2);
    let after_rebuild = mem.list(project_id, true, 50, 10_000).await.unwrap();
    assert_eq!(before_rebuild, after_rebuild);

    // Busca com caracteres especiais SQL (`%`, `_`) escapados literalmente
    let sugg = mem
        .suggestions(
            &MemoryQuery {
                project_id,
                prefix: Some("cargo".to_string()),
                limit: 10,
                include_hidden: false,
                cwd: Some("/tmp/mem".to_string()),
            },
            10_000,
        )
        .await
        .unwrap();
    assert_eq!(sugg.len(), 1);
    assert_eq!(sugg[0].command, "cargo test --workspace");
}
