//! Leitura segura e limitada (≤ 2 MiB) de `package.json`, `composer.json` e `Cargo.toml`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::fs_util::read_capped;

const MAX_MANIFEST_BYTES: usize = 2 * 1024 * 1024;
const NPM_DEFAULT_TEST_SCRIPT: &str = "echo \"Error: no test specified\" && exit 1";

/// Dados relevantes extraídos de `package.json`.
#[derive(Debug, Clone, Default)]
pub struct PackageJsonInfo {
    /// Indica se o arquivo `package.json` existe na raiz.
    pub exists: bool,
    /// Indica se o JSON de `package.json` é válido.
    pub valid_json: bool,
    /// Valor bruto do campo `"packageManager"`, quando presente.
    pub package_manager_raw: Option<String>,
    /// Mapa de scripts não vazios declarados em `"scripts"`.
    pub scripts: BTreeMap<String, String>,
}

impl PackageJsonInfo {
    /// Verifica se existe um script `name` não vazio (ignorando o script `test` padrão do `npm init` — CA-28).
    #[must_use]
    pub fn has_usable_script(&self, name: &str) -> bool {
        let Some(body) = self.scripts.get(name) else {
            return false;
        };
        let trimmed = body.trim();
        if trimmed.is_empty() {
            return false;
        }
        if name == "test" && trimmed == NPM_DEFAULT_TEST_SCRIPT {
            return false;
        }
        true
    }
}

/// Dados relevantes extraídos de `composer.json`.
#[derive(Debug, Clone, Default)]
pub struct ComposerJsonInfo {
    /// Indica se o arquivo `composer.json` existe na raiz.
    pub exists: bool,
    /// Indica se o JSON de `composer.json` é válido.
    pub valid_json: bool,
    /// Indica se `laravel/framework` está presente em `require` ou `require-dev`.
    pub is_laravel: bool,
    /// Conjunto de scripts declarados em `"scripts"`.
    pub scripts: BTreeMap<String, bool>,
}

impl ComposerJsonInfo {
    /// Verifica se existe um script Composer não vazio com o nome `name`.
    #[must_use]
    pub fn has_usable_script(&self, name: &str) -> bool {
        self.scripts.get(name).copied().unwrap_or(false)
    }
}

/// Lê e interpreta `package.json` em `root`.
#[must_use]
pub fn read_package_json(root: &Path) -> PackageJsonInfo {
    let path = root.join("package.json");
    let Ok(Some(raw)) = read_capped(&path, MAX_MANIFEST_BYTES) else {
        let exists = path.symlink_metadata().is_ok();
        return PackageJsonInfo {
            exists,
            valid_json: false,
            ..Default::default()
        };
    };

    #[derive(Deserialize)]
    struct RawPkg {
        #[serde(default, rename = "packageManager")]
        package_manager: Option<serde_json::Value>,
        #[serde(default)]
        scripts: BTreeMap<String, serde_json::Value>,
    }

    let Ok(parsed) = serde_json::from_str::<RawPkg>(&raw) else {
        return PackageJsonInfo {
            exists: true,
            valid_json: false,
            ..Default::default()
        };
    };

    let package_manager_raw = match parsed.package_manager {
        Some(serde_json::Value::String(s)) => Some(s),
        Some(other) => Some(other.to_string()),
        None => None,
    };

    let mut scripts = BTreeMap::new();
    for (k, v) in parsed.scripts {
        if let serde_json::Value::String(s) = v {
            scripts.insert(k, s);
        }
    }

    PackageJsonInfo {
        exists: true,
        valid_json: true,
        package_manager_raw,
        scripts,
    }
}

/// Lê e interpreta `composer.json` em `root`.
#[must_use]
pub fn read_composer_json(root: &Path) -> ComposerJsonInfo {
    let path = root.join("composer.json");
    let Ok(Some(raw)) = read_capped(&path, MAX_MANIFEST_BYTES) else {
        let exists = path.symlink_metadata().is_ok();
        return ComposerJsonInfo {
            exists,
            valid_json: false,
            ..Default::default()
        };
    };

    #[derive(Deserialize)]
    struct RawComposer {
        #[serde(default)]
        require: BTreeMap<String, serde_json::Value>,
        #[serde(default, rename = "require-dev")]
        require_dev: BTreeMap<String, serde_json::Value>,
        #[serde(default)]
        scripts: BTreeMap<String, serde_json::Value>,
    }

    let Ok(parsed) = serde_json::from_str::<RawComposer>(&raw) else {
        return ComposerJsonInfo {
            exists: true,
            valid_json: false,
            ..Default::default()
        };
    };

    let is_laravel = (parsed.require.contains_key("laravel/framework")
        || parsed.require_dev.contains_key("laravel/framework"))
        && !parsed.require.contains_key("laravel/lumen-framework");

    let mut scripts = BTreeMap::new();
    for (k, v) in parsed.scripts {
        let non_empty = match v {
            serde_json::Value::String(s) => !s.trim().is_empty(),
            serde_json::Value::Array(arr) => arr
                .iter()
                .any(|item| item.as_str().is_some_and(|s| !s.trim().is_empty())),
            _ => false,
        };
        scripts.insert(k, non_empty);
    }

    ComposerJsonInfo {
        exists: true,
        valid_json: true,
        is_laravel,
        scripts,
    }
}
