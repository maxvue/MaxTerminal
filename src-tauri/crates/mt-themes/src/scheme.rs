//! Modelos canônicos de paleta ANSI (`AnsiPalette`), esquema de cores (`ColorScheme`),
//! estilo de cursor (`CursorStyle`) e metadados (`ThemeMetadata`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::color::{HexColor, RgbaColor};
use crate::ids::{ThemeId, ThemeOrigin, ThemePolarity};

/// Estilo visual do cursor do terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CursorStyle {
    /// Bloco preenchido (padrão).
    #[default]
    Block,
    /// Sublinhado na base da célula.
    Underline,
    /// Barra vertical fina.
    Bar,
}

impl CursorStyle {
    /// Nome compatível com a propriedade `cursorStyle` do `xterm.js`.
    #[must_use]
    pub const fn as_xterm_str(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Underline => "underline",
            Self::Bar => "bar",
        }
    }
}

/// Paleta ANSI completa de 16 cores (8 normais `0..=7` + 8 brilhantes `8..=15`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AnsiPalette {
    /// ANSI 0 — Preto.
    pub black: HexColor,
    /// ANSI 1 — Vermelho.
    pub red: HexColor,
    /// ANSI 2 — Verde.
    pub green: HexColor,
    /// ANSI 3 — Amarelo.
    pub yellow: HexColor,
    /// ANSI 4 — Azul.
    pub blue: HexColor,
    /// ANSI 5 — Magenta.
    pub magenta: HexColor,
    /// ANSI 6 — Ciano.
    pub cyan: HexColor,
    /// ANSI 7 — Branco.
    pub white: HexColor,
    /// ANSI 8 — Preto brilhante.
    pub bright_black: HexColor,
    /// ANSI 9 — Vermelho brilhante.
    pub bright_red: HexColor,
    /// ANSI 10 — Verde brilhante.
    pub bright_green: HexColor,
    /// ANSI 11 — Amarelo brilhante.
    pub bright_yellow: HexColor,
    /// ANSI 12 — Azul brilhante.
    pub bright_blue: HexColor,
    /// ANSI 13 — Magenta brilhante.
    pub bright_magenta: HexColor,
    /// ANSI 14 — Ciano brilhante.
    pub bright_cyan: HexColor,
    /// ANSI 15 — Branco brilhante.
    pub bright_white: HexColor,
}

impl AnsiPalette {
    /// Retorna as 16 cores na ordem canônica ANSI `0..=15`.
    #[must_use]
    pub const fn as_array(&self) -> [HexColor; 16] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.magenta,
            self.cyan,
            self.white,
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_magenta,
            self.bright_cyan,
            self.bright_white,
        ]
    }

    /// Constrói um `AnsiPalette` a partir de um array de 16 cores na ordem `0..=15`.
    #[must_use]
    pub const fn from_array(c: [HexColor; 16]) -> Self {
        Self {
            black: c[0],
            red: c[1],
            green: c[2],
            yellow: c[3],
            blue: c[4],
            magenta: c[5],
            cyan: c[6],
            white: c[7],
            bright_black: c[8],
            bright_red: c[9],
            bright_green: c[10],
            bright_yellow: c[11],
            bright_blue: c[12],
            bright_magenta: c[13],
            bright_cyan: c[14],
            bright_white: c[15],
        }
    }
}

/// Metadados de proveniência e integridade do esquema de cores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeMetadata {
    /// Autor ou organização responsável pelo esquema (opcional).
    pub author: Option<String>,
    /// Formato de origem (ex.: `"tabby-scheme-yaml"`, `"windows-terminal-json"`).
    pub source_format: Option<String>,
    /// Nome de arquivo original (apenas basename sanitizado, sem caminho absoluto do host).
    pub source_filename: Option<String>,
    /// Hash SHA-256 em hexadecimal (64 chars) do conteúdo canônico das cores do esquema.
    pub sha256: String,
    /// Timestamp Unix em milissegundos de criação.
    pub created_at: i64,
    /// Timestamp Unix em milissegundos da última atualização.
    pub updated_at: i64,
}

/// Esquema de cores canônico do `MaxTerminal`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ColorScheme {
    /// Identificador único estável (`slug`).
    pub id: ThemeId,
    /// Nome legível exibido na interface (1..=80 caracteres).
    pub name: String,
    /// Origem do esquema (`builtin`, `imported`, `custom`).
    pub origin: ThemeOrigin,
    /// Polaridade visual classificada a partir da luminância do fundo.
    pub polarity: ThemePolarity,
    /// Cor principal de texto do terminal.
    pub foreground: HexColor,
    /// Cor principal de fundo do terminal.
    pub background: HexColor,
    /// Cor do cursor do terminal.
    pub cursor: HexColor,
    /// Cor do caractere sob o cursor em modo bloco.
    pub cursor_accent: HexColor,
    /// Estilo geométrico do cursor.
    pub cursor_style: CursorStyle,
    /// Cor de fundo da seleção de texto (com canal alfa).
    pub selection_background: RgbaColor,
    /// Cor opcional de texto selecionado (`None` mantém a cor original do glifo).
    pub selection_foreground: Option<HexColor>,
    /// Cor opcional de destaque da aba ativa.
    pub active_tab_accent: Option<HexColor>,
    /// Paleta ANSI de 16 cores (`0..=15`).
    pub ansi: AnsiPalette,
    /// Paleta estendida opcional (`16..=255`).
    pub indexed_colors: BTreeMap<u8, HexColor>,
    /// Metadados de autoria, formato e hash de conteúdo.
    pub metadata: ThemeMetadata,
}

impl ColorScheme {
    /// Calcula o SHA-256 determinístico (64 caracteres hexadecimais) sobre os campos de cor do esquema,
    /// ignorando timestamps e ID para permitir deduplicação por conteúdo.
    #[must_use]
    pub fn compute_content_sha256(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.foreground.to_hex_string().as_bytes());
        hasher.update(b"|");
        hasher.update(self.background.to_hex_string().as_bytes());
        hasher.update(b"|");
        hasher.update(self.cursor.to_hex_string().as_bytes());
        hasher.update(b"|");
        hasher.update(self.cursor_accent.to_hex_string().as_bytes());
        hasher.update(b"|");
        hasher.update(self.cursor_style.as_xterm_str().as_bytes());
        hasher.update(b"|");
        hasher.update(self.selection_background.to_hex8_string().as_bytes());
        hasher.update(b"|");
        if let Some(sel_fg) = self.selection_foreground {
            hasher.update(sel_fg.to_hex_string().as_bytes());
        }
        hasher.update(b"|");
        if let Some(accent) = self.active_tab_accent {
            hasher.update(accent.to_hex_string().as_bytes());
        }
        for c in self.ansi.as_array() {
            hasher.update(b"|");
            hasher.update(c.to_hex_string().as_bytes());
        }
        for (idx, c) in &self.indexed_colors {
            hasher.update(b"|");
            hasher.update([*idx]);
            hasher.update(b":");
            hasher.update(c.to_hex_string().as_bytes());
        }
        hex::encode(hasher.finalize())
    }
}
