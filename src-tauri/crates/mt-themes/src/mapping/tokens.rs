//! Derivação determinística dos tokens CSS do `@maxvue/max-components-ui` (`MaxUiTokens`)
//! e dos tokens suplementares de terminal (`TerminalUiTokens`).

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::color::{HexColor, RgbaColor};
use crate::ids::{ThemeId, ThemeOrigin, ThemePolarity};
use crate::oklch::Oklch;
use crate::scale::{BackgroundRamp, ColorScale11, generate_background_ramp, generate_scale};
use crate::scheme::ColorScheme;

/// Tokens CSS suplementares específicos do `MaxTerminal` (`--mt-term-*`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct TerminalUiTokens {
    /// `--mt-term-bg`
    pub bg: HexColor,
    /// `--mt-term-fg`
    pub fg: HexColor,
    /// `--mt-term-cursor`
    pub cursor: HexColor,
    /// `--mt-term-cursor-accent`
    pub cursor_accent: HexColor,
    /// `--mt-term-selection-bg`
    pub selection_bg: RgbaColor,
    /// `--mt-term-selection-fg`
    pub selection_fg: Option<HexColor>,
    /// `--mt-term-tab-active-accent`
    pub tab_active_accent: HexColor,
    /// `--mt-term-scrollbar-thumb`
    pub scrollbar_thumb: RgbaColor,
    /// `--mt-term-find-match-bg`
    pub find_match_bg: RgbaColor,
    /// `--mt-term-block-bg`
    pub block_bg: HexColor,
    /// `--mt-term-block-border`
    pub block_border: HexColor,
    /// `--mt-term-block-error-bg`
    pub block_error_bg: RgbaColor,
}

/// Conjunto completo de tokens semânticos do `@maxvue/max-components-ui` e do terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct MaxUiTokens {
    /// Polaridade visual (`dark` ou `light`).
    pub polarity: ThemePolarity,
    /// Escala `--max-primary-50..950` + `--max-primary-content`.
    pub primary: ColorScale11,
    /// Escala `--max-success-50..950` + `--max-success-content`.
    pub success: ColorScale11,
    /// Escala `--max-warning-50..950` + `--max-warning-content`.
    pub warning: ColorScale11,
    /// Escala `--max-danger-50..950` + `--max-danger-content`.
    pub danger: ColorScale11,
    /// Escala `--max-info-50..950` + `--max-info-content`.
    pub info: ColorScale11,
    /// Rampa `--background-0..950` (39 passos de 25 em 25).
    pub background: BackgroundRamp,
    /// Tokens suplementares `--mt-term-*`.
    pub terminal: TerminalUiTokens,
}

/// Deriva todos os tokens CSS da UI (`MaxUiTokens`) a partir de um `ColorScheme`.
///
/// Para os temas nativos `max-navy-teal` e `max-light`, preserva bit-a-bit os valores canônicos
/// de `@maxvue/max-components-ui` (`tokens.scss` e `colors.scss`).
#[must_use]
pub fn derive_max_ui_tokens(scheme: &ColorScheme) -> MaxUiTokens {
    let is_canonical_max_dark =
        scheme.origin == ThemeOrigin::Builtin && scheme.id.as_str() == ThemeId::DEFAULT_DARK;
    let is_canonical_max_light =
        scheme.origin == ThemeOrigin::Builtin && scheme.id.as_str() == ThemeId::DEFAULT_LIGHT;

    let (primary, success, warning, danger, info, background) = if is_canonical_max_dark {
        (
            canonical_max_primary_scale(),
            canonical_max_success_scale(),
            canonical_max_warning_scale(),
            canonical_max_danger_scale(),
            canonical_max_info_scale(),
            canonical_max_dark_background_ramp(),
        )
    } else if is_canonical_max_light {
        (
            canonical_max_primary_scale(),
            canonical_max_success_scale(),
            canonical_max_warning_scale(),
            canonical_max_danger_scale(),
            canonical_max_info_scale(),
            canonical_max_light_background_ramp(),
        )
    } else {
        let primary_seed = select_primary_seed(scheme);
        let primary = generate_scale(primary_seed, scheme.polarity);
        let success = generate_scale(scheme.ansi.green, scheme.polarity);
        let warning = generate_scale(scheme.ansi.yellow, scheme.polarity);
        let danger = generate_scale(scheme.ansi.red, scheme.polarity);
        let info = generate_scale(select_info_seed(scheme), scheme.polarity);
        let background =
            generate_background_ramp(scheme.background, scheme.foreground, scheme.polarity);
        (primary, success, warning, danger, info, background)
    };

    let tab_active_accent = scheme.active_tab_accent.unwrap_or(primary.p500);
    let scrollbar_thumb = RgbaColor::from_hex_with_alpha(background.b200, 0.45);
    let find_match_bg = RgbaColor::from_hex_with_alpha(warning.p500, 0.32);
    let block_bg = background.b25;
    let block_border = background.b75;
    let block_error_bg = RgbaColor::from_hex_with_alpha(danger.p500, 0.14);

    let terminal = TerminalUiTokens {
        bg: scheme.background,
        fg: scheme.foreground,
        cursor: scheme.cursor,
        cursor_accent: scheme.cursor_accent,
        selection_bg: scheme.selection_background,
        selection_fg: scheme.selection_foreground,
        tab_active_accent,
        scrollbar_thumb,
        find_match_bg,
        block_bg,
        block_border,
        block_error_bg,
    };

    MaxUiTokens {
        polarity: scheme.polarity,
        primary,
        success,
        warning,
        danger,
        info,
        background,
        terminal,
    }
}

