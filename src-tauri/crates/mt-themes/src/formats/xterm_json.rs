//! Parser do formato JSON `ITheme` do `@xterm/xterm`.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::ThemeError;
use crate::formats::RawColorScheme;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct XtermJsonEntry {
    name: Option<String>,
    author: Option<String>,
    foreground: Option<String>,
    background: Option<String>,
    cursor: Option<String>,
    cursor_accent: Option<String>,
    selection_background: Option<String>,
    selection_foreground: Option<String>,
    black: Option<String>,
    red: Option<String>,
    green: Option<String>,
    yellow: Option<String>,
    blue: Option<String>,
    magenta: Option<String>,
    cyan: Option<String>,
    white: Option<String>,
    bright_black: Option<String>,
    bright_red: Option<String>,
    bright_green: Option<String>,
    bright_yellow: Option<String>,
    bright_blue: Option<String>,
    bright_magenta: Option<String>,
    bright_cyan: Option<String>,
    bright_white: Option<String>,
    #[serde(default)]
    extended_ansi: Option<BTreeMap<u8, String>>,
}

impl XtermJsonEntry {
    fn into_raw(self) -> Result<RawColorScheme, ThemeError> {
        let req = |opt: Option<String>, field: &str| -> Result<String, ThemeError> {
            opt.ok_or_else(|| ThemeError::InvalidField {
                field: field.to_string(),
                reason: "Cor ANSI obrigatória ausente no Xterm JSON".to_string(),
            })
        };

        let mut colors = vec![
            req(self.black, "black")?,
            req(self.red, "red")?,
            req(self.green, "green")?,
            req(self.yellow, "yellow")?,
            req(self.blue, "blue")?,
            req(self.magenta, "magenta")?,
            req(self.cyan, "cyan")?,
            req(self.white, "white")?,
        ];

        if let (Some(b0), Some(b1), Some(b2), Some(b3), Some(b4), Some(b5), Some(b6), Some(b7)) = (
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_magenta,
            self.bright_cyan,
            self.bright_white,
        ) {
            colors.extend([b0, b1, b2, b3, b4, b5, b6, b7]);
        }

        Ok(RawColorScheme {
            name: self.name.unwrap_or_else(|| "Xterm Theme".to_string()),
            author: self.author,
            foreground: self.foreground.unwrap_or_default(),
            background: self.background.unwrap_or_default(),
            cursor: self.cursor,
            cursor_accent: self.cursor_accent,
            cursor_style: None,
            selection_background: self.selection_background,
            selection_foreground: self.selection_foreground,
            active_tab_accent: None,
            colors,
            indexed_colors: self.extended_ansi.unwrap_or_default(),
            source_format: None,
        })
    }
}

/// Faz o parsing de um arquivo JSON no formato `ITheme` do `@xterm/xterm`.
pub fn parse_xterm_theme_json(content: &str) -> Result<Vec<RawColorScheme>, ThemeError> {
    let cleaned = content.trim_start_matches('\u{feff}');
    let entry: XtermJsonEntry =
        serde_json::from_str(cleaned).map_err(|e| ThemeError::UnsupportedFormat {
            reason: format!("JSON Xterm ITheme inválido: {e}"),
        })?;
    Ok(vec![entry.into_raw()?])
}
