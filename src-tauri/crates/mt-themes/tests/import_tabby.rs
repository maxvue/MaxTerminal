//! Testes de integração para os 5 formatos de importação, exportação round-trip e defesas adversariais YAML/FS.

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;

use mt_themes::{
    ExportFormat, SchemeFileFormat, ThemeError, ThemeOrigin, WarningCode, export_scheme,
    import::read_safe_theme_file, normalize_raw_scheme, parse_schemes_by_format,
    validate_safe_yaml,
};
use rstest::rstest;
use tempfile::TempDir;

fn fixture_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

#[test]
fn parses_tabby_full_config_yaml_and_ignores_ssh_secrets() {
    let raw_yaml = fs::read_to_string(fixture_path("tabby/config-full.yaml")).unwrap();
    let (fmt, schemes) = parse_schemes_by_format(&raw_yaml, Some("config.yaml"), None).unwrap();
    assert_eq!(fmt, SchemeFileFormat::TabbyConfigYaml);
    assert_eq!(schemes.len(), 3);
    assert_eq!(schemes[0].name, "Tabby Active Teal");
    assert_eq!(schemes[1].name, "Custom Gruvbox Dark");
    assert_eq!(schemes[2].name, "Eight Colors Legacy");

    let norm_legacy =
        normalize_raw_scheme(schemes[2].clone(), ThemeOrigin::Imported, None, None, 1000).unwrap();
    assert!(
        norm_legacy
            .warnings
            .iter()
            .any(|w| w.code == WarningCode::SynthesizedBrightPalette)
    );
}

#[test]
fn parses_tabby_standalone_yaml_and_array() {
    let single = fs::read_to_string(fixture_path("tabby/dracula-scheme.yaml")).unwrap();
    let (fmt1, s1) = parse_schemes_by_format(&single, Some("dracula.yaml"), None).unwrap();
    assert_eq!(fmt1, SchemeFileFormat::TabbySchemeYaml);
    assert_eq!(s1.len(), 1);
    let norm =
        normalize_raw_scheme(s1[0].clone(), ThemeOrigin::Imported, None, None, 1000).unwrap();
    assert_eq!(norm.scheme.id.as_str(), "dracula-custom");

    let arr = fs::read_to_string(fixture_path("tabby/schemes-array.yaml")).unwrap();
    let (fmt2, s2) = parse_schemes_by_format(&arr, Some("schemes.yaml"), None).unwrap();
    assert_eq!(fmt2, SchemeFileFormat::TabbySchemeYaml);
    assert_eq!(s2.len(), 2);
}

#[test]
#[allow(clippy::similar_names)]
fn parses_tabby_json_windows_terminal_json_and_xterm_json() {
    let tj = fs::read_to_string(fixture_path("tabby/scheme.json")).unwrap();
    let (fmt_tj, s_tj) = parse_schemes_by_format(&tj, Some("scheme.json"), None).unwrap();
    assert_eq!(fmt_tj, SchemeFileFormat::TabbySchemeJson);
    assert_eq!(s_tj[0].name, "Monokai Pro JSON");

    let wt = fs::read_to_string(fixture_path("windows-terminal/one-half-dark.json")).unwrap();
    let (fmt_wt, s_wt) = parse_schemes_by_format(&wt, Some("one-half-dark.json"), None).unwrap();
    assert_eq!(fmt_wt, SchemeFileFormat::WindowsTerminalJson);
    let norm_wt =
        normalize_raw_scheme(s_wt[0].clone(), ThemeOrigin::Imported, None, None, 1000).unwrap();
    assert_eq!(norm_wt.scheme.ansi.magenta.to_hex_string(), "#c678dd");

    let xt = fs::read_to_string(fixture_path("xterm/theme.json")).unwrap();
    let (fmt_xt, s_xt) = parse_schemes_by_format(&xt, Some("theme.json"), None).unwrap();
    assert_eq!(fmt_xt, SchemeFileFormat::XtermThemeJson);
    assert_eq!(s_xt[0].name, "Ayu Mirage Xterm");
}

