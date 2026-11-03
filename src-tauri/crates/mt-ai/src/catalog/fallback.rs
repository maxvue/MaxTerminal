//! Carregamento do catálogo fallback estático contendo apenas as identidades visuais
//! dos 4 harnesses canônicos (`claude-code`, `codex`, `agy`, `opencode`) com `models: []`.

use crate::catalog::Harness;
use serde::Deserialize;

const FALLBACK_JSON: &str = include_str!("../../assets/catalog.fallback.json");

#[derive(Deserialize)]
struct FallbackFile {
    harnesses: Vec<Harness>,
}

/// Retorna a lista dos 4 harnesses canônicos com `models: []` embutida no binário.
#[must_use]
pub fn canonical_fallback_harnesses() -> Vec<Harness> {
    serde_json::from_str::<FallbackFile>(FALLBACK_JSON).map_or_else(
        |_| {
            vec![
                Harness {
                    id: "claude-code".to_string(),
                    label: "Claude Code".to_string(),
                    icon: "simple-icons:claude".to_string(),
                    models: Vec::new(),
                },
                Harness {
                    id: "codex".to_string(),
                    label: "Codex".to_string(),
                    icon: "simple-icons:openai".to_string(),
                    models: Vec::new(),
                },
                Harness {
                    id: "agy".to_string(),
                    label: "Antigravity (agy)".to_string(),
                    icon: "simple-icons:google".to_string(),
                    models: Vec::new(),
                },
                Harness {
                    id: "opencode".to_string(),
                    label: "OpenCode".to_string(),
                    icon: "mdi:code-braces".to_string(),
                    models: Vec::new(),
                },
            ]
        },
        |f| f.harnesses,
    )
}
