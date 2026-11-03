//! Resolução determinística do namespace PSR-4 para `app/Console/Commands` a partir do `composer.json`.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use crate::fs_util::read_capped;

const MAX_COMPOSER_BYTES: usize = 2 * 1024 * 1024;
const TARGET_REL_DIR: &str = "app/Console/Commands/";

/// Resultado da resolução PSR-4 para `app/Console/Commands`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Psr4Resolution {
    /// Namespace resolvido com sucesso (ex.: `"App\\Console\\Commands"`).
    Resolved(String),
    /// Mapeamento PSR-4 ausente ou ambíguo para `app/`.
    Unresolved(String),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Psr4Paths {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Deserialize)]
struct ComposerAutoload {
    #[serde(default, rename = "psr-4")]
    psr4: BTreeMap<String, Psr4Paths>,
}

#[derive(Deserialize)]
struct ComposerFile {
    #[serde(default)]
    autoload: Option<ComposerAutoload>,
}

/// Resolve o namespace PSR-4 correspondente a `app/Console/Commands/` lendo `composer.json` na raiz.
#[must_use]
pub fn resolve_commands_namespace(root: &Path) -> Psr4Resolution {
    let composer_path = root.join("composer.json");
    let Ok(Some(content)) = read_capped(&composer_path, MAX_COMPOSER_BYTES) else {
        return Psr4Resolution::Unresolved(
            "Não foi possível ler composer.json para resolver o namespace PSR-4.".to_string(),
        );
    };

    let Ok(parsed) = serde_json::from_str::<ComposerFile>(&content) else {
        return Psr4Resolution::Unresolved(
            "O arquivo composer.json contém JSON inválido.".to_string(),
        );
    };

    let Some(autoload) = parsed.autoload else {
        return Psr4Resolution::Unresolved(
            "A seção autoload.psr-4 não foi encontrada no composer.json.".to_string(),
        );
    };

    let mut best_len: Option<usize> = None;
    let mut best_namespaces: Vec<String> = Vec::new();

    for (ns_prefix, paths) in autoload.psr4 {
        let dir_list = match paths {
            Psr4Paths::Single(s) => vec![s],
            Psr4Paths::Multiple(v) => v,
        };
        for raw_dir in dir_list {
            let norm = normalize_psr4_dir(&raw_dir);
            if TARGET_REL_DIR.starts_with(&norm) {
                let len = norm.len();
                let suffix = &TARGET_REL_DIR[len..];
                let ns = build_namespace(&ns_prefix, suffix);
                match best_len {
                    None => {
                        best_len = Some(len);
                        best_namespaces.push(ns);
                    }
                    Some(cur) if len > cur => {
                        best_len = Some(len);
                        best_namespaces.clear();
                        best_namespaces.push(ns);
                    }
                    Some(cur) if len == cur => {
                        if !best_namespaces.contains(&ns) {
                            best_namespaces.push(ns);
                        }
                    }
                    Some(_) => {}
                }
            }
        }
    }

    match best_namespaces.len() {
        1 => Psr4Resolution::Resolved(best_namespaces.remove(0)),
        0 => Psr4Resolution::Unresolved(
            "Nenhum mapeamento autoload.psr-4 no composer.json cobre o diretório app/Console/Commands/."
                .to_string(),
        ),
        _ => Psr4Resolution::Unresolved(format!(
            "Mapeamento PSR-4 ambíguo para app/Console/Commands/: {}.",
            best_namespaces.join(", ")
        )),
    }
}

fn normalize_psr4_dir(raw: &str) -> String {
    let trimmed = raw.trim().replace('\\', "/");
    let without_dot = trimmed.strip_prefix("./").unwrap_or(&trimmed);
    let clean = without_dot.trim_matches('/');
    if clean.is_empty() {
        String::new()
    } else {
        format!("{clean}/")
    }
}

fn build_namespace(prefix: &str, rest_dir: &str) -> String {
    let mut parts: Vec<&str> = prefix
        .trim()
        .trim_matches('\\')
        .split('\\')
        .filter(|s| !s.is_empty())
        .collect();
    for seg in rest_dir.split('/').filter(|s| !s.is_empty()) {
        parts.push(seg);
    }
    parts.join("\\")
}
