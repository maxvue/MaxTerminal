//! Testes de integração de fidelidade de tokens `MaxComponentsUi`, escalas OKLCH e `XtermThemePayload`.

#![allow(clippy::unwrap_used)]

use mt_themes::{
    BUILTIN_THEME_IDS, BackgroundRamp, ColorScale11, CursorStyle, HexColor, Oklch, RgbaColor,
    ThemeId, ThemeOrigin, ThemePolarity, builtin_resolved_themes, classify_polarity,
    contrast_ratio, ensure_min_contrast, find_builtin_resolved, hsl_to_srgb, linear_to_srgb_u8,
    parse_color_with_bg, pick_content_color, relative_luminance, srgb_u8_to_linear,
};
use rstest::rstest;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CanonicalTokensFixture {
    primary: ColorScale11,
    background: BackgroundRamp,
}

#[test]
fn max_navy_teal_matches_canonical_max_components_ui_tokens_bit_for_bit() {
    let fixture_json = include_str!("fixtures/max-components-ui/dark-tokens.json");
    let expected: CanonicalTokensFixture = serde_json::from_str(fixture_json).unwrap();

    let id = ThemeId::parse("max-navy-teal").unwrap();
    let resolved = find_builtin_resolved(&id).unwrap();

    assert_eq!(resolved.ui_tokens.polarity, ThemePolarity::Dark);
    assert_eq!(resolved.ui_tokens.primary, expected.primary);
    assert_eq!(resolved.ui_tokens.background, expected.background);
    assert_eq!(
        resolved.ui_tokens.success.p500,
        HexColor::parse("#10b981").unwrap()
    );
    assert_eq!(
        resolved.ui_tokens.warning.p500,
        HexColor::parse("#f59e0b").unwrap()
    );
    assert_eq!(
        resolved.ui_tokens.danger.p500,
        HexColor::parse("#ef4444").unwrap()
    );
    assert_eq!(
        resolved.ui_tokens.info.p500,
        HexColor::parse("#0ea5e9").unwrap()
    );
}

#[test]
fn max_light_matches_canonical_max_components_ui_tokens_bit_for_bit() {
    let fixture_json = include_str!("fixtures/max-components-ui/light-tokens.json");
    let expected: CanonicalTokensFixture = serde_json::from_str(fixture_json).unwrap();

    let id = ThemeId::parse("max-light").unwrap();
    let resolved = find_builtin_resolved(&id).unwrap();

    assert_eq!(resolved.ui_tokens.polarity, ThemePolarity::Light);
    assert_eq!(resolved.ui_tokens.primary, expected.primary);
    assert_eq!(resolved.ui_tokens.background, expected.background);
}

#[test]
fn all_six_builtin_themes_load_with_valid_scales_and_contrast() {
    let builtins = builtin_resolved_themes();
    assert_eq!(builtins.len(), 6);

    for (idx, expected_id) in BUILTIN_THEME_IDS.iter().enumerate() {
        let rt = &builtins[idx];
        assert_eq!(rt.scheme.id.as_str(), *expected_id);
        let ratio = contrast_ratio(rt.scheme.foreground, rt.scheme.background);
        assert!(
            ratio >= 4.5,
            "Builtin {} deve ter contraste >= 4.5:1 (obteve {ratio})",
            rt.scheme.id
        );

        for (family_name, scale) in [
            ("primary", &rt.ui_tokens.primary),
            ("success", &rt.ui_tokens.success),
            ("warning", &rt.ui_tokens.warning),
            ("danger", &rt.ui_tokens.danger),
            ("info", &rt.ui_tokens.info),
        ] {
            let steps = scale.steps();
            for pair in steps.windows(2) {
                let l_a = Oklch::from_srgb(pair[0].1).l;
                let l_b = Oklch::from_srgb(pair[1].1).l;
                assert!(
                    l_a >= l_b,
                    "Tema {} família {family_name} não monotônica entre {} ({l_a}) e {} ({l_b})",
                    rt.scheme.id,
                    pair[0].0,
                    pair[1].0
                );
            }
            let content_ratio = contrast_ratio(scale.content, scale.p500);
            assert!(
                content_ratio >= 4.5,
                "Tema {} família {family_name} content ratio {content_ratio} < 4.5",
                rt.scheme.id
            );
        }
    }
}

#[rstest]
#[case("#000000", 0, 0, 0)]
#[case("#ffffff", 255, 255, 255)]
#[case("#0f0", 0, 255, 0)]
#[case("#123456", 0x12, 0x34, 0x56)]
#[case("rgb(10, 20, 30)", 10, 20, 30)]
#[case("rgb(100%, 0%, 50%)", 255, 0, 128)]
#[case("hsl(120, 100%, 50%)", 0, 255, 0)]
#[case("hsl(240deg, 100%, 50%)", 0, 0, 255)]
#[case("black", 0, 0, 0)]
#[case("red", 255, 0, 0)]
#[case("cyan", 0, 255, 255)]
#[case("magenta", 255, 0, 255)]
#[case("teal", 0, 128, 128)]
#[case("navy", 0, 0, 128)]
fn color_parser_accepts_valid_inputs(
    #[case] input: &str,
    #[case] r: u8,
    #[case] g: u8,
    #[case] b: u8,
) {
    let parsed = HexColor::parse(input).unwrap();
    assert_eq!(parsed, HexColor::new(r, g, b));
}

