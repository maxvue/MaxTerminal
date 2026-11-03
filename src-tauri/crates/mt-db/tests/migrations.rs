//! Testes de integração do runner e registro de migrations (`mt-db`).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::fs;

use mt_db::{
    CodeMigration, Db, DbConfig, DbError, MigrationEntry, MigrationRegistry, MigrationRunner,
    compute_migration_checksum, open_writer, parse_migration_filename,
};
use tempfile::TempDir;

#[tokio::test]
async fn fresh_db_applies_base() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    assert_eq!(db.schema_version(), 11);
    assert_eq!(db.applied_migrations().len(), 11);

    let applied = db
        .read_blocking(|conn| {
            let count: i64 = conn
                .query_row("SELECT COUNT(*) FROM _schema_migrations;", [], |r| r.get(0))
                .unwrap();
            let (version, name, checksum): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 1;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v2, name2, checksum2): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 2;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v3, name3, checksum3): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 3;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v4, name4, checksum4): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 4;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v5, name5, checksum5): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 5;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v6, name6, checksum6): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 6;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v7, name7, checksum7): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 7;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v8, name8, checksum8): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 8;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v9, name9, checksum9): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 9;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v10, name10, checksum10): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 10;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            let (v11, name11, checksum11): (i64, String, String) = conn
                .query_row(
                    "SELECT version, name, checksum FROM _schema_migrations WHERE version = 11;",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .unwrap();
            Ok((
                count, version, name, checksum, v2, name2, checksum2, v3, name3, checksum3, v4,
                name4, checksum4, v5, name5, checksum5, v6, name6, checksum6, v7, name7, checksum7,
                v8, name8, checksum8, v9, name9, checksum9, v10, name10, checksum10, v11, name11,
                checksum11,
            ))
        })
        .unwrap();

    assert_eq!(applied.0, 11);
    assert_eq!(applied.1, 1);
    assert_eq!(applied.2, "0001_03_base");
    assert_eq!(applied.3.len(), 64);
    assert_eq!(applied.4, 2);
    assert_eq!(applied.5, "0002_05_shell_sessions_command_blocks");
    assert_eq!(applied.6.len(), 64);
    assert_eq!(applied.7, 3);
    assert_eq!(applied.8, "0003_06_projects");
    assert_eq!(applied.9.len(), 64);
    assert_eq!(applied.10, 4);
    assert_eq!(applied.11, "0004_07_workspace_templates_tabs_sessions");
    assert_eq!(applied.12.len(), 64);
    assert_eq!(applied.13, 5);
    assert_eq!(applied.14, "0005_09_os_settings");
    assert_eq!(applied.15.len(), 64);
    assert_eq!(applied.16, 6);
    assert_eq!(applied.17, "0006_10_themes");
    assert_eq!(applied.18.len(), 64);
    assert_eq!(applied.19, 7);
    assert_eq!(applied.20, "0007_12_ai");
    assert_eq!(applied.21.len(), 64);
    assert_eq!(applied.22, 8);
    assert_eq!(applied.23, "0008_13_agents_skills_memory");
    assert_eq!(applied.24.len(), 64);
    assert_eq!(applied.25, 9);
    assert_eq!(applied.26, "0009_15_command_groups");
    assert_eq!(applied.27.len(), 64);
    assert_eq!(applied.28, 10);
    assert_eq!(applied.29, "0010_16_github");
    assert_eq!(applied.30.len(), 64);
    assert_eq!(applied.31, 11);
    assert_eq!(applied.32, "0011_17_search");
    assert_eq!(applied.33.len(), 64);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn idempotent_reopen() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));

    {
        let db = Db::open(cfg.clone()).unwrap();
        assert_eq!(db.schema_version(), 11);
        assert_eq!(db.applied_migrations().len(), 11);
        db.shutdown().await.unwrap();
    }

    {
        let db = Db::open(cfg.clone()).unwrap();
        assert_eq!(db.schema_version(), 11);
        assert_eq!(db.applied_migrations().len(), 0);
        db.shutdown().await.unwrap();
    }
}

