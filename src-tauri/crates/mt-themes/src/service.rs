//! Serviço central de temas e aparência (`ThemeService`), eventos de domínio e validação de fontes.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mt_core::ids::ProjectId;
use mt_db::{StoredAppearanceRow, StoredColorSchemeRow, ThemesRepo};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::builtin::{
    builtin_resolved_themes, builtin_schemes, find_builtin_resolved, is_builtin_theme_id,
};
use crate::cache::ThemeCache;
use crate::error::ThemeError;
use crate::export::{ExportFormat, export_scheme};
use crate::ids::{ThemeId, ThemeOrigin, ThemePolarity};
use crate::import::{
    ImportDirOptions, ImportFailure, ImportFileOptions, ImportReport, OverwritePolicy, SkipReason,
    SkippedEntry, ThemeSummary, collect_candidate_theme_files, parse_and_normalize_file,
};
use crate::mapping::ResolvedTheme;
use crate::scheme::{ColorScheme, CursorStyle};

/// Configurações globais de aparência e tipografia do terminal + overrides por projeto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    /// Tema global atualmente ativo.
    pub active_theme_id: ThemeId,
    /// Se o tema deve alternar automaticamente conforme o modo claro/escuro do sistema.
    pub follow_system: bool,
    /// Tema preferido quando o sistema está em modo escuro.
    pub preferred_dark_theme_id: ThemeId,
    /// Tema preferido quando o sistema está em modo claro.
    pub preferred_light_theme_id: ThemeId,
    /// Família de fontes monoespaçadas do terminal.
    pub font_family: String,
    /// Tamanho da fonte em pixels (`8..=32`).
    pub font_size: u8,
    /// Altura de linha (`1.0..=2.0`).
    pub line_height: f32,
    /// Se ligaduras tipográficas estão habilitadas.
    pub font_ligatures: bool,
    /// Se o cursor pisca.
    pub cursor_blink: bool,
    /// Sobrescrita opcional de estilo de cursor (`None` usa o estilo definido pelo tema).
    pub cursor_style_override: Option<CursorStyle>,
    /// Razão mínima de contraste aplicada pelo `@xterm/xterm` (`1.0..=21.0`).
    pub minimum_contrast_ratio: f32,
    /// Mapa de overrides de tema por projeto (`ProjectId -> ThemeId`).
    pub project_overrides: BTreeMap<ProjectId, ThemeId>,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        let dark = ThemeId::parse(ThemeId::DEFAULT_DARK).unwrap_or_else(|_| unreachable!());
        let light = ThemeId::parse(ThemeId::DEFAULT_LIGHT).unwrap_or_else(|_| unreachable!());
        Self {
            active_theme_id: dark.clone(),
            follow_system: false,
            preferred_dark_theme_id: dark,
            preferred_light_theme_id: light,
            font_family: String::from("JetBrains Mono, Fira Code, Cascadia Code, monospace"),
            font_size: 13,
            line_height: 1.2,
            font_ligatures: true,
            cursor_blink: true,
            cursor_style_override: None,
            minimum_contrast_ratio: 1.0,
            project_overrides: BTreeMap::new(),
        }
    }
}

/// Patch parcial para atualização atômica de `AppearanceSettings`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettingsPatch {
    /// Novo tema global ativo.
    pub active_theme_id: Option<ThemeId>,
    /// Ativar/desativar acompanhamento do modo do sistema.
    pub follow_system: Option<bool>,
    /// Novo tema escuro preferido.
    pub preferred_dark_theme_id: Option<ThemeId>,
    /// Novo tema claro preferido.
    pub preferred_light_theme_id: Option<ThemeId>,
    /// Nova pilha de fontes monoespaçadas.
    pub font_family: Option<String>,
    /// Novo tamanho de fonte (`8..=32`).
    pub font_size: Option<u8>,
    /// Nova altura de linha (`1.0..=2.0`).
    pub line_height: Option<f32>,
    /// Habilitar/desabilitar ligaduras.
    pub font_ligatures: Option<bool>,
    /// Habilitar/desabilitar cursor piscante.
    pub cursor_blink: Option<bool>,
    /// Atualizar `cursor_style_override` (`Some(Some(style))` define, `Some(None)` limpa).
    pub cursor_style_override: Option<Option<CursorStyle>>,
    /// Nova razão mínima de contraste (`1.0..=21.0`).
    pub minimum_contrast_ratio: Option<f32>,
}

