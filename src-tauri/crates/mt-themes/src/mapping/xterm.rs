//! Derivação do objeto `XtermThemePayload` (compatível 1:1 com `ITheme` de `@xterm/xterm`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::color::HexColor;
use crate::scheme::ColorScheme;

/// Payload compatível 1:1 com a interface `ITheme` de `@xterm/xterm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct XtermThemePayload {
    /// Cor principal de texto.
    pub foreground: String,
    /// Cor principal de fundo.
    pub background: String,
    /// Cor do cursor.
    pub cursor: String,
    /// Cor do texto sob o cursor em bloco.
    pub cursor_accent: String,
    /// Cor de fundo da seleção (`#rrggbbaa` ou `rgba(...)`).
    pub selection_background: String,
    /// Cor opcional do texto selecionado.
    pub selection_foreground: Option<String>,
    /// Cor de fundo da seleção quando a janela está inativa.
    pub selection_inactive_background: String,
    /// ANSI 0.
    pub black: String,
    /// ANSI 1.
    pub red: String,
    /// ANSI 2.
    pub green: String,
    /// ANSI 3.
    pub yellow: String,
    /// ANSI 4.
    pub blue: String,
    /// ANSI 5.
    pub magenta: String,
    /// ANSI 6.
    pub cyan: String,
    /// ANSI 7.
    pub white: String,
    /// ANSI 8.
    pub bright_black: String,
    /// ANSI 9.
    pub bright_red: String,
    /// ANSI 10.
    pub bright_green: String,
    /// ANSI 11.
    pub bright_yellow: String,
    /// ANSI 12.
    pub bright_blue: String,
    /// ANSI 13.
    pub bright_magenta: String,
    /// ANSI 14.
    pub bright_cyan: String,
    /// ANSI 15.
    pub bright_white: String,
    /// Paleta estendida opcional (`16..=255`).
    pub extended_ansi: Option<BTreeMap<u8, String>>,
}

/// Deriva o `XtermThemePayload` a partir de um `ColorScheme`.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn derive_xterm_theme(scheme: &ColorScheme) -> XtermThemePayload {
    let inactive_alpha = ((scheme.selection_background.alpha_f32() * 0.55) * 255.0)
        .round()
        .clamp(0.0, 255.0) as u8;
    let inactive = crate::color::RgbaColor::new(
        scheme.selection_background.r,
        scheme.selection_background.g,
        scheme.selection_background.b,
        inactive_alpha,
    );

    let extended_ansi = if scheme.indexed_colors.is_empty() {
        None
    } else {
        Some(
            scheme
                .indexed_colors
                .iter()
                .map(|(k, v): (&u8, &HexColor)| (*k, v.to_hex_string()))
                .collect(),
        )
    };

    XtermThemePayload {
        foreground: scheme.foreground.to_hex_string(),
        background: scheme.background.to_hex_string(),
        cursor: scheme.cursor.to_hex_string(),
        cursor_accent: scheme.cursor_accent.to_hex_string(),
        selection_background: scheme.selection_background.to_hex8_string(),
        selection_foreground: scheme.selection_foreground.map(HexColor::to_hex_string),
        selection_inactive_background: inactive.to_hex8_string(),
        black: scheme.ansi.black.to_hex_string(),
        red: scheme.ansi.red.to_hex_string(),
        green: scheme.ansi.green.to_hex_string(),
        yellow: scheme.ansi.yellow.to_hex_string(),
        blue: scheme.ansi.blue.to_hex_string(),
        magenta: scheme.ansi.magenta.to_hex_string(),
        cyan: scheme.ansi.cyan.to_hex_string(),
        white: scheme.ansi.white.to_hex_string(),
        bright_black: scheme.ansi.bright_black.to_hex_string(),
        bright_red: scheme.ansi.bright_red.to_hex_string(),
        bright_green: scheme.ansi.bright_green.to_hex_string(),
        bright_yellow: scheme.ansi.bright_yellow.to_hex_string(),
        bright_blue: scheme.ansi.bright_blue.to_hex_string(),
        bright_magenta: scheme.ansi.bright_magenta.to_hex_string(),
        bright_cyan: scheme.ansi.bright_cyan.to_hex_string(),
        bright_white: scheme.ansi.bright_white.to_hex_string(),
        extended_ansi,
    }
}
