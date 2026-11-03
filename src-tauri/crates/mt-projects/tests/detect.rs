//! Testes unitários e de integração de `detect.rs`, `identity.rs` e `paths.rs` (`mt-projects`).

#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::symlink;

use mt_core::error::codes;
use mt_projects::{
    FALLBACK_COLORS, FALLBACK_ICONS, FolderProbe, IdentityMap, StackDetector, StackKind,
    default_title, expand_home, is_hidden, is_inside_ignored_discovery_dir, validate_color,
    validate_icon, validate_project_dir, validate_title,
};
use tempfile::TempDir;

#[test]
fn stack_priority_and_combinations() {
    let detector = StackDetector::default_rules();

    // 1. Tauri + Vue + Rust -> [Tauri, Vue, Rust], primária = Tauri
    let tauri_dir = TempDir::new().unwrap();
    fs::create_dir_all(tauri_dir.path().join("src-tauri")).unwrap();
    fs::write(
        tauri_dir.path().join("src-tauri/tauri.conf.json"),
        r#"{"productName":"MaxTerminal"}"#,
    )
    .unwrap();
    fs::write(
        tauri_dir.path().join("Cargo.toml"),
        "[package]\nname = \"app\"\n",
    )
    .unwrap();
    fs::write(
        tauri_dir.path().join("package.json"),
        r#"{"dependencies":{"vue":"^3.6.0"}}"#,
    )
    .unwrap();

    let rep_tauri = detector.detect(tauri_dir.path());
    assert!(rep_tauri.complete);
    assert_eq!(
        rep_tauri.detected,
        vec![StackKind::Tauri, StackKind::Vue, StackKind::Rust]
    );
    let id_tauri = IdentityMap::resolve(tauri_dir.path(), rep_tauri.detected);
    assert_eq!(id_tauri.stack, StackKind::Tauri);
    assert_eq!(id_tauri.icon, "logos:tauri");
    assert_eq!(id_tauri.color, "#FFC131");

    // 2. Extensão VS Code + Vue -> [VscodeExtension, Vue], primária = VscodeExtension
    let vsc_dir = TempDir::new().unwrap();
    fs::write(
        vsc_dir.path().join("package.json"),
        r#"{"engines":{"vscode":"^1.90.0"},"devDependencies":{"vue":"^3.5.0"}}"#,
    )
    .unwrap();
    let rep_vsc = detector.detect(vsc_dir.path());
    assert_eq!(
        rep_vsc.detected,
        vec![StackKind::VscodeExtension, StackKind::Vue]
    );
    let id_vsc = IdentityMap::resolve(vsc_dir.path(), rep_vsc.detected);
    assert_eq!(id_vsc.stack, StackKind::VscodeExtension);
    assert_eq!(id_vsc.icon, "logos:visual-studio-code");
    assert_eq!(id_vsc.color, "#007ACC");

    // 3. Laravel + Vue -> [Laravel, Vue], primária = Laravel
    let laravel_dir = TempDir::new().unwrap();
    fs::write(laravel_dir.path().join("artisan"), "#!/usr/bin/env php\n").unwrap();
    fs::write(
        laravel_dir.path().join("composer.json"),
        r#"{"name":"laravel/laravel"}"#,
    )
    .unwrap();
    fs::write(
        laravel_dir.path().join("package.json"),
        r#"{"peerDependencies":{"vue":"^3.0.0"}}"#,
    )
    .unwrap();
    let rep_lar = detector.detect(laravel_dir.path());
    assert_eq!(rep_lar.detected, vec![StackKind::Laravel, StackKind::Vue]);
    let id_lar = IdentityMap::resolve(laravel_dir.path(), rep_lar.detected);
    assert_eq!(id_lar.stack, StackKind::Laravel);
    assert_eq!(id_lar.icon, "logos:laravel");
    assert_eq!(id_lar.color, "#FF2D20");

    // 4. artisan sozinho sem composer.json -> [Unknown]
    let only_artisan = TempDir::new().unwrap();
    fs::write(only_artisan.path().join("artisan"), "#!/usr/bin/env php\n").unwrap();
    let rep_only_art = detector.detect(only_artisan.path());
    assert_eq!(rep_only_art.detected, vec![StackKind::Unknown]);

    // 5. Python (pyproject.toml ou requirements.txt)
    let py_dir = TempDir::new().unwrap();
    fs::write(py_dir.path().join("requirements.txt"), "fastapi==0.110.0\n").unwrap();
    let rep_py = detector.detect(py_dir.path());
    assert_eq!(rep_py.detected, vec![StackKind::Python]);
    let id_py = IdentityMap::resolve(py_dir.path(), rep_py.detected);
    assert_eq!(id_py.icon, "logos:python");
    assert_eq!(id_py.color, "#3776AB");
}