#[rstest]
#[case("")]
#[case("#1")]
#[case("#12345")]
#[case("#1234567")]
#[case("#123456789")]
#[case("#gggggg")]
#[case("rgb(256, 0, 0)")]
#[case("rgb(0, -1, 0)")]
#[case("rgba(0, 0, 0, 2.0)")]
#[case("hsl(0, 100, 50%)")]
#[case("hsl(0, 100%, 50)")]
#[case("unknowncolor")]
fn color_parser_rejects_invalid_inputs(#[case] input: &str) {
    assert!(HexColor::parse(input).is_err());
}

#[test]
fn rgba_color_formatting_and_alpha_helpers() {
    let c = RgbaColor::parse("#00768e80").unwrap();
    assert_eq!(c.r, 0x00);
    assert_eq!(c.g, 0x76);
    assert_eq!(c.b, 0x8e);
    assert_eq!(c.a, 0x80);
    assert_eq!(c.to_hex8_string(), "#00768e80");
    assert!(c.to_css_rgba().starts_with("rgba(0, 118, 142,"));

    let opaque = RgbaColor::new(10, 20, 30, 255);
    assert_eq!(opaque.to_css_rgba(), "#0a141e");
}

#[test]
fn ensure_min_contrast_adjusts_low_contrast_pair() {
    let bg = HexColor::parse("#0f172a").unwrap();
    let fg = HexColor::parse("#1e293b").unwrap();
    assert!(contrast_ratio(fg, bg) < 2.0);
    let adjusted = ensure_min_contrast(fg, bg, 4.5);
    assert!(contrast_ratio(adjusted, bg) >= 4.5);
}

#[test]
fn pick_content_color_selects_high_contrast_ink() {
    let dark_bg = HexColor::parse("#004860").unwrap();
    assert_eq!(pick_content_color(dark_bg), HexColor::new(255, 255, 255));

    let light_bg = HexColor::parse("#fbbf24").unwrap();
    assert_eq!(
        pick_content_color(light_bg),
        HexColor::new(0x0a, 0x12, 0x20)
    );
}

#[test]
fn theme_id_validation_and_slug_generation() {
    assert!(ThemeId::parse("max-navy-teal").is_ok());
    assert!(ThemeId::parse("dracula_v2").is_ok());
    assert!(ThemeId::parse("ab").is_err());
    assert!(ThemeId::parse("-bad").is_err());
    assert!(ThemeId::parse("bad-").is_err());
    assert!(ThemeId::parse("BadUpper").is_err());

    let slug = ThemeId::slug_from_name("Meu Tema Café & Pão!", "seed");
    assert_eq!(slug.as_str(), "meu-tema-cafe-pao");

    let short = ThemeId::slug_from_name("A", "seed123");
    assert!(short.as_str().len() >= 3);
}

#[test]
fn theme_origin_and_polarity_db_roundtrip() {
    for orig in [
        ThemeOrigin::Builtin,
        ThemeOrigin::Imported,
        ThemeOrigin::Custom,
    ] {
        assert_eq!(ThemeOrigin::parse_db(orig.as_str()).unwrap(), orig);
    }
    assert!(ThemeOrigin::parse_db("invalid").is_err());

    for pol in [ThemePolarity::Dark, ThemePolarity::Light] {
        assert_eq!(ThemePolarity::parse_db(pol.as_str()).unwrap(), pol);
    }
    assert!(ThemePolarity::parse_db("invalid").is_err());
}

#[test]
fn cursor_style_xterm_strings() {
    assert_eq!(CursorStyle::Block.as_xterm_str(), "block");
    assert_eq!(CursorStyle::Underline.as_xterm_str(), "underline");
    assert_eq!(CursorStyle::Bar.as_xterm_str(), "bar");
}

#[test]
fn srgb_linear_and_hsl_helpers() {
    assert_eq!(linear_to_srgb_u8(srgb_u8_to_linear(0)), 0);
    assert_eq!(linear_to_srgb_u8(srgb_u8_to_linear(128)), 128);
    assert_eq!(linear_to_srgb_u8(srgb_u8_to_linear(255)), 255);
    assert_eq!(hsl_to_srgb(0.0, 0.0, 0.0), HexColor::new(0, 0, 0));
    assert_eq!(hsl_to_srgb(0.0, 0.0, 1.0), HexColor::new(255, 255, 255));
    assert!(relative_luminance(HexColor::new(128, 128, 128)) > 0.2);
    assert_eq!(
        classify_polarity(HexColor::new(10, 10, 10)),
        ThemePolarity::Dark
    );
    assert_eq!(
        parse_color_with_bg("f", "#ff0000", None).unwrap(),
        HexColor::new(255, 0, 0)
    );
    assert!(parse_color_with_bg("f", "#ff000080", None).is_err());
}
