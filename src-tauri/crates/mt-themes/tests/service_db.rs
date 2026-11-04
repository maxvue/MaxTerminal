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
    LibraryChangeReason, NoopThemeEventSink, OverwritePolicy, SkipReason, ThemeDraftInput,
    ThemeError, ThemeId, ThemeOrigin, ThemePolarity, ThemeScope, ThemeService,
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

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn create_update_and_duplicate_themes_preserve_metadata_and_persist_to_sqlite() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("crud_themes_test.db"))).unwrap();
    let sink = Arc::new(RecordingSink::default());
    let svc = ThemeService::new(db.themes(), sink.clone()).unwrap();

    let base_navy = svc
        .get_resolved(&ThemeId::parse("max-navy-teal").unwrap())
        .unwrap();

    // 1. Criar tema customizado com canal alfa na seleção e indexed_colors
    let mut draft = ThemeDraftInput::from_scheme(&base_navy.scheme);
    draft.name = String::from("Oceano Profundo");
    draft.author = Some(String::from("Equipe Max"));
    draft.cursor_style = CursorStyle::Underline;
    draft.selection_background = String::from("#22d3ee66");
    draft.selection_foreground = Some(String::from("#f8fafc"));
    draft.active_tab_accent = Some(String::from("#14b8a6"));
    draft.indexed_colors.insert(16, String::from("#112233"));
    draft.indexed_colors.insert(231, String::from("#ddeeff"));

    let created = svc.create_theme(draft.clone()).unwrap();
    assert_eq!(created.scheme.id.as_str(), "oceano-profundo");
    assert_eq!(created.scheme.name, "Oceano Profundo");
    assert_eq!(created.scheme.origin, ThemeOrigin::Custom);
    assert_eq!(created.scheme.polarity, ThemePolarity::Dark);
    assert_eq!(created.scheme.cursor_style, CursorStyle::Underline);
    assert_eq!(
        created.scheme.selection_background.to_hex8_string(),
        "#22d3ee66"
    );
    assert_eq!(
        created
            .scheme
            .selection_foreground
            .map(mt_themes::HexColor::to_hex_string),
        Some(String::from("#f8fafc"))
    );
    assert_eq!(
        created
            .scheme
            .active_tab_accent
            .map(mt_themes::HexColor::to_hex_string),
        Some(String::from("#14b8a6"))
    );
    assert_eq!(created.scheme.indexed_colors.len(), 2);
    assert_eq!(
        created.scheme.indexed_colors[&16].to_hex_string(),
        "#112233"
    );
    assert_eq!(
        sink.library.lock().last().unwrap().reason,
        LibraryChangeReason::Created
    );

    // 2. Nomes diferentes que geram o mesmo slug base recebem desambiguação automática (-2)
    let mut slug_collision_draft = draft.clone();
    slug_collision_draft.name = String::from("Oceano---Profundo");
    let created_slug2 = svc.create_theme(slug_collision_draft).unwrap();
    assert_eq!(created_slug2.scheme.id.as_str(), "oceano-profundo-2");

    // 3. Conflito de nome case-insensitive ao criar deve falhar sem emitir evento nem alterar cache
    let lib_events_before = sink.library.lock().len();
    let mut dup_name_draft = draft.clone();
    dup_name_draft.name = String::from("oceano profundo");
    let conflict_err = svc.create_theme(dup_name_draft).unwrap_err();
    assert!(matches!(conflict_err, ThemeError::Conflict { .. }));
    assert_eq!(sink.library.lock().len(), lib_events_before);

    // 4. Contraste insuficiente (< 3.0:1) deve falhar sem sujar cache
    let mut low_contrast_draft = draft.clone();
    low_contrast_draft.name = String::from("Contraste Ruim");
    low_contrast_draft.background = String::from("#101418");
    low_contrast_draft.foreground = String::from("#181c20");
    let contrast_err = svc.create_theme(low_contrast_draft).unwrap_err();
    assert!(matches!(
        contrast_err,
        ThemeError::InsufficientContrast { .. }
    ));
    assert_eq!(sink.library.lock().len(), lib_events_before);

    // 5. Atualizar tema builtin deve ser rejeitado com BuiltinReadOnly
    let builtin_err = svc
        .update_theme(&ThemeId::parse("max-navy-teal").unwrap(), draft.clone())
        .unwrap_err();
    assert!(matches!(builtin_err, ThemeError::BuiltinReadOnly { .. }));

    // 6. Atualizar tema customizado (convertendo para claro e mudando cores) preserva ID, origin, created_at e indexed_colors
    let mut update_draft = ThemeDraftInput::from_scheme(&created.scheme);
    update_draft.name = String::from("Oceano Claro");
    update_draft.background = String::from("#f8fafc");
    update_draft.foreground = String::from("#0f172a");
    update_draft.cursor = String::from("#0284c7");
    update_draft.cursor_accent = String::from("#f8fafc");

    let updated = svc.update_theme(&created.scheme.id, update_draft).unwrap();
    assert_eq!(updated.scheme.id, created.scheme.id);
    assert_eq!(updated.scheme.origin, ThemeOrigin::Custom);
    assert_eq!(updated.scheme.polarity, ThemePolarity::Light);
    assert_eq!(
        updated.scheme.metadata.created_at,
        created.scheme.metadata.created_at
    );
    assert!(updated.scheme.metadata.updated_at >= created.scheme.metadata.updated_at);
    assert_ne!(
        updated.scheme.metadata.sha256,
        created.scheme.metadata.sha256
    );
    assert_eq!(updated.scheme.indexed_colors.len(), 2);
    assert_eq!(
        sink.library.lock().last().unwrap().reason,
        LibraryChangeReason::Updated
    );
    // Verifica que o cache LRU foi invalidado e get_resolved retorna o novo fundo claro
    assert_eq!(
        svc.get_resolved(&created.scheme.id)
            .unwrap()
            .scheme
            .background
            .to_hex_string(),
        "#f8fafc"
    );

    // 7. Importar tema e editá-lo preserva origin == Imported, source_format e source_filename
    let import_rep = svc
        .import_file(
            &fixture_path("tabby/dracula-scheme.yaml"),
            ImportFileOptions::default(),
        )
        .unwrap();
    let imported_id = import_rep.imported[0].id.clone();
    let imported_before = svc.get_resolved(&imported_id).unwrap();
    assert_eq!(imported_before.scheme.origin, ThemeOrigin::Imported);
    assert!(imported_before.scheme.metadata.source_format.is_some());
    assert!(imported_before.scheme.metadata.source_filename.is_some());

    let mut imported_edit = ThemeDraftInput::from_scheme(&imported_before.scheme);
    imported_edit.cursor = String::from("#22d3ee");
    let imported_after = svc.update_theme(&imported_id, imported_edit).unwrap();
    assert_eq!(imported_after.scheme.origin, ThemeOrigin::Imported);
    assert_eq!(
        imported_after.scheme.metadata.source_format,
        imported_before.scheme.metadata.source_format
    );
    assert_eq!(
        imported_after.scheme.metadata.source_filename,
        imported_before.scheme.metadata.source_filename
    );
    assert_eq!(imported_after.scheme.cursor.to_hex_string(), "#22d3ee");

    // 8. Duplicar tema builtin e duplicar duas vezes seguidas mesmo com SHA-256 idêntico
    let dup1 = svc
        .duplicate_theme(&ThemeId::parse("max-navy-teal").unwrap())
        .unwrap();
    assert_eq!(dup1.scheme.name, "Max Navy/Teal (cópia)");
    assert_eq!(dup1.scheme.origin, ThemeOrigin::Custom);
    assert_eq!(
        dup1.scheme.metadata.sha256,
        base_navy.scheme.metadata.sha256
    );
    assert!(dup1.scheme.metadata.source_format.is_none());
    assert!(dup1.scheme.metadata.source_filename.is_none());
    assert_eq!(
        sink.library.lock().last().unwrap().reason,
        LibraryChangeReason::Duplicated
    );

    let dup2 = svc
        .duplicate_theme(&ThemeId::parse("max-navy-teal").unwrap())
        .unwrap();
    assert_eq!(dup2.scheme.name, "Max Navy/Teal (cópia 2)");
    assert_ne!(dup2.scheme.id, dup1.scheme.id);
    assert_eq!(
        dup2.scheme.metadata.sha256,
        base_navy.scheme.metadata.sha256
    );

    // Duplicar tema customizado preserva indexed_colors e selection_background com alfa
    let dup_custom = svc.duplicate_theme(&updated.scheme.id).unwrap();
    assert_eq!(dup_custom.scheme.name, "Oceano Claro (cópia)");
    assert_eq!(dup_custom.scheme.indexed_colors.len(), 2);
    assert_eq!(
        dup_custom.scheme.selection_background.to_hex8_string(),
        "#22d3ee66"
    );

    // 9. Reabrir ThemeService do zero confirma persistência completa no SQLite
    let svc_reloaded = ThemeService::new(db.themes(), Arc::new(NoopThemeEventSink)).unwrap();
    let reloaded_oceano = svc_reloaded.get_resolved(&updated.scheme.id).unwrap();
    assert_eq!(reloaded_oceano.scheme.name, "Oceano Claro");
    assert_eq!(reloaded_oceano.scheme.indexed_colors.len(), 2);
    assert!(svc_reloaded.exists(&dup1.scheme.id));
    assert!(svc_reloaded.exists(&dup2.scheme.id));
    assert!(svc_reloaded.exists(&dup_custom.scheme.id));

    db.shutdown().await.unwrap();
}
