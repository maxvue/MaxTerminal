//! Testes de integração para `AgentRegistry`, overrides `agents.toml` e validação de `AgentsSettings`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::collections::HashSet;
use std::fs;

use mt_agents::error::codes;
use mt_agents::registry::{AgentKind, AgentRegistry};
use mt_agents::settings::{
    AgentsSettings, default_reserved_shortcuts, normalize_and_validate_shortcut, shift_variant_of,
    validate_settings,
};
use mt_core::AppPaths;
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
fn loads_four_builtin_descriptors_in_canonical_order() {
    let tmp = TempDir::new().unwrap();
    let paths = make_test_paths(&tmp);

    let (reg, err) = AgentRegistry::load(&paths);
    assert!(err.is_none());

    let descriptors = reg.descriptors();
    assert_eq!(descriptors.len(), 4);
    assert_eq!(descriptors[0].kind, AgentKind::Claude);
    assert_eq!(descriptors[0].display_name, "Claude Code");
    assert_eq!(descriptors[0].binary, "claude");
    assert_eq!(descriptors[0].shortcut, "Ctrl+Alt+C");
    assert!(descriptors[0].capabilities.statusline_hook);
    assert!(descriptors[0].capabilities.native_attention_hooks);

    assert_eq!(descriptors[1].kind, AgentKind::Codex);
    assert_eq!(descriptors[1].display_name, "Codex");
    assert_eq!(descriptors[1].binary, "codex");
    assert_eq!(descriptors[1].shortcut, "Ctrl+Alt+X");

    assert_eq!(descriptors[2].kind, AgentKind::Agy);
    assert_eq!(descriptors[2].display_name, "Antigravity (agy)");
    assert_eq!(descriptors[2].binary, "agy");
    assert_eq!(descriptors[2].shortcut, "Ctrl+Alt+A");
    assert!(descriptors[2].model_flag.is_none());

    assert_eq!(descriptors[3].kind, AgentKind::OpenCode);
    assert_eq!(descriptors[3].display_name, "OpenCode");
    assert_eq!(descriptors[3].binary, "opencode");
    assert_eq!(descriptors[3].shortcut, "Ctrl+Alt+O");
    assert!(descriptors[3].model_flag.is_none());
}

#[test]
fn valid_agents_toml_overrides_fields_and_preserves_defaults() {
    let tmp = TempDir::new().unwrap();
    let paths = make_test_paths(&tmp);

    fs::write(
        paths.config_dir.join("agents.toml"),
        r#"
[claude]
binary = "/opt/claude/bin/claude"
extra_args = ["--verbose"]
shortcut = "ctrl+alt+k"

[claude.env]
CLAUDE_CUSTOM = "1"

[agy]
enabled = false
"#,
    )
    .unwrap();

    let (reg, err) = AgentRegistry::load(&paths);
    assert!(err.is_none(), "Não deveria haver erro: {err:?}");

    let claude = reg.get(AgentKind::Claude);
    assert_eq!(claude.binary, "/opt/claude/bin/claude");
    assert_eq!(claude.extra_args, vec!["--verbose".to_string()]);
    assert_eq!(claude.shortcut, "Ctrl+Alt+K");
    assert_eq!(
        claude.env.get("CLAUDE_CUSTOM").map(String::as_str),
        Some("1")
    );
    assert_eq!(claude.display_name, "Claude Code");

    assert!(reg.is_enabled(AgentKind::Claude));
    assert!(!reg.is_enabled(AgentKind::Agy));
}

#[test]
fn corrupted_or_unknown_field_in_agents_toml_keeps_builtins_and_reports_error() {
    let tmp = TempDir::new().unwrap();
    let paths = make_test_paths(&tmp);

    fs::write(
        paths.config_dir.join("agents.toml"),
        r#"
[claude]
unknown_field = "bad"
"#,
    )
    .unwrap();

    let (reg, err) = AgentRegistry::load(&paths);
    let err = err.expect("Deveria retornar erro para chave desconhecida");
    assert_eq!(err.code.0, codes::SETTINGS_INVALID);

    // Builtins continuam intactos (CA-07)
    let claude = reg.get(AgentKind::Claude);
    assert_eq!(claude.binary, "claude");
    assert_eq!(claude.shortcut, "Ctrl+Alt+C");
}

#[test]
fn shortcut_normalization_and_shift_collision_validation() {
    assert_eq!(
        normalize_and_validate_shortcut("alt+ctrl+c").unwrap(),
        "Ctrl+Alt+C"
    );
    assert_eq!(
        shift_variant_of("Ctrl+Alt+C"),
        Some("Ctrl+Alt+Shift+C".to_string())
    );
    assert!(normalize_and_validate_shortcut("C").is_err());
    assert!(normalize_and_validate_shortcut("Ctrl+Alt+F12").is_err());

    let reg = AgentRegistry::with_builtins();
    let reserved = default_reserved_shortcuts();

    // Atribuir a Codex a variante +Shift de Claude ("Ctrl+Alt+Shift+C") deve falhar com shortcut_conflict (CA-11)
    let mut bad_settings = AgentsSettings::default();
    bad_settings
        .shortcuts
        .insert(AgentKind::Codex, "Ctrl+Alt+Shift+C".to_string());

    let err = validate_settings(&bad_settings, &reg, &reserved).unwrap_err();
    assert_eq!(err.code.0, codes::SHORTCUT_CONFLICT);

    // Colisão com atalho reservado do sistema ("Ctrl+Shift+C") deve falhar
    let mut bad_sys = AgentsSettings::default();
    bad_sys
        .shortcuts
        .insert(AgentKind::Codex, "Ctrl+Shift+C".to_string());
    let err_sys = validate_settings(&bad_sys, &reg, &reserved).unwrap_err();
    assert_eq!(err_sys.code.0, codes::SHORTCUT_CONFLICT);

    // Configuração válida
    let mut ok_settings = AgentsSettings::default();
    ok_settings
        .shortcuts
        .insert(AgentKind::Codex, "Ctrl+Alt+M".to_string());
    let validated = validate_settings(&ok_settings, &reg, &HashSet::new()).unwrap();
    assert_eq!(
        validated
            .shortcuts
            .get(&AgentKind::Codex)
            .map(String::as_str),
        Some("Ctrl+Alt+M")
    );
}