/// Escopo da troca de tema solicitada em `theme_set_active`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", content = "projectId", rename_all = "snake_case")]
pub enum ThemeScope {
    /// Altera o tema global do aplicativo.
    Global,
    /// Define um tema específico para o projeto informado.
    Project(ProjectId),
    /// Remove o override do projeto informado, voltando a herdar o tema global.
    ClearProject(ProjectId),
}

/// Evento emitido quando o tema ativo (global ou de projeto) ou configurações de aparência mudam (`theme://changed`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeChangedEvent {
    /// Escopo que originou a mudança.
    pub scope: ThemeScope,
    /// Tema resolvido correspondente.
    pub resolved: ResolvedTheme,
    /// Estado atualizado das configurações de aparência.
    pub appearance: AppearanceSettings,
}

/// Motivo de alteração na biblioteca de esquemas (`theme://library-changed`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum LibraryChangeReason {
    /// Um ou mais temas foram importados.
    Imported,
    /// Um tema foi excluído.
    Deleted,
}

/// Evento emitido quando o catálogo de temas disponíveis muda (`theme://library-changed`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ThemeLibraryChangedEvent {
    /// Lista atualizada de resumos de todos os temas (`builtin` + `imported` + `custom`).
    pub themes: Vec<ThemeSummary>,
    /// Motivo da atualização.
    pub reason: LibraryChangeReason,
}

/// Sink abstrato para emissão de eventos Tauri do subsistema de temas.
pub trait ThemeEventSink: Send + Sync + 'static {
    /// Emite `theme://changed`.
    fn emit_theme_changed(&self, payload: ThemeChangedEvent);
    /// Emite `theme://library-changed`.
    fn emit_library_changed(&self, payload: ThemeLibraryChangedEvent);
}

/// Implementação no-op de `ThemeEventSink` para testes unitários e de integração sem runtime Tauri.
#[derive(Debug, Default)]
pub struct NoopThemeEventSink;

impl ThemeEventSink for NoopThemeEventSink {
    fn emit_theme_changed(&self, _payload: ThemeChangedEvent) {}
    fn emit_library_changed(&self, _payload: ThemeLibraryChangedEvent) {}
}

/// Serviço principal de gerenciamento de temas, importação Tabby e aparência do terminal.
pub struct ThemeService {
    repo: ThemesRepo,
    cache: ThemeCache,
    sink: Arc<dyn ThemeEventSink>,
}

impl std::fmt::Debug for ThemeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThemeService")
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

impl ThemeService {
    /// Inicializa o `ThemeService` carregando esquemas customizados e configurações do SQLite.
    ///
    /// Caso o `active_theme_id` salvo aponte para um tema inexistente, restaura automaticamente
    /// para `max-navy-teal`.
    pub fn new(repo: ThemesRepo, sink: Arc<dyn ThemeEventSink>) -> Result<Arc<Self>, ThemeError> {
        let svc = Arc::new(Self {
            repo,
            cache: ThemeCache::new(),
            sink,
        });
        svc.reload_from_db()?;
        Ok(svc)
    }

