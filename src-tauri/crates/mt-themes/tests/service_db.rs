//! Testes de integração de `ThemeService` + `ThemesRepo` (SQLite real em `TempDir`).

#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use mt_core::ids::ProjectId;
use mt_db::{
    Db, DbConfig, ProjectMutation, StoredNewProject, StoredProject, sanitizer::SanitizedText,
};
use mt_themes::{
    AppearanceSettingsPatch, CursorStyle, ExportFormat, ImportDirOptions, ImportFileOptions,
    NoopThemeEventSink, OverwritePolicy, SkipReason, ThemeError, ThemeId, ThemeScope, ThemeService,
    validate_font_family,
};
use parking_lot::Mutex;
use tempfile::TempDir;

#[derive(Default)]
struct RecordingSink {
    changed: Mutex<Vec<mt_themes::ThemeChangedEvent>>,
    library: Mutex<Vec<mt_themes::ThemeLibraryChangedEvent>>,
}

impl mt_themes::ThemeEventSink for RecordingSink {
    fn emit_theme_changed(&self, payload: mt_themes::ThemeChangedEvent) {
        self.changed.lock().push(payload);
    }
    fn emit_library_changed(&self, payload: mt_themes::ThemeLibraryChangedEvent) {
        self.library.lock().push(payload);
    }
}

