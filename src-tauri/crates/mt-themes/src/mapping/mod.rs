//! Mapeamento determinístico entre `ColorScheme`, `XtermThemePayload` e `MaxUiTokens`.

pub mod tokens;
pub mod xterm;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::scheme::ColorScheme;
use tokens::{MaxUiTokens, derive_max_ui_tokens};
use xterm::{XtermThemePayload, derive_xterm_theme};

/// Tema completamente resolvido contendo o esquema canônico, o payload para `@xterm/xterm`
/// e todas as variáveis CSS semânticas do `@maxvue/max-components-ui`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedTheme {
    /// Esquema de cores canônico.
    pub scheme: ColorScheme,
    /// Objeto `ITheme` pronto para aplicar nas instâncias `@xterm/xterm`.
    pub xterm: XtermThemePayload,
    /// Escalas e tokens CSS da UI (`--max-*`, `--background-*`, `--mt-term-*`).
    pub ui_tokens: MaxUiTokens,
}

impl ResolvedTheme {
    /// Resolve deterministicamente um `ColorScheme` em `ResolvedTheme`.
    #[must_use]
    pub fn from_scheme(scheme: ColorScheme) -> Self {
        let xterm = derive_xterm_theme(&scheme);
        let ui_tokens = derive_max_ui_tokens(&scheme);
        Self {
            scheme,
            xterm,
            ui_tokens,
        }
    }
}
