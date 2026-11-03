//! Parser do `config.yaml` completo do Tabby Terminal (`terminal.colorScheme` e `terminal.customColorSchemes`).

use serde::Deserialize;

use crate::error::ThemeError;
use crate::formats::tabby_scheme::TabbyYamlSchemeEntry;
use crate::formats::{RawColorScheme, validate_safe_yaml};

#[derive(Debug, Deserialize)]
struct TabbyConfigRoot {
    terminal: Option<TabbyTerminalSection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TabbyTerminalSection {
    color_scheme: Option<TabbyYamlSchemeEntry>,
    #[serde(default)]
    custom_color_schemes: Vec<TabbyYamlSchemeEntry>,
}

/// Extrai os esquemas de cores presentes em um `config.yaml` completo do Tabby Terminal,
/// ignorando com segurança perfis SSH, hotkeys, senhas e plugins de terceiros.
pub fn parse_tabby_config_yaml(content: &str) -> Result<Vec<RawColorScheme>, ThemeError> {
    let cleaned = content.trim_start_matches('\u{feff}');
    validate_safe_yaml(cleaned)?;

    let root: TabbyConfigRoot =
        serde_yaml_ng::from_str(cleaned).map_err(|e| ThemeError::UnsupportedFormat {
            reason: format!("Falha ao interpretar config.yaml do Tabby: {e}"),
        })?;

    let Some(term) = root.terminal else {
        return Err(ThemeError::NoSchemesFound);
    };

    let mut out = Vec::new();
    if let Some(active) = term.color_scheme {
        out.push(active.into_raw());
    }
    for custom in term.custom_color_schemes {
        out.push(custom.into_raw());
    }

    if out.is_empty() {
        return Err(ThemeError::NoSchemesFound);
    }

    Ok(out)
}