fn select_primary_seed(scheme: &ColorScheme) -> HexColor {
    if let Some(accent) = scheme.active_tab_accent {
        if Oklch::from_srgb(accent).c >= 0.04 {
            return accent;
        }
    }
    let cyan_c = Oklch::from_srgb(scheme.ansi.cyan).c;
    let blue_c = Oklch::from_srgb(scheme.ansi.blue).c;
    if cyan_c >= 0.04 && cyan_c >= blue_c {
        scheme.ansi.cyan
    } else if blue_c >= 0.04 {
        scheme.ansi.blue
    } else {
        HexColor::new(0x00, 0x76, 0x8e)
    }
}

fn select_info_seed(scheme: &ColorScheme) -> HexColor {
    let bright_blue_c = Oklch::from_srgb(scheme.ansi.bright_blue).c;
    if bright_blue_c >= 0.04 {
        scheme.ansi.bright_blue
    } else {
        scheme.ansi.cyan
    }
}

const fn canonical_max_primary_scale() -> ColorScale11 {
    ColorScale11 {
        p50: HexColor::new(0x67, 0xc8, 0xdb),
        p100: HexColor::new(0x56, 0xc2, 0xd7),
        p200: HexColor::new(0x46, 0xbc, 0xd4),
        p300: HexColor::new(0x2e, 0xa4, 0xbc),
        p400: HexColor::new(0x17, 0x8d, 0xa5),
        p500: HexColor::new(0x00, 0x76, 0x8e),
        p600: HexColor::new(0x00, 0x5f, 0x77),
        p700: HexColor::new(0x00, 0x48, 0x60),
        p800: HexColor::new(0x00, 0x30, 0x48),
        p900: HexColor::new(0x00, 0x19, 0x31),
        p950: HexColor::new(0x00, 0x15, 0x2a),
        content: HexColor::new(0xff, 0xff, 0xff),
    }
}

fn canonical_max_success_scale() -> ColorScale11 {
    generate_scale(HexColor::new(0x10, 0xb9, 0x81), ThemePolarity::Dark)
}

fn canonical_max_warning_scale() -> ColorScale11 {
    generate_scale(HexColor::new(0xf5, 0x9e, 0x0b), ThemePolarity::Dark)
}

fn canonical_max_danger_scale() -> ColorScale11 {
    generate_scale(HexColor::new(0xef, 0x44, 0x44), ThemePolarity::Dark)
}

fn canonical_max_info_scale() -> ColorScale11 {
    generate_scale(HexColor::new(0x0e, 0xa5, 0xe9), ThemePolarity::Dark)
}

