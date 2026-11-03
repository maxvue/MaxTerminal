//! Testes de `OsSettings`, `Accelerator`, `AutostartSync` e preservação de campos desconhecidos (`mt-os`).

#![allow(clippy::unwrap_used)]

use std::path::PathBuf;

use mt_os::{
    Accelerator, AutostartSync, CloseBehavior, OsSettings, OsSettingsPatch, QuakeSettingsPatch,
    codes,
};
use tempfile::TempDir;

#[test]
fn settings_defaults_patch_diff_validation_and_unknown_fields_preserved() {
    let (defaults, raw) = OsSettings::from_persisted_row(1, "{}").unwrap();
    assert_eq!(defaults.close_behavior, CloseBehavior::Auto);
    assert_eq!(defaults.quake.accelerator, "Ctrl+Alt+Backquote");
    assert_eq!(defaults.quake.drop_down_height_pct, 45);
    assert!(raw.is_empty());

    // Patch válido produz SettingsDiff esperado
    let (updated, diff) = defaults
        .apply_patch(OsSettingsPatch {
            close_behavior: Some(CloseBehavior::AlwaysTray),
            quake: Some(QuakeSettingsPatch {
                accelerator: Some(String::from("ctrl + shift + f12")),
                drop_down_height_pct: Some(60),
                ..Default::default()
            }),
            autostart: Some(true),
            ..Default::default()
        })
        .unwrap();

    assert!(diff.shortcut);
    assert!(diff.quake_geometry);
    assert!(diff.autostart);
    assert!(!diff.editor);
    assert_eq!(updated.quake.accelerator, "Ctrl+Shift+F12");

    // Faixa inválida de altura (< 20%)
    let bad_height = defaults
        .apply_patch(OsSettingsPatch {
            quake: Some(QuakeSettingsPatch {
                drop_down_height_pct: Some(10),
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap_err();
    assert_eq!(bad_height.code.0, codes::SETTINGS_INVALID);

    // Schema futuro deve ser recusado sem sobrescrever
    let future_err = OsSettings::from_persisted_row(2, "{}").unwrap_err();
    assert_eq!(future_err.code.0, codes::SETTINGS_INVALID);

    // Campos desconhecidos no JSON são preservados no merge
    let (s_extra, raw_extra) = OsSettings::from_persisted_row(
        1,
        "{\"futureFlag\":true,\"closeBehavior\":\"always_quit\"}",
    )
    .unwrap();
    assert_eq!(s_extra.close_behavior, CloseBehavior::AlwaysQuit);
    let merged = s_extra.to_merged_json(&raw_extra).unwrap();
    assert!(merged.contains("\"futureFlag\":true"));
}

#[test]
fn accelerator_parses_formats_and_rejects_reserved_combos() {
    let acc = Accelerator::parse("Ctrl+Alt+`").unwrap();
    assert_eq!(acc.to_canonical_string(), "Ctrl+Alt+Backquote");
    assert_eq!(acc.to_portal_trigger(), "CTRL+ALT+grave");

    // Combos reservados de terminal ou sem modificador são rejeitados
    assert!(Accelerator::parse("Ctrl+C").is_err());
    assert!(Accelerator::parse("Ctrl+Shift+C").is_err());
    assert!(Accelerator::parse("F12").is_err());
    assert!(Accelerator::parse("Ctrl+").is_err());
    assert!(Accelerator::parse("Foo+A").is_err());
}

#[test]
fn autostart_sync_writes_and_reconciles_start_minimized_arg() {
    let tmp = TempDir::new().unwrap();
    let sync = AutostartSync::new(
        tmp.path().join("autostart"),
        PathBuf::from("/opt/MaxTerminal/maxterminal"),
    );

    assert!(!sync.status().unwrap().enabled);

    // Habilita com start_minimized = true -> inclui --start-hidden
    let st1 = sync.apply(true, true).unwrap();
    assert!(st1.enabled);
    let content1 = std::fs::read_to_string(sync.desktop_file_path()).unwrap();
    assert!(content1.contains("Exec=/opt/MaxTerminal/maxterminal --start-hidden\n"));

    // Alterna start_minimized = false -> remove --start-hidden
    let st2 = sync.apply(true, false).unwrap();
    assert!(st2.enabled);
    let content2 = std::fs::read_to_string(sync.desktop_file_path()).unwrap();
    assert!(content2.contains("Exec=/opt/MaxTerminal/maxterminal\n"));
    assert!(!content2.contains("--start-hidden"));

    // Desabilita -> remove arquivo
    let st3 = sync.apply(false, false).unwrap();
    assert!(!st3.enabled);
    assert!(!sync.desktop_file_path().exists());
}
