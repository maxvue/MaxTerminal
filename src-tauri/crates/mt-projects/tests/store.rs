//! Testes de integração de `SqliteProjectStore` e constraints da migration `0003_06_projects.sql`.

#![allow(clippy::unwrap_used)]

use mt_core::error::codes;
use mt_core::ids::ProjectId;
use mt_db::{Db, DbConfig};
use mt_projects::{
    CatalogMutation, DiscoveryMode, DismissedRow, IdentitySource, NewProjectRow, Project,
    ProjectRootId, ProjectRootRow, ProjectStore, ReconcileBatch, RootEntryRow, SqliteProjectStore,
    StackKind,
};
use tempfile::TempDir;

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn sqlite_project_store_roundtrip_and_constraints() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg.clone()).unwrap();
    assert!(db.schema_version() >= 4);

    // Verifica PRAGMA foreign_key_check limpo
    let fk_violations: i64 = db
        .read(|conn| {
            let mut stmt = conn.prepare("PRAGMA foreign_key_check;").unwrap();
            let count = stmt.query_map([], |_| Ok(1)).unwrap().count();
            #[allow(clippy::cast_possible_wrap)]
            Ok(count as i64)
        })
        .await
        .unwrap();
    assert_eq!(fk_violations, 0);

    let store = SqliteProjectStore::from_db(&db);

    // 1. AddRoot + auto_import
    let root_id = ProjectRootId::new();
    let proj1_id = ProjectId::new();
    let root_row = ProjectRootRow {
        id: root_id,
        path: String::from("/tmp/root-a"),
        discovery_mode: DiscoveryMode::Suggest,
        watch_enabled: true,
        created_at: 1_000,
        updated_at: 1_000,
    };

    let proj1 = Project {
        id: proj1_id,
        title: String::from("Projeto Um"),
        icon: String::from("logos:rust"),
        color: String::from("#CE422B"),
        path: String::from("/tmp/root-a/proj-1"),
        root_id: Some(root_id),
        stack: StackKind::Rust,
        identity_source: IdentitySource::Auto,
        sort_order: 0,
        missing: false,
        created_at: 1_000,
        updated_at: 1_000,
    };

    let commit_root = store
        .commit(CatalogMutation::AddRoot {
            root: root_row,
            reconciliation: ReconcileBatch {
                upserted_entries: vec![RootEntryRow {
                    root_id,
                    entry_path: String::from("/tmp/root-a/proj-1"),
                    canonical_path: String::from("/tmp/root-a/proj-1"),
                    observed_at: 1_000,
                }],
                removed_entries: Vec::new(),
                auto_imports: vec![NewProjectRow {
                    project: proj1.clone(),
                    clear_dismissed_paths: Vec::new(),
                    if_exists_noop: true,
                }],
                identity_updates: Vec::new(),
                missing_updates: Vec::new(),
            },
        })
        .await
        .unwrap();

    assert_eq!(commit_root.inserted_projects.len(), 1);
    assert_eq!(commit_root.catalog.projects.len(), 1);
    assert_eq!(commit_root.catalog.entries.len(), 1);

    // 2. Add segundo projeto e verificar sort_order = 1
    let proj2_id = ProjectId::new();
    let proj2 = Project {
        id: proj2_id,
        title: String::from("Projeto Dois"),
        icon: String::from("logos:vue"),
        color: String::from("#42B883"),
        path: String::from("/tmp/proj-2"),
        root_id: None,
        stack: StackKind::Vue,
        identity_source: IdentitySource::Manual,
        sort_order: 0,
        missing: false,
        created_at: 2_000,
        updated_at: 2_000,
    };
    let commit_add2 = store
        .commit(CatalogMutation::Add(NewProjectRow {
            project: proj2,
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap();
    assert_eq!(commit_add2.upserted_projects[0].sort_order, 1);

    // Duplicata manual deve falhar com project.duplicate_path
    let dup_err = store
        .commit(CatalogMutation::Add(NewProjectRow {
            project: Project {
                id: ProjectId::new(),
                path: String::from("/tmp/proj-2"),
                ..proj1.clone()
            },
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap_err();
    assert_eq!(dup_err.code, codes::PROJECT_DUPLICATE_PATH);

    // 3. Reorder invertendo [proj2_id, proj1_id]
    store
        .commit(CatalogMutation::Reorder(vec![proj2_id, proj1_id]))
        .await
        .unwrap();

    let catalog = store.load_catalog().await.unwrap();
    assert_eq!(catalog.projects[0].id, proj2_id);
    assert_eq!(catalog.projects[0].sort_order, 0);
    assert_eq!(catalog.projects[1].id, proj1_id);
    assert_eq!(catalog.projects[1].sort_order, 1);

    // Reorder com lista incompleta deve falhar com project.reorder_mismatch
    let reorder_err = store
        .commit(CatalogMutation::Reorder(vec![proj1_id]))
        .await
        .unwrap_err();
    assert_eq!(reorder_err.code, codes::PROJECT_REORDER_MISMATCH);

    // 4. RemoveRoot deve aplicar ON DELETE SET NULL em projects.root_id e CASCADE em entries/dismissed
    store
        .commit(CatalogMutation::Dismiss(vec![DismissedRow {
            root_id,
            path: String::from("/tmp/root-a/ignored"),
            dismissed_at: 3_000,
        }]))
        .await
        .unwrap();

    store
        .commit(CatalogMutation::RemoveRoot(root_id))
        .await
        .unwrap();

    let after_root_del = store.load_catalog().await.unwrap();
    assert_eq!(after_root_del.roots.len(), 0);
    assert_eq!(after_root_del.entries.len(), 0);
    assert_eq!(after_root_del.dismissed.len(), 0);
    assert_eq!(after_root_del.projects.len(), 2);
    let p1_after = after_root_del
        .projects
        .iter()
        .find(|p| p.id == proj1_id)
        .unwrap();
    assert!(p1_after.root_id.is_none());

    db.shutdown().await.unwrap();
}
