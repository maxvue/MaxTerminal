//! Testes de integração de capabilities mínimas, negação de plugins JS diretos e validação de origem/janela (`CA-31`, `CA-34`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use maxterminal_lib::security::validate_window_and_origin;
use serde_json::Value;

fn read_json(rel_path: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(rel_path);
    let raw = fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Falha ao ler {}: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("JSON inválido em {}: {e}", path.display()))
}

#[test]
fn effective_capabilities_preserve_os_integration_and_deny_direct_js_plugins() {
    let default_cap = read_json("capabilities/default.json");
    let os_cap = read_json("capabilities/os.json");
    let main_cap = read_json("capabilities/main.json");

    for cap in [&default_cap, &os_cap, &main_cap] {
        let windows = cap["windows"]
            .as_array()
            .expect("campo windows deve ser array")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        assert_eq!(windows, vec!["main"]);
        assert!(cap.get("remote").is_none());
        assert!(cap.get("urls").is_none());
    }

    let mut effective_permissions = HashSet::new();
    for cap in [&default_cap, &os_cap, &main_cap] {
        for perm in cap["permissions"]
            .as_array()
            .expect("permissions deve ser array")
            .iter()
            .filter_map(Value::as_str)
        {
            effective_permissions.insert(perm.to_owned());
        }
    }

    // Preserva os comandos Rust autorizados da Fase 09 (os_get_autostart / os_set_autostart / allow-os-integration)
    assert!(effective_permissions.contains("allow-os-integration"));
    assert!(effective_permissions.contains("allow-os-get-autostart"));
    assert!(effective_permissions.contains("allow-os-set-autostart"));
    assert!(effective_permissions.contains("allow-search-query"));
    assert!(effective_permissions.contains("allow-backup-create"));

    // Nega chamadas JS diretas de plugins sensíveis (autostart JS, opener JS, deep-link JS, global-shortcut JS, shell exec JS)
    for forbidden_js_perm in [
        "core:default",
        "autostart:default",
        "autostart:allow-enable",
        "autostart:allow-disable",
        "autostart:allow-is-enabled",
        "opener:default",
        "opener:allow-open-url",
        "opener:allow-open-path",
        "deep-link:default",
        "global-shortcut:default",
        "shell:allow-execute",
        "shell:allow-spawn",
    ] {
        assert!(
            !effective_permissions.contains(forbidden_js_perm),
            "Permissão JS direta proibida encontrada nas capabilities: {forbidden_js_perm}"
        );
    }
}

#[test]
fn validate_window_and_origin_enforces_main_window_and_local_origin() {
    let ok_tauri = validate_window_and_origin("main", "tauri://localhost/index.html")
        .expect("janela main com origem tauri://localhost deve ser autorizada");
    assert_eq!(ok_tauri.window_label, "main");

    let ok_dev = validate_window_and_origin("main", "http://127.0.0.1:5177/")
        .expect("janela main com origem local dev deve ser autorizada");
    assert_eq!(ok_dev.window_label, "main");

    let err_win = validate_window_and_origin("secondary", "tauri://localhost/index.html")
        .expect_err("janela secondary deve ser recusada");
    assert_eq!(err_win.code.0, "security.unauthorized_window");

    let err_origin = validate_window_and_origin("main", "https://evil.example.com/pwn")
        .expect_err("origem externa remota deve ser recusada");
    assert_eq!(err_origin.code.0, "security.unauthorized_origin");

    let err_invalid = validate_window_and_origin("main", ":::not_a_valid_url")
        .expect_err("URL malformada deve ser recusada");
    assert_eq!(err_invalid.code.0, "security.invalid_origin");
}
