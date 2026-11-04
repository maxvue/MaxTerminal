//! Pipeline de validação, preenchimento de defaults, normalização de nomes e cálculo de avisos (`ImportWarning`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::color::{HexColor, RgbaColor, parse_color_with_bg};
use crate::contrast::{
    MIN_FG_BG_CONTRAST, RECOMMENDED_FG_BG_CONTRAST, classify_polarity, contrast_ratio,
};
use crate::error::ThemeError;
use crate::formats::RawColorScheme;
use crate::ids::{ThemeId, ThemeOrigin};
use crate::oklch::Oklch;
use crate::scheme::{AnsiPalette, ColorScheme, CursorStyle, ThemeDraftInput, ThemeMetadata};

/// Códigos semânticos de aviso emitidos durante a normalização de um esquema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WarningCode {
    /// Apenas 8 cores ANSI foram fornecidas; as 8 brilhantes (`8..=15`) foram derivadas em OKLCH.
    SynthesizedBrightPalette,
    /// `cursor` estava ausente e foi derivado de `foreground`.
    DefaultedCursor,
    /// `selection_background` estava ausente e foi derivado de `foreground`/`background`.
    DefaultedSelection,
    /// Contraste entre `foreground` e `background` está entre `3.0:1` e `4.5:1`.
    LowFgBgContrast,
    /// Uma ou mais cores ANSI têm contraste `< 1.8:1` contra `background`.
    LowAnsiContrast,
    /// Mais de 240 entradas em `indexed_colors` foram truncadas.
    ClampedIndexedColorCount,
    /// Nome continha caracteres de controle e foi sanitizado.
    SanitizedName,
}

/// Aviso não fatal registrado durante a importação/normalização de um esquema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ImportWarning {
    /// Nome do esquema ao qual o aviso se refere.
    pub scheme_name: String,
    /// Código estruturado do aviso.
    pub code: WarningCode,
    /// Mensagem amigável em `pt-BR`.
    pub message: String,
}

/// Resultado da normalização de um `RawColorScheme`: esquema pronto + avisos gerados.
#[derive(Debug, Clone)]
pub struct NormalizedSchemeOutput {
    /// Esquema de cores validado e normalizado.
    pub scheme: ColorScheme,
    /// Lista de avisos não fatais emitidos.
    pub warnings: Vec<ImportWarning>,
}