#[rstest]
#[case(ExportFormat::TabbyYaml)]
#[case(ExportFormat::TabbyJson)]
#[case(ExportFormat::WindowsTerminalJson)]
#[case(ExportFormat::XtermJson)]
fn export_and_reimport_roundtrip_preserves_colors(#[case] export_fmt: ExportFormat) {
    let single = fs::read_to_string(fixture_path("tabby/dracula-scheme.yaml")).unwrap();
    let (_, mut raws) = parse_schemes_by_format(&single, Some("dracula.yaml"), None).unwrap();
    let original = normalize_raw_scheme(raws.remove(0), ThemeOrigin::Imported, None, None, 1000)
        .unwrap()
        .scheme;

    let exported = export_scheme(&original, export_fmt).unwrap();
    let (_, mut re_raws) = parse_schemes_by_format(&exported, None, None).unwrap();
    let re_norm = normalize_raw_scheme(re_raws.remove(0), ThemeOrigin::Imported, None, None, 2000)
        .unwrap()
        .scheme;
    assert_eq!(re_norm.foreground, original.foreground);
    assert_eq!(re_norm.background, original.background);
    assert_eq!(re_norm.cursor, original.cursor);
    assert_eq!(re_norm.ansi, original.ansi);
}

#[rstest]
#[case("billion-laughs.yaml")]
#[case("merge-keys.yaml")]
#[case("custom-tag.yaml")]
fn rejects_adversarial_yaml_constructs(#[case] file: &str) {
    let content = fs::read_to_string(fixture_path(&format!("adversarial/{file}"))).unwrap();
    let err = parse_schemes_by_format(&content, Some(file), None).unwrap_err();
    assert!(
        matches!(err, ThemeError::UnsafeYamlConstruct { .. }),
        "Esperava UnsafeYamlConstruct para {file}, obteve {err:?}"
    );
}

#[test]
fn rejects_too_many_yaml_documents_and_deep_nesting() {
    let many_docs = "---\na: 1\n---\nb: 2\n---\nc: 3\n---\nd: 4\n";
    assert!(matches!(
        validate_safe_yaml(many_docs),
        Err(ThemeError::UnsafeYamlConstruct { .. })
    ));

    let deep = "[[[[[[[[[[[[[[[[[[1]]]]]]]]]]]]]]]]]]";
    assert!(matches!(
        validate_safe_yaml(deep),
        Err(ThemeError::UnsafeYamlConstruct { .. })
    ));
}

#[test]
fn rejects_low_contrast_scheme() {
    let low = fs::read_to_string(fixture_path("adversarial/low-contrast.yaml")).unwrap();
    let (_, mut raws) = parse_schemes_by_format(&low, Some("low.yaml"), None).unwrap();
    let err =
        normalize_raw_scheme(raws.remove(0), ThemeOrigin::Imported, None, None, 1000).unwrap_err();
    assert!(matches!(err, ThemeError::InsufficientContrast { .. }));
}

#[test]
fn rejects_oversized_files_and_symlinks() {
    let dir = TempDir::new().unwrap();
    let big_path = dir.path().join("huge.yaml");
    let big_data = vec![b'a'; 600 * 1024];
    fs::write(&big_path, big_data).unwrap();

    let err = read_safe_theme_file(&big_path).unwrap_err();
    assert!(matches!(err, ThemeError::FileTooLarge { .. }));

    #[cfg(unix)]
    {
        let target = fixture_path("tabby/dracula-scheme.yaml");
        let link_path = dir.path().join("symlink.yaml");
        std::os::unix::fs::symlink(&target, &link_path).unwrap();
        let link_err = read_safe_theme_file(&link_path).unwrap_err();
        assert!(matches!(link_err, ThemeError::UnsafePath { .. }));
    }
}