    /// Recarrega o cache em memória a partir do SQLite.
    pub fn reload_from_db(&self) -> Result<(), ThemeError> {
        let rows = self.repo.list_schemes_blocking()?;
        let mut custom_map = BTreeMap::new();
        for row in rows {
            if let Ok(scheme) = serde_json::from_str::<ColorScheme>(&row.payload_json) {
                if !is_builtin_theme_id(&scheme.id) {
                    custom_map.insert(scheme.id.clone(), scheme);
                }
            }
        }

        let (app_row, override_rows) = self.repo.get_appearance_and_overrides_blocking()?;
        let default_dark = ThemeId::parse(ThemeId::DEFAULT_DARK)?;
        let default_light = ThemeId::parse(ThemeId::DEFAULT_LIGHT)?;

        let parse_or_fallback = |raw: &str, fallback: &ThemeId| -> ThemeId {
            ThemeId::parse(raw)
                .ok()
                .filter(|id| is_builtin_theme_id(id) || custom_map.contains_key(id))
                .unwrap_or_else(|| fallback.clone())
        };

        let active_theme_id = parse_or_fallback(&app_row.active_theme_id, &default_dark);
        let preferred_dark_theme_id =
            parse_or_fallback(&app_row.preferred_dark_theme_id, &default_dark);
        let preferred_light_theme_id =
            parse_or_fallback(&app_row.preferred_light_theme_id, &default_light);

        let cursor_style_override = match app_row.cursor_style_override.as_deref() {
            Some("block") => Some(CursorStyle::Block),
            Some("underline") => Some(CursorStyle::Underline),
            Some("bar") => Some(CursorStyle::Bar),
            _ => None,
        };

        let mut project_overrides = BTreeMap::new();
        for ov in override_rows {
            if let (Ok(pid), Ok(tid)) = (
                ov.project_id.parse::<ProjectId>(),
                ThemeId::parse(&ov.theme_id),
            ) {
                if is_builtin_theme_id(&tid) || custom_map.contains_key(&tid) {
                    project_overrides.insert(pid, tid);
                }
            }
        }

        let appearance = AppearanceSettings {
            active_theme_id,
            follow_system: app_row.follow_system,
            preferred_dark_theme_id,
            preferred_light_theme_id,
            font_family: validate_font_family(&app_row.font_family).unwrap_or_else(|_| {
                String::from("JetBrains Mono, Fira Code, Cascadia Code, monospace")
            }),
            font_size: app_row.font_size.clamp(8, 32),
            line_height: app_row.line_height.clamp(1.0, 2.0),
            font_ligatures: app_row.font_ligatures,
            cursor_blink: app_row.cursor_blink,
            cursor_style_override,
            minimum_contrast_ratio: app_row.minimum_contrast_ratio.clamp(1.0, 21.0),
            project_overrides,
        };

        self.cache.replace_all(custom_map, appearance);
        Ok(())
    }

