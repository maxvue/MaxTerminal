//! Comandos IPC Tauri para Comandos Rápidos Artisan (`>`) e Atalhos Universais (`/`) — Fase 14.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use mt_commands::artisan::generator::CommandGenerator;
use mt_commands::artisan::generator::form::{
    ArtisanCommandForm, ArtisanCreateResult, ArtisanPreview, ValidationReport, form_json_schema,
};
use mt_commands::artisan::model::{ArtisanCatalog, ArtisanRoot};
use mt_commands::artisan::root::RootResolver;
use mt_commands::artisan::runner::RunnerSelector;
use mt_commands::artisan::service::ArtisanService;
use mt_commands::ports::{CommandsEventSink, MaxCodeConfigLocator, ProjectLookup, SystemClock};
use mt_commands::shortcuts::maxcode_config::SystemMaxCodeConfigLocator;
use mt_commands::shortcuts::model::{ResolvedShortcut, ShortcutId, ShortcutList};
use mt_commands::shortcuts::service::ShortcutService;
use mt_core::{MtError, ProjectId};
use mt_db::Db;
use tauri::State;

/// Implementação de [`ProjectLookup`] apoiada na fachada [`Db`] do `MaxTerminal`.
pub struct DbProjectLookup {
    db: Db,
}

impl DbProjectLookup {
    /// Cria um novo [`DbProjectLookup`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    fn list_projects_sync(&self) -> Vec<(ProjectId, PathBuf)> {
        self.db
            .read_blocking(|conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, path FROM projects WHERE missing = 0 ORDER BY sort_order ASC",
                )?;
                let rows = stmt
                    .query_map([], |row| {
                        let id_str: String = row.get(0)?;
                        let path_str: String = row.get(1)?;
                        Ok((id_str, path_str))
                    })?
                    .filter_map(Result::ok)
                    .filter_map(|(id_s, path_s)| {
                        let pid = ProjectId::from_str(&id_s).ok()?;
                        let canon = std::fs::canonicalize(&path_s)
                            .unwrap_or_else(|_| PathBuf::from(&path_s));
                        Some((pid, canon))
                    })
                    .collect::<Vec<_>>();
                Ok(rows)
            })
            .unwrap_or_default()
    }
}

impl ProjectLookup for DbProjectLookup {
    fn owning_project(&self, path: &Path) -> Option<(ProjectId, PathBuf)> {
        let canon = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let projects = self.list_projects_sync();

        let mut best: Option<(usize, ProjectId, PathBuf)> = None;
        for (pid, proj_path) in &projects {
            if canon == *proj_path || canon.starts_with(proj_path) {
                let depth = proj_path.components().count();
                if best.as_ref().is_none_or(|(d, _, _)| depth > *d) {
                    best = Some((depth, *pid, proj_path.clone()));
                }
            }
        }
        if let Some((_, pid, proj_path)) = best {
            return Some((pid, proj_path));
        }

        // Verifica worktrees irmãs em `.worktrees/` ou `.max-code-worktrees/`
        let s = canon.to_string_lossy();
        if s.contains("/.worktrees/") || s.contains("/.max-code-worktrees/") {
            for (pid, proj_path) in &projects {
                if let Some(parent) = proj_path.parent()
                    && canon.starts_with(parent)
                {
                    return Some((*pid, canon));
                }
            }
        }

        None
    }

    fn project_by_id(&self, id: ProjectId) -> Option<PathBuf> {
        self.list_projects_sync()
            .into_iter()
            .find_map(|(pid, path)| (pid == id).then_some(path))
    }
}

/// Estado gerenciado pelo Tauri para Comandos Rápidos Artisan e Atalhos Universais (Fase 14 — D-09).
pub struct QuickCommandsState {
    /// Resolvedor de raízes Laravel com cache TTL.
    pub roots: Arc<RootResolver>,
    /// Serviço de descoberta e cache de catálogos Artisan.
    pub artisan: Arc<ArtisanService>,
    /// Gerador transacional de novos comandos Artisan.
    pub generator: Arc<CommandGenerator>,
    /// Serviço de resolução de atalhos universais (`/`).
    pub shortcuts: Arc<ShortcutService>,
    /// Seletor de runner (`php` local vs Laravel Sail).
    pub runners: Arc<RunnerSelector>,
}

impl QuickCommandsState {
    /// Constrói o estado completo da Fase 14.
    #[must_use]
    pub fn new(db: Db, sink: Arc<dyn CommandsEventSink>) -> Self {
        let lookup: Arc<dyn ProjectLookup> = Arc::new(DbProjectLookup::new(db));
        let clock = Arc::new(SystemClock);
        let runners = Arc::new(RunnerSelector::default());
        let roots = Arc::new(RootResolver::new(Arc::clone(&lookup), clock.clone()));
        let artisan = ArtisanService::new((*runners).clone(), Arc::clone(&sink), clock);
        let generator = Arc::new(CommandGenerator::new(Arc::clone(&artisan), sink));
        let config_locator: Arc<dyn MaxCodeConfigLocator> = Arc::new(SystemMaxCodeConfigLocator);
        let shortcuts = Arc::new(ShortcutService::new(
            lookup,
            config_locator,
            Some(Arc::clone(&artisan)),
        ));
        Self {
            roots,
            artisan,
            generator,
            shortcuts,
            runners,
        }
    }