#[test]
fn checksum_mismatch() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let mut conn = open_writer(&cfg).unwrap().conn;

    let registry = MigrationRegistry::builtin().unwrap();
    let runner = MigrationRunner::new(registry);
    let outcome = runner
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap();
    assert_eq!(outcome.schema_version, 11);

    conn.execute(
        "UPDATE _schema_migrations SET checksum = '0000000000000000000000000000000000000000000000000000000000000000' WHERE version = 1;",
        [],
    )
    .unwrap();

    let err = runner
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap_err();
    match err {
        DbError::MigrationChecksumMismatch { version, .. } => assert_eq!(version, 1),
        other => panic!("esperava MigrationChecksumMismatch, obteve: {other:?}"),
    }
}

#[test]
fn failed_migration_rolls_back() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let mut conn = open_writer(&cfg).unwrap().conn;

    let bad_sql =
        "CREATE TABLE test_partial (id INTEGER PRIMARY KEY) STRICT;\nTHIS IS INVALID SQL;";
    let entry = MigrationEntry::from_sql("0012_03_bad_migration.sql", bad_sql).unwrap();
    let registry = MigrationRegistry::builtin()
        .unwrap()
        .with_entry(entry)
        .unwrap();
    let runner = MigrationRunner::new(registry);

    let err = runner
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap_err();
    match err {
        DbError::MigrationFailed { version, .. } => assert_eq!(version, 12),
        other => panic!("esperava MigrationFailed, obteve: {other:?}"),
    }

    let current_ver: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM _schema_migrations;",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(current_ver, 11);

    let table_exists: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'test_partial';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(table_exists, 0);
}

#[test]
fn schema_too_new() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let mut conn = open_writer(&cfg).unwrap().conn;

    let registry = MigrationRegistry::builtin().unwrap();
    let runner = MigrationRunner::new(registry);
    runner
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap();

    conn.execute_batch("PRAGMA user_version = 99;").unwrap();

    let err = runner
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap_err();
    match err {
        DbError::SchemaNewerThanApp {
            db_version,
            app_max_version,
        } => {
            assert_eq!(db_version, 99);
            assert_eq!(app_max_version, 11);
        }
        other => panic!("esperava SchemaNewerThanApp, obteve: {other:?}"),
    }
}

#[test]
fn naming_contract() {
    assert_eq!(
        parse_migration_filename("0001_03_base.sql").unwrap(),
        (1, 3, "0001_03_base".to_owned())
    );
    assert_eq!(
        parse_migration_filename("0002_05_projects_and_tabs.sql").unwrap(),
        (2, 5, "0002_05_projects_and_tabs".to_owned())
    );

    assert!(parse_migration_filename("1_03_base.sql").is_err());
    assert!(parse_migration_filename("0000_03_zero.sql").is_err());
    assert!(parse_migration_filename("0001_base.sql").is_err());
    assert!(parse_migration_filename("0001_03_BaseUpper.sql").is_err());
    assert!(parse_migration_filename("0001_03_base.txt").is_err());
    assert_eq!(
        compute_migration_checksum("SELECT 1;\r\n"),
        compute_migration_checksum("SELECT 1;\n")
    );
}

#[test]
fn code_migration_runs_and_premigration_backup_created() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let mut conn = open_writer(&cfg).unwrap().conn;

    let base_registry = MigrationRegistry::builtin().unwrap();
    let first = MigrationRunner::new(base_registry)
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap();
    assert_eq!(first.schema_version, 11);
    assert!(first.pre_migration_backup.is_none());

    let code_mig = CodeMigration::new(12, 3, "0012_03_with_code", "code-mig-seed-v1", |tx| {
        tx.execute(
            "INSERT INTO _db_meta (key, value_text, updated_at) VALUES ('code_mig', 'ok', 12345);",
            [],
        )
        .map_err(|e| DbError::MigrationFailed {
            version: 12,
            name: "0012_03_with_code".to_owned(),
            reason: e.to_string(),
        })?;
        Ok(())
    });

    let v12_registry = MigrationRegistry::builtin()
        .unwrap()
        .with_code_migration(code_mig)
        .unwrap();

    let second = MigrationRunner::new(v12_registry)
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap();
    assert_eq!(second.schema_version, 12);
    assert_eq!(second.applied.len(), 1);
    assert!(second.pre_migration_backup.is_some());

    let backup_path = second.pre_migration_backup.unwrap();
    assert!(backup_path.exists());

    let meta_val: String = conn
        .query_row(
            "SELECT value_text FROM _db_meta WHERE key = 'code_mig';",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(meta_val, "ok");

    let backups_on_disk: Vec<_> = fs::read_dir(&cfg.backup_dir)
        .unwrap()
        .filter_map(Result::ok)
        .collect();
    assert_eq!(backups_on_disk.len(), 1);
}

