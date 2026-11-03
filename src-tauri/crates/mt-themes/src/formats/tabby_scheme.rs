//! Parser de esquemas avulsos do Tabby Terminal em YAML ou JSON (único objeto, array ou `{ schemes: [...] }`).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::ThemeError;
use crate::formats::{RawColorScheme, validate_safe_yaml};

/// Estrutura deserializável de um esquema Tabby (suporta nomes longos e abreviados `fg`/`bg`).
#[derive(Debug, Clone, Deserialize)]
pub struct TabbyYamlSchemeEntry {
    /// Nome do esquema.
    pub name: Option<String>,
    /// Autor opcional.
    pub author: Option<String>,
    /// Cor de texto (`foreground` ou `fg`).
    #[serde(alias = "fg")]
    pub foreground: Option<String>,
    /// Cor de fundo (`background` ou `bg`).
    #[serde(alias = "bg")]
    pub background: Option<String>,
    /// Cor do cursor.
    #[serde(alias = "cursorColor")]
    pub cursor: Option<String>,
    /// Cor de destaque sob o cursor.
    #[serde(alias = "cursorAccent", alias = "cursor_accent")]
    pub cursor_accent: Option<String>,
    /// Estilo do cursor (`block`, `underline`, `bar`).
    #[serde(alias = "cursorStyle", alias = "cursor_style")]
    pub cursor_style: Option<String>,
    /// Cor de fundo da seleção (`selection` ou `selectionBackground`).
    #[serde(alias = "selection", alias = "selectionBackground")]
    pub selection_background: Option<String>,
    /// Cor do texto selecionado (`selectionForeground`).
    #[serde(alias = "selectionFg", alias = "selectionForeground")]
    pub selection_foreground: Option<String>,
    /// Cor de destaque da aba ativa (`activeTabAccent`).
    #[serde(alias = "activeTabAccent", alias = "active_tab_accent")]
    pub active_tab_accent: Option<String>,
    /// Lista ou mapa das 8/16 cores ANSI.
    pub colors: Option<TabbyColorsField>,
    /// Paleta estendida (`16..=255`).
    #[serde(default, alias = "indexedColors", alias = "indexed_colors")]
    pub indexed_colors: BTreeMap<u8, String>,
}

/// Representação de `colors` em esquemas Tabby (lista posicional de 8/16 strings ou mapa nomeado).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TabbyColorsField {
    /// Sequência ordenada de 8 ou 16 cores.
    List(Vec<String>),
    /// Mapa nomeado (`black`, `red`, ..., `brightWhite`).
    Named(Box<NamedAnsiColors>),
}

/// Mapa nomeado de cores ANSI.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedAnsiColors {
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
    #[serde(alias = "purple")]
    pub magenta: String,
    /// ANSI 6.
    pub cyan: String,
    /// ANSI 7.
    pub white: String,
    /// ANSI 8.
    #[serde(alias = "bright_black")]
    pub bright_black: Option<String>,
    /// ANSI 9.
    #[serde(alias = "bright_red")]
    pub bright_red: Option<String>,
    /// ANSI 10.
    #[serde(alias = "bright_green")]
    pub bright_green: Option<String>,
    /// ANSI 11.
    #[serde(alias = "bright_yellow")]
    pub bright_yellow: Option<String>,
    /// ANSI 12.
    #[serde(alias = "bright_blue")]
    pub bright_blue: Option<String>,
    /// ANSI 13.
    #[serde(alias = "bright_magenta", alias = "brightPurple")]
    pub bright_magenta: Option<String>,
    /// ANSI 14.
    #[serde(alias = "bright_cyan")]
    pub bright_cyan: Option<String>,
    /// ANSI 15.
    #[serde(alias = "bright_white")]
    pub bright_white: Option<String>,
}

impl TabbyYamlSchemeEntry {
    /// Converte para `RawColorScheme`.
    #[must_use]
    pub fn into_raw(self) -> RawColorScheme {
        let colors = match self.colors {
            Some(TabbyColorsField::List(list)) => list,
            Some(TabbyColorsField::Named(named)) => {
                let mut v = vec![
                    named.black,
                    named.red,
                    named.green,
                    named.yellow,
                    named.blue,
                    named.magenta,
                    named.cyan,
                    named.white,
                ];
                if let (
                    Some(b0),
                    Some(b1),
                    Some(b2),
                    Some(b3),
                    Some(b4),
                    Some(b5),
                    Some(b6),
                    Some(b7),
                ) = (
                    named.bright_black,
                    named.bright_red,
                    named.bright_green,
                    named.bright_yellow,
                    named.bright_blue,
                    named.bright_magenta,
                    named.bright_cyan,
                    named.bright_white,
                ) {
                    v.extend([b0, b1, b2, b3, b4, b5, b6, b7]);
                }
                v
            }
            None => Vec::new(),
        };

        RawColorScheme {
            name: self.name.unwrap_or_default(),
            author: self.author,
            foreground: self.foreground.unwrap_or_default(),
            background: self.background.unwrap_or_default(),
            cursor: self.cursor,
            cursor_accent: self.cursor_accent,
            cursor_style: self.cursor_style,
            selection_background: self.selection_background,
            selection_foreground: self.selection_foreground,
            active_tab_accent: self.active_tab_accent,
            colors,
            indexed_colors: self.indexed_colors,
            source_format: None,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum TabbySchemeContainer {
    Wrapper { schemes: Vec<TabbyYamlSchemeEntry> },
    List(Vec<TabbyYamlSchemeEntry>),
    Single(Box<TabbyYamlSchemeEntry>),
}

/// Faz o parsing de um arquivo YAML contendo um ou mais esquemas Tabby.
pub fn parse_tabby_scheme_yaml(content: &str) -> Result<Vec<RawColorScheme>, ThemeError> {
    let cleaned = content.trim_start_matches('\u{feff}');
    validate_safe_yaml(cleaned)?;

    let container: TabbySchemeContainer =
        serde_yaml_ng::from_str(cleaned).map_err(|e| ThemeError::UnsupportedFormat {
            reason: format!("Falha ao interpretar esquema YAML do Tabby: {e}"),
        })?;

    Ok(extract_entries(container))
}

/// Faz o parsing de um arquivo JSON contendo um ou mais esquemas Tabby.
pub fn parse_tabby_scheme_json(content: &str) -> Result<Vec<RawColorScheme>, ThemeError> {
    let cleaned = content.trim_start_matches('\u{feff}');
    let container: TabbySchemeContainer =
        serde_json::from_str(cleaned).map_err(|e| ThemeError::UnsupportedFormat {
            reason: format!("Falha ao interpretar esquema JSON do Tabby: {e}"),
        })?;

    Ok(extract_entries(container))
}

fn extract_entries(container: TabbySchemeContainer) -> Vec<RawColorScheme> {
    match container {
        TabbySchemeContainer::Wrapper { schemes } | TabbySchemeContainer::List(schemes) => schemes
            .into_iter()
            .map(TabbyYamlSchemeEntry::into_raw)
            .collect(),
        TabbySchemeContainer::Single(single) => vec![single.into_raw()],
    }
}
