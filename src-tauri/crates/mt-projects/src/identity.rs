//! Resolução de identidade visual (`IdentityMap`), fallback determinístico FNV-1a 64
//! e validação de título, ícone e cor hexadecimal.

use std::path::Path;
use std::sync::LazyLock;

use mt_core::error::codes;
use mt_core::{MtError, MtResult};
use mt_db::sanitizer::SanitizedText;
use regex::Regex;

use crate::model::{ProjectIdentity, StackKind};

/// 12 ícones de fallback determinístico para projetos `unknown`.
pub const FALLBACK_ICONS: [&str; 12] = [
    "mdi:folder",
    "mdi:folder-star",
    "mdi:folder-code",
    "mdi:folder-heart",
    "mdi:folder-cog",
    "mdi:folder-wrench",
    "mdi:folder-table",
    "mdi:folder-text",
    "mdi:folder-multiple",
    "mdi:folder-network",
    "mdi:folder-zip",
    "mdi:folder-file",
];

/// 12 cores hexadecimais de alto contraste sobre o tema Max Navy/Teal para projetos `unknown`.
pub const FALLBACK_COLORS: [&str; 12] = [
    "#14B8A6", "#0EA5E9", "#6366F1", "#A855F7", "#EC4899", "#F43F5E", "#F97316", "#EAB308",
    "#84CC16", "#22C55E", "#06B6D4", "#64748B",
];

static ICON_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[a-z0-9]+(-[a-z0-9]+)*:[a-z0-9]+(-[a-z0-9]+)*$")
        .unwrap_or_else(|e| panic!("Regex de ícone inválida: {e}"))
});

static COLOR_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^#[0-9A-Fa-f]{6}$").unwrap_or_else(|e| panic!("Regex de cor inválida: {e}"))
});

/// Mapa de identidade visual por stack e fallback determinístico por caminho canônico.
pub struct IdentityMap;

impl IdentityMap {
    /// Retorna `(icon, color)` canônico para uma stack conhecida, ou `None` para [`StackKind::Unknown`].
    #[must_use]
    pub const fn for_stack(stack: StackKind) -> Option<(&'static str, &'static str)> {
        match stack {
            StackKind::Tauri => Some(("logos:tauri", "#FFC131")),
            StackKind::VscodeExtension => Some(("logos:visual-studio-code", "#007ACC")),
            StackKind::Laravel => Some(("logos:laravel", "#FF2D20")),
            StackKind::Vue => Some(("logos:vue", "#42B883")),
            StackKind::Rust => Some(("logos:rust", "#CE422B")),
            StackKind::Python => Some(("logos:python", "#3776AB")),
            StackKind::Unknown => None,
        }
    }

    /// Resolve a [`ProjectIdentity`] final a partir do caminho canônico e da lista de stacks detectadas.
    #[must_use]
    pub fn resolve(canonical_path: &Path, mut detected: Vec<StackKind>) -> ProjectIdentity {
        if detected.is_empty() {
            detected.push(StackKind::Unknown);
        }
        let primary = detected[0];
        let (icon, color) =
            Self::for_stack(primary).unwrap_or_else(|| Self::fallback(canonical_path));

        ProjectIdentity {
            icon: icon.to_string(),
            color: color.to_string(),
            stack: primary,
            detected,
        }
    }

    /// Calcula o par `(icon, color)` determinístico via FNV-1a 64 sobre os bytes UTF-8 do caminho canônico.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    pub fn fallback(canonical_path: &Path) -> (&'static str, &'static str) {
        let utf8 = canonical_path.to_string_lossy();
        let h = fnv1a_64(utf8.as_bytes());
        let icon_idx = (h % 12) as usize;
        let color_idx = ((h >> 32) % 12) as usize;
        (FALLBACK_ICONS[icon_idx], FALLBACK_COLORS[color_idx])
    }
}

/// Calcula o hash FNV-1a de 64 bits sobre `bytes`.
#[must_use]
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    hash
}

/// Valida um nome de ícone Iconify (`^[a-z0-9]+(-[a-z0-9]+)*:[a-z0-9]+(-[a-z0-9]+)*$`, `<= 64` bytes).
pub fn validate_icon(icon: &str) -> MtResult<()> {
    if icon.is_empty() || icon.len() > 64 || !ICON_RE.is_match(icon) {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ICON,
            "O ícone informado deve seguir o formato Iconify 'colecao:nome' (máx. 64 caracteres).",
        ));
    }
    Ok(())
}

/// Valida uma cor hexadecimal (`^#[0-9A-Fa-f]{6}$`) e a normaliza para letras maiúsculas (`#RRGGBB`).
pub fn validate_color(color: &str) -> MtResult<String> {
    let trimmed = color.trim();
    if !COLOR_RE.is_match(trimmed) {
        return Err(MtError::new(
            codes::PROJECT_INVALID_COLOR,
            "A cor informada deve estar no formato hexadecimal '#RRGGBB'.",
        ));
    }
    Ok(trimmed.to_ascii_uppercase())
}

/// Valida e higieniza um título de projeto (`trim`, `1..=120` caracteres, sem caracteres de controle).
pub fn validate_title(title: &str) -> MtResult<String> {
    if title.chars().any(char::is_control) {
        return Err(MtError::new(
            codes::PROJECT_INVALID_TITLE,
            "O título do projeto não pode conter quebras de linha ou caracteres de controle.",
        ));
    }

    let trimmed_raw = title.trim();
    if trimmed_raw.is_empty() {
        return Err(MtError::new(
            codes::PROJECT_INVALID_TITLE,
            "O título do projeto deve conter entre 1 e 120 caracteres.",
        ));
    }

    let sanitized = SanitizedText::new(trimmed_raw);
    let final_str = sanitized.as_str().trim();
    let char_count = final_str.chars().count();
    if !(1..=120).contains(&char_count) || final_str.chars().any(char::is_control) {
        return Err(MtError::new(
            codes::PROJECT_INVALID_TITLE,
            "O título do projeto deve conter entre 1 e 120 caracteres após higienização.",
        ));
    }

    Ok(final_str.to_string())
}
