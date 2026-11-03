//! Testes de integração SQLite para `SqliteCommandGroupRepo` (`CA-02`, `CA-03`, `CA-23`, `CA-24`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::collections::BTreeMap;

use mt_commands::groups::model::{
    CommandGroupInput, CommandGroupItemInput, ExecutionMode, GroupErrorCode, GroupRequestId,
    GroupRunId, GroupRunItemId, GroupRunItemSnapshot, GroupRunSnapshot, GroupScope, ItemStatus,
    ReorderGroupEntry, RunStatus, SequentialSeparator, ShellDialect,
};
use mt_commands::groups::repo::{
    AcceptRunOutcome, CommandGroupRepo, CreateRunRecord, RunItemPatch, SqliteCommandGroupRepo,
};
use mt_core::ids::ProjectId;
use mt_db::{Db, DbConfig};
use tempfile::TempDir;

async fn setup_db_with_projects() -> (TempDir, Db, ProjectId, ProjectId) {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    let p1 = ProjectId::new();
    let p2 = ProjectId::new();
    let p1_str = p1.to_string();
    let p2_str = p2.to_string();

    db.write(move |tx| {
        tx.execute(
            "INSERT INTO projects (id, title, path, icon, color, stack, sort_order, created_at, updated_at)
             VALUES (?1, 'Projeto 1', '/tmp/p1', 'lucide:box', '#22C55E', 'rust', 0, 1000, 1000),
                    (?2, 'Projeto 2', '/tmp/p2', 'lucide:box', '#3B82F6', 'vue', 1, 1000, 1000);",
            rusqlite::params![p1_str, p2_str],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    (tmp, db, p1, p2)
}

fn make_input(
    scope: GroupScope,
    project_id: Option<ProjectId>,
    name: &str,
    cmds: &[&str],
) -> CommandGroupInput {
    let items = cmds
        .iter()
        .enumerate()
        .map(|(idx, c)| CommandGroupItemInput {
            id: None,
            title: format!("Passo {}", idx + 1),
            command: (*c).to_string(),
            cwd: None,
            env: BTreeMap::new(),
            continuous: false,
        })
        .collect();

    CommandGroupInput {
        scope,
        project_id,
        name: name.to_string(),
        description: "Descrição".to_string(),
        labels: vec!["dev".to_string(), "ci".to_string()],
        mode: ExecutionMode::Sequential,
        separator: SequentialSeparator::And,
        cwd: None,
        env: BTreeMap::new(),
        items,
    }
}

#[tokio::test]
async fn local_before_global_ordering_and_unicode_uniqueness() {
    let (_tmp, db, p1, p2) = setup_db_with_projects().await;
    let repo = SqliteCommandGroupRepo::new(db.clone());

    let g_global_1 = repo
        .create(&make_input(
            GroupScope::Global,
            None,
            "Global Alpha",
            &["echo g1"],
        ))
        .unwrap();
    let g_global_2 = repo
        .create(&make_input(
            GroupScope::Global,
            None,
            "Global Beta",
            &["echo g2"],
        ))
        .unwrap();

    let g_local_1 = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p1),
            "  Árvore Local ",
            &["echo l1", "echo l2"],
        ))
        .unwrap();
    let g_local_2 = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p1),
            "Segundo Local",
            &["echo l3"],
        ))
        .unwrap();

    let _g_same_global = repo
        .create(&make_input(
            GroupScope::Global,
            None,
            "árvore local",
            &["echo global_same"],
        ))
        .unwrap();
    let _g_same_p2 = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p2),
            "árvore local",
            &["echo p2_same"],
        ))
        .unwrap();

    let dup_err = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p1),
            "a\u{0301}RVORE local",
            &["echo dup"],
        ))
        .unwrap_err();
    assert_eq!(dup_err.code.0, GroupErrorCode::NameConflict.as_str());

    let cat_p1 = repo.list(Some(&p1)).unwrap();
    assert_eq!(cat_p1.local_groups.len(), 2);
    assert_eq!(cat_p1.global_groups.len(), 3);
    assert_eq!(cat_p1.local_groups[0].id, g_local_1.id);
    assert_eq!(cat_p1.local_groups[1].id, g_local_2.id);
    assert_eq!(cat_p1.global_groups[0].id, g_global_1.id);
    assert_eq!(cat_p1.global_groups[1].id, g_global_2.id);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn atomic_update_revision_conflict_and_reorder_validation() {
    let (_tmp, db, p1, _p2) = setup_db_with_projects().await;
    let repo = SqliteCommandGroupRepo::new(db.clone());

    let g1 = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p1),
            "Grupo 1",
            &["echo 1", "echo 2"],
        ))
        .unwrap();
    let g2 = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p1),
            "Grupo 2",
            &["echo 3"],
        ))
        .unwrap();

    let updated = repo
        .update(
            &g1.id,
            1,
            &make_input(
                GroupScope::Local,
                Some(p1),
                "Grupo 1 Editado",
                &["echo novo"],
            ),
        )
        .unwrap();
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.items.len(), 1);
    assert_eq!(updated.items[0].command, "echo novo");

    let conflict_err = repo
        .update(
            &g1.id,
            1,
            &make_input(
                GroupScope::Local,
                Some(p1),
                "Tentativa Stale",
                &["echo a", "echo b", "echo c"],
            ),
        )
        .unwrap_err();
    assert_eq!(
        conflict_err.code.0,
        GroupErrorCode::RevisionConflict.as_str()
    );

    let reloaded = repo.get(&g1.id).unwrap();
    assert_eq!(reloaded.name, "Grupo 1 Editado");
    assert_eq!(reloaded.items.len(), 1);

    let reorder_err = repo
        .reorder(
            GroupScope::Local,
            Some(&p1),
            &[ReorderGroupEntry {
                id: g1.id,
                revision: 2,
            }],
        )
        .unwrap_err();
    assert_eq!(reorder_err.code.0, GroupErrorCode::ReorderMismatch.as_str());

    let cat = repo
        .reorder(
            GroupScope::Local,
            Some(&p1),
            &[
                ReorderGroupEntry {
                    id: g2.id,
                    revision: 1,
                },
                ReorderGroupEntry {
                    id: g1.id,
                    revision: 2,
                },
            ],
        )
        .unwrap();
    assert_eq!(cat.local_groups[0].id, g2.id);
    assert_eq!(cat.local_groups[1].id, g1.id);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn project_deletion_cascades_local_groups_and_preserves_globals() {
    let (_tmp, db, p1, _p2) = setup_db_with_projects().await;
    let repo = SqliteCommandGroupRepo::new(db.clone());

    let local_group = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p1),
            "Local P1",
            &["echo p1"],
        ))
        .unwrap();
    let global_group = repo
        .create(&make_input(
            GroupScope::Global,
            None,
            "Global",
            &["echo global"],
        ))
        .unwrap();

    let p1_str = p1.to_string();
    db.write(move |tx| {
        tx.execute(
            "DELETE FROM projects WHERE id = ?1;",
            rusqlite::params![p1_str],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let err = repo.get(&local_group.id).unwrap_err();
    assert_eq!(err.code.0, GroupErrorCode::NotFound.as_str());
    assert!(repo.get(&global_group.id).is_ok());

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn request_id_idempotency_and_prune_retention() {
    let (_tmp, db, p1, _p2) = setup_db_with_projects().await;
    let repo = SqliteCommandGroupRepo::new(db.clone());

    let g = repo
        .create(&make_input(
            GroupScope::Local,
            Some(p1),
            "Runner Group",
            &["echo ok"],
        ))
        .unwrap();

    let req_id = GroupRequestId("req-idemp-1".to_string());
    let item_id = GroupRunItemId::new();
    let rec = CreateRunRecord {
        run_id: GroupRunId::new(),
        request_id: req_id.clone(),
        request_hash: "hash123".to_string(),
        group_id: g.id,
        project_id: p1,
        origin_session_id: None,
        snapshot: GroupRunSnapshot {
            group_id: g.id,
            group_revision: g.revision,
            scope: g.scope,
            name: g.name.clone(),
            description: g.description.clone(),
            labels: g.labels.clone(),
            defined_mode: g.mode,
            effective_mode: ExecutionMode::Parallel,
            separator: g.separator,
            cwd: None,
            env: BTreeMap::new(),
            project_root: "/tmp/p1".to_string(),
            shell_path: "/bin/bash".to_string(),
            shell_dialect: ShellDialect::Bash,
            origin_tab_id: None,
            origin_pane_id: None,
            origin_pty_id: None,
            origin_session_id: None,
            origin_generation: None,
        },
        mode: ExecutionMode::Parallel,
        items: vec![(
            item_id.clone(),
            GroupRunItemSnapshot {
                source_item_id: g.items[0].id.clone(),
                position: 0,
                title: "Passo 1".to_string(),
                command: "echo ok".to_string(),
                defined_cwd: None,
                resolved_cwd: "/tmp/p1".to_string(),
                env: BTreeMap::new(),
                continuous: false,
                template_id: None,
                shell_path: "/bin/bash".to_string(),
                shell_dialect: ShellDialect::Bash,
            },
        )],
        created_at: 1000,
    };

    let outcome1 = repo.accept_run(rec.clone()).unwrap();
    let created_run = match outcome1 {
        AcceptRunOutcome::Created(r) => r,
        AcceptRunOutcome::Existing(_) => panic!("esperava Created"),
    };

    // Segundo accept_run com o mesmo request_id e mesmo hash retorna Existing
    let outcome2 = repo.accept_run(rec).unwrap();
    assert!(matches!(outcome2, AcceptRunOutcome::Existing(r) if r.id == created_run.id));

    let found = repo
        .find_run_by_request_id(&req_id)
        .unwrap()
        .expect("deve encontrar por request_id");
    assert_eq!(found.1.id, created_run.id);

    // Transiciona para Running e depois conclui o item com Succeeded
    repo.update_run_item(
        &created_run.id,
        &item_id,
        RunItemPatch {
            status: Some(ItemStatus::Running),
            started_at: Some(Some(1100)),
            ..RunItemPatch::default()
        },
        1100,
    )
    .unwrap();

    let (finished_run, _) = repo
        .update_run_item(
            &created_run.id,
            &item_id,
            RunItemPatch {
                status: Some(ItemStatus::Succeeded),
                exit_code: Some(Some(0)),
                finished_at: Some(Some(2000)),
                output_tail: Some(("ok\n".to_string(), false)),
                ..RunItemPatch::default()
            },
            2000,
        )
        .unwrap();
    assert_eq!(finished_run.status, RunStatus::Succeeded);

    let pruned = repo.prune_finished_runs_older_than(5000).unwrap();
    assert_eq!(pruned, 1);

    db.shutdown().await.unwrap();
}
