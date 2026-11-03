//! Exportador determinístico de `ColorScheme` para Tabby YAML/JSON, Windows Terminal JSON e Xterm JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::ThemeError;
use crate::mapping::xterm::derive_xterm_theme;
use crate::scheme::ColorScheme;

/// Formatos suportados na exportação de um esquema de cores (`theme_export`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// Esquema YAML compatível com Tabby Terminal.
    TabbyYaml,
    /// Esquema JSON compatível com Tabby Terminal.
    TabbyJson,
    /// Esquema JSON compatível com Windows Terminal / iTerm2-Color-Schemes.
    WindowsTerminalJson,
    /// Objeto JSON `ITheme` compatível com `@xterm/xterm`.
    XtermJson,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TabbyExportPayload<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<&'a str>,
    foreground: String,
    background: String,
    cursor: String,
    cursor_accent: String,
    cursor_style: &'static str,
    selection: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection_foreground: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_tab_accent: Option<String>,
    colors: [String; 16],
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    indexed_colors: BTreeMap<u8, String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowsTerminalExportPayload<'a> {
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<&'a str>,
    foreground: String,
    background: String,
    cursor_color: String,
    selection_background: String,
    black: String,
    red: String,
    green: String,
    yellow: String,
    blue: String,
    purple: String,
    cyan: String,
    white: String,
    bright_black: String,
    bright_red: String,
    bright_green: String,
    bright_yellow: String,
    bright_blue: String,
    bright_purple: String,
    bright_cyan: String,
    bright_white: String,
}

/// Serializa um `ColorScheme` para o formato solicitado (`ExportFormat`).
pub fn export_scheme(scheme: &ColorScheme, format: ExportFormat) -> Result<String, ThemeError> {
    match format {
        ExportFormat::TabbyYaml => {
            let payload = to_tabby_payload(scheme);
            serde_yaml_ng::to_string(&payload).map_err(|e| ThemeError::UnsupportedFormat {
                reason: format!("Falha ao serializar YAML Tabby: {e}"),
            })
        }
        ExportFormat::TabbyJson => {
            let payload = to_tabby_payload(scheme);
            serde_json::to_string_pretty(&payload).map_err(|e| ThemeError::UnsupportedFormat {
                reason: format!("Falha ao serializar JSON Tabby: {e}"),
            })
        }
        ExportFormat::WindowsTerminalJson => {
            let payload = WindowsTerminalExportPayload {
                name: &scheme.name,
                author: scheme.metadata.author.as_deref(),
                foreground: scheme.foreground.to_hex_string(),
                background: scheme.background.to_hex_string(),
                cursor_color: scheme.cursor.to_hex_string(),
                selection_background: scheme.selection_background.to_hex8_string(),
                black: scheme.ansi.black.to_hex_string(),
                red: scheme.ansi.red.to_hex_string(),
                green: scheme.ansi.green.to_hex_string(),
                yellow: scheme.ansi.yellow.to_hex_string(),
                blue: scheme.ansi.blue.to_hex_string(),
                purple: scheme.ansi.magenta.to_hex_string(),
                cyan: scheme.ansi.cyan.to_hex_string(),
                white: scheme.ansi.white.to_hex_string(),
                bright_black: scheme.ansi.bright_black.to_hex_string(),
                bright_red: scheme.ansi.bright_red.to_hex_string(),
                bright_green: scheme.ansi.bright_green.to_hex_string(),
                bright_yellow: scheme.ansi.bright_yellow.to_hex_string(),
                bright_blue: scheme.ansi.bright_blue.to_hex_string(),
                bright_purple: scheme.ansi.bright_magenta.to_hex_string(),
                bright_cyan: scheme.ansi.bright_cyan.to_hex_string(),
                bright_white: scheme.ansi.bright_white.to_hex_string(),
            };
            serde_json::to_string_pretty(&payload).map_err(|e| ThemeError::UnsupportedFormat {
                reason: format!("Falha ao serializar JSON Windows Terminal: {e}"),
            })
        }
        ExportFormat::XtermJson => {
            let xterm = derive_xterm_theme(scheme);
            serde_json::to_string_pretty(&xterm).map_err(|e| ThemeError::UnsupportedFormat {
                reason: format!("Falha ao serializar JSON Xterm: {e}"),
            })
        }
    }
}

fn to_tabby_payload(scheme: &ColorScheme) -> TabbyExportPayload<'_> {
    let colors = scheme
        .ansi
        .as_array()
        .map(super::color::HexColor::to_hex_string);
    let indexed_colors = scheme
        .indexed_colors
        .iter()
        .map(|(k, v)| (*k, v.to_hex_string()))
        .collect();

    TabbyExportPayload {
        name: &scheme.name,
        author: scheme.metadata.author.as_deref(),
        foreground: scheme.foreground.to_hex_string(),
        background: scheme.background.to_hex_string(),
        cursor: scheme.cursor.to_hex_string(),
        cursor_accent: scheme.cursor_accent.to_hex_string(),
        cursor_style: scheme.cursor_style.as_xterm_str(),
        selection: scheme.selection_background.to_hex8_string(),
        selection_foreground: scheme
            .selection_foreground
            .map(super::color::HexColor::to_hex_string),
        active_tab_accent: scheme
            .active_tab_accent
            .map(super::color::HexColor::to_hex_string),
        colors,
        indexed_colors,
    }
}