fn fixture_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn theme_service_persists_imports_appearance_and_project_overrides() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("themes_test.db"))).unwrap();
    let sink = Arc::new(RecordingSink::default());
    let svc = ThemeService::new(db.themes(), sink.clone()).unwrap();

    // Inicialmente possui os 6 temas nativos e max-navy-teal ativo
    let initial_list = svc.list_summaries();
    assert_eq!(initial_list.len(), 6);
    assert_eq!(
        svc.appearance_get().active_theme_id.as_str(),
        "max-navy-teal"
    );

    // Importa arquivo Tabby com activate_if_single
    let report = svc
        .import_file(
            &fixture_path("tabby/dracula-scheme.yaml"),
            ImportFileOptions {
                format_hint: None,
                dry_run: false,
                overwrite_policy: OverwritePolicy::RenameWithSuffix,
                activate_if_single: true,
            },
        )
        .unwrap();
    assert_eq!(report.imported.len(), 1);
    assert_eq!(report.imported[0].id.as_str(), "dracula-custom");
    assert_eq!(
        svc.appearance_get().active_theme_id.as_str(),
        "dracula-custom"
    );
    assert_eq!(sink.library.lock().len(), 1);
    assert_eq!(sink.changed.lock().len(), 1);

    // Reimportar o mesmo arquivo com RenameWithSuffix detecta SHA-256 idêntico e pula
    let dup_report = svc
        .import_file(
            &fixture_path("tabby/dracula-scheme.yaml"),
            ImportFileOptions::default(),
        )
        .unwrap();
    assert_eq!(dup_report.imported.len(), 0);
    assert_eq!(dup_report.skipped.len(), 1);
    assert_eq!(
        dup_report.skipped[0].reason,
        SkipReason::IdenticalAlreadyExists
    );

    // Atualiza configurações de fonte e cursor
    let updated = svc
        .appearance_update(AppearanceSettingsPatch {
            font_size: Some(15),
            line_height: Some(1.35),
            cursor_style_override: Some(Some(CursorStyle::Bar)),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(updated.font_size, 15);
    assert_eq!(updated.cursor_style_override, Some(CursorStyle::Bar));

    // Rejeita fonte com tentativa de injeção CSS
    let bad_font = svc
        .appearance_update(AppearanceSettingsPatch {
            font_family: Some("JetBrains Mono; body { display: none }".to_string()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(bad_font, ThemeError::InvalidFont { .. }));

    // Cria um projeto e vincula override de tema por projeto
    let proj_id = ProjectId::new();
    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj_id,
                title: SanitizedText::new("App Teste"),
                icon: String::from("logos:vue"),
                color: String::from("#10B981"),
                path: String::from("/tmp/app-teste"),
                root_id: None,
                stack: String::from("vue"),
                identity_source: String::from("auto"),
                sort_order: 0,
                missing: false,
                created_at: 1000,
                updated_at: 1000,
            },
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap();

    let nord_id = ThemeId::parse("nord").unwrap();
    svc.set_active(nord_id.clone(), ThemeScope::Project(proj_id))
        .unwrap();
    assert_eq!(
        svc.appearance_get().project_overrides.get(&proj_id),
        Some(&nord_id)
    );

    // Recria o serviço do zero a partir do banco para validar persistência
    let svc2 = ThemeService::new(db.themes(), Arc::new(NoopThemeEventSink)).unwrap();
    assert_eq!(svc2.list_summaries().len(), 7);
    assert_eq!(
        svc2.appearance_get().active_theme_id.as_str(),
        "dracula-custom"
    );
    assert_eq!(
        svc2.appearance_get().project_overrides.get(&proj_id),
        Some(&nord_id)
    );

    // Excluir tema builtin deve falhar
    let err_builtin = svc2
        .delete_theme(&ThemeId::parse("max-navy-teal").unwrap(), None)
        .unwrap_err();
    assert!(matches!(err_builtin, ThemeError::BuiltinReadOnly { .. }));

    // Excluir tema custom ativo reverte automaticamente para max-navy-teal
    svc2.delete_theme(&ThemeId::parse("dracula-custom").unwrap(), None)
        .unwrap();
    assert_eq!(
        svc2.appearance_get().active_theme_id.as_str(),
        "max-navy-teal"
    );

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn import_dir_scans_valid_files_and_isolates_invalid_ones() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("dir_test.db"))).unwrap();
    let svc = ThemeService::new(db.themes(), Arc::new(NoopThemeEventSink)).unwrap();

    let report = svc
        .import_dir(&fixture_path("tabby"), ImportDirOptions::default())
        .unwrap();
    assert!(report.imported.len() >= 5);
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn overwrite_policies_and_dry_run_behave_as_specified() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("policy_test.db"))).unwrap();
    let svc = ThemeService::new(db.themes(), Arc::new(NoopThemeEventSink)).unwrap();

    // Dry run não persiste
    let dry = svc
        .import_file(
            &fixture_path("tabby/dracula-scheme.yaml"),
            ImportFileOptions {
                dry_run: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(dry.imported.len(), 1);
    assert_eq!(svc.list_summaries().len(), 6);

    // Importa de verdade
    svc.import_file(
        &fixture_path("tabby/dracula-scheme.yaml"),
        ImportFileOptions::default(),
    )
    .unwrap();
    assert_eq!(svc.list_summaries().len(), 7);

    // Cria variante com mesmo nome ("Dracula Custom") mas cor de cursor diferente
    let modified_path = tmp.path().join("dracula-mod.yaml");
    let orig = fs::read_to_string(fixture_path("tabby/dracula-scheme.yaml")).unwrap();
    let modified = orig.replace("cursor: '#ff79c6'", "cursor: '#50fa7b'");
    fs::write(&modified_path, modified).unwrap();

    // Policy Reject deve falhar com Conflict
    let err_reject = svc
        .import_file(
            &modified_path,
            ImportFileOptions {
                overwrite_policy: OverwritePolicy::Reject,
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(err_reject, ThemeError::Conflict { .. }));

    // Policy SkipExisting ignora
    let skip_rep = svc
        .import_file(
            &modified_path,
            ImportFileOptions {
                overwrite_policy: OverwritePolicy::SkipExisting,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(skip_rep.skipped.len(), 1);
    assert_eq!(skip_rep.skipped[0].reason, SkipReason::ExistingIdProtected);

    // Policy OverwriteImported atualiza no mesmo ID
    let ov_rep = svc
        .import_file(
            &modified_path,
            ImportFileOptions {
                overwrite_policy: OverwritePolicy::OverwriteImported,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(ov_rep.imported.len(), 1);
    assert_eq!(ov_rep.imported[0].id.as_str(), "dracula-custom");
    assert_eq!(ov_rep.imported[0].cursor, "#50fa7b");

    // Exporta via ThemeService
    let exported_yaml = svc
        .export_theme(
            &ThemeId::parse("dracula-custom").unwrap(),
            ExportFormat::TabbyYaml,
        )
        .unwrap();
    assert!(exported_yaml.contains("#50fa7b"));

    db.shutdown().await.unwrap();
}

#[test]
fn font_family_validation_accepts_safe_stacks_and_rejects_injections() {
    assert!(validate_font_family("JetBrains Mono, 'Fira Code', monospace").is_ok());
    assert!(validate_font_family("").is_err());
    assert!(validate_font_family("Mono; color: red").is_err());
    assert!(validate_font_family("url(evil)").is_err());
}
