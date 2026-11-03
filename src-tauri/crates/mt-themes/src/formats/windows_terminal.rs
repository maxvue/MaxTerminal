//! Parser do formato JSON do Windows Terminal / iTerm2-Color-Schemes.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::ThemeError;
use crate::formats::RawColorScheme;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WtSchemeEntry {
    name: Option<String>,
    author: Option<String>,
    foreground: Option<String>,
    background: Option<String>,
    #[serde(alias = "cursor")]
    cursor_color: Option<String>,
    cursor_accent: Option<String>,
    #[serde(alias = "selection")]
    selection_background: Option<String>,
    selection_foreground: Option<String>,
    black: Option<String>,
    red: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    blue: Option<String>,
    #[serde(alias = "magenta")]
    purple: Option<String>,
    cyan: Option<String>,
    white: Option<String>,
    bright_black: Option<String>,
    bright_red: Option<String>,
    bright_green: Option<String>,
    bright_yellow: Option<String>,
    bright_blue: Option<String>,
    #[serde(alias = "brightMagenta")]
    bright_purple: Option<String>,
    bright_cyan: Option<String>,
    bright_white: Option<String>,
}

impl WtSchemeEntry {
    fn into_raw(self) -> Result<RawColorScheme, ThemeError> {
        let req = |opt: Option<String>, field: &str| -> Result<String, ThemeError> {
            opt.ok_or_else(|| ThemeError::InvalidField {
                field: field.to_string(),
                reason: "Cor ANSI obrigatória ausente no JSON Windows Terminal".to_string(),
            })
        };

        let mut colors = vec![
            req(self.black, "black")?,
            req(self.red, "red")?,
            req(self.green, "green")?,
            req(self.yellow, "yellow")?,
            req(self.blue, "blue")?,
            req(self.purple, "purple/magenta")?,
            req(self.cyan, "cyan")?,
            req(self.white, "white")?,
        ];

        if let (Some(b0), Some(b1), Some(b2), Some(b3), Some(b4), Some(b5), Some(b6), Some(b7)) = (
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_purple,
            self.bright_cyan,
            self.bright_white,
        ) {
            colors.extend([b0, b1, b2, b3, b4, b5, b6, b7]);
        }

        Ok(RawColorScheme {
            name: self.name.unwrap_or_default(),
            author: self.author,
            foreground: self.foreground.unwrap_or_default(),
            background: self.background.unwrap_or_default(),
            cursor: self.cursor_color,
            cursor_accent: self.cursor_accent,
            cursor_style: None,
            selection_background: self.selection_background,
            selection_foreground: self.selection_foreground,
            active_tab_accent: None,
            colors,
            indexed_colors: BTreeMap::new(),
            source_format: None,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum WtContainer {
    Wrapper { schemes: Vec<WtSchemeEntry> },
    List(Vec<WtSchemeEntry>),
    Single(Box<WtSchemeEntry>),
}

/// Faz o parsing de um arquivo JSON no formato Windows Terminal (objeto único, lista ou `{ "schemes": [...] }`).
pub fn parse_windows_terminal_json(content: &str) -> Result<Vec<RawColorScheme>, ThemeError> {
    let cleaned = content.trim_start_matches('\u{feff}');
    let container: WtContainer =
        serde_json::from_str(cleaned).map_err(|e| ThemeError::UnsupportedFormat {
            reason: format!("JSON Windows Terminal inválido: {e}"),
        })?;

    let entries = match container {
        WtContainer::Wrapper { schemes } | WtContainer::List(schemes) => schemes,
        WtContainer::Single(single) => vec![*single],
    };

    entries.into_iter().map(WtSchemeEntry::into_raw).collect()
}