    fn require_laravel_root(&self, root: &str) -> Result<PathBuf, MtError> {
        let root_path = Path::new(root);
        let runner = self.runners.pick(root_path);
        let (canon, _) = self
            .roots
            .require_authorized_laravel_root(root_path, runner.as_ref())?;
        Ok(canon)
    }

    fn existing_commands(
        &self,
        canon_root: &Path,
    ) -> Vec<mt_commands::artisan::model::ArtisanCommand> {
        if let Some(cat) = self.artisan.peek_memory(canon_root) {
            return cat.commands;
        }
        if let Ok(Some(disk)) = mt_commands::artisan::cache::read_cache_files(canon_root) {
            return disk.commands;
        }
        Vec::new()
    }
}

/// Resolve a raiz Laravel mais próxima de `cwd` respeitando fronteiras de projeto e `.git` (`artisan_resolve_root`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_resolve_root(
    state: State<'_, QuickCommandsState>,
    cwd: String,
    project_id: Option<ProjectId>,
) -> Result<Option<ArtisanRoot>, MtError> {
    let roots = Arc::clone(&state.roots);
    let runners = Arc::clone(&state.runners);
    tokio::task::spawn_blocking(move || {
        let cwd_path = Path::new(&cwd);
        let runner = runners.pick(cwd_path);
        roots.resolve(cwd_path, project_id, runner.as_ref())
    })
    .await
    .map_err(|e| mt_commands::error::write_failed(format!("Falha ao resolver raiz Laravel: {e}")))?
}

/// Obtém o catálogo de comandos Artisan para `root`, aplicando cache e stale-while-revalidate (`artisan_catalog_get`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_catalog_get(
    state: State<'_, QuickCommandsState>,
    root: String,
) -> Result<ArtisanCatalog, MtError> {
    let canon = state.require_laravel_root(&root)?;
    state.artisan.get(&canon).await
}

/// Força a redescoberta imediata do catálogo Artisan (`artisan_catalog_refresh`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_catalog_refresh(
    state: State<'_, QuickCommandsState>,
    root: String,
) -> Result<ArtisanCatalog, MtError> {
    let canon = state.require_laravel_root(&root)?;
    state.artisan.refresh(&canon).await
}

/// Retorna o JSON Schema para geração assistida via IA do formulário Artisan (`artisan_form_schema`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_form_schema(
    _state: State<'_, QuickCommandsState>,
) -> Result<serde_json::Value, MtError> {
    Ok(form_json_schema())
}

/// Valida o formulário `ArtisanCommandForm` contra as regras e catálogo da raiz (`artisan_validate_form`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_validate_form(
    state: State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
) -> Result<ValidationReport, MtError> {
    let canon = state.require_laravel_root(&root)?;
    let existing = state.existing_commands(&canon);
    state.generator.validate(&canon, &form, &existing)
}

/// Gera o preview determinístico do comando Artisan e patch de `routes/console.php` (`artisan_preview_command`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_preview_command(
    state: State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
) -> Result<ArtisanPreview, MtError> {
    let canon = state.require_laravel_root(&root)?;
    let existing = state.existing_commands(&canon);
    state.generator.preview(&canon, &form, &existing)
}

/// Cria exclusivamente o arquivo PHP do comando Artisan no disco (`artisan_create_command`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_create_command(
    state: State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
    expected_revision: String,
) -> Result<ArtisanCreateResult, MtError> {
    let canon = state.require_laravel_root(&root)?;
    let existing = state.existing_commands(&canon);
    state
        .generator
        .create(&canon, &form, &expected_revision, &existing)
        .await
}

/// Formata o prompt canônico para abertura no Plan Mode do `MaxCode` (`artisan_format_maxcode_prompt`).
#[tauri::command]
#[specta::specta]
pub async fn artisan_format_maxcode_prompt(
    state: State<'_, QuickCommandsState>,
    root: String,
    form: ArtisanCommandForm,
) -> Result<String, MtError> {
    let canon = state.require_laravel_root(&root)?;
    state.generator.format_maxcode_prompt(&canon, &form)
}

/// Lista os 7 atalhos universais (`/`) com disponibilidade resolvida para `cwd` (`shortcut_list`).
#[tauri::command]
#[specta::specta]
pub async fn shortcut_list(
    state: State<'_, QuickCommandsState>,
    cwd: String,
) -> Result<ShortcutList, MtError> {
    let svc = Arc::clone(&state.shortcuts);
    tokio::task::spawn_blocking(move || svc.list(Path::new(&cwd)))
        .await
        .map_err(|e| mt_commands::error::write_failed(format!("Falha ao listar atalhos: {e}")))?
}

/// Resolve um atalho universal específico (`/install`, `/build`, etc.) para execução (`shortcut_resolve`).
#[tauri::command]
#[specta::specta]
pub async fn shortcut_resolve(
    state: State<'_, QuickCommandsState>,
    cwd: String,
    id: ShortcutId,
) -> Result<ResolvedShortcut, MtError> {
    let svc = Arc::clone(&state.shortcuts);
    tokio::task::spawn_blocking(move || svc.resolve(Path::new(&cwd), id))
        .await
        .map_err(|e| mt_commands::error::write_failed(format!("Falha ao resolver atalho: {e}")))?
}