/// Normaliza um `RawColorScheme` em `ColorScheme` aplicando todas as regras de segurança,
/// defaults de campos opcionais e validações de contraste da Fase 10.
#[allow(clippy::too_many_lines)]
pub fn normalize_raw_scheme(
    raw: RawColorScheme,
    origin: ThemeOrigin,
    explicit_id: Option<ThemeId>,
    source_filename: Option<String>,
    now_ms: i64,
) -> Result<NormalizedSchemeOutput, ThemeError> {
    let mut warnings = Vec::new();

    let fallback_name = source_filename
        .as_deref()
        .and_then(|f| f.split('.').next())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or("Tema Importado");

    let raw_name = if raw.name.trim().is_empty() {
        fallback_name.to_string()
    } else {
        raw.name.clone()
    };

    let (name, was_sanitized) = sanitize_display_name(&raw_name)?;
    if was_sanitized {
        warnings.push(ImportWarning {
            scheme_name: name.clone(),
            code: WarningCode::SanitizedName,
            message: "Caracteres de controle foram removidos do nome do esquema.".to_string(),
        });
    }

    let author = raw
        .author
        .as_deref()
        .map(sanitize_optional_text)
        .filter(|s| !s.is_empty());

    let background = HexColor::parse_field("background", &raw.background)?;
    let foreground = parse_color_with_bg("foreground", &raw.foreground, Some(background))?;

    let fg_bg_ratio = contrast_ratio(foreground, background);
    if fg_bg_ratio < MIN_FG_BG_CONTRAST {
        return Err(ThemeError::InsufficientContrast { ratio: fg_bg_ratio });
    }
    if fg_bg_ratio < RECOMMENDED_FG_BG_CONTRAST {
        warnings.push(ImportWarning {
            scheme_name: name.clone(),
            code: WarningCode::LowFgBgContrast,
            message: format!(
                "Contraste entre texto e fundo ({fg_bg_ratio:.2}:1) é inferior ao recomendado pela WCAG AA (4.5:1)."
            ),
        });
    }

    let cursor = if let Some(c_str) = raw.cursor.as_deref().filter(|s| !s.trim().is_empty()) {
        parse_color_with_bg("cursor", c_str, Some(background))?
    } else {
        warnings.push(ImportWarning {
            scheme_name: name.clone(),
            code: WarningCode::DefaultedCursor,
            message: "Cor do cursor ausente; utilizando a cor de texto principal.".to_string(),
        });
        foreground
    };

    let cursor_accent = match raw
        .cursor_accent
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(ca_str) => parse_color_with_bg("cursorAccent", ca_str, Some(cursor))?,
        None => background,
    };

    let cursor_style = match raw
        .cursor_style
        .as_deref()
        .map(|s| s.trim().to_ascii_lowercase())
        .as_deref()
    {
        None | Some("" | "block") => CursorStyle::Block,
        Some("underline" | "under") => CursorStyle::Underline,
        Some("bar" | "beam" | "line") => CursorStyle::Bar,
        Some(other) => {
            return Err(ThemeError::InvalidField {
                field: "cursorStyle".to_string(),
                reason: format!("Estilo de cursor inválido: {other}"),
            });
        }
    };

    let selection_background = if let Some(sel_str) = raw
        .selection_background
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        let parsed = RgbaColor::parse_field("selection", sel_str)?;
        if parsed.a == 255 && parsed.without_alpha() == background {
            let mix = foreground.mix_oklch(background, 0.72);
            RgbaColor::from_hex_with_alpha(mix, 0.32)
        } else {
            parsed
        }
    } else {
        warnings.push(ImportWarning {
            scheme_name: name.clone(),
            code: WarningCode::DefaultedSelection,
            message: "Cor de seleção ausente; derivada automaticamente a partir do esquema."
                .to_string(),
        });
        let mix = foreground.mix_oklch(background, 0.72);
        RgbaColor::from_hex_with_alpha(mix, 0.32)
    };

    let selection_foreground = match raw
        .selection_foreground
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(sfg) => Some(parse_color_with_bg(
            "selectionForeground",
            sfg,
            Some(background),
        )?),
        None => None,
    };

    let active_tab_accent = match raw
        .active_tab_accent
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        Some(acc) => Some(parse_color_with_bg(
            "activeTabAccent",
            acc,
            Some(background),
        )?),
        None => None,
    };

    let ansi = build_ansi_palette(&raw.colors, background, &name, &mut warnings)?;

    let mut low_ansi_count = 0usize;
    for ansi_color in ansi.as_array() {
        if contrast_ratio(ansi_color, background) < 1.8 {
            low_ansi_count += 1;
        }
    }
    if low_ansi_count > 0 {
        warnings.push(ImportWarning {
            scheme_name: name.clone(),
            code: WarningCode::LowAnsiContrast,
            message: format!(
                "{low_ansi_count} cor(es) da paleta ANSI possuem contraste baixo (< 1.8:1) contra o fundo."
            ),
        });
    }

    let mut indexed_colors = BTreeMap::new();
    let mut skipped_indexed = false;
    for (idx, val_str) in raw.indexed_colors {
        if idx < 16 {
            continue;
        }
        if indexed_colors.len() >= 240 {
            skipped_indexed = true;
            break;
        }
        let c = parse_color_with_bg(&format!("indexedColors[{idx}]"), &val_str, Some(background))?;
        indexed_colors.insert(idx, c);
    }
    if skipped_indexed {
        warnings.push(ImportWarning {
            scheme_name: name.clone(),
            code: WarningCode::ClampedIndexedColorCount,
            message: "Paleta indexada truncada em 240 entradas (índices 16..=255).".to_string(),
        });
    }

    let polarity = classify_polarity(background);
    let sanitized_filename = source_filename.and_then(|p| {
        std::path::Path::new(&p)
            .file_name()
            .and_then(|n| n.to_str())
            .map(ToString::to_string)
    });

    let provisional_id =
        explicit_id.unwrap_or_else(|| ThemeId::slug_from_name(&name, &background.to_hex_string()));

    let mut scheme = ColorScheme {
        id: provisional_id,
        name,
        origin,
        polarity,
        foreground,
        background,
        cursor,
        cursor_accent,
        cursor_style,
        selection_background,
        selection_foreground,
        active_tab_accent,
        ansi,
        indexed_colors,
        metadata: ThemeMetadata {
            author,
            source_format: raw.source_format.map(|f| f.as_metadata_str().to_string()),
            source_filename: sanitized_filename,
            sha256: String::new(),
            created_at: now_ms,
            updated_at: now_ms,
        },
    };

    let sha256 = scheme.compute_content_sha256();
    scheme.metadata.sha256 = sha256;

    Ok(NormalizedSchemeOutput { scheme, warnings })
}

