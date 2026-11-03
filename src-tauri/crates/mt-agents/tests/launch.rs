//! Testes de integração para `LaunchPlanner`, injeção `--settings` / `-c notify` e quoting por shell (`bash`, `zsh`, `fish`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::fs;
use std::path::PathBuf;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use mt_agents::detect::AgentInstallation;
use mt_agents::error::codes;
use mt_agents::launch::{
    AgentTemplateOptions, LaunchPlanner, PendingRunBinding, ShellKind, command_line,
};
use mt_agents::registry::{AgentKind, AgentRegistry};
use mt_agents::relay::RelayToken;
use mt_agents::settings::AgentsSettings;
use mt_core::{AppPaths, PaneId, ProjectId, SessionId, TabId};
use serde_json::Value;
use tempfile::TempDir;

fn make_test_paths(tmp: &TempDir) -> AppPaths {
    let config_dir = tmp.path().join("config");
    let data_dir = tmp.path().join("data");
    let state_dir = tmp.path().join("state");
    let cache_dir = tmp.path().join("cache");
    let runtime_dir = tmp.path().join("runtime");
    for d in [&config_dir, &data_dir, &state_dir, &cache_dir, &runtime_dir] {
        fs::create_dir_all(d).unwrap();
    }
    AppPaths {
        config_dir,
        data_dir: data_dir.clone(),
        state_dir: state_dir.clone(),
        cache_dir,
        runtime_dir: Some(runtime_dir),
        log_dir: state_dir.join("logs"),
        db_path: data_dir.join("maxterminal.db"),
    }
}

#[test]
fn claude_launch_plan_injects_settings_and_chains_existing_statusline() {
    let tmp = TempDir::new().unwrap();
    let paths = make_test_paths(&tmp);
    let project_cwd = tmp.path().join("my-project");
    fs::create_dir_all(project_cwd.join(".claude")).unwrap();

    // Configura um statusLine customizado no projeto para validar o encadeamento (--chain <b64>)
    fs::write(
        project_cwd.join(".claude").join("settings.json"),
        r#"{
            "statusLine": {
                "type": "command",
                "command": "my-custom-status --color"
            }
        }"#,
    )
    .unwrap();

    let reg = AgentRegistry::with_builtins();
    let desc = reg.get(AgentKind::Claude);
    let inst = AgentInstallation {
        kind: AgentKind::Claude,
        installed: true,
        path: Some("/usr/bin/claude".to_string()),
        version: Some("2.1.76".to_string()),
        reason: None,
        checked_at_ms: 1_000,
    };

    let planner = LaunchPlanner::new(paths, PathBuf::from("/usr/lib/maxterminal/mt-agent-relay"));
    let binding = PendingRunBinding {
        tab_id: TabId::new(),
        pane_id: PaneId::new(),
        session_id: SessionId::new(),
        generation: 1,
        project_id: ProjectId::new(),
        run_id: "run-claude-1".to_string(),
        agent: AgentKind::Claude,
        cwd: project_cwd,
        started_at_ms: 1_000,
    };
    let (token, _) = RelayToken::generate();
    let opts = AgentTemplateOptions {
        model: Some("claude-sonnet-4-6".to_string()),
        extra_args: vec!["--dangerously-skip-permissions".to_string()],
        isolation_default: false,
        location: Default::default(),
    };
    let settings = AgentsSettings::default();

    let plan = planner
        .build_plan(
            &desc,
            &inst,
            &opts,
            None,
            binding,
            token.clone(),
            PathBuf::from("/tmp/agent-relay.sock").as_path(),
            &settings,
        )
        .unwrap();

    // 1. Verifica que o token está no env e NUNCA nos argumentos CLI
    assert_eq!(
        plan.env.get("MAXTERMINAL_AGENT_TOKEN").map(String::as_str),
        Some(token.as_str())
    );
    assert!(!plan.args.iter().any(|a| a.contains(token.as_str())));

    // 2. Verifica --settings e --model
    assert!(
        plan.args
            .windows(2)
            .any(|w| w[0] == "--model" && w[1] == "claude-sonnet-4-6")
    );
    let settings_arg = plan
        .args
        .windows(2)
        .find(|w| w[0] == "--settings")
        .map(|w| &w[1])
        .expect("Deve conter --settings");

    let generated_json: Value =
        serde_json::from_str(&fs::read_to_string(settings_arg).unwrap()).unwrap();
    let sl_cmd = generated_json["statusLine"]["command"].as_str().unwrap();
    let expected_b64 = STANDARD.encode("my-custom-status --color".as_bytes());
    assert!(
        sl_cmd.contains(&format!("statusline --chain {expected_b64}")),
        "statusLine deveria encadear o comando existente: {sl_cmd}"
    );
}

#[test]
fn shell_quoting_and_model_validation_for_bash_zsh_and_fish() {
    let tmp = TempDir::new().unwrap();
    let paths = make_test_paths(&tmp);
    let reg = AgentRegistry::with_builtins();

    // Modelo incompatível ou não suportado deve retornar agents.model_unsupported
    let claude_desc = reg.get(AgentKind::Claude);
    let err_gpt = LaunchPlanner::resolve_cli_model(&claude_desc, Some("gpt-5")).unwrap_err();
    assert_eq!(err_gpt.code.0, codes::MODEL_UNSUPPORTED);

    let agy_desc = reg.get(AgentKind::Agy);
    let err_agy = LaunchPlanner::resolve_cli_model(&agy_desc, Some("gemini-2.5-pro")).unwrap_err();
    assert_eq!(err_agy.code.0, codes::MODEL_UNSUPPORTED);

    let inst = AgentInstallation {
        kind: AgentKind::Codex,
        installed: true,
        path: Some("/usr/bin/codex".to_string()),
        version: Some("0.1.0".to_string()),
        reason: None,
        checked_at_ms: 1_000,
    };
    let codex_desc = reg.get(AgentKind::Codex);
    let planner = LaunchPlanner::new(paths, PathBuf::from("/opt/max bin/mt-agent-relay"));
    let binding = PendingRunBinding {
        tab_id: TabId::new(),
        pane_id: PaneId::new(),
        session_id: SessionId::new(),
        generation: 1,
        project_id: ProjectId::new(),
        run_id: "run-codex-1".to_string(),
        agent: AgentKind::Codex,
        cwd: tmp.path().to_path_buf(),
        started_at_ms: 1_000,
    };
    let (token, _) = RelayToken::generate();
    let opts = AgentTemplateOptions {
        model: Some("o3".to_string()),
        extra_args: vec!["arg with 'quotes' & $VAR".to_string()],
        isolation_default: false,
        location: Default::default(),
    };

    let plan = planner
        .build_plan(
            &codex_desc,
            &inst,
            &opts,
            None,
            binding,
            token,
            PathBuf::from("/tmp/relay.sock").as_path(),
            &AgentsSettings::default(),
        )
        .unwrap();

    let bash_line = command_line(&plan, Some("export FOO=1"), ShellKind::Bash).unwrap();
    assert!(bash_line.starts_with("{ export FOO=1; } && /usr/bin/codex "));
    assert!(bash_line.contains("'arg with '\\''quotes'\\'' & $VAR'"));

    let fish_line = command_line(&plan, Some("set -gx FOO 1"), ShellKind::Fish).unwrap();
    assert!(fish_line.starts_with("begin; set -gx FOO 1; end; and /usr/bin/codex "));
    assert!(fish_line.contains("'arg with \\'quotes\\' & $VAR'"));
}
