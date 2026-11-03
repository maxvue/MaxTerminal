//! Testes de integração de auto-descoberta via `RootWatcher` (`notify`) no `ProjectService`.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mt_db::{Db, DbConfig};
use mt_projects::{
    DiscoveryMode, MemoryProjectEventSink, NewProjectRoot, ProjectEventSink, ProjectService,
    SqliteProjectStore, StackKind, WatchStatus,
};
use tempfile::TempDir;

#[tokio::test]
async fn watcher_detects_new_subfolder_in_suggest_and_auto_import_modes() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();
    let store = Arc::new(SqliteProjectStore::from_db(&db));
    let sink = Arc::new(MemoryProjectEventSink::new());
    let sink_dyn: Arc<dyn ProjectEventSink> = sink.clone();
    let svc = ProjectService::new(store, sink_dyn);
    svc.start().await.unwrap();

    let root_dir = tmp.path().join("watched-github");
    fs::create_dir_all(&root_dir).unwrap();

    let rep = svc
        .add_root(NewProjectRoot {
            path: root_dir.to_string_lossy().into_owned(),
            discovery_mode: DiscoveryMode::Suggest,
            import_existing: false,
            watch_enabled: true,
        })
        .await
        .unwrap();
    assert_eq!(rep.root.watch_status, WatchStatus::Watching);

    // Cria uma nova subpasta Rust dentro da raiz monitorada
    let new_sub = root_dir.join("novo-crate-rust");
    fs::create_dir_all(&new_sub).unwrap();
    fs::write(new_sub.join("Cargo.toml"), "[package]\nname = \"novo\"\n").unwrap();

    // Aguarda a auto-descoberta (DEBOUNCE 500ms + SETTLE 1500ms <= 3.5s)
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut found = false;
    while Instant::now() < deadline {
        let disc = svc.list_discovered().await.unwrap();
        if disc.len() == 1 && disc[0].suggested_title == "novo-crate-rust" {
            assert_eq!(disc[0].identity.stack, StackKind::Rust);
            found = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        found,
        "A nova subpasta deveria ter sido auto-descoberta pelo watcher"
    );
    assert!(!sink.discovered_events.lock().is_empty());

    svc.shutdown().await;
    db.shutdown().await.unwrap();
}