/// Valida e normaliza um `ThemeDraftInput` oriundo do editor visual de temas (`theme_create` / `theme_update`),
/// exigindo o preenchimento explícito de todos os campos obrigatórios (nome, cores principais, cursor,
/// seleção e as 16 cores ANSI) e preservando `indexed_colors` (`16..=255`).
pub fn normalize_draft_input(
    draft: ThemeDraftInput,
    origin: ThemeOrigin,
    explicit_id: Option<ThemeId>,
    now_ms: i64,
) -> Result<NormalizedSchemeOutput, ThemeError> {
    if draft.name.trim().is_empty() {
        return Err(ThemeError::InvalidField {
            field: "name".to_string(),
            reason: "Nome do esquema não pode ser vazio".to_string(),
        });
    }

    let background = HexColor::parse_field("background", &draft.background)?;
    let _foreground = parse_color_with_bg("foreground", &draft.foreground, Some(background))?;
    let cursor = parse_color_with_bg("cursor", &draft.cursor, Some(background))?;
    let _cursor_accent = parse_color_with_bg("cursorAccent", &draft.cursor_accent, Some(cursor))?;
    let _selection_bg = RgbaColor::parse_field("selectionBackground", &draft.selection_background)?;

    for (field_name, value) in draft.ansi.as_named_entries() {
        let _ = parse_color_with_bg(field_name, value, Some(background))?;
    }

    for idx in draft.indexed_colors.keys() {
        if *idx < 16 {
            return Err(ThemeError::InvalidField {
                field: format!("indexedColors[{idx}]"),
                reason: "Índices da paleta estendida devem estar entre 16 e 255".to_string(),
            });
        }
    }

    let raw = RawColorScheme {
        name: draft.name,
        author: draft.author,
        foreground: draft.foreground,
        background: draft.background,
        cursor: Some(draft.cursor),
        cursor_accent: Some(draft.cursor_accent),
        cursor_style: Some(draft.cursor_style.as_xterm_str().to_string()),
        selection_background: Some(draft.selection_background),
        selection_foreground: draft.selection_foreground,
        active_tab_accent: draft.active_tab_accent,
        colors: draft.ansi.to_vec(),
        indexed_colors: draft.indexed_colors,
        source_format: None,
    };

    normalize_raw_scheme(raw, origin, explicit_id, None, now_ms)
}

fn build_ansi_palette(
    raw_colors: &[String],
    background: HexColor,
    scheme_name: &str,
    warnings: &mut Vec<ImportWarning>,
) -> Result<AnsiPalette, ThemeError> {
    if raw_colors.len() != 8 && raw_colors.len() != 16 {
        return Err(ThemeError::InvalidField {
            field: "colors".to_string(),
            reason: format!(
                "Paleta ANSI deve conter 8 ou 16 cores (recebido: {})",
                raw_colors.len()
            ),
        });
    }

    let mut parsed = [HexColor::new(0, 0, 0); 16];
    for (idx, raw_c) in raw_colors.iter().enumerate() {
        parsed[idx] = parse_color_with_bg(&format!("colors[{idx}]"), raw_c, Some(background))?;
    }

    if raw_colors.len() == 8 {
        warnings.push(ImportWarning {
            scheme_name: scheme_name.to_string(),
            code: WarningCode::SynthesizedBrightPalette,
            message: "As 8 cores ANSI brilhantes (8..=15) foram sintetizadas em OKLCH a partir das cores base.".to_string(),
        });
        for i in 0..8 {
            let base_ok = Oklch::from_srgb(parsed[i]);
            let bright_l = (base_ok.l + 0.12).min(0.95);
            let bright_c = (base_ok.c * 1.08).min(0.32);
            parsed[i + 8] = Oklch::new(bright_l, bright_c, base_ok.h).to_srgb_gamut_mapped();
        }
    }

    Ok(AnsiPalette::from_array(parsed))
}

fn sanitize_display_name(raw: &str) -> Result<(String, bool), ThemeError> {
    let mut cleaned = String::with_capacity(raw.len());
    let mut had_control = false;
    let mut prev_space = false;

    for ch in raw.chars() {
        if ch.is_control() {
            had_control = true;
            continue;
        }
        if ch.is_whitespace() {
            if !prev_space && !cleaned.is_empty() {
                cleaned.push(' ');
                prev_space = true;
            }
        } else {
            cleaned.push(ch);
            prev_space = false;
        }
    }

    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        return Err(ThemeError::InvalidField {
            field: "name".to_string(),
            reason: "Nome do esquema não pode ser vazio".to_string(),
        });
    }

    let char_count = trimmed.chars().count();
    if char_count > 80 {
        return Err(ThemeError::InvalidField {
            field: "name".to_string(),
            reason: format!("Nome do esquema excede 80 caracteres ({char_count})"),
        });
    }

    Ok((trimmed.to_string(), had_control))
}

fn sanitize_optional_text(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        if !ch.is_control() && out.chars().count() < 120 {
            out.push(ch);
        }
    }
    out.trim().to_string()
}