#[test]
fn package_json_over_1mib_and_invalid_json_do_not_panic() {
    let detector = StackDetector::default_rules();

    let huge_dir = TempDir::new().unwrap();
    let huge_content = format!(
        r#"{{"name":"{}","dependencies":{{"vue":"3"}}}}"#,
        "a".repeat(1024 * 1024 + 128)
    );
    fs::write(huge_dir.path().join("package.json"), huge_content).unwrap();

    let probe = FolderProbe::read(huge_dir.path()).unwrap();
    assert!(probe.package_json.is_none());
    assert_eq!(probe.warnings.len(), 1);

    let rep = detector.detect(huge_dir.path());
    assert_eq!(rep.detected, vec![StackKind::Unknown]);

    // package.json inválido + Cargo.toml válido -> detecta Rust normalmente
    let broken_dir = TempDir::new().unwrap();
    fs::write(broken_dir.path().join("package.json"), "{ invalid json ").unwrap();
    fs::write(broken_dir.path().join("Cargo.toml"), "[package]\n").unwrap();
    let rep_rust = detector.detect(broken_dir.path());
    assert_eq!(rep_rust.detected, vec![StackKind::Rust]);

    // Symlink como package.json deve ser ignorado com segurança
    let sym_dir = TempDir::new().unwrap();
    let real_pkg = broken_dir.path().join("Cargo.toml");
    symlink(&real_pkg, sym_dir.path().join("package.json")).unwrap();
    let sym_probe = FolderProbe::read(sym_dir.path()).unwrap();
    assert!(sym_probe.package_json.is_none());
}

#[test]
fn deterministic_fallback_and_validators() {
    let tmp = TempDir::new().unwrap();
    let first = IdentityMap::fallback(tmp.path());
    assert!(FALLBACK_ICONS.contains(&first.0));
    assert!(FALLBACK_COLORS.contains(&first.1));

    for _ in 0..100 {
        assert_eq!(IdentityMap::fallback(tmp.path()), first);
    }

    assert!(validate_icon("logos:laravel").is_ok());
    assert!(validate_icon("mdi:folder-code").is_ok());
    assert_eq!(
        validate_icon("Invalid:Icon").unwrap_err().code,
        codes::PROJECT_INVALID_ICON
    );
    assert_eq!(
        validate_icon("sem-dois-pontos").unwrap_err().code,
        codes::PROJECT_INVALID_ICON
    );

    assert_eq!(validate_color("#42b883").unwrap(), "#42B883");
    assert_eq!(
        validate_color("42B883").unwrap_err().code,
        codes::PROJECT_INVALID_COLOR
    );
    assert_eq!(
        validate_color("#12345").unwrap_err().code,
        codes::PROJECT_INVALID_COLOR
    );

    assert_eq!(validate_title("  Meu Projeto  ").unwrap(), "Meu Projeto");
    assert_eq!(
        validate_title("   ").unwrap_err().code,
        codes::PROJECT_INVALID_TITLE
    );
    assert_eq!(
        validate_title("Linha1\nLinha2").unwrap_err().code,
        codes::PROJECT_INVALID_TITLE
    );
    assert_eq!(
        validate_title(&"x".repeat(121)).unwrap_err().code,
        codes::PROJECT_INVALID_TITLE
    );

    // Título contendo segredo deve ser sanitizado
    let sanitized_title = validate_title("App TOKEN=super_secret_token_12345").unwrap();
    assert!(sanitized_title.contains("[REDACTED:"));
    assert!(!sanitized_title.contains("super_secret_token_12345"));
}

#[test]
fn path_validation_and_filters() {
    assert_eq!(
        validate_project_dir("/").unwrap_err().code,
        codes::PROJECT_PATH_FORBIDDEN
    );
    assert_eq!(
        validate_project_dir("~").unwrap_err().code,
        codes::PROJECT_PATH_FORBIDDEN
    );
    assert_eq!(
        expand_home("caminho/relativo").unwrap_err().code,
        codes::PROJECT_INVALID_ARGUMENT
    );
    assert_eq!(
        expand_home("~outro_usuario/proj").unwrap_err().code,
        codes::PROJECT_INVALID_ARGUMENT
    );

    assert!(is_hidden(std::ffi::OsStr::new(".git")));
    assert!(is_hidden(std::ffi::OsStr::new(".worktrees")));
    assert!(!is_hidden(std::ffi::OsStr::new("maxterminal")));

    assert!(is_inside_ignored_discovery_dir(std::path::Path::new(
        "/home/user/repo/.max-code-worktrees/wt-123"
    )));
    assert!(!is_inside_ignored_discovery_dir(std::path::Path::new(
        "/home/user/repo/packages/ui"
    )));
    assert_eq!(
        default_title(std::path::Path::new("/home/user/GitHub/MaxTerminal")),
        "MaxTerminal"
    );
}
