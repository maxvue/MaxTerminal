//! Testes de determinismo e invariantes das rampas `BackgroundRamp` e `ColorScale11` nos 6 temas nativos.

#![allow(clippy::unwrap_used)]

use mt_themes::{
    HexColor, Oklch, ThemeId, ThemePolarity, builtin_resolved_themes, find_builtin_resolved,
    generate_background_ramp, generate_scale,
};
use rstest::rstest;

#[rstest]
#[case("max-navy-teal")]
#[case("max-light")]
#[case("dracula")]
#[case("nord")]
#[case("solarized-dark")]
#[case("solarized-light")]
fn builtin_derived_tokens_are_deterministic_for_each_builtin(#[case] id_str: &str) {
    let id = ThemeId::parse(id_str).unwrap();
    let a = find_builtin_resolved(&id).unwrap();
    let b = mt_themes::ResolvedTheme::from_scheme(a.scheme.clone());
    assert_eq!(a.ui_tokens, b.ui_tokens);
    assert_eq!(a.xterm, b.xterm);
    assert_eq!(a.scheme.compute_content_sha256().len(), 64);
}

#[test]
fn background_ramp_steps_cover_all_39_positions_from_0_to_950() {
    for rt in builtin_resolved_themes() {
        let steps = rt.ui_tokens.background.steps();
        assert_eq!(steps.len(), 39);
        assert_eq!(steps[0].0, 0);
        assert_eq!(steps[38].0, 950);
        for (i, (step_num, _)) in steps.iter().enumerate() {
            assert_eq!(usize::from(*step_num), i * 25);
        }
    }
}

#[test]
fn custom_dark_and_light_background_ramps_follow_polarity_direction() {
    let dark_ramp = generate_background_ramp(
        HexColor::parse("#1a1b26").unwrap(),
        HexColor::parse("#c0caf5").unwrap(),
        ThemePolarity::Dark,
    );
    assert_eq!(dark_ramp.b0, HexColor::parse("#1a1b26").unwrap());
    assert_eq!(dark_ramp.b850, HexColor::parse("#ffffff").unwrap());
    assert_eq!(dark_ramp.b950, HexColor::parse("#ffffff").unwrap());
    assert!(Oklch::from_srgb(dark_ramp.b200).l > Oklch::from_srgb(dark_ramp.b25).l);

    let light_ramp = generate_background_ramp(
        HexColor::parse("#fafafa").unwrap(),
        HexColor::parse("#1e293b").unwrap(),
        ThemePolarity::Light,
    );
    assert_eq!(light_ramp.b0, HexColor::parse("#fafafa").unwrap());
    assert!(Oklch::from_srgb(light_ramp.b900).l < Oklch::from_srgb(light_ramp.b100).l);
}

#[rstest]
#[case("#000000")]
#[case("#ffffff")]
#[case("#808080")]
#[case("#ff0000")]
#[case("#00ff00")]
#[case("#0000ff")]
#[case("#00768e")]
#[case("#bd93f9")]
#[case("#88c0d0")]
#[case("#f59e0b")]
fn scale_generation_handles_seed_monotonically(#[case] hex: &str) {
    let c = HexColor::parse(hex).unwrap();
    let s = generate_scale(c, ThemePolarity::Dark);
    let steps = s.steps();
    for pair in steps.windows(2) {
        let l1 = Oklch::from_srgb(pair[0].1).l;
        let l2 = Oklch::from_srgb(pair[1].1).l;
        assert!(l1 > l2, "Seed {hex} falhou monotonicidade: {l1} <= {l2}");
    }
}