#[test]
fn foreign_key_check_after_migrate() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let mut conn = open_writer(&cfg).unwrap().conn;

    let fk_sql = "
        PRAGMA defer_foreign_keys = ON;
        CREATE TABLE parent_t (id INTEGER PRIMARY KEY) STRICT;
        CREATE TABLE child_t (id INTEGER PRIMARY KEY, parent_id INTEGER NOT NULL REFERENCES parent_t(id)) STRICT;
        INSERT INTO child_t (id, parent_id) VALUES (1, 999);
    ";
    let entry = MigrationEntry::from_sql("0012_03_fk_violation.sql", fk_sql).unwrap();
    let registry = MigrationRegistry::builtin()
        .unwrap()
        .with_entry(entry)
        .unwrap();

    let err = MigrationRunner::new(registry)
        .run(&mut conn, &cfg.db_path, &cfg.backup_dir, 7)
        .unwrap_err();
    assert!(matches!(err, DbError::MigrationFailed { version: 12, .. }));
}

#[tokio::test]
async fn os_settings_repo_default_and_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    let repo = db.os_settings();
    let initial = repo.get().await.unwrap();
    assert_eq!(initial.schema_version, 1);
    assert_eq!(initial.value_json, "{}");

    let saved = repo
        .save(1, String::from("{\"closeBehavior\":\"always_tray\"}"))
        .await
        .unwrap();
    assert_eq!(saved.schema_version, 1);
    assert_eq!(saved.value_json, "{\"closeBehavior\":\"always_tray\"}");

    let loaded = repo.get_blocking().unwrap();
    assert_eq!(loaded, saved);

    // JSON inválido deve ser rejeitado
    assert!(repo.save(1, String::from("nao-json")).await.is_err());

    db.shutdown().await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn migration_0002_05_constraints_and_repos_roundtrip() {
    use mt_core::ids::{CommandBlockId, PtyId, SessionId, TabId};
    use mt_core::shell::{
        BlockEndReason, BlockStatus, CommandBlockListQuery, CommandTextQuality, IntegrationMode,
        ObservationConfidence, ObservationSource, OutputKind, ShellKind,
    };
    use mt_db::{
        FinishCommandBlock, NewCommandBlock, NewSession, SessionEnd, sanitizer::SanitizedText,
    };

    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    let sessions = db.sessions();
    let blocks = db.command_blocks();

    let session_id = SessionId::new();
    let pty_id = PtyId::new();
    let tab_id = TabId::new();

    sessions
        .prepare(NewSession {
            id: session_id,
            tab_id: Some(tab_id),
            pane_id: None,
            project_id: None,
            tab_session_id: None,
            pty_id,
            shell_kind: ShellKind::Bash,
            shell_executable: SanitizedText::new("/bin/bash"),
            initial_cwd: SanitizedText::new("/tmp/project"),
            integration_mode: IntegrationMode::Osc133,
            started_at: 1_000,
        })
        .await
        .unwrap();

    // 1. Bloco em running e recuperado via close_dangling
    let dangling_id = CommandBlockId::new();
    blocks
        .start(NewCommandBlock {
            id: dangling_id,
            session_id,
            tab_id: Some(tab_id),
            pane_id: None,
            pty_id,
            command_seq: 1,
            command_text: Some(SanitizedText::new("sleep 60")),
            command_text_quality: CommandTextQuality::Exact,
            cwd: Some(SanitizedText::new("/tmp/project")),
            started_at: 1_100,
            is_continuous: false,
            source: ObservationSource::Osc133,
            confidence: ObservationConfidence::Exact,
            start_pty_seq: 1,
            start_byte_offset: 0,
            output_start_pty_seq: Some(2),
            output_start_byte_offset: Some(12),
        })
        .await
        .unwrap();

    let closed_dangling = blocks.close_dangling(2_100).await.unwrap();
    assert_eq!(closed_dangling, 1);

    let recovered = blocks.get(dangling_id).await.unwrap().unwrap();
    assert_eq!(recovered.status, BlockStatus::InterruptedByRestart);
    assert_eq!(
        recovered.end_reason,
        Some(BlockEndReason::RecoveredFromCrash)
    );
    assert_eq!(recovered.duration_ms, Some(1_000));

    // 2. Bloco finalizado com erro e segredo redigido
    let err_block_id = CommandBlockId::new();
    blocks
        .finish(FinishCommandBlock {
            id: err_block_id,
            session_id,
            tab_id: Some(tab_id),
            pane_id: None,
            pty_id,
            command_seq: 2,
            command_text: Some(SanitizedText::new("php artisan migrate --no-interaction")),
            command_text_quality: CommandTextQuality::Exact,
            cwd: Some(SanitizedText::new("/tmp/project")),
            started_at: 3_000,
            ended_at: 3_500,
            duration_ms: 500,
            exit_code: Some(1),
            signal: None,
            status: BlockStatus::Failed,
            end_reason: BlockEndReason::ExitNonZero,
            is_continuous: false,
            source: ObservationSource::Osc133,
            confidence: ObservationConfidence::Exact,
            output_text: Some(SanitizedText::new(
                "Erro em app/Models/User.php:42 com TOKEN=segredo123",
            )),
            output_kind: OutputKind::Full,
            output_bytes: 50,
            output_lines: 1,
            error_excerpt: Some(SanitizedText::new(
                "Erro em app/Models/User.php:42 com TOKEN=segredo123",
            )),
            inspectable: true,
            start_pty_seq: 3,
            start_byte_offset: 24,
            output_start_pty_seq: Some(4),
            output_start_byte_offset: Some(36),
            end_pty_seq: 5,
            end_byte_offset: 86,
        })
        .await
        .unwrap();

    let page = blocks
        .list(CommandBlockListQuery {
            session_id: Some(session_id),
            limit: Some(1),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, err_block_id);
    assert!(page.next_cursor.is_some());

    let page2 = blocks
        .list(CommandBlockListQuery {
            session_id: Some(session_id),
            limit: Some(1),
            cursor: page.next_cursor,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert_eq!(page2.items[0].id, dangling_id);
    assert!(page2.next_cursor.is_none());

    sessions
        .close(
            session_id,
            SessionEnd {
                ended_at: 4_000,
                exit_code: Some(0),
                exit_signal: None,
                last_cwd: Some(SanitizedText::new("/tmp/project")),
                integration_mode: IntegrationMode::Osc133,
                command_count: 2,
                error_count: 1,
            },
        )
        .await
        .unwrap();

    let sess_dto = sessions.get(session_id).await.unwrap().unwrap();
    assert_eq!(sess_dto.ended_at, Some(4_000));
    assert_eq!(sess_dto.command_count, 2);
    assert_eq!(sess_dto.error_count, 1);

    db.shutdown().await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn migration_0004_07_backfill_templates_tabs_sessions_scrollback_and_cascades() {
    use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
    use mt_core::shell::{IntegrationMode, ShellKind};
    use mt_core::workspace::{
        InitialDispatchState, LaunchSnapshot, ScrollbackChunkKind, SessionEndReason,
        SplitDirection, TabOrigin, TabTemplateInput, TemplateEnvironment,
    };
    use mt_db::{
        NewSession, NewTabTxInput, ProjectMutation, ScrollbackRecord, SessionEnd, StoredNewProject,
        StoredProject, insert_session_tx,
        repos::{
            archive_tab_session_tx, attach_pane_session_tx, close_pane_session_tx,
            create_tab_with_initial_pane_tx, split_pane_tx, start_tab_session_tx,
        },
        sanitizer::SanitizedText,
    };
    use std::collections::BTreeMap;

    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    // 1. Inserir projeto cria template padrão "Terminal" na mesma transação
    let proj_id = ProjectId::new();
    let commit = db
        .projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj_id,
                title: SanitizedText::new("MaxTerminal"),
                icon: String::from("logos:tauri"),
                color: String::from("#0EA5E9"),
                path: String::from("/tmp/maxterminal"),
                root_id: None,
                stack: String::from("tauri"),
                identity_source: String::from("auto"),
                sort_order: 0,
                missing: false,
                created_at: 1_000,
                updated_at: 1_000,
            },
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap();
    assert_eq!(commit.inserted_projects.len(), 1);

    let templates = db.templates().list_by_project(proj_id).await.unwrap();
    assert_eq!(templates.len(), 1);
    assert_eq!(templates[0].config.name, "Terminal");
    assert!(templates[0].is_default);
    assert_eq!(templates[0].config.icon, "mdi:console");

    // 2. Criar segundo template e promovê-lo a padrão, verificando que o padrão não pode ser excluído
    let created_tpl = db
        .templates()
        .create(
            proj_id,
            TabTemplateInput {
                name: String::from("Dev Server"),
                icon: String::from("mdi:play"),
                cwd: None,
                initial_command: Some(String::from("npm run dev")),
                add_to_workspace: true,
                continuous: true,
                environment: TemplateEnvironment::default(),
                env_vars: BTreeMap::new(),
                make_default: false,
            },
            2_000,
        )
        .await
        .unwrap();
    assert!(!created_tpl.is_default);

    let del_default_err = db
        .templates()
        .delete(templates[0].id, templates[0].revision)
        .await
        .unwrap_err();
    assert!(matches!(
        del_default_err,
        DbError::ConstraintViolation { ref constraint } if constraint == "template.default_required"
    ));

    let after_set_default = db
        .templates()
        .set_default(created_tpl.id, 2_200)
        .await
        .unwrap();
    assert_eq!(after_set_default[0].id, created_tpl.id);
    assert!(after_set_default[0].is_default);

    // 3. Criar aba + painel inicial + sessão de aba + split + scrollback + busca FTS5
    let tab_id = TabId::new();
    let pane1_id = PaneId::new();
    let pane2_id = PaneId::new();
    let ts_id = TabSessionId::new();
    let sess1_id = SessionId::new();
    let sess2_id = SessionId::new();

    db.write(move |tx| {
        let (_tab, _pane) = create_tab_with_initial_pane_tx(
            tx,
            &NewTabTxInput {
                id: tab_id,
                project_id: proj_id,
                template_id: Some(created_tpl.id),
                origin: TabOrigin::Workspace,
                title: SanitizedText::new("Dev Server"),
                icon: String::from("mdi:play"),
                continuous: true,
                launch_snapshot: LaunchSnapshot {
                    template_id: Some(created_tpl.id),
                    title: String::from("Dev Server"),
                    icon: String::from("mdi:play"),
                    resolved_cwd: String::from("/tmp/maxterminal"),
                    configured_cwd: None,
                    initial_command: Some(String::from("npm run dev")),
                    continuous: true,
                    environment: TemplateEnvironment::default(),
                    env_vars: BTreeMap::new(),
                },
                initial_pane_id: pane1_id,
                initial_cwd: SanitizedText::new("/tmp/maxterminal"),
                opened_at: 3_000,
            },
        )?;

        start_tab_session_tx(tx, ts_id, tab_id, 3_000)?;

        insert_session_tx(
            tx,
            &NewSession {
                id: sess1_id,
                tab_id: Some(tab_id),
                pane_id: Some(pane1_id),
                project_id: Some(proj_id),
                tab_session_id: Some(ts_id),
                pty_id: PtyId::new(),
                shell_kind: ShellKind::Zsh,
                shell_executable: SanitizedText::new("/bin/zsh"),
                initial_cwd: SanitizedText::new("/tmp/maxterminal"),
                integration_mode: IntegrationMode::Osc133,
                started_at: 3_000,
            },
        )?;

        attach_pane_session_tx(
            tx,
            sess1_id,
            ts_id,
            pane1_id,
            1,
            3_000,
            InitialDispatchState::Pending,
        )?;

        let (_updated_tab, _new_pane) = split_pane_tx(
            tx,
            tab_id,
            pane1_id,
            pane2_id,
            String::from("split-node-1"),
            SplitDirection::Vertical,
            1,
            &SanitizedText::new("/tmp/maxterminal"),
            3_100,
        )?;

        insert_session_tx(
            tx,
            &NewSession {
                id: sess2_id,
                tab_id: Some(tab_id),
                pane_id: Some(pane2_id),
                project_id: Some(proj_id),
                tab_session_id: Some(ts_id),
                pty_id: PtyId::new(),
                shell_kind: ShellKind::Zsh,
                shell_executable: SanitizedText::new("/bin/zsh"),
                initial_cwd: SanitizedText::new("/tmp/maxterminal"),
                integration_mode: IntegrationMode::Osc133,
                started_at: 3_100,
            },
        )?;

        attach_pane_session_tx(
            tx,
            sess2_id,
            ts_id,
            pane2_id,
            1,
            3_100,
            InitialDispatchState::None,
        )?;

        Ok(())
    })
    .await
    .unwrap();

    // Claim initial dispatch deve funcionar exatamente uma vez
    assert!(
        db.tab_sessions()
            .claim_initial_dispatch(sess1_id)
            .await
            .unwrap()
    );
    assert!(
        !db.tab_sessions()
            .claim_initial_dispatch(sess1_id)
            .await
            .unwrap()
    );
    db.tab_sessions()
        .mark_initial_dispatch_sent(sess1_id)
        .await
        .unwrap();

    // Append scrollback e busca FTS5
    db.scrollback()
        .append_chunk(ScrollbackRecord::new(
            sess1_id,
            1,
            3_200,
            SanitizedText::new("Vite v8.0.0 ready in 180ms at http://localhost:5177"),
            ScrollbackChunkKind::Output,
        ))
        .await
        .unwrap();

    let chunks_page = db
        .scrollback()
        .list_chunks(ts_id, sess1_id, None, 50)
        .await
        .unwrap();
    assert_eq!(chunks_page.items.len(), 1);
    assert!(chunks_page.items[0].text.contains("localhost:5177"));

    let hits = db
        .scrollback()
        .search_in_tab(tab_id, String::from("localhost"), 10)
        .await
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].tab_session_id, ts_id);
    assert_eq!(hits[0].session_id, sess1_id);
    assert_eq!(hits[0].pane_id, pane1_id);

    // Fechar e arquivar sessões e grupo
    db.write(move |tx| {
        let end = SessionEnd {
            ended_at: 4_000,
            exit_code: Some(0),
            exit_signal: None,
            last_cwd: Some(SanitizedText::new("/tmp/maxterminal")),
            integration_mode: IntegrationMode::Osc133,
            command_count: 1,
            error_count: 0,
        };
        close_pane_session_tx(tx, sess1_id, &end)?;
        close_pane_session_tx(tx, sess2_id, &end)?;
        let summary = archive_tab_session_tx(tx, ts_id, SessionEndReason::TabClosed, 4_100)?;
        assert!(summary.archived);
        assert_eq!(summary.ended_at, Some(4_100));
        assert_eq!(summary.end_reason, Some(SessionEndReason::TabClosed));
        assert!(summary.scrollback_bytes > 0);
        Ok(())
    })
    .await
    .unwrap();

    // Remover projeto apaga em cascata abas, painéis, agrupamentos, sessões e scrollback sem violar RESTRICT
    db.projects()
        .commit(ProjectMutation::Remove {
            id: proj_id,
            dismiss: Vec::new(),
        })
        .await
        .unwrap();

    assert!(db.tabs().get(tab_id).await.unwrap().is_none());
    assert!(
        db.tab_sessions()
            .get_summary(ts_id)
            .await
            .unwrap()
            .is_none()
    );
    assert!(db.sessions().get(sess1_id).await.unwrap().is_none());

    db.shutdown().await.unwrap();
}