    /// Lista o resumo (`ThemeSummary`) de todos os temas disponíveis:
    /// os 6 nativos primeiro (na ordem canônica), seguidos pelos importados/customizados em ordem alfabética.
    #[must_use]
    pub fn list_summaries(&self) -> Vec<ThemeSummary> {
        let mut out: Vec<ThemeSummary> = builtin_resolved_themes()
            .iter()
            .map(|r| ThemeSummary::from_scheme(&r.scheme))
            .collect();

        let mut customs = self.cache.list_custom_schemes();
        customs.sort_by(|a, b| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.id.cmp(&b.id))
        });
        out.extend(customs.iter().map(ThemeSummary::from_scheme));
        out
    }

    /// Obtém o `ResolvedTheme` completo pelo `ThemeId` (resolvido em `< 1 ms` via cache/builtin).
    pub fn get_resolved(&self, id: &ThemeId) -> Result<ResolvedTheme, ThemeError> {
        if let Some(builtin) = find_builtin_resolved(id) {
            return Ok(builtin.clone());
        }
        if let Some(scheme) = self.cache.get_custom_scheme(id) {
            return Ok(self.cache.get_or_resolve(&scheme));
        }
        Err(ThemeError::NotFound {
            id: id.as_str().to_string(),
        })
    }

    /// Verifica se um `ThemeId` existe no catálogo.
    #[must_use]
    pub fn exists(&self, id: &ThemeId) -> bool {
        is_builtin_theme_id(id) || self.cache.get_custom_scheme(id).is_some()
    }

    /// Retorna as configurações atuais de aparência (`AppearanceSettings`).
    #[must_use]
    pub fn appearance_get(&self) -> AppearanceSettings {
        self.cache.appearance()
    }

    /// Atualiza as configurações de aparência aplicando validação estrita de fonte, tamanhos e IDs de tema.
    pub fn appearance_update(
        &self,
        patch: AppearanceSettingsPatch,
    ) -> Result<AppearanceSettings, ThemeError> {
        let mut current = self.cache.appearance();

        if let Some(active_id) = patch.active_theme_id {
            if !self.exists(&active_id) {
                return Err(ThemeError::NotFound {
                    id: active_id.to_string(),
                });
            }
            current.active_theme_id = active_id;
        }

        if let Some(follow) = patch.follow_system {
            current.follow_system = follow;
        }

        if let Some(dark_id) = patch.preferred_dark_theme_id {
            if !self.exists(&dark_id) {
                return Err(ThemeError::NotFound {
                    id: dark_id.to_string(),
                });
            }
            current.preferred_dark_theme_id = dark_id;
        }

        if let Some(light_id) = patch.preferred_light_theme_id {
            if !self.exists(&light_id) {
                return Err(ThemeError::NotFound {
                    id: light_id.to_string(),
                });
            }
            current.preferred_light_theme_id = light_id;
        }

        if let Some(font) = patch.font_family {
            current.font_family = validate_font_family(&font)?;
        }

        if let Some(size) = patch.font_size {
            if !(8..=32).contains(&size) {
                return Err(ThemeError::InvalidField {
                    field: "fontSize".to_string(),
                    reason: format!(
                        "Tamanho de fonte deve estar entre 8 e 32 px (recebido: {size})"
                    ),
                });
            }
            current.font_size = size;
        }

        if let Some(lh) = patch.line_height {
            if !lh.is_finite() || !(1.0..=2.0).contains(&lh) {
                return Err(ThemeError::InvalidField {
                    field: "lineHeight".to_string(),
                    reason: format!("Altura de linha deve estar entre 1.0 e 2.0 (recebido: {lh})"),
                });
            }
            current.line_height = lh;
        }

        if let Some(lig) = patch.font_ligatures {
            current.font_ligatures = lig;
        }

        if let Some(blink) = patch.cursor_blink {
            current.cursor_blink = blink;
        }

        if let Some(cursor_override) = patch.cursor_style_override {
            current.cursor_style_override = cursor_override;
        }

        if let Some(contrast) = patch.minimum_contrast_ratio {
            if !contrast.is_finite() || !(1.0..=21.0).contains(&contrast) {
                return Err(ThemeError::InvalidField {
                    field: "minimumContrastRatio".to_string(),
                    reason: format!(
                        "Contraste mínimo deve estar entre 1.0 e 21.0 (recebido: {contrast})"
                    ),
                });
            }
            current.minimum_contrast_ratio = contrast;
        }

        self.persist_appearance(&current)?;
        self.cache.set_appearance(current.clone());

        let resolved = self.get_resolved(&current.active_theme_id)?;
        self.sink.emit_theme_changed(ThemeChangedEvent {
            scope: ThemeScope::Global,
            resolved,
            appearance: current.clone(),
        });

        Ok(current)
    }

    /// Define o tema ativo global ou o override de um projeto específico (`ThemeScope`).
    pub fn set_active(&self, id: ThemeId, scope: ThemeScope) -> Result<ResolvedTheme, ThemeError> {
        let resolved = self.get_resolved(&id)?;
        let mut current = self.cache.appearance();

        match &scope {
            ThemeScope::Global => {
                current.active_theme_id = id.clone();
                match resolved.scheme.polarity {
                    ThemePolarity::Dark => current.preferred_dark_theme_id = id,
                    ThemePolarity::Light => current.preferred_light_theme_id = id,
                }
                self.persist_appearance(&current)?;
            }
            ThemeScope::Project(project_id) => {
                self.repo
                    .set_project_override_blocking(&project_id.to_string(), Some(id.as_str()))?;
                current.project_overrides.insert(*project_id, id);
            }
            ThemeScope::ClearProject(project_id) => {
                self.repo
                    .set_project_override_blocking(&project_id.to_string(), None)?;
                current.project_overrides.remove(project_id);
            }
        }

        self.cache.set_appearance(current.clone());
        let effective_resolved = match &scope {
            ThemeScope::ClearProject(_) => self.get_resolved(&current.active_theme_id)?,
            _ => resolved,
        };

        self.sink.emit_theme_changed(ThemeChangedEvent {
            scope,
            resolved: effective_resolved.clone(),
            appearance: current,
        });

        Ok(effective_resolved)
    }

    /// Remove um tema `imported` ou `custom`, revertendo o tema ativo para `fallback_id` (ou `max-navy-teal`)
    /// caso o tema removido estivesse ativo.
    pub fn delete_theme(
        &self,
        id: &ThemeId,
        fallback_id: Option<ThemeId>,
    ) -> Result<(), ThemeError> {
        if is_builtin_theme_id(id) {
            return Err(ThemeError::BuiltinReadOnly { id: id.to_string() });
        }

        if !self.exists(id) {
            return Err(ThemeError::NotFound { id: id.to_string() });
        }

        let current = self.cache.appearance();
        let was_active = &current.active_theme_id == id
            || current.project_overrides.values().any(|tid| tid == id);

        let fallback = match fallback_id {
            Some(fb) => {
                if &fb == id || !self.exists(&fb) {
                    return Err(ThemeError::CannotDeleteActive { id: id.to_string() });
                }
                fb
            }
            None => ThemeId::parse(ThemeId::DEFAULT_DARK)?,
        };

        self.repo
            .delete_scheme_with_fallback_blocking(id.as_str(), fallback.as_str())?;
        self.cache.remove_custom_scheme(id);
        self.reload_from_db()?;

        self.sink.emit_library_changed(ThemeLibraryChangedEvent {
            themes: self.list_summaries(),
            reason: LibraryChangeReason::Deleted,
        });

        if was_active {
            let updated_app = self.cache.appearance();
            let resolved = self.get_resolved(&updated_app.active_theme_id)?;
            self.sink.emit_theme_changed(ThemeChangedEvent {
                scope: ThemeScope::Global,
                resolved,
                appearance: updated_app,
            });
        }

        Ok(())
    }

    /// Exporta um esquema existente no catálogo para YAML ou JSON.
    pub fn export_theme(&self, id: &ThemeId, format: ExportFormat) -> Result<String, ThemeError> {
        let resolved = self.get_resolved(id)?;
        export_scheme(&resolved.scheme, format)
    }

    /// Importa um arquivo de esquema de cores aplicando normalização, deduplicação e `OverwritePolicy`.
    pub fn import_file(
        &self,
        path: &Path,
        options: ImportFileOptions,
    ) -> Result<ImportReport, ThemeError> {
        let now_ms = mt_db::time::now_unix_ms();
        let batch = parse_and_normalize_file(path, options.format_hint, now_ms)?;
        let report = self.apply_import_batch(
            batch.schemes,
            batch.warnings,
            batch.errors,
            options.overwrite_policy,
            options.dry_run,
        )?;

        if !options.dry_run && options.activate_if_single && report.imported.len() == 1 {
            let imported_id = report.imported[0].id.clone();
            let _ = self.set_active(imported_id, ThemeScope::Global)?;
        }

        Ok(report)
    }

    /// Importa todos os arquivos `.yaml`, `.yml` e `.json` de um diretório (até `MAX_FILES_PER_DIR_IMPORT`).
    pub fn import_dir(
        &self,
        dir_path: &Path,
        options: ImportDirOptions,
    ) -> Result<ImportReport, ThemeError> {
        let files = collect_candidate_theme_files(dir_path)?;
        let now_ms = mt_db::time::now_unix_ms();

        let mut all_schemes = Vec::new();
        let mut all_warnings = Vec::new();
        let mut all_errors = Vec::new();

        for file in files {
            let fname = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();
            match parse_and_normalize_file(&file, None, now_ms) {
                Ok(batch) => {
                    all_schemes.extend(batch.schemes);
                    all_warnings.extend(batch.warnings);
                    all_errors.extend(batch.errors);
                }
                Err(err) => {
                    all_errors.push(ImportFailure {
                        source: fname,
                        error: err,
                    });
                }
            }
        }

        self.apply_import_batch(
            all_schemes,
            all_warnings,
            all_errors,
            options.overwrite_policy,
            options.dry_run,
        )
    }

    #[allow(clippy::too_many_lines)]
    fn apply_import_batch(
        &self,
        candidates: Vec<ColorScheme>,
        warnings: Vec<crate::normalize::ImportWarning>,
        mut errors: Vec<ImportFailure>,
        policy: OverwritePolicy,
        dry_run: bool,
    ) -> Result<ImportReport, ThemeError> {
        let mut existing_by_id: BTreeMap<ThemeId, ColorScheme> = BTreeMap::new();
        let mut existing_by_name_lower: BTreeMap<String, ThemeId> = BTreeMap::new();
        let mut existing_by_sha: BTreeMap<String, ThemeId> = BTreeMap::new();

        for b in builtin_schemes() {
            existing_by_name_lower.insert(b.name.to_lowercase(), b.id.clone());
            existing_by_sha.insert(b.metadata.sha256.clone(), b.id.clone());
            existing_by_id.insert(b.id.clone(), b);
        }
        for c in self.cache.list_custom_schemes() {
            existing_by_name_lower.insert(c.name.to_lowercase(), c.id.clone());
            existing_by_sha.insert(c.metadata.sha256.clone(), c.id.clone());
            existing_by_id.insert(c.id.clone(), c);
        }

        let mut imported_summaries = Vec::new();
        let mut skipped = Vec::new();
        let mut rows_to_persist = Vec::new();
        let mut schemes_to_cache = Vec::new();

        for mut candidate in candidates {
            if let Some(dup_id) = existing_by_sha.get(&candidate.metadata.sha256) {
                if policy == OverwritePolicy::RenameWithSuffix
                    || policy == OverwritePolicy::SkipExisting
                {
                    skipped.push(SkippedEntry {
                        name: candidate.name.clone(),
                        existing_id: dup_id.clone(),
                        reason: SkipReason::IdenticalAlreadyExists,
                    });
                    continue;
                }
            }

            let id_collision = existing_by_id.get(&candidate.id).cloned();
            let name_collision_id = existing_by_name_lower
                .get(&candidate.name.to_lowercase())
                .cloned();
            let colliding_scheme = id_collision.or_else(|| {
                name_collision_id
                    .as_ref()
                    .and_then(|cid| existing_by_id.get(cid).cloned())
            });

            if let Some(existing) = colliding_scheme {
                match policy {
                    OverwritePolicy::Reject => {
                        return Err(ThemeError::Conflict {
                            id: existing.id.to_string(),
                        });
                    }
                    OverwritePolicy::SkipExisting => {
                        skipped.push(SkippedEntry {
                            name: candidate.name.clone(),
                            existing_id: existing.id,
                            reason: SkipReason::ExistingIdProtected,
                        });
                        continue;
                    }
                    OverwritePolicy::OverwriteImported => {
                        if existing.origin == ThemeOrigin::Builtin {
                            skipped.push(SkippedEntry {
                                name: candidate.name.clone(),
                                existing_id: existing.id,
                                reason: SkipReason::BuiltinProtected,
                            });
                            continue;
                        }
                        candidate.id = existing.id.clone();
                        candidate.metadata.created_at = existing.metadata.created_at;
                    }
                    OverwritePolicy::RenameWithSuffix => {
                        let short = &candidate.metadata.sha256[..8];
                        candidate.id = candidate.id.with_short_hash(&candidate.metadata.sha256);
                        let suffix = format!(" ({})", &short[..6]);
                        let max_name_chars = 80usize.saturating_sub(suffix.chars().count());
                        let base_name: String =
                            candidate.name.chars().take(max_name_chars).collect();
                        candidate.name = format!("{base_name}{suffix}");
                    }
                }
            }

            let payload_json = match serde_json::to_string(&candidate) {
                Ok(j) => j,
                Err(e) => {
                    errors.push(ImportFailure {
                        source: candidate.name.clone(),
                        error: ThemeError::UnsupportedFormat {
                            reason: e.to_string(),
                        },
                    });
                    continue;
                }
            };

            rows_to_persist.push(StoredColorSchemeRow {
                id: candidate.id.as_str().to_string(),
                name: candidate.name.clone(),
                origin: candidate.origin.as_str().to_string(),
                polarity: candidate.polarity.as_str().to_string(),
                author: candidate.metadata.author.clone(),
                source_format: candidate.metadata.source_format.clone(),
                source_filename: candidate.metadata.source_filename.clone(),
                sha256: candidate.metadata.sha256.clone(),
                payload_json,
                created_at: candidate.metadata.created_at,
                updated_at: candidate.metadata.updated_at,
            });

            existing_by_name_lower.insert(candidate.name.to_lowercase(), candidate.id.clone());
            existing_by_sha.insert(candidate.metadata.sha256.clone(), candidate.id.clone());
            existing_by_id.insert(candidate.id.clone(), candidate.clone());
            imported_summaries.push(ThemeSummary::from_scheme(&candidate));
            schemes_to_cache.push(candidate);
        }

        if !dry_run && !rows_to_persist.is_empty() {
            self.repo.upsert_schemes_blocking(rows_to_persist)?;
            for s in schemes_to_cache {
                self.cache.put_custom_scheme(s);
            }
            self.sink.emit_library_changed(ThemeLibraryChangedEvent {
                themes: self.list_summaries(),
                reason: LibraryChangeReason::Imported,
            });
        }

        Ok(ImportReport {
            imported: imported_summaries,
            skipped,
            warnings,
            errors,
        })
    }

    fn persist_appearance(&self, current: &AppearanceSettings) -> Result<(), ThemeError> {
        let row = StoredAppearanceRow {
            active_theme_id: current.active_theme_id.as_str().to_string(),
            follow_system: current.follow_system,
            preferred_dark_theme_id: current.preferred_dark_theme_id.as_str().to_string(),
            preferred_light_theme_id: current.preferred_light_theme_id.as_str().to_string(),
            font_family: current.font_family.clone(),
            font_size: current.font_size,
            line_height: current.line_height,
            font_ligatures: current.font_ligatures,
            cursor_blink: current.cursor_blink,
            cursor_style_override: current
                .cursor_style_override
                .map(|c| c.as_xterm_str().to_string()),
            minimum_contrast_ratio: current.minimum_contrast_ratio,
            updated_at: 0,
        };
        self.repo.save_appearance_blocking(row)?;
        Ok(())
    }
}

/// Valida uma pilha CSS de fontes monoespaçadas (`font_family`), rejeitando caracteres de injeção CSS (`;`, `{`, `}`, `<`, `>`, `\`, `/`, `(` ou `)`).
pub fn validate_font_family(raw: &str) -> Result<String, ThemeError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 200 {
        return Err(ThemeError::InvalidFont {
            family: raw.to_string(),
            reason: "A lista de fontes deve ter entre 1 e 200 caracteres".to_string(),
        });
    }

    for ch in trimmed.chars() {
        let allowed =
            ch.is_ascii_alphanumeric() || matches!(ch, ' ' | ',' | '-' | '_' | '\'' | '"');
        if !allowed {
            return Err(ThemeError::InvalidFont {
                family: raw.to_string(),
                reason: format!("Caractere não permitido na família de fontes: '{ch}'"),
            });
        }
    }

    Ok(trimmed.to_string())
}
