//! Testes do `ShortcutService` para Laravel+Vue (CA-26), Tauri+pnpm (CA-27),
//! script `test` padrão do `npm init` (CA-28), conflito de lockfiles e `packageManager` (CA-29)
//! e override `config-trabalho.json` do `MaxCode` (CA-30).

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
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mt_commands::ports::{MaxCodeConfigLocator, ProjectLookup};
use mt_commands::shortcuts::model::ShortcutId;
use mt_commands::shortcuts::service::ShortcutService;
use mt_core::ProjectId;
use tempfile::TempDir;

struct SimpleLookup {
    id: ProjectId,
    root: PathBuf,
}

impl ProjectLookup for SimpleLookup {
    fn owning_project(&self, path: &Path) -> Option<(ProjectId, PathBuf)> {
        let canon = fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());
        path.starts_with(&canon).then_some((self.id, canon))
    }

    fn project_by_id(&self, id: ProjectId) -> Option<PathBuf> {
        (id == self.id).then(|| self.root.clone())
    }
}

struct FixedConfigLocator(Vec<PathBuf>);
impl MaxCodeConfigLocator for FixedConfigLocator {
    fn candidate_files(&self) -> Vec<PathBuf> {
        self.0.clone()
    }
}

fn make_service(project_root: &Path, config_files: Vec<PathBuf>) -> ShortcutService {
    let lookup = Arc::new(SimpleLookup {
        id: ProjectId::new(),
        root: project_root.to_path_buf(),
    });
    let locator = Arc::new(FixedConfigLocator(config_files));
    ShortcutService::new(lookup, locator, None)
}

#[test]
fn laravel_vue_npm_composes_multi_manager_shortcuts_ca26() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(tmp.path().join("artisan"), "#!/usr/bin/env php\n").expect("artisan");
    fs::write(
        tmp.path().join("composer.json"),
        r#"{ "require": { "laravel/framework": "^11.0" } }"#,
    )
    .expect("composer");
    fs::write(
        tmp.path().join("package.json"),
        r#"{ "scripts": { "build": "vite build" } }"#,
    )
    .expect("package.json");
    fs::write(tmp.path().join("package-lock.json"), "{}").expect("lock");

    let svc = make_service(tmp.path(), Vec::new());
    let list = svc.list(tmp.path()).expect("list");

    let by_id = |id: ShortcutId| {
        list.items
            .iter()
            .find(|i| i.id == id)
            .expect("shortcut")
            .clone()
    };

    assert_eq!(
        by_id(ShortcutId::Install).line.as_deref(),
        Some("composer install && npm install")
    );
    assert_eq!(
        by_id(ShortcutId::UpdateForce).line.as_deref(),
        Some("composer update --no-cache && npm update --force")
    );
    assert!(by_id(ShortcutId::UpdateForce).requires_confirmation);
    assert_eq!(
        by_id(ShortcutId::Build).line.as_deref(),
        Some("npm run build")
    );
    assert_eq!(
        by_id(ShortcutId::Test).line.as_deref(),
        Some("php artisan test")
    );
    assert!(by_id(ShortcutId::Deploy).line.is_none());
}

#[test]
fn tauri_pnpm_resolves_manifest_path_for_cargo_ca27() {
    let tmp = TempDir::new().expect("tempdir");
    fs::create_dir_all(tmp.path().join("src-tauri")).expect("mkdir");
    fs::write(
        tmp.path().join("package.json"),
        r#"{ "packageManager": "pnpm@9.12.0", "scripts": { "test": "vitest run", "build": "vite build" } }"#,
    )
    .expect("package.json");
    fs::write(
        tmp.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .expect("pnpm-lock");
    fs::write(
        tmp.path().join("src-tauri/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.1.0\"\n",
    )
    .expect("Cargo.toml");

    let svc = make_service(tmp.path(), Vec::new());
    let install = svc
        .resolve(tmp.path(), ShortcutId::Install)
        .expect("install");
    assert_eq!(
        install.line.as_deref(),
        Some("pnpm install && cargo build --manifest-path src-tauri/Cargo.toml")
    );

    let test = svc.resolve(tmp.path(), ShortcutId::Test).expect("test");
    assert_eq!(
        test.line.as_deref(),
        Some("pnpm run test && cargo test --manifest-path src-tauri/Cargo.toml")
    );
}

#[test]
fn npm_default_test_script_is_ignored_ca28() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(
        tmp.path().join("package.json"),
        r#"{ "scripts": { "test": "echo \"Error: no test specified\" && exit 1" } }"#,
    )
    .expect("package.json");

    let svc = make_service(tmp.path(), Vec::new());
    let list = svc.list(tmp.path()).expect("list");
    let test_item = list
        .items
        .iter()
        .find(|i| i.id == ShortcutId::Test)
        .expect("test");
    assert!(test_item.line.is_none());
    assert_eq!(
        test_item.disabled_reason.as_deref(),
        Some("Nenhum comando de teste encontrado")
    );
}

#[test]
fn conflicting_lockfiles_without_package_manager_disable_js_shortcuts_ca29() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(
        tmp.path().join("package.json"),
        r#"{ "scripts": { "build": "vite build" } }"#,
    )
    .expect("package.json");
    fs::write(tmp.path().join("package-lock.json"), "{}").expect("npm lock");
    fs::write(
        tmp.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '9.0'\n",
    )
    .expect("pnpm lock");

    let svc = make_service(tmp.path(), Vec::new());
    let list = svc.list(tmp.path()).expect("list");
    assert!(!list.warnings.is_empty());
    let install = list
        .items
        .iter()
        .find(|i| i.id == ShortcutId::Install)
        .expect("install");
    assert!(
        install.line.is_none(),
        "Conflito sem packageManager deve desabilitar /install"
    );

    // Declarar packageManager resolve o conflito mantendo aviso
    fs::write(
        tmp.path().join("package.json"),
        r#"{ "packageManager": "pnpm@9.0.0", "scripts": { "build": "vite build" } }"#,
    )
    .expect("package.json with pm");
    let list2 = svc.list(tmp.path()).expect("list2");
    let install2 = list2
        .items
        .iter()
        .find(|i| i.id == ShortcutId::Install)
        .expect("install2");
    assert_eq!(install2.line.as_deref(), Some("pnpm install"));
}

#[test]
fn maxcode_config_overrides_build_and_deploy_per_project_ca30() {
    let tmp = TempDir::new().expect("tempdir");
    fs::write(
        tmp.path().join("package.json"),
        r#"{ "scripts": { "build": "npm-default-build" } }"#,
    )
    .expect("package.json");

    let canon_root = fs::canonicalize(tmp.path()).expect("canon");
    let cfg_path = tmp.path().join("config-trabalho.json");
    let cfg_json = serde_json::json!({
        "versao": 1,
        "geral": {
            "comandoBuild": "make build-global",
            "comandoDeploy": "make deploy-global"
        },
        "projetos": {
            canon_root.display().to_string(): {
                "comandoBuild": "./scripts/build-prod.sh"
            }
        }
    });
    fs::write(
        &cfg_path,
        serde_json::to_string_pretty(&cfg_json).expect("json"),
    )
    .expect("write cfg");

    let svc = make_service(tmp.path(), vec![cfg_path]);
    let build = svc.resolve(tmp.path(), ShortcutId::Build).expect("build");
    assert_eq!(build.line.as_deref(), Some("./scripts/build-prod.sh"));

    let deploy = svc.resolve(tmp.path(), ShortcutId::Deploy).expect("deploy");
    assert_eq!(deploy.line.as_deref(), Some("make deploy-global"));
    assert!(deploy.requires_confirmation);
}
