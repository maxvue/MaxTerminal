//! Testes de integração fim-a-fim com Zsh real em PTY (`shells_zsh.rs`).

#![allow(clippy::unwrap_used)]

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

fn find_zsh() -> Option<String> {
    for candidate in ["/usr/bin/zsh", "/bin/zsh"] {
        if Path::new(candidate).exists() {
            return Some(candidate.to_string());
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zsh_real_pty_preserves_zshrc_and_tracks_commands() {
    let Some(zsh_bin) = find_zsh() else {
        eprintln!("Zsh não encontrado no ambiente; pulando teste.");
        return;
    };

    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    let runtime_dir = tmp.path().join("runtime");
    fs::create_dir_all(&home_dir).unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();

    fs::write(home_dir.join(".zshrc"), "export ZSH_USER_RC=ativo\n").unwrap();

    let db = Db::open(DbConfig::for_path(tmp.path().join("test-zsh.db"))).unwrap();
    let pty_mgr = PtyManager::new(PtyConfig::default()).unwrap();
    let shell_svc = ShellService::new(Arc::clone(&pty_mgr), db.clone(), runtime_dir, None);

    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home_dir.to_string_lossy().into_owned());

    let summary = shell_svc
        .spawn_pty(
            SpawnSpec {
                shell: Some(zsh_bin),
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
            "Timeout aguardando IntegrationMode::Osc133 no Zsh"
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }

    let session_id = shell_svc.integration_info(pty_id).unwrap().session_id;
    pty_mgr
        .write(pty_id, b"echo \"ZSH=${ZSH_USER_RC}\"\nfalse\n")
        .unwrap();

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
            "Timeout aguardando blocos no Zsh"
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