const fn canonical_max_dark_background_ramp() -> BackgroundRamp {
    BackgroundRamp {
        b0: HexColor::new(0x17, 0x29, 0x3d),
        b25: HexColor::new(0x23, 0x35, 0x49),
        b50: HexColor::new(0x2f, 0x41, 0x55),
        b75: HexColor::new(0x3b, 0x4d, 0x61),
        b100: HexColor::new(0x47, 0x59, 0x6d),
        b125: HexColor::new(0x52, 0x64, 0x78),
        b150: HexColor::new(0x5e, 0x70, 0x84),
        b175: HexColor::new(0x6a, 0x7c, 0x90),
        b200: HexColor::new(0x76, 0x88, 0x9c),
        b225: HexColor::new(0x82, 0x94, 0xa8),
        b250: HexColor::new(0x8e, 0x9f, 0xb4),
        b275: HexColor::new(0x99, 0xab, 0xc0),
        b300: HexColor::new(0xa3, 0xb2, 0xc4),
        b325: HexColor::new(0xac, 0xba, 0xcb),
        b350: HexColor::new(0xb6, 0xc3, 0xd2),
        b375: HexColor::new(0xbf, 0xcb, 0xd9),
        b400: HexColor::new(0xc9, 0xd4, 0xe0),
        b425: HexColor::new(0xcb, 0xd5, 0xe1),
        b450: HexColor::new(0xd1, 0xda, 0xe5),
        b475: HexColor::new(0xd7, 0xdf, 0xe8),
        b500: HexColor::new(0xdd, 0xe4, 0xec),
        b525: HexColor::new(0xe2, 0xe8, 0xf0),
        b550: HexColor::new(0xe5, 0xea, 0xf1),
        b575: HexColor::new(0xe8, 0xed, 0xf3),
        b600: HexColor::new(0xeb, 0xf0, 0xf5),
        b625: HexColor::new(0xee, 0xf2, 0xf7),
        b650: HexColor::new(0xf1, 0xf5, 0xf9),
        b675: HexColor::new(0xf3, 0xf6, 0xfa),
        b700: HexColor::new(0xf4, 0xf7, 0xfa),
        b725: HexColor::new(0xf6, 0xf9, 0xfb),
        b750: HexColor::new(0xf8, 0xfa, 0xfc),
        b775: HexColor::new(0xf9, 0xfb, 0xfc),
        b800: HexColor::new(0xfb, 0xfc, 0xfd),
        b825: HexColor::new(0xfa, 0xfb, 0xfc),
        b850: HexColor::new(0xff, 0xff, 0xff),
        b875: HexColor::new(0xff, 0xff, 0xff),
        b900: HexColor::new(0xff, 0xff, 0xff),
        b925: HexColor::new(0xff, 0xff, 0xff),
        b950: HexColor::new(0xff, 0xff, 0xff),
    }
}

const fn canonical_max_light_background_ramp() -> BackgroundRamp {
    BackgroundRamp {
        b0: HexColor::new(0xff, 0xff, 0xff),
        b25: HexColor::new(0xfa, 0xfb, 0xfc),
        b50: HexColor::new(0xfb, 0xfc, 0xfd),
        b75: HexColor::new(0xf9, 0xfb, 0xfc),
        b100: HexColor::new(0xf8, 0xfa, 0xfc),
        b125: HexColor::new(0xf6, 0xf9, 0xfb),
        b150: HexColor::new(0xf4, 0xf7, 0xfa),
        b175: HexColor::new(0xf3, 0xf6, 0xfa),
        b200: HexColor::new(0xf1, 0xf5, 0xf9),
        b225: HexColor::new(0xee, 0xf2, 0xf7),
        b250: HexColor::new(0xeb, 0xf0, 0xf5),
        b275: HexColor::new(0xe8, 0xed, 0xf3),
        b300: HexColor::new(0xe5, 0xea, 0xf1),
        b325: HexColor::new(0xe2, 0xe8, 0xf0),
        b350: HexColor::new(0xdd, 0xe4, 0xec),
        b375: HexColor::new(0xd7, 0xdf, 0xe8),
        b400: HexColor::new(0xd1, 0xda, 0xe5),
        b425: HexColor::new(0xcb, 0xd5, 0xe1),
        b450: HexColor::new(0xc9, 0xd4, 0xe0),
        b475: HexColor::new(0xbf, 0xcb, 0xd9),
        b500: HexColor::new(0xb6, 0xc3, 0xd2),
        b525: HexColor::new(0xac, 0xba, 0xcb),
        b550: HexColor::new(0xa3, 0xb2, 0xc4),
        b575: HexColor::new(0x99, 0xab, 0xc0),
        b600: HexColor::new(0x8e, 0x9f, 0xb4),
        b625: HexColor::new(0x82, 0x94, 0xa8),
        b650: HexColor::new(0x76, 0x88, 0x9c),
        b675: HexColor::new(0x6a, 0x7c, 0x90),
        b700: HexColor::new(0x5e, 0x70, 0x84),
        b725: HexColor::new(0x52, 0x64, 0x78),
        b750: HexColor::new(0x47, 0x59, 0x6d),
        b775: HexColor::new(0x3b, 0x4d, 0x61),
        b800: HexColor::new(0x2f, 0x41, 0x55),
        b825: HexColor::new(0x23, 0x35, 0x49),
        b850: HexColor::new(0x17, 0x29, 0x3d),
        b875: HexColor::new(0x0c, 0x1e, 0x32),
        b900: HexColor::new(0x00, 0x12, 0x26),
        b925: HexColor::new(0x00, 0x0c, 0x1a),
        b950: HexColor::new(0x00, 0x06, 0x0e),
    }
}
