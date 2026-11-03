//! Luminância relativa WCAG 2.1, razão de contraste, classificação de polaridade e ajuste de legibilidade.

use crate::color::HexColor;
use crate::ids::ThemePolarity;
use crate::oklch::{Oklch, srgb_u8_to_linear};

/// Limiar de luminância relativa para classificar um esquema como `Light` (`>= 0.35`) ou `Dark` (`< 0.35`).
pub const POLARITY_LUMINANCE_THRESHOLD: f32 = 0.35;

/// Contraste mínimo absoluto exigido entre `foreground` e `background` para aceitar um tema.
pub const MIN_FG_BG_CONTRAST: f32 = 3.0;

/// Contraste recomendado WCAG AA para texto normal (gera aviso quando inferior).
pub const RECOMMENDED_FG_BG_CONTRAST: f32 = 4.5;

/// Calcula a luminância relativa WCAG 2.1 (`0.0` para `#000000` até `1.0` para `#ffffff`).
#[must_use]
#[allow(clippy::suboptimal_flops)]
pub fn relative_luminance(color: HexColor) -> f32 {
    let r = srgb_u8_to_linear(color.r);
    let g = srgb_u8_to_linear(color.g);
    let b = srgb_u8_to_linear(color.b);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Calcula a razão de contraste WCAG 2.1 entre duas cores (`1.0..=21.0`).
#[must_use]
pub fn contrast_ratio(a: HexColor, b: HexColor) -> f32 {
    let la = relative_luminance(a);
    let lb = relative_luminance(b);
    let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
    (lighter + 0.05) / (darker + 0.05)
}

/// Classifica a polaridade do tema com base na luminância relativa do `background`.
#[must_use]
pub fn classify_polarity(background: HexColor) -> ThemePolarity {
    if relative_luminance(background) < POLARITY_LUMINANCE_THRESHOLD {
        ThemePolarity::Dark
    } else {
        ThemePolarity::Light
    }
}

/// Seleciona a cor de conteúdo (`#ffffff` ou `#0a1220`) que garante contraste `>= 4.5:1` sobre `bg`.
///
/// Prefere `#ffffff` caso ambas atinjam `>= 4.5:1`; caso nenhuma atinja, retorna a de maior contraste.
#[must_use]
pub fn pick_content_color(bg: HexColor) -> HexColor {
    let white = HexColor::new(0xff, 0xff, 0xff);
    let dark_ink = HexColor::new(0x0a, 0x12, 0x20);
    let cw = contrast_ratio(white, bg);
    let cd = contrast_ratio(dark_ink, bg);
    if cw >= 4.5 && cw >= cd {
        white
    } else if cd >= 4.5 {
        dark_ink
    } else if cw >= cd {
        white
    } else {
        dark_ink
    }
}

/// Ajusta a luminosidade OKLCH de `fg` para tentar atingir pelo menos `min_ratio` de contraste contra `bg`.
#[must_use]
pub fn ensure_min_contrast(fg: HexColor, bg: HexColor, min_ratio: f32) -> HexColor {
    if contrast_ratio(fg, bg) >= min_ratio {
        return fg;
    }

    let bg_lum = relative_luminance(bg);
    let oklch = Oklch::from_srgb(fg);
    let lighten = bg_lum < 0.5;

    let mut best = fg;
    let mut best_ratio = contrast_ratio(fg, bg);

    for step in 1u8..=24 {
        let t = f32::from(step) / 24.0;
        let target_l = if lighten {
            oklch.l + (0.98 - oklch.l) * t
        } else {
            oklch.l * (1.0 - t) + 0.05 * t
        };
        let candidate = oklch.with_l(target_l).to_srgb_gamut_mapped();
        let ratio = contrast_ratio(candidate, bg);
        if ratio > best_ratio {
            best = candidate;
            best_ratio = ratio;
        }
        if ratio >= min_ratio {
            return candidate;
        }
    }

    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wcag_luminance_and_contrast_extremes() {
        let black = HexColor::new(0, 0, 0);
        let white = HexColor::new(255, 255, 255);
        assert!(relative_luminance(black).abs() < 1e-6);
        assert!((relative_luminance(white) - 1.0).abs() < 1e-5);
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio(white, black) - 21.0).abs() < 1e-3);
        assert!((contrast_ratio(black, black) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn polarity_threshold_classification() {
        let navy = HexColor::new(0x0f, 0x1c, 0x2e);
        let paper = HexColor::new(0xf8, 0xfa, 0xfc);
        assert_eq!(classify_polarity(navy), ThemePolarity::Dark);
        assert_eq!(classify_polarity(paper), ThemePolarity::Light);
    }
}
