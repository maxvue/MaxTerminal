//! Testes de integração fim-a-fim com Fish Shell real em PTY (`shells_fish.rs`).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mt_core::ids::{PaneId, TabId};
use mt_core::shell::{
    BlockStatus, CommandBlockListQuery, ContinuousPolicy, IntegrationMode, ShellIntegrationRequest,
};
use mt_db::{Db, DbConfig};
use mt_pty::{PtyConfig, PtyManager, PtyOwner, PtySize, SpawnSpec};
use mt_shell::ShellService;

fn find_fish() -> Option<String> {
    for candidate in ["/usr/bin/fish", "/bin/fish", "/usr/local/bin/fish"] {
        if Path::new(candidate).exists() {
            return Some(candidate.to_string());
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fish_real_pty_emits_osc133_and_tracks_exit_codes() {
    let Some(fish_bin) = find_fish() else {
        eprintln!("Fish não encontrado no ambiente; pulando teste.");
        return;
    };

    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    let runtime_dir = tmp.path().join("runtime");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();

    let db = Db::open(DbConfig::for_path(tmp.path().join("test-fish.db"))).unwrap();
    let pty_mgr = PtyManager::new(PtyConfig::default()).unwrap();
    let shell_svc = ShellService::new(Arc::clone(&pty_mgr), db.clone(), runtime_dir, None);

    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home_dir.to_string_lossy().into_owned());
    env.insert(
        "XDG_CONFIG_HOME".to_string(),
        home_dir.join(".config").to_string_lossy().into_owned(),
    );
    env.insert(
        "XDG_DATA_HOME".to_string(),
        home_dir.join(".local/share").to_string_lossy().into_owned(),
    );

    let summary = shell_svc
        .spawn_pty(
            SpawnSpec {
                shell: Some(fish_bin),
                args: Vec::new(),
                login: true,
                cwd: Some(home_dir.to_string_lossy().into_owned()),
                env,
                env_remove: Vec::new(),
                size: PtySize {
                    cols: 100,
                    rows: 30,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                owner: PtyOwner {
                    tab_id: Some(tab_id),
                    pane_id: Some(pane_id),
                },
            },
            Some(ShellIntegrationRequest {
                enabled: true,
                continuous_policy: ContinuousPolicy::Auto,
                project_id: None,
                tab_session_id: None,
            }),
        )
        .unwrap();

    let pty_id = summary.id;
    let start_wait = Instant::now();
    loop {
        let info = shell_svc.integration_info(pty_id).unwrap();
        if info.mode == IntegrationMode::Osc133 {
            break;
        }
        assert!(
            start_wait.elapsed() < Duration::from_secs(5),
            "Timeout aguardando IntegrationMode::Osc133 no Fish (info: {:?}, summary: {:?})",
            info,
            pty_mgr.list()
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }

    let session_id = shell_svc.integration_info(pty_id).unwrap().session_id;
    pty_mgr.write(pty_id, b"echo fish_ok\nfalse\n").unwrap();

    let wait_blocks = Instant::now();
    let blocks = loop {
        let page = shell_svc
            .command_block_list(CommandBlockListQuery {
                session_id: Some(session_id),
                limit: Some(10),
                ..Default::default()
            })
            .await
            .unwrap();
        let finished: Vec<_> = page
            .items
            .into_iter()
            .filter(|b| b.status != BlockStatus::Running)
            .collect();
        if finished.len() >= 2 {
            break finished;
        }
        assert!(
            wait_blocks.elapsed() < Duration::from_secs(6),
            "Timeout aguardando blocos no Fish"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    assert!(
        blocks
            .iter()
            .any(|b| b.status == BlockStatus::Succeeded && b.exit_code == Some(0))
    );
    assert!(
        blocks
            .iter()
            .any(|b| b.status == BlockStatus::Failed && b.exit_code == Some(1))
    );

    shell_svc.shutdown();
    let _ = pty_mgr.shutdown(Duration::from_secs(2));
}
