//! Testes de integração fim-a-fim com GNU Bash real em PTY (`shells_bash.rs`).

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

fn find_bash() -> Option<String> {
    for candidate in ["/usr/bin/bash", "/bin/bash"] {
        if Path::new(candidate).exists() {
            return Some(candidate.to_string());
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bash_real_pty_emits_osc133_captures_blocks_and_sanitizes_error_context() {
    let Some(bash_bin) = find_bash() else {
        eprintln!("Bash não encontrado no ambiente; pulando teste.");
        return;
    };

    let tmp = tempfile::tempdir().unwrap();
    let home_dir = tmp.path().join("home");
    let runtime_dir = tmp.path().join("runtime");
    let work_dir = home_dir.join("subpasta");
    fs::create_dir_all(&work_dir).unwrap();
    fs::create_dir_all(&runtime_dir).unwrap();

    // Dotfile do usuário no HOME temporário para provar que o .bashrc continua sendo carregado
    fs::write(home_dir.join(".bashrc"), "export USER_RC_LOADED=sim\n").unwrap();

    let db = Db::open(DbConfig::for_path(tmp.path().join("test-bash.db"))).unwrap();
    let pty_mgr = PtyManager::new(PtyConfig::default()).unwrap();
    let shell_svc = ShellService::new(Arc::clone(&pty_mgr), db.clone(), runtime_dir, None);

    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home_dir.to_string_lossy().into_owned());

    let spec = SpawnSpec {
        shell: Some(bash_bin),
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
    };

    let summary = shell_svc
        .spawn_pty(
            spec,
            Some(ShellIntegrationRequest {
                enabled: true,
                continuous_policy: ContinuousPolicy::Auto,
                project_id: None,
                tab_session_id: None,
            }),
        )
        .unwrap();

    let pty_id = summary.id;

    // Aguarda o prompt inicial ativar IntegrationMode::Osc133
    let start_wait = Instant::now();
    loop {
        let info = shell_svc.integration_info(pty_id).unwrap();
        if info.mode == IntegrationMode::Osc133 {
            break;
        }
        assert!(
            start_wait.elapsed() < Duration::from_secs(5),
            "Timeout aguardando IntegrationMode::Osc133 no Bash (modo atual: {:?})",
            info.mode
        );
        tokio::time::sleep(Duration::from_millis(40)).await;
    }

    let session_id = shell_svc.integration_info(pty_id).unwrap().session_id;

    // Envia 3 comandos:
    // 1) `cd subpasta && echo "RC=${USER_RC_LOADED}"` (exit 0)
    // 2) `(exit 42)` (exit 42)
    // 3) `echo 'Authorization: Bearer sk-1234567890abcdef12345678'; false` (exit 1 com segredo)
    let cmds = b"cd subpasta && echo \"RC=${USER_RC_LOADED}\"\n(exit 42)\necho 'Authorization: Bearer sk-1234567890abcdef12345678'; false\n";
    pty_mgr.write(pty_id, cmds).unwrap();

    // Aguarda os 3 blocos finalizarem no banco
    let wait_blocks = Instant::now();
    let blocks = loop {
        let page = shell_svc
            .command_block_list(CommandBlockListQuery {
                session_id: Some(session_id),
                limit: Some(20),
                ..Default::default()
            })
            .await
            .unwrap();

        let finished: Vec<_> = page
            .items
            .clone()
            .into_iter()
            .filter(|b| b.status != BlockStatus::Running)
            .collect();
        if finished.len() >= 3 {
            break finished;
        }
        assert!(
            wait_blocks.elapsed() < Duration::from_secs(6),
            "Timeout aguardando os 3 blocos finalizarem no Bash (encontrados: {}, todos na pagina: {:?}, info: {:?})",
            finished.len(),
            page.items,
            shell_svc.integration_info(pty_id)
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };

    // Verifica que o cwd mudou para `subpasta`
    let info_after = shell_svc.integration_info(pty_id).unwrap();
    assert!(
        info_after
            .cwd
            .as_deref()
            .unwrap_or_default()
            .ends_with("subpasta")
    );

    // Encontra o bloco com exit 42 e o bloco com segredo Bearer
    let b_42 = blocks.iter().find(|b| b.exit_code == Some(42)).unwrap();
    assert_eq!(b_42.status, BlockStatus::Failed);
    assert!(b_42.inspectable);

    let b_secret = blocks
        .iter()
        .find(|b| {
            b.command_text
                .as_deref()
                .unwrap_or_default()
                .contains("Authorization")
                || b.output_text
                    .as_deref()
                    .unwrap_or_default()
                    .contains("[REDACTED:")
        })
        .unwrap();
    assert_eq!(b_secret.status, BlockStatus::Failed);
    assert!(b_secret.inspectable);

    let err_ctx = shell_svc
        .command_block_error_context(b_secret.id)
        .await
        .unwrap();
    assert!(!err_ctx.command.contains("sk-1234567890abcdef12345678"));
    assert!(!err_ctx.error.contains("sk-1234567890abcdef12345678"));
    assert!(
        err_ctx
            .plan_mode_prompt
            .starts_with("Ao executar o comando \"")
    );
    assert!(
        err_ctx
            .plan_mode_prompt
            .contains("\" tive o seguinte erro: \"")
    );

    shell_svc.shutdown();
    let _ = pty_mgr.shutdown(Duration::from_secs(2));
}
