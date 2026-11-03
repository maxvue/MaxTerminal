//! Identificadores tipados de esquemas de cores (`ThemeId`), origens e polaridades.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use specta::Type;

use crate::error::ThemeError;

/// Identificador canônico de um esquema de cores (`[a-z0-9][a-z0-9-_]{1,62}[a-z0-9]`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ThemeId(String);

impl ThemeId {
    /// ID canônico do tema padrão escuro (`max-navy-teal`).
    pub const DEFAULT_DARK: &'static str = "max-navy-teal";
    /// ID canônico do tema padrão claro (`max-light`).
    pub const DEFAULT_LIGHT: &'static str = "max-light";

    /// Valida e constrói um `ThemeId` conforme `^[a-z0-9][a-z0-9-_]{1,62}[a-z0-9]$`.
    pub fn parse(raw: &str) -> Result<Self, ThemeError> {
        let bytes = raw.as_bytes();
        if !(3..=64).contains(&bytes.len()) {
            return Err(ThemeError::InvalidField {
                field: "id".to_string(),
                reason: "Identificador deve ter entre 3 e 64 caracteres".to_string(),
            });
        }

        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if !Self::is_ascii_alnum_lower(first) || !Self::is_ascii_alnum_lower(last) {
            return Err(ThemeError::InvalidField {
                field: "id".to_string(),
                reason: "Identificador deve começar e terminar com letra minúscula ou dígito"
                    .to_string(),
            });
        }

        if !bytes
            .iter()
            .copied()
            .all(|b| Self::is_ascii_alnum_lower(b) || b == b'-' || b == b'_')
        {
            return Err(ThemeError::InvalidField {
                field: "id".to_string(),
                reason: "Identificador contém caracteres inválidos".to_string(),
            });
        }

        Ok(Self(raw.to_string()))
    }

    /// Deriva um slug determinístico a partir do nome exibido e conteúdo canônico.
    ///
    /// Caso o slug possua menos de 3 caracteres após normalização, anexa `-theme` ou sufixo hash.
    #[must_use]
    pub fn slug_from_name(name: &str, canonical_seed: &str) -> Self {
        let mut out = String::with_capacity(name.len());
        let mut prev_hyphen = false;

        for ch in name.chars() {
            let mapped = match ch {
                'a'..='z' | '0'..='9' => Some(ch),
                'A'..='Z' => Some(ch.to_ascii_lowercase()),
                'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => Some('a'),
                'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => Some('e'),
                'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => Some('i'),
                'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => Some('o'),
                'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => Some('u'),
                'ç' | 'Ç' => Some('c'),
                'ñ' | 'Ñ' => Some('n'),
                _ => None,
            };

            if let Some(c) = mapped {
                out.push(c);
                prev_hyphen = false;
            } else if !out.is_empty() && !prev_hyphen {
                out.push('-');
                prev_hyphen = true;
            }
        }

        while out.ends_with('-') || out.ends_with('_') {
            out.pop();
        }

        if out.len() > 54 {
            out.truncate(54);
            while out.ends_with('-') || out.ends_with('_') {
                out.pop();
            }
        }

        if out.len() < 3 {
            let mut hasher = Sha256::new();
            hasher.update(canonical_seed.as_bytes());
            let hex_digest = hex::encode(hasher.finalize());
            let short = &hex_digest[..8];
            if out.is_empty() {
                out = format!("theme-{short}");
            } else {
                out = format!("{out}-{short}");
            }
        }

        Self(out)
    }

    /// Constrói um novo `ThemeId` anexando o sufixo `-<short_hash>` de 8 caracteres hexadecimais.
    #[must_use]
    pub fn with_short_hash(&self, sha256_hex: &str) -> Self {
        let short = &sha256_hex[..sha256_hex.len().min(8)];
        let mut base = self.0.clone();
        let max_base = 64usize.saturating_sub(short.len() + 1);
        if base.len() > max_base {
            base.truncate(max_base);
            while base.ends_with('-') || base.ends_with('_') {
                base.pop();
            }
        }
        Self(format!("{base}-{short}"))
    }

    /// Retorna a representação textual do identificador.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    const fn is_ascii_alnum_lower(b: u8) -> bool {
        b.is_ascii_lowercase() || b.is_ascii_digit()
    }
}

impl fmt::Display for ThemeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for ThemeId {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl AsRef<str> for ThemeId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Origem do esquema de cores no catálogo do `MaxTerminal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemeOrigin {
    /// Tema nativo embutido no binário (`include_str!`).
    Builtin,
    /// Tema importado de arquivo Tabby / Windows Terminal / Xterm JSON.
    Imported,
    /// Tema personalizado criado pelo usuário.
    Custom,
}

impl ThemeOrigin {
    /// Representação textual persistida no SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Imported => "imported",
            Self::Custom => "custom",
        }
    }

    /// Converte a string persistida no SQLite em `ThemeOrigin`.
    pub fn parse_db(raw: &str) -> Result<Self, ThemeError> {
        match raw {
            "builtin" => Ok(Self::Builtin),
            "imported" => Ok(Self::Imported),
            "custom" => Ok(Self::Custom),
            other => Err(ThemeError::InvalidField {
                field: "origin".to_string(),
                reason: format!("Origem de tema inválida: {other}"),
            }),
        }
    }
}

/// Polaridade visual do esquema de cores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ThemePolarity {
    /// Fundo escuro (`luminance(background) < 0.35`).
    Dark,
    /// Fundo claro (`luminance(background) >= 0.35`).
    Light,
}

impl ThemePolarity {
    /// Representação textual persistida no SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dark => "dark",
            Self::Light => "light",
        }
    }

    /// Converte a string persistida no SQLite em `ThemePolarity`.
    pub fn parse_db(raw: &str) -> Result<Self, ThemeError> {
        match raw {
            "dark" => Ok(Self::Dark),
            "light" => Ok(Self::Light),
            other => Err(ThemeError::InvalidField {
                field: "polarity".to_string(),
                reason: format!("Polaridade de tema inválida: {other}"),
            }),
        }
    }
}
