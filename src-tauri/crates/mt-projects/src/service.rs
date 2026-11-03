//! Orquestrador central de projetos, Pastas de Projetos e auto-descoberta (`ProjectService`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use mt_core::error::codes;
use mt_core::ids::ProjectId;
use mt_core::{MtError, MtResult};
use parking_lot::{Mutex, RwLock};
use serde_json::json;
use tokio::sync::{OwnedRwLockReadGuard, RwLock as AsyncRwLock, Semaphore, mpsc};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::detect::StackDetector;
use crate::events::{
    ProjectChangedPayload, ProjectDiscoveredPayload, ProjectEvent, ProjectEventSink,
    ProjectRemovedPayload, ProjectRootChangedPayload, RemovedKind,
};
use crate::identity::{IdentityMap, validate_color, validate_icon, validate_title};
use crate::model::{
    DiscoveredFolder, DiscoveryMode, IdentitySource, NewProject, NewProjectRoot, Project,
    ProjectIdentity, ProjectPatch, ProjectRoot, ProjectRootId, ProjectRootPatch, ProjectSnapshot,
    RootImportReport, ScanWarning, StackKind, WatchStatus,
};
use crate::paths::{
    canonicalize_existing_dir, default_title, expand_home, validate_project_dir, validate_root_dir,
};
use crate::scanner::{RootEntryRow, ScanEntry, ScanSnapshot, diff, scan_level1};
use crate::store::{
    CatalogMutation, CatalogStateRows, DismissedRow, IdentityUpdateRow, MissingUpdateRow,
    NewProjectRow, ProjectRootRow, ProjectStore, ReconcileBatch,
};
use crate::watcher::{RootWatcher, WatchSignal};

/// Limite máximo de caminhos aceitos por chamada de `accept_discovered` ou `dismiss_discovered`.
const MAX_DISCOVERY_BATCH_PATHS: usize = 1000;

/// Concorrência máxima de workers de varredura e detecção de stack.
const MAX_SCAN_WORKERS: usize = 8;

/// Teto máximo de atraso ao coalescer eventos de `RootChanged` em rajada (`5 s`).
const MAX_COALESCE_DELAY: Duration = Duration::from_secs(5);

/// Intervalo de reconciliação periódica das raízes habilitadas (`30 s`).
const PERIODIC_RECONCILE_INTERVAL: Duration = Duration::from_secs(30);

/// Hook executado antes de excluir um projeto do cadastro (ex.: Fase 07 encerra abas/PTYs do projeto).
#[async_trait]
pub trait ProjectRemovalHook: Send + Sync + 'static {
    /// Executado antes do `DELETE` no banco. Retornar `Err` aborta a remoção do projeto.
    async fn before_remove(&self, id: ProjectId) -> MtResult<()>;
}

/// Lease RAII compartilhada garantindo que um projeto não seja removido enquanto uma operação
/// vinculada (ex.: criação de aba/PTY na Fase 07) estiver em andamento.
#[derive(Debug)]
pub struct ProjectActivityGuard {
    _project_id: ProjectId,
    _guard: OwnedRwLockReadGuard<()>,
}

#[derive(Debug)]
struct CoordinatorState {
    epoch: String,
    revision: u32,
    catalog: CatalogStateRows,
    pending: HashMap<String, DiscoveredFolder>,
    watch_status: HashMap<ProjectRootId, WatchStatus>,
    root_generations: HashMap<ProjectRootId, u64>,
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self {
            epoch: Uuid::new_v4().to_string(),
            revision: 0,
            catalog: CatalogStateRows::default(),
            pending: HashMap::new(),
            watch_status: HashMap::new(),
            root_generations: HashMap::new(),
        }
    }
}

impl CoordinatorState {
    fn bump_revision(&mut self) -> (String, u32) {
        if self.revision == u32::MAX {
            self.epoch = Uuid::new_v4().to_string();
            self.revision = 1;
        } else {
            self.revision += 1;
        }
        (self.epoch.clone(), self.revision)
    }

    fn next_root_generation(&mut self, id: ProjectRootId) -> u64 {
        let entry = self.root_generations.entry(id).or_insert(0);
        *entry = entry.wrapping_add(1);
        *entry
    }

    fn current_root_generation(&self, id: ProjectRootId) -> u64 {
        self.root_generations.get(&id).copied().unwrap_or(0)
    }
}

#[derive(Debug, Clone)]
struct ScannedCandidate {
    canonical_utf8: String,
    suggested_title: String,
    identity: ProjectIdentity,
    warnings: Vec<ScanWarning>,
}

/// Serviço assíncrono de gerenciamento de projetos, Pastas de Projetos e auto-descoberta.
pub struct ProjectService {
    store: Arc<dyn ProjectStore>,
    sink: Arc<dyn ProjectEventSink>,
    detector: Arc<StackDetector>,
    watcher: Mutex<RootWatcher>,
    signal_rx: AsyncRwLock<Option<mpsc::Receiver<WatchSignal>>>,
    dirty_roots: Arc<Mutex<HashSet<ProjectRootId>>>,
    removal_hooks: RwLock<Vec<Arc<dyn ProjectRemovalHook>>>,
    activity_gates: Mutex<HashMap<ProjectId, Arc<AsyncRwLock<()>>>>,
    removing: Mutex<HashSet<ProjectId>>,
    coordinator: AsyncRwLock<CoordinatorState>,
    scan_semaphore: Arc<Semaphore>,
    shutting_down: AtomicBool,
    bg_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl std::fmt::Debug for ProjectService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectService")
            .field("shutting_down", &self.shutting_down.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl ProjectService {
    /// Cria uma nova instância de [`ProjectService`].
    #[must_use]
    pub fn new(store: Arc<dyn ProjectStore>, sink: Arc<dyn ProjectEventSink>) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(64);
        let dirty_roots = Arc::new(Mutex::new(HashSet::new()));
        let watcher = RootWatcher::new(tx, Arc::clone(&dirty_roots));

        Arc::new(Self {
            store,
            sink,
            detector: Arc::new(StackDetector::default_rules()),
            watcher: Mutex::new(watcher),
            signal_rx: AsyncRwLock::new(Some(rx)),
            dirty_roots,
            removal_hooks: RwLock::new(Vec::new()),
            activity_gates: Mutex::new(HashMap::new()),
            removing: Mutex::new(HashSet::new()),
            coordinator: AsyncRwLock::new(CoordinatorState::default()),
            scan_semaphore: Arc::new(Semaphore::new(MAX_SCAN_WORKERS)),
            shutting_down: AtomicBool::new(false),
            bg_tasks: Mutex::new(Vec::new()),
        })
    }

    /// Carrega o catálogo persistido, inicializa watchers, executa a varredura inicial e inicia loops em background.
    pub async fn start(self: &Arc<Self>) -> MtResult<()> {
        self.shutting_down.store(false, Ordering::SeqCst);

        let initial_rows = self.store.load_catalog().await?;
        let roots_to_init: Vec<ProjectRootRow> = initial_rows.roots.clone();

        {
            let mut coord = self.coordinator.write().await;
            coord.catalog = initial_rows;
            for r in &roots_to_init {
                coord.root_generations.entry(r.id).or_insert(1);
                let initial_status = if r.watch_enabled {
                    WatchStatus::Watching
                } else {
                    WatchStatus::Disabled
                };
                coord.watch_status.insert(r.id, initial_status);
            }
        }

        // Registra watchers antes do scan inicial
        for root in &roots_to_init {
            if root.watch_enabled {
                let status = self.setup_root_watch(root.id, Path::new(&root.path)).0;
                self.coordinator
                    .write()
                    .await
                    .watch_status
                    .insert(root.id, status);
            }
        }

        // Faz scan inicial de cada raiz (mesmo com watch_enabled=false o estado de origens é reconciliado)
        for root in &roots_to_init {
            let _ = self.scan_and_reconcile_root(root.id, false).await;
        }

        // Verifica presença de todos os projetos (incluindo avulsos sem raiz)
        let _ = self.refresh_missing().await;

        self.spawn_background_workers();

        Ok(())
    }

    /// Encerra tarefas em background e libera todos os observadores `notify` sem emitir novos eventos.
    #[allow(clippy::unused_async)]
    pub async fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);

        let tasks: Vec<JoinHandle<()>> = std::mem::take(&mut *self.bg_tasks.lock());
        for handle in tasks {
            handle.abort();
        }

        // Descarta debouncers em thread separada para nunca bloquear a thread chamadora
        let mut watcher_guard = self.watcher.lock();
        watcher_guard.clear_all();
        drop(watcher_guard);
    }

    /// Registra um [`ProjectRemovalHook`] executado antes de remover qualquer projeto.
    pub fn register_removal_hook(&self, hook: Arc<dyn ProjectRemovalHook>) {
        self.removal_hooks.write().push(hook);
    }

    /// Adquire uma lease compartilhada de atividade sobre o projeto `id`.
    pub async fn acquire_activity(&self, id: ProjectId) -> MtResult<ProjectActivityGuard> {
        if self.removing.lock().contains(&id) {
            return Err(MtError::new(
                codes::PROJECT_REMOVAL_BLOCKED,
                "O projeto está sendo removido no momento.",
            ));
        }

        let gate = {
            let mut gates = self.activity_gates.lock();
            Arc::clone(
                gates
                    .entry(id)
                    .or_insert_with(|| Arc::new(AsyncRwLock::new(()))),
            )
        };

        let read_guard = gate.read_owned().await;

        if self.removing.lock().contains(&id) {
            return Err(MtError::new(
                codes::PROJECT_REMOVAL_BLOCKED,
                "O projeto está sendo removido no momento.",
            ));
        }

        let exists = {
            let coord = self.coordinator.read().await;
            coord.catalog.projects.iter().any(|p| p.id == id)
        };

        if !exists {
            return Err(MtError::new(
                codes::PROJECT_NOT_FOUND,
                "O projeto solicitado não foi encontrado.",
            ));
        }

        Ok(ProjectActivityGuard {
            _project_id: id,
            _guard: read_guard,
        })
    }

    /// Retorna um [`ProjectSnapshot`] consistente e versionado de todo o catálogo.
    pub async fn snapshot(&self) -> MtResult<ProjectSnapshot> {
        let mut coord = self.coordinator.write().await;
        if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
            coord.catalog = self.store.load_catalog().await?;
        }
        Ok(build_snapshot(&coord))
    }

    /// Lista todos os projetos cadastrados ordenados por `(sort_order, id)`.
    pub async fn list(&self) -> MtResult<Vec<Project>> {
        Ok(self.snapshot().await?.projects)
    }

    /// Cadastra um novo projeto individual.
    #[allow(clippy::too_many_lines)]
    pub async fn add(&self, input: NewProject) -> MtResult<Project> {
        let raw_path = input.path.clone();
        let detector = Arc::clone(&self.detector);

        let (canonical_path, canonical_utf8, report) = tokio::task::spawn_blocking(move || {
            let (canon, utf8) = validate_project_dir(&raw_path)?;
            let det = detector.detect(&canon);
            Ok::<_, MtError>((canon, utf8, det))
        })
        .await
        .map_err(|_| MtError::internal("Falha ao aguardar validação de diretório do projeto."))??;

        let raw_title = input
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map_or_else(|| default_title(&canonical_path), ToString::to_string);
        let clean_title = validate_title(&raw_title)?;

        let auto_identity = IdentityMap::resolve(&canonical_path, report.detected);
        let has_manual_icon = input.icon.as_deref().is_some_and(|s| !s.trim().is_empty());
        let has_manual_color = input.color.as_deref().is_some_and(|s| !s.trim().is_empty());

        let icon = if let Some(ref ic) = input.icon
            && !ic.trim().is_empty()
        {
            let trimmed = ic.trim();
            validate_icon(trimmed)?;
            trimmed.to_string()
        } else {
            auto_identity.icon
        };

        let color = if let Some(ref col) = input.color
            && !col.trim().is_empty()
        {
            validate_color(col)?
        } else {
            auto_identity.color
        };

        let identity_source = if has_manual_icon || has_manual_color {
            IdentitySource::Manual
        } else {
            IdentitySource::Auto
        };

        let mut coord = self.coordinator.write().await;
        if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
            coord.catalog = self.store.load_catalog().await?;
        }

        if coord
            .catalog
            .projects
            .iter()
            .any(|p| p.path == canonical_utf8)
        {
            return Err(MtError::new(
                codes::PROJECT_DUPLICATE_PATH,
                format!("A pasta \"{canonical_utf8}\" já está cadastrada como projeto."),
            ));
        }

        let root_id = resolve_primary_root_id(&coord.catalog, &canonical_path, &canonical_utf8);
        let now = now_ms();
        let candidate_project = Project {
            id: ProjectId::new(),
            title: clean_title,
            icon,
            color,
            path: canonical_utf8.clone(),
            root_id,
            stack: auto_identity.stack,
            identity_source,
            sort_order: 0,
            missing: false,
            created_at: now,
            updated_at: now,
        };

        let commit = self
            .store
            .commit(CatalogMutation::Add(NewProjectRow {
                project: candidate_project,
                clear_dismissed_paths: vec![canonical_utf8.clone()],
                if_exists_noop: false,
            }))
            .await?;

        coord.catalog = commit.catalog;
        let removed_suggestion = coord.pending.remove(&canonical_utf8);
        let (epoch, revision) = coord.bump_revision();
        let created = commit
            .upserted_projects
            .into_iter()
            .next()
            .ok_or_else(|| MtError::internal("Projeto não retornado após inserção."))?;
        drop(coord);

        if let Some(sug) = removed_suggestion {
            self.emit_removed(
                &epoch,
                revision,
                ProjectRemovedPayload {
                    root_id: Some(sug.root_id),
                    kind: RemovedKind::SuggestionGone,
                    path: canonical_utf8,
                    project_id: None,
                },
            );
        }

        self.emit_changed(
            &epoch,
            revision,
            ProjectChangedPayload::Upserted {
                projects: vec![created.clone()],
            },
        );

        Ok(created)
    }

    /// Atualiza título, ícone, cor ou reseta a identidade de um projeto existente.
    #[allow(clippy::too_many_lines)]
    pub async fn update(&self, id: ProjectId, patch: ProjectPatch) -> MtResult<Project> {
        let reset = patch.reset_identity.unwrap_or(false);
        let has_icon = patch.icon.is_some();
        let has_color = patch.color.is_some();
        let has_title = patch.title.is_some();

        if reset && (has_icon || has_color) {
            return Err(MtError::new(
                codes::PROJECT_INVALID_PATCH,
                "Não é permitido informar ícone ou cor manual junto com resetIdentity.",
            ));
        }

        if !reset && !has_icon && !has_color && !has_title {
            return Err(MtError::new(
                codes::PROJECT_INVALID_PATCH,
                "Nenhuma alteração foi informada para atualizar o projeto.",
            ));
        }

        let validated_title = match patch.title {
            Some(ref t) => Some(validate_title(t)?),
            None => None,
        };
        let validated_icon = match patch.icon {
            Some(ref ic) => {
                let trimmed = ic.trim();
                validate_icon(trimmed)?;
                Some(trimmed.to_string())
            }
            None => None,
        };
        let validated_color = match patch.color {
            Some(ref c) => Some(validate_color(c)?),
            None => None,
        };

        let existing = {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            coord
                .catalog
                .projects
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or_else(|| {
                    MtError::new(
                        codes::PROJECT_NOT_FOUND,
                        "O projeto solicitado não foi encontrado.",
                    )
                })?
        };

        let redetected = if reset {
            let path_buf = PathBuf::from(&existing.path);
            let detector = Arc::clone(&self.detector);
            let ident = tokio::task::spawn_blocking(move || {
                let rep = detector.detect(&path_buf);
                IdentityMap::resolve(&path_buf, rep.detected)
            })
            .await
            .map_err(|_| MtError::internal("Falha ao re-detectar identidade do projeto."))?;
            Some(ident)
        } else {
            None
        };

        let mut coord = self.coordinator.write().await;
        let mut current = coord
            .catalog
            .projects
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| {
                MtError::new(
                    codes::PROJECT_NOT_FOUND,
                    "O projeto solicitado não foi encontrado.",
                )
            })?;

        if let Some(t) = validated_title {
            current.title = t;
        }
        if let Some(ident) = redetected {
            current.stack = ident.stack;
            current.icon = ident.icon;
            current.color = ident.color;
            current.identity_source = IdentitySource::Auto;
        } else {
            if let Some(ic) = validated_icon {
                current.icon = ic;
                current.identity_source = IdentitySource::Manual;
            }
            if let Some(col) = validated_color {
                current.color = col;
                current.identity_source = IdentitySource::Manual;
            }
        }
        current.updated_at = now_ms();

        let commit = self.store.commit(CatalogMutation::Update(current)).await?;
        coord.catalog = commit.catalog;
        let (epoch, revision) = coord.bump_revision();
        let updated = commit
            .upserted_projects
            .into_iter()
            .next()
            .ok_or_else(|| MtError::internal("Projeto não retornado após atualização."))?;
        drop(coord);

        self.emit_changed(
            &epoch,
            revision,
            ProjectChangedPayload::Upserted {
                projects: vec![updated.clone()],
            },
        );

        Ok(updated)
    }

    /// Remove um projeto do cadastro executando previamente todos os [`ProjectRemovalHook`] registrados
    /// e dispensando o caminho nas Pastas de Projetos que o contêm.
    pub async fn remove(&self, id: ProjectId) -> MtResult<()> {
        let gate = {
            let mut gates = self.activity_gates.lock();
            Arc::clone(
                gates
                    .entry(id)
                    .or_insert_with(|| Arc::new(AsyncRwLock::new(()))),
            )
        };

        // Adquire lease exclusiva antes de marcar removing
        let _write_guard = gate.write_owned().await;
        {
            let mut rem = self.removing.lock();
            if !rem.insert(id) {
                return Err(MtError::new(
                    codes::PROJECT_REMOVAL_BLOCKED,
                    "O projeto já está em processo de remoção.",
                ));
            }
        }

        let result = self.execute_remove_inner(id).await;
        self.removing.lock().remove(&id);
        if result.is_ok() {
            self.activity_gates.lock().remove(&id);
        }
        result
    }

    async fn execute_remove_inner(&self, id: ProjectId) -> MtResult<()> {
        {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            if !coord.catalog.projects.iter().any(|p| p.id == id) {
                return Err(MtError::new(
                    codes::PROJECT_NOT_FOUND,
                    "O projeto solicitado não foi encontrado.",
                ));
            }
        }

        let hooks: Vec<Arc<dyn ProjectRemovalHook>> = self.removal_hooks.read().clone();
        for hook in hooks {
            if let Err(err) = hook.before_remove(id).await {
                let reason = err.message.to_string();
                return Err(MtError::new(
                    codes::PROJECT_REMOVAL_BLOCKED,
                    format!("Não foi possível remover o projeto: {reason}"),
                )
                .with_details(json!({ "reason": reason })));
            }
        }

        let mut coord = self.coordinator.write().await;
        let existing = coord
            .catalog
            .projects
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| {
                MtError::new(
                    codes::PROJECT_NOT_FOUND,
                    "O projeto solicitado não foi encontrado.",
                )
            })?;

        let now = now_ms();
        let dismiss =
            collect_dismissals_for_path(&coord.catalog, &existing.path, existing.root_id, now);

        let commit = self
            .store
            .commit(CatalogMutation::Remove { id, dismiss })
            .await?;

        coord.catalog = commit.catalog;
        let removed_sug = coord.pending.remove(&existing.path);
        let (epoch, revision) = coord.bump_revision();
        drop(coord);

        if let Some(sug) = removed_sug {
            self.emit_removed(
                &epoch,
                revision,
                ProjectRemovedPayload {
                    root_id: Some(sug.root_id),
                    kind: RemovedKind::SuggestionGone,
                    path: existing.path,
                    project_id: None,
                },
            );
        }

        self.emit_changed(
            &epoch,
            revision,
            ProjectChangedPayload::Deleted { ids: vec![id] },
        );

        Ok(())
    }

    /// Reordena o catálogo inteiro de projetos (`0..N-1`).
    pub async fn reorder(&self, ids: Vec<ProjectId>) -> MtResult<()> {
        let mut coord = self.coordinator.write().await;
        let commit = self
            .store
            .commit(CatalogMutation::Reorder(ids.clone()))
            .await?;
        coord.catalog = commit.catalog;
        let (epoch, revision) = coord.bump_revision();
        drop(coord);

        self.emit_changed(&epoch, revision, ProjectChangedPayload::Reordered { ids });

        Ok(())
    }

    /// Re-detecta a stack de um projeto cadastrado (preservando ícone/cor se `identity_source == Manual`).
    pub async fn redetect(&self, id: ProjectId) -> MtResult<Project> {
        let existing = {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            coord
                .catalog
                .projects
                .iter()
                .find(|p| p.id == id)
                .cloned()
                .ok_or_else(|| {
                    MtError::new(
                        codes::PROJECT_NOT_FOUND,
                        "O projeto solicitado não foi encontrado.",
                    )
                })?
        };

        let path_buf = PathBuf::from(&existing.path);
        let detector = Arc::clone(&self.detector);
        let ident = tokio::task::spawn_blocking(move || -> MtResult<ProjectIdentity> {
            let meta = fs::metadata(&path_buf).map_err(|_| {
                MtError::new(
                    codes::PROJECT_PATH_NOT_FOUND,
                    "A pasta do projeto não existe mais no disco.",
                )
            })?;
            if !meta.is_dir() {
                return Err(MtError::new(
                    codes::PROJECT_PATH_NOT_DIRECTORY,
                    "O caminho do projeto não é mais um diretório.",
                ));
            }
            let rep = detector.detect(&path_buf);
            Ok(IdentityMap::resolve(&path_buf, rep.detected))
        })
        .await
        .map_err(|_| MtError::internal("Falha ao re-detectar stack do projeto."))??;

        let mut coord = self.coordinator.write().await;
        let mut current = coord
            .catalog
            .projects
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| {
                MtError::new(
                    codes::PROJECT_NOT_FOUND,
                    "O projeto solicitado não foi encontrado.",
                )
            })?;

        current.stack = ident.stack;
        current.missing = false;
        if current.identity_source == IdentitySource::Auto {
            current.icon = ident.icon;
            current.color = ident.color;
        }
        current.updated_at = now_ms();

        let commit = self.store.commit(CatalogMutation::Update(current)).await?;
        coord.catalog = commit.catalog;
        let (epoch, revision) = coord.bump_revision();
        let updated = commit
            .upserted_projects
            .into_iter()
            .next()
            .ok_or_else(|| MtError::internal("Projeto não retornado após re-detecção."))?;
        drop(coord);

        self.emit_changed(
            &epoch,
            revision,
            ProjectChangedPayload::Upserted {
                projects: vec![updated.clone()],
            },
        );

        Ok(updated)
    }

    /// Calcula a [`ProjectIdentity`] de uma pasta sem persistir nada no banco.
    pub async fn preview_identity(&self, path: String) -> MtResult<ProjectIdentity> {
        let detector = Arc::clone(&self.detector);
        tokio::task::spawn_blocking(move || {
            let (canonical, _) = validate_project_dir(&path)?;
            let rep = detector.detect(&canonical);
            Ok(IdentityMap::resolve(&canonical, rep.detected))
        })
        .await
        .map_err(|_| MtError::internal("Falha ao calcular preview de identidade."))?
    }

    /// Verifica no sistema de arquivos a presença de todos os projetos cadastrados e retorna apenas os alterados.
    pub async fn refresh_missing(&self) -> MtResult<Vec<Project>> {
        let projects_snapshot = {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            coord.catalog.projects.clone()
        };

        if projects_snapshot.is_empty() {
            return Ok(Vec::new());
        }

        let updates = tokio::task::spawn_blocking(move || {
            let now = now_ms();
            let mut out = Vec::new();
            for proj in projects_snapshot {
                let p = Path::new(&proj.path);
                let new_missing = match fs::metadata(p) {
                    Ok(meta) => !meta.is_dir(),
                    Err(err)
                        if err.kind() == ErrorKind::NotFound
                            || err.kind() == ErrorKind::NotADirectory =>
                    {
                        true
                    }
                    Err(_) => proj.missing, // Preserva último valor em erro de permissão/I/O
                };
                if new_missing != proj.missing {
                    out.push(MissingUpdateRow {
                        id: proj.id,
                        missing: new_missing,
                        updated_at: now,
                    });
                }
            }
            out
        })
        .await
        .map_err(|_| MtError::internal("Falha ao verificar presença de pastas de projetos."))?;

        if updates.is_empty() {
            return Ok(Vec::new());
        }

        let mut coord = self.coordinator.write().await;
        let commit = self
            .store
            .commit(CatalogMutation::Reconcile(ReconcileBatch {
                missing_updates: updates,
                ..ReconcileBatch::default()
            }))
            .await?;

        coord.catalog = commit.catalog;
        let changed_projects = commit.upserted_projects;
        if changed_projects.is_empty() {
            return Ok(Vec::new());
        }

        let (epoch, revision) = coord.bump_revision();
        drop(coord);

        for proj in &changed_projects {
            if proj.missing {
                self.emit_removed(
                    &epoch,
                    revision,
                    ProjectRemovedPayload {
                        root_id: proj.root_id,
                        kind: RemovedKind::ProjectMissing,
                        path: proj.path.clone(),
                        project_id: Some(proj.id),
                    },
                );
            }
        }

        self.emit_changed(
            &epoch,
            revision,
            ProjectChangedPayload::Upserted {
                projects: changed_projects.clone(),
            },
        );

        Ok(changed_projects)
    }

    /// Lista todas as Pastas de Projetos cadastradas com seu `watch_status` atual.
    pub async fn list_roots(&self) -> MtResult<Vec<ProjectRoot>> {
        Ok(self.snapshot().await?.roots)
    }

    /// Cadastra uma nova Pasta de Projetos, inicia seu watcher (quando habilitado) e executa a varredura inicial.
    #[allow(clippy::too_many_lines)]
    pub async fn add_root(&self, input: NewProjectRoot) -> MtResult<RootImportReport> {
        let raw_path = input.path.clone();
        let (canonical_path, canonical_utf8) =
            tokio::task::spawn_blocking(move || validate_root_dir(&raw_path))
                .await
                .map_err(|_| MtError::internal("Falha ao validar Pasta de Projetos."))??;

        {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            if coord.catalog.roots.iter().any(|r| r.path == canonical_utf8) {
                return Err(MtError::new(
                    codes::PROJECT_ROOT_DUPLICATE_PATH,
                    format!(
                        "A pasta \"{canonical_utf8}\" já está cadastrada como Pasta de Projetos."
                    ),
                ));
            }
        }

        let root_id = ProjectRootId::new();
        let now = now_ms();
        let root_row = ProjectRootRow {
            id: root_id,
            path: canonical_utf8.clone(),
            discovery_mode: input.discovery_mode,
            watch_enabled: input.watch_enabled,
            created_at: now,
            updated_at: now,
        };

        // Registra watcher antes do scan inicial para não perder eventos durante o scan
        let (watch_status, watch_warning) = if input.watch_enabled {
            self.setup_root_watch(root_id, &canonical_path)
        } else {
            (WatchStatus::Disabled, None)
        };

        // Executa scan de nível 1 e detecção de stack fora de locks
        let (scan_snap, candidates) = match self.run_scan_and_detect(&canonical_path).await {
            Ok(res) => res,
            Err(err) => {
                if input.watch_enabled {
                    self.watcher.lock().unwatch(root_id);
                }
                return Err(err);
            }
        };

        let should_import =
            input.import_existing || input.discovery_mode == DiscoveryMode::AutoImport;

        let mut coord = self.coordinator.write().await;
        if coord.catalog.roots.iter().any(|r| r.path == canonical_utf8) {
            if input.watch_enabled {
                self.watcher.lock().unwatch(root_id);
            }
            return Err(MtError::new(
                codes::PROJECT_ROOT_DUPLICATE_PATH,
                format!("A pasta \"{canonical_utf8}\" já está cadastrada como Pasta de Projetos."),
            ));
        }

        let existing_project_paths: HashSet<String> = coord
            .catalog
            .projects
            .iter()
            .map(|p| p.path.clone())
            .collect();

        let upserted_entries: Vec<RootEntryRow> = scan_snap
            .entries
            .iter()
            .filter_map(|e| {
                Some(RootEntryRow {
                    root_id,
                    entry_path: e.entry_path.to_str()?.to_string(),
                    canonical_path: e.canonical_path.to_str()?.to_string(),
                    observed_at: now,
                })
            })
            .collect();

        let mut auto_imports = Vec::new();
        let mut suggested_folders = Vec::new();
        let mut skipped = scan_snap.skipped_non_candidates;
        let mut warnings = scan_snap.warnings;
        if let Some(w) = watch_warning {
            warnings.push(w);
        }

        for cand in candidates {
            warnings.extend(cand.warnings);
            if existing_project_paths.contains(&cand.canonical_utf8) {
                skipped = skipped.saturating_add(1);
                continue;
            }
            if should_import {
                auto_imports.push(NewProjectRow {
                    project: Project {
                        id: ProjectId::new(),
                        title: cand.suggested_title,
                        icon: cand.identity.icon,
                        color: cand.identity.color,
                        path: cand.canonical_utf8,
                        root_id: Some(root_id),
                        stack: cand.identity.stack,
                        identity_source: IdentitySource::Auto,
                        sort_order: 0,
                        missing: false,
                        created_at: now,
                        updated_at: now,
                    },
                    clear_dismissed_paths: Vec::new(),
                    if_exists_noop: true,
                });
            } else {
                let folder = DiscoveredFolder {
                    root_id,
                    path: cand.canonical_utf8.clone(),
                    suggested_title: cand.suggested_title,
                    identity: cand.identity,
                    discovered_at: now,
                };
                suggested_folders.push(folder);
            }
        }

        let commit = match self
            .store
            .commit(CatalogMutation::AddRoot {
                root: root_row.clone(),
                reconciliation: ReconcileBatch {
                    upserted_entries,
                    removed_entries: Vec::new(),
                    auto_imports,
                    identity_updates: Vec::new(),
                    missing_updates: Vec::new(),
                },
            })
            .await
        {
            Ok(c) => c,
            Err(err) => {
                if input.watch_enabled {
                    self.watcher.lock().unwatch(root_id);
                }
                return Err(err);
            }
        };

        coord.catalog = commit.catalog;
        coord.watch_status.insert(root_id, watch_status);
        coord.next_root_generation(root_id);

        for folder in &suggested_folders {
            // Se já havia sugestão de outra raiz, mantém a raiz representante mais antiga
            coord
                .pending
                .entry(folder.path.clone())
                .or_insert_with(|| folder.clone());
        }
        // Remove de pending qualquer projeto que acabou de ser importado
        for imp in &commit.inserted_projects {
            coord.pending.remove(&imp.path);
        }

        let (epoch, revision) = coord.bump_revision();
        let root_dto = to_project_root(&root_row, watch_status);
        let imported = commit.inserted_projects;
        let upserted = commit.upserted_projects;
        drop(coord);

        self.emit_root_changed(
            &epoch,
            revision,
            ProjectRootChangedPayload::Upserted {
                root: root_dto.clone(),
            },
        );

        if !imported.is_empty() || !suggested_folders.is_empty() {
            let mode = if should_import {
                DiscoveryMode::AutoImport
            } else {
                DiscoveryMode::Suggest
            };
            self.emit_discovered(
                &epoch,
                revision,
                ProjectDiscoveredPayload {
                    root_id,
                    mode,
                    folders: suggested_folders.clone(),
                    imported: imported.clone(),
                },
            );
        }

        if !upserted.is_empty() {
            self.emit_changed(
                &epoch,
                revision,
                ProjectChangedPayload::Upserted { projects: upserted },
            );
        }

        Ok(RootImportReport {
            root: root_dto,
            imported,
            suggested: suggested_folders,
            skipped,
            warnings,
            complete: scan_snap.complete,
        })
    }

    /// Atualiza `discovery_mode` e/ou `watch_enabled` de uma Pasta de Projetos.
    pub async fn update_root(
        &self,
        id: ProjectRootId,
        patch: ProjectRootPatch,
    ) -> MtResult<ProjectRoot> {
        if patch.discovery_mode.is_none() && patch.watch_enabled.is_none() {
            return Err(MtError::new(
                codes::PROJECT_INVALID_PATCH,
                "Nenhuma alteração foi informada para atualizar a Pasta de Projetos.",
            ));
        }

        let mut coord = self.coordinator.write().await;
        if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
            coord.catalog = self.store.load_catalog().await?;
        }

        let mut current = coord
            .catalog
            .roots
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| {
                MtError::new(
                    codes::PROJECT_ROOT_NOT_FOUND,
                    "A Pasta de Projetos solicitada não foi encontrada.",
                )
            })?;

        if let Some(mode) = patch.discovery_mode {
            current.discovery_mode = mode;
        }
        if let Some(we) = patch.watch_enabled {
            current.watch_enabled = we;
        }
        current.updated_at = now_ms();

        let commit = self
            .store
            .commit(CatalogMutation::UpdateRoot(current.clone()))
            .await?;

        coord.catalog = commit.catalog;
        coord.next_root_generation(id);

        let watch_status = if current.watch_enabled {
            self.setup_root_watch(id, Path::new(&current.path)).0
        } else {
            self.watcher.lock().unwatch(id);
            WatchStatus::Disabled
        };

        coord.watch_status.insert(id, watch_status);
        let (epoch, revision) = coord.bump_revision();
        let root_dto = to_project_root(&current, watch_status);
        drop(coord);

        self.emit_root_changed(
            &epoch,
            revision,
            ProjectRootChangedPayload::Upserted {
                root: root_dto.clone(),
            },
        );

        // Se mudou para AutoImport e havia sugestões elegíveis ou pastas na raiz, reconcilia
        if patch.discovery_mode == Some(DiscoveryMode::AutoImport) {
            let _ = self.scan_and_reconcile_root(id, false).await;
        }

        Ok(root_dto)
    }

    /// Remove uma Pasta de Projetos (`projects` cadastrados são preservados com `root_id = None`).
    pub async fn remove_root(&self, id: ProjectRootId) -> MtResult<()> {
        self.watcher.lock().unwatch(id);

        let mut coord = self.coordinator.write().await;
        let affected_project_ids: Vec<ProjectId> = coord
            .catalog
            .projects
            .iter()
            .filter(|p| p.root_id == Some(id))
            .map(|p| p.id)
            .collect();

        let commit = self.store.commit(CatalogMutation::RemoveRoot(id)).await?;
        coord.catalog = commit.catalog;
        coord.watch_status.remove(&id);
        coord.next_root_generation(id);

        // Recalcula ou remove sugestões cujo representante era a raiz removida
        let mut removed_suggestions = Vec::new();
        let mut updated_suggestions = Vec::new();
        let pending_keys: Vec<String> = coord.pending.keys().cloned().collect();
        for path in pending_keys {
            if let Some(sug) = coord.pending.get(&path).cloned()
                && sug.root_id == id
            {
                if let Some(new_rep) = find_eligible_representative_root(&coord.catalog, &path) {
                    let mut updated = sug;
                    updated.root_id = new_rep;
                    coord.pending.insert(path, updated.clone());
                    updated_suggestions.push(updated);
                } else {
                    coord.pending.remove(&path);
                    removed_suggestions.push(sug);
                }
            }
        }

        let affected_projects: Vec<Project> = coord
            .catalog
            .projects
            .iter()
            .filter(|p| affected_project_ids.contains(&p.id))
            .cloned()
            .collect();

        let (epoch, revision) = coord.bump_revision();
        drop(coord);

        self.emit_root_changed(&epoch, revision, ProjectRootChangedPayload::Deleted { id });

        for sug in removed_suggestions {
            self.emit_removed(
                &epoch,
                revision,
                ProjectRemovedPayload {
                    root_id: Some(id),
                    kind: RemovedKind::SuggestionGone,
                    path: sug.path,
                    project_id: None,
                },
            );
        }

        if !affected_projects.is_empty() {
            self.emit_changed(
                &epoch,
                revision,
                ProjectChangedPayload::Upserted {
                    projects: affected_projects,
                },
            );
        }

        Ok(())
    }

    /// Executa uma nova varredura explícita na Pasta de Projetos `id`.
    pub async fn rescan_root(&self, id: ProjectRootId) -> MtResult<RootImportReport> {
        self.scan_and_reconcile_root(id, false).await
    }

    /// Lista todas as subpastas descobertas e pendentes de decisão, ordenadas por `(root_id, path)`.
    pub async fn list_discovered(&self) -> MtResult<Vec<DiscoveredFolder>> {
        Ok(self.snapshot().await?.discovered)
    }

    /// Aceita em lote subpastas sugeridas, cadastrando-as como projetos.
    #[allow(clippy::too_many_lines)]
    pub async fn accept_discovered(&self, paths: Vec<String>) -> MtResult<Vec<Project>> {
        let deduped = validate_and_dedup_discovery_paths(paths)?;

        let (to_verify, roots_to_rescan) = {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            let mut items = Vec::with_capacity(deduped.len());
            let mut roots = HashSet::new();
            for p in &deduped {
                if let Some(sug) = coord.pending.get(p).cloned() {
                    roots.insert(sug.root_id);
                    items.push((p.clone(), Some(sug)));
                } else if let Some(existing) =
                    coord.catalog.projects.iter().find(|proj| &proj.path == p)
                {
                    // Já cadastrado por outro fluxo concorrente -> idempotente
                    items.push((p.clone(), None));
                    if let Some(rid) = existing.root_id {
                        roots.insert(rid);
                    }
                } else {
                    return Err(MtError::new(
                        codes::PROJECT_DISCOVERY_NOT_FOUND,
                        format!("A sugestão para a pasta \"{p}\" não foi encontrada."),
                    ));
                }
            }
            (items, roots)
        };

        // Recanonicaliza e valida existência de todo o lote antes do commit
        let verified = tokio::task::spawn_blocking(move || -> MtResult<Vec<(String, Option<DiscoveredFolder>)>> {
            for (expected_path, _) in &to_verify {
                let p = Path::new(expected_path);
                let canon = canonicalize_existing_dir(p).map_err(|_| {
                    MtError::new(
                        codes::PROJECT_DISCOVERY_STALE,
                        format!("A pasta sugerida \"{expected_path}\" mudou ou não existe mais no disco."),
                    )
                })?;
                if canon.to_str() != Some(expected_path.as_str()) {
                    return Err(MtError::new(
                        codes::PROJECT_DISCOVERY_STALE,
                        format!("O destino canônico de \"{expected_path}\" foi alterado antes da confirmação."),
                    ));
                }
            }
            Ok(to_verify)
        })
        .await
        .map_err(|_| MtError::internal("Falha ao validar caminhos de descoberta."))?;

        let verified = match verified {
            Ok(v) => v,
            Err(err) => {
                for rid in roots_to_rescan {
                    self.dirty_roots.lock().insert(rid);
                }
                return Err(err);
            }
        };

        let mut coord = self.coordinator.write().await;
        let now = now_ms();
        let mut new_rows = Vec::with_capacity(verified.len());
        for (path, sug_opt) in &verified {
            // Revalida que alguma origem ainda existe ou que já é projeto
            let is_already_project = coord.catalog.projects.iter().any(|p| &p.path == path);
            let has_entry = coord
                .catalog
                .entries
                .iter()
                .any(|e| &e.canonical_path == path);
            if !is_already_project && !has_entry && !coord.pending.contains_key(path) {
                return Err(MtError::new(
                    codes::PROJECT_DISCOVERY_NOT_FOUND,
                    format!("A sugestão para a pasta \"{path}\" não está mais disponível."),
                ));
            }

            if let Some(sug) = sug_opt {
                let title = validate_title(&sug.suggested_title)
                    .unwrap_or_else(|_| default_title(Path::new(path)));
                let rep_root =
                    find_eligible_representative_root(&coord.catalog, path).or(Some(sug.root_id));
                new_rows.push(NewProjectRow {
                    project: Project {
                        id: ProjectId::new(),
                        title,
                        icon: sug.identity.icon.clone(),
                        color: sug.identity.color.clone(),
                        path: path.clone(),
                        root_id: rep_root,
                        stack: sug.identity.stack,
                        identity_source: IdentitySource::Auto,
                        sort_order: 0,
                        missing: false,
                        created_at: now,
                        updated_at: now,
                    },
                    clear_dismissed_paths: vec![path.clone()],
                    if_exists_noop: true,
                });
            }
        }

        let commit = self.store.commit(CatalogMutation::Accept(new_rows)).await?;
        coord.catalog = commit.catalog;

        let mut removed_suggestions = Vec::new();
        for (path, _) in &verified {
            if let Some(sug) = coord.pending.remove(path) {
                removed_suggestions.push(sug);
            }
        }

        let accepted_projects: Vec<Project> = verified
            .iter()
            .filter_map(|(path, _)| {
                coord
                    .catalog
                    .projects
                    .iter()
                    .find(|p| &p.path == path)
                    .cloned()
            })
            .collect();

        let (epoch, revision) = coord.bump_revision();
        drop(coord);

        for sug in removed_suggestions {
            self.emit_removed(
                &epoch,
                revision,
                ProjectRemovedPayload {
                    root_id: Some(sug.root_id),
                    kind: RemovedKind::SuggestionGone,
                    path: sug.path,
                    project_id: None,
                },
            );
        }

        if !commit.upserted_projects.is_empty() {
            self.emit_changed(
                &epoch,
                revision,
                ProjectChangedPayload::Upserted {
                    projects: commit.upserted_projects,
                },
            );
        }

        Ok(accepted_projects)
    }

    /// Dispensa em lote subpastas sugeridas, gravando a dispensa em todas as raízes que observam o destino.
    pub async fn dismiss_discovered(&self, paths: Vec<String>) -> MtResult<()> {
        let deduped = validate_and_dedup_discovery_paths(paths)?;

        let roots_to_rescan = {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            let mut roots = HashSet::new();
            for p in &deduped {
                if let Some(sug) = coord.pending.get(p) {
                    roots.insert(sug.root_id);
                } else {
                    return Err(MtError::new(
                        codes::PROJECT_DISCOVERY_NOT_FOUND,
                        format!("A sugestão para a pasta \"{p}\" não foi encontrada."),
                    ));
                }
            }
            roots
        };

        let paths_for_check = deduped.clone();
        let verified = tokio::task::spawn_blocking(move || -> MtResult<()> {
            for expected_path in &paths_for_check {
                let p = Path::new(expected_path);
                let canon = canonicalize_existing_dir(p).map_err(|_| {
                    MtError::new(
                        codes::PROJECT_DISCOVERY_STALE,
                        format!(
                            "A pasta sugerida \"{expected_path}\" mudou ou não existe mais no disco."
                        ),
                    )
                })?;
                if canon.to_str() != Some(expected_path.as_str()) {
                    return Err(MtError::new(
                        codes::PROJECT_DISCOVERY_STALE,
                        format!(
                            "O destino canônico de \"{expected_path}\" foi alterado antes da dispensa."
                        ),
                    ));
                }
            }
            Ok(())
        })
        .await
        .map_err(|_| MtError::internal("Falha ao validar caminhos para dispensa."))?;

        if let Err(err) = verified {
            for rid in roots_to_rescan {
                self.dirty_roots.lock().insert(rid);
            }
            return Err(err);
        }

        let mut coord = self.coordinator.write().await;
        let now = now_ms();
        let mut dismiss_rows = Vec::new();
        let mut removed_suggestions = Vec::new();

        for path in &deduped {
            let Some(sug) = coord.pending.get(path).cloned() else {
                return Err(MtError::new(
                    codes::PROJECT_DISCOVERY_NOT_FOUND,
                    format!("A sugestão para a pasta \"{path}\" não foi encontrada."),
                ));
            };
            let rows = collect_dismissals_for_path(&coord.catalog, path, Some(sug.root_id), now);
            dismiss_rows.extend(rows);
        }

        let commit = self
            .store
            .commit(CatalogMutation::Dismiss(dismiss_rows))
            .await?;
        coord.catalog = commit.catalog;

        for path in &deduped {
            if let Some(sug) = coord.pending.remove(path) {
                removed_suggestions.push(sug);
            }
        }

        let (epoch, revision) = coord.bump_revision();
        drop(coord);

        for sug in removed_suggestions {
            self.emit_removed(
                &epoch,
                revision,
                ProjectRemovedPayload {
                    root_id: Some(sug.root_id),
                    kind: RemovedKind::SuggestionGone,
                    path: sug.path,
                    project_id: None,
                },
            );
        }

        Ok(())
    }

    fn setup_root_watch(
        &self,
        id: ProjectRootId,
        path: &Path,
    ) -> (WatchStatus, Option<ScanWarning>) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return (WatchStatus::Disabled, None);
        }
        match self.watcher.lock().watch(id, path) {
            Ok(()) => (WatchStatus::Watching, None),
            Err(err) if err.code == codes::PROJECT_ROOT_MISSING => (
                WatchStatus::Missing,
                Some(ScanWarning {
                    path: path.to_string_lossy().into_owned(),
                    code: codes::PROJECT_ROOT_MISSING.0.to_string(),
                    message: String::from("A Pasta de Projetos está ausente no disco."),
                }),
            ),
            Err(err) => {
                tracing::warn!(
                    root_id = %id,
                    code = %err.code,
                    "Falha ao iniciar monitoramento da Pasta de Projetos"
                );
                (
                    WatchStatus::Error,
                    Some(ScanWarning {
                        path: path.to_string_lossy().into_owned(),
                        code: codes::PROJECT_ROOT_WATCH_FAILED.0.to_string(),
                        message: err.message.into_owned(),
                    }),
                )
            }
        }
    }

    async fn run_scan_and_detect(
        &self,
        root_path: &Path,
    ) -> MtResult<(ScanSnapshot, Vec<ScannedCandidate>)> {
        let _permit = self
            .scan_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| MtError::internal("Semáforo de varredura encerrado."))?;

        let path_owned = root_path.to_path_buf();
        let detector = Arc::clone(&self.detector);

        tokio::task::spawn_blocking(move || {
            let snap = scan_level1(&path_owned)?;
            let mut seen_canonical = HashSet::new();
            let mut candidates = Vec::new();

            for entry in &snap.entries {
                let Some(canon_utf8) = entry.canonical_path.to_str() else {
                    continue;
                };
                if !seen_canonical.insert(canon_utf8.to_string()) {
                    continue;
                }
                let det_report = detector.detect(&entry.canonical_path);
                let identity = IdentityMap::resolve(&entry.canonical_path, det_report.detected);
                let suggested_title = default_title(&entry.canonical_path);

                candidates.push(ScannedCandidate {
                    canonical_utf8: canon_utf8.to_string(),
                    suggested_title,
                    identity,
                    warnings: det_report.warnings,
                });
            }

            Ok((snap, candidates))
        })
        .await
        .map_err(|_| MtError::internal("Tarefa de varredura interrompida."))?
    }

    /// Executa a varredura de nível 1 e reconcilia origens, auto-imports, sugestões, identidades `auto/unknown`
    /// e flags `missing` para a Pasta de Projetos `id`.
    #[allow(clippy::too_many_lines)]
    pub async fn scan_and_reconcile_root(
        &self,
        id: ProjectRootId,
        recheck_unknown_identities: bool,
    ) -> MtResult<RootImportReport> {
        let (root_row, generation, prev_entries, unknown_projects_to_check) = {
            let mut coord = self.coordinator.write().await;
            if coord.catalog.projects.is_empty() && coord.catalog.roots.is_empty() {
                coord.catalog = self.store.load_catalog().await?;
            }
            let root = coord
                .catalog
                .roots
                .iter()
                .find(|r| r.id == id)
                .cloned()
                .ok_or_else(|| {
                    MtError::new(
                        codes::PROJECT_ROOT_NOT_FOUND,
                        "A Pasta de Projetos solicitada não foi encontrada.",
                    )
                })?;
            let generation = coord.current_root_generation(id);
            let prev_entries: Vec<RootEntryRow> = coord
                .catalog
                .entries
                .iter()
                .filter(|e| e.root_id == id)
                .cloned()
                .collect();
            let unknowns: Vec<(ProjectId, String)> = if recheck_unknown_identities {
                coord
                    .catalog
                    .projects
                    .iter()
                    .filter(|p| {
                        p.identity_source == IdentitySource::Auto
                            && p.stack == StackKind::Unknown
                            && !p.missing
                    })
                    .map(|p| (p.id, p.path.clone()))
                    .collect()
            } else {
                Vec::new()
            };
            (root, generation, prev_entries, unknowns)
        };

        let root_path_buf = PathBuf::from(&root_row.path);
        let scan_res = self.run_scan_and_detect(&root_path_buf).await;

        let (scan_snap, candidates) = match scan_res {
            Ok(pair) => pair,
            Err(err) if err.code == codes::PROJECT_ROOT_MISSING => {
                self.mark_root_missing(id).await;
                return Err(err);
            }
            Err(err) => return Err(err),
        };

        // Se a raiz estava Missing e voltou a responder ao scan, reativa o watcher se watch_enabled=true
        {
            let current_status = self.coordinator.read().await.watch_status.get(&id).copied();
            if current_status == Some(WatchStatus::Missing) && root_row.watch_enabled {
                let (st, _) = self.setup_root_watch(id, &root_path_buf);
                let mut coord = self.coordinator.write().await;
                if coord.watch_status.get(&id).copied() != Some(st) {
                    coord.watch_status.insert(id, st);
                    let (epoch, rev) = coord.bump_revision();
                    let dto = to_project_root(&root_row, st);
                    drop(coord);
                    self.emit_root_changed(
                        &epoch,
                        rev,
                        ProjectRootChangedPayload::Upserted { root: dto },
                    );
                }
            }
        }

        // Reavalia projetos auto/unknown quando solicitado pela reconciliação periódica
        let identity_updates = if unknown_projects_to_check.is_empty() {
            Vec::new()
        } else {
            let detector = Arc::clone(&self.detector);
            tokio::task::spawn_blocking(move || {
                let now = now_ms();
                let mut updates = Vec::new();
                for (pid, path_str) in unknown_projects_to_check {
                    let p = Path::new(&path_str);
                    let rep = detector.detect(p);
                    if rep.complete
                        && rep.detected.first().copied().unwrap_or(StackKind::Unknown)
                            != StackKind::Unknown
                    {
                        let ident = IdentityMap::resolve(p, rep.detected);
                        updates.push(IdentityUpdateRow {
                            id: pid,
                            stack: ident.stack,
                            icon: ident.icon,
                            color: ident.color,
                            updated_at: now,
                        });
                    }
                }
                updates
            })
            .await
            .unwrap_or_default()
        };

        // Verifica também missing dos projetos atuais fora do lock
        let projects_for_missing = {
            let coord = self.coordinator.read().await;
            coord.catalog.projects.clone()
        };
        let missing_updates = tokio::task::spawn_blocking(move || {
            let now = now_ms();
            let mut out = Vec::new();
            for proj in projects_for_missing {
                let p = Path::new(&proj.path);
                let new_missing = match fs::metadata(p) {
                    Ok(meta) => !meta.is_dir(),
                    Err(err)
                        if err.kind() == ErrorKind::NotFound
                            || err.kind() == ErrorKind::NotADirectory =>
                    {
                        true
                    }
                    Err(_) => proj.missing,
                };
                if new_missing != proj.missing {
                    out.push(MissingUpdateRow {
                        id: proj.id,
                        missing: new_missing,
                        updated_at: now,
                    });
                }
            }
            out
        })
        .await
        .unwrap_or_default();

        let scan_diff = diff(&scan_snap, &prev_entries);

        let mut coord = self.coordinator.write().await;
        // Verifica se a geração da raiz mudou enquanto o scan estava em voo
        if coord.current_root_generation(id) != generation {
            let current_root = coord
                .catalog
                .roots
                .iter()
                .find(|r| r.id == id)
                .cloned()
                .unwrap_or(root_row);
            let st = coord
                .watch_status
                .get(&id)
                .copied()
                .unwrap_or(WatchStatus::Disabled);
            return Ok(RootImportReport {
                root: to_project_root(&current_root, st),
                imported: Vec::new(),
                suggested: Vec::new(),
                skipped: scan_snap.skipped_non_candidates,
                warnings: scan_snap.warnings,
                complete: scan_snap.complete,
            });
        }

        let current_root = coord
            .catalog
            .roots
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or_else(|| {
                MtError::new(
                    codes::PROJECT_ROOT_NOT_FOUND,
                    "A Pasta de Projetos solicitada não foi encontrada.",
                )
            })?;

        let now = now_ms();
        let upserted_entries: Vec<RootEntryRow> = scan_diff
            .upserted_entries
            .iter()
            .filter_map(|e: &ScanEntry| {
                Some(RootEntryRow {
                    root_id: id,
                    entry_path: e.entry_path.to_str()?.to_string(),
                    canonical_path: e.canonical_path.to_str()?.to_string(),
                    observed_at: now,
                })
            })
            .collect();

        let removed_entries: Vec<(ProjectRootId, String)> = scan_diff
            .removed_entry_paths
            .iter()
            .map(|p| (id, p.clone()))
            .collect();

        // Simula o estado atualizado de entries e dismissed para decidir elegibilidade de cada candidato
        let mut simulated_entries: Vec<RootEntryRow> = coord
            .catalog
            .entries
            .iter()
            .filter(|e| !(e.root_id == id && scan_diff.removed_entry_paths.contains(&e.entry_path)))
            .cloned()
            .collect();
        for up in &upserted_entries {
            if let Some(ex) = simulated_entries
                .iter_mut()
                .find(|e| e.root_id == up.root_id && e.entry_path == up.entry_path)
            {
                ex.canonical_path.clone_from(&up.canonical_path);
                ex.observed_at = up.observed_at;
            } else {
                simulated_entries.push(up.clone());
            }
        }

        let existing_project_paths: HashSet<String> = coord
            .catalog
            .projects
            .iter()
            .map(|p| p.path.clone())
            .collect();

        let mut auto_imports = Vec::new();
        let mut suggested_folders = Vec::new();
        let mut skipped = scan_snap.skipped_non_candidates;
        let mut warnings = scan_snap.warnings;

        for cand in candidates {
            warnings.extend(cand.warnings);
            if existing_project_paths.contains(&cand.canonical_utf8) {
                skipped = skipped.saturating_add(1);
                continue;
            }

            // Verifica raízes que observam este destino e não o dispensaram
            let eligible_roots: Vec<&ProjectRootRow> = coord
                .catalog
                .roots
                .iter()
                .filter(|r| {
                    let observes = simulated_entries
                        .iter()
                        .any(|e| e.root_id == r.id && e.canonical_path == cand.canonical_utf8);
                    let is_dismissed = coord
                        .catalog
                        .dismissed
                        .iter()
                        .any(|d| d.root_id == r.id && d.path == cand.canonical_utf8);
                    observes && !is_dismissed
                })
                .collect();

            if eligible_roots.is_empty() {
                skipped = skipped.saturating_add(1);
                continue;
            }

            let rep_root = eligible_roots[0].id;
            let any_auto_import = eligible_roots
                .iter()
                .any(|r| r.discovery_mode == DiscoveryMode::AutoImport);

            if any_auto_import {
                auto_imports.push(NewProjectRow {
                    project: Project {
                        id: ProjectId::new(),
                        title: cand.suggested_title,
                        icon: cand.identity.icon,
                        color: cand.identity.color,
                        path: cand.canonical_utf8,
                        root_id: Some(rep_root),
                        stack: cand.identity.stack,
                        identity_source: IdentitySource::Auto,
                        sort_order: 0,
                        missing: false,
                        created_at: now,
                        updated_at: now,
                    },
                    clear_dismissed_paths: Vec::new(),
                    if_exists_noop: true,
                });
            } else {
                let prev_discovered_at = coord
                    .pending
                    .get(&cand.canonical_utf8)
                    .map_or(now, |prev| prev.discovered_at);
                let folder = DiscoveredFolder {
                    root_id: rep_root,
                    path: cand.canonical_utf8.clone(),
                    suggested_title: cand.suggested_title,
                    identity: cand.identity,
                    discovered_at: prev_discovered_at,
                };
                let is_new_or_changed = coord
                    .pending
                    .get(&cand.canonical_utf8)
                    .is_none_or(|prev| prev != &folder);
                if is_new_or_changed {
                    suggested_folders.push(folder.clone());
                }
                coord.pending.insert(cand.canonical_utf8, folder);
            }
        }

        let commit = self
            .store
            .commit(CatalogMutation::Reconcile(ReconcileBatch {
                upserted_entries,
                removed_entries,
                auto_imports,
                identity_updates,
                missing_updates,
            }))
            .await?;

        coord.catalog = commit.catalog;

        // Remove sugestões que foram autoimportadas ou que perderam todas as origens elegíveis
        let mut gone_suggestions = Vec::new();
        for imp in &commit.inserted_projects {
            if let Some(sug) = coord.pending.remove(&imp.path) {
                gone_suggestions.push(sug);
            }
        }

        if scan_snap.complete {
            let pending_paths: Vec<String> = coord.pending.keys().cloned().collect();
            for p_path in pending_paths {
                let is_proj = coord.catalog.projects.iter().any(|p| p.path == p_path);
                let rep = find_eligible_representative_root(&coord.catalog, &p_path);
                let dir_exists = Path::new(&p_path).is_dir();
                if is_proj || rep.is_none() || !dir_exists {
                    if let Some(sug) = coord.pending.remove(&p_path) {
                        gone_suggestions.push(sug);
                    }
                } else if let Some(new_rep) = rep
                    && let Some(sug) = coord.pending.get_mut(&p_path)
                    && sug.root_id != new_rep
                {
                    sug.root_id = new_rep;
                }
            }
        }

        let has_observable_changes = !commit.upserted_projects.is_empty()
            || !commit.inserted_projects.is_empty()
            || !suggested_folders.is_empty()
            || !gone_suggestions.is_empty();

        let (epoch, revision) = if has_observable_changes {
            coord.bump_revision()
        } else {
            (coord.epoch.clone(), coord.revision)
        };

        let watch_status = coord
            .watch_status
            .get(&id)
            .copied()
            .unwrap_or(WatchStatus::Disabled);
        let root_dto = to_project_root(&current_root, watch_status);
        let imported = commit.inserted_projects;
        let upserted = commit.upserted_projects;
        drop(coord);

        for sug in gone_suggestions {
            self.emit_removed(
                &epoch,
                revision,
                ProjectRemovedPayload {
                    root_id: Some(sug.root_id),
                    kind: RemovedKind::SuggestionGone,
                    path: sug.path,
                    project_id: None,
                },
            );
        }

        for proj in &upserted {
            if proj.missing {
                self.emit_removed(
                    &epoch,
                    revision,
                    ProjectRemovedPayload {
                        root_id: proj.root_id,
                        kind: RemovedKind::ProjectMissing,
                        path: proj.path.clone(),
                        project_id: Some(proj.id),
                    },
                );
            }
        }

        if !imported.is_empty() {
            self.emit_discovered(
                &epoch,
                revision,
                ProjectDiscoveredPayload {
                    root_id: id,
                    mode: DiscoveryMode::AutoImport,
                    folders: Vec::new(),
                    imported: imported.clone(),
                },
            );
        }

        if !suggested_folders.is_empty() {
            self.emit_discovered(
                &epoch,
                revision,
                ProjectDiscoveredPayload {
                    root_id: id,
                    mode: DiscoveryMode::Suggest,
                    folders: suggested_folders.clone(),
                    imported: Vec::new(),
                },
            );
        }

        if !upserted.is_empty() {
            self.emit_changed(
                &epoch,
                revision,
                ProjectChangedPayload::Upserted { projects: upserted },
            );
        }

        Ok(RootImportReport {
            root: root_dto,
            imported,
            suggested: suggested_folders,
            skipped,
            warnings,
            complete: scan_snap.complete,
        })
    }

    async fn mark_root_missing(&self, id: ProjectRootId) {
        self.watcher.lock().unwatch(id);
        let mut coord = self.coordinator.write().await;
        let Some(root_row) = coord.catalog.roots.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        if !root_row.watch_enabled {
            return;
        }
        let prev = coord.watch_status.insert(id, WatchStatus::Missing);
        if prev == Some(WatchStatus::Missing) {
            drop(coord);
        } else {
            let (epoch, revision) = coord.bump_revision();
            let dto = to_project_root(&root_row, WatchStatus::Missing);
            drop(coord);
            self.emit_root_changed(
                &epoch,
                revision,
                ProjectRootChangedPayload::Upserted { root: dto },
            );
        }
        // Verifica imediatamente os projetos para marcar missing os que desapareceram junto com a raiz
        let _ = self.refresh_missing().await;
    }

    async fn mark_root_error(&self, id: ProjectRootId) {
        let mut coord = self.coordinator.write().await;
        let Some(root_row) = coord.catalog.roots.iter().find(|r| r.id == id).cloned() else {
            return;
        };
        if !root_row.watch_enabled {
            return;
        }
        let prev = coord.watch_status.insert(id, WatchStatus::Error);
        if prev != Some(WatchStatus::Error) {
            let (epoch, revision) = coord.bump_revision();
            let dto = to_project_root(&root_row, WatchStatus::Error);
            drop(coord);
            self.emit_root_changed(
                &epoch,
                revision,
                ProjectRootChangedPayload::Upserted { root: dto },
            );
        }
    }

    #[allow(clippy::too_many_lines)]
    fn spawn_background_workers(self: &Arc<Self>) {
        let weak_self = Arc::downgrade(self);
        let worker_task = tokio::spawn(async move {
            let Some(svc) = weak_self.upgrade() else {
                return;
            };
            let Some(mut rx) = svc.signal_rx.write().await.take() else {
                return;
            };
            drop(svc);

            let mut dirty_tick = tokio::time::interval(Duration::from_millis(250));
            let mut periodic_tick = tokio::time::interval(PERIODIC_RECONCILE_INTERVAL);
            // Consome o primeiro disparo imediato do interval periódico
            periodic_tick.tick().await;

            // (first_seen, due_at) para coalescer sinais por raiz (SETTLE = 1.5s, máx 5s)
            let mut scheduled_scans: HashMap<ProjectRootId, (Instant, Instant)> = HashMap::new();
            // (next_retry_at, current_backoff) para raízes em WatchStatus::Missing (5s -> 60s)
            let mut missing_retries: HashMap<ProjectRootId, (Instant, Duration)> = HashMap::new();

            loop {
                tokio::select! {
                    maybe_sig = rx.recv() => {
                        let Some(sig) = maybe_sig else { break; };
                        let Some(svc) = weak_self.upgrade() else { break; };
                        if svc.shutting_down.load(Ordering::SeqCst) { break; }

                        match sig {
                            WatchSignal::RootChanged(rid) => {
                                svc.dirty_roots.lock().remove(&rid);
                                schedule_coalesced_scan(&mut scheduled_scans, rid);
                            }
                            WatchSignal::RootMissing(rid) => {
                                svc.dirty_roots.lock().remove(&rid);
                                scheduled_scans.remove(&rid);
                                svc.mark_root_missing(rid).await;
                                let now = Instant::now();
                                missing_retries
                                    .entry(rid)
                                    .or_insert((now + Duration::from_secs(5), Duration::from_secs(5)));
                            }
                            WatchSignal::WatchError(rid, _msg) => {
                                svc.dirty_roots.lock().remove(&rid);
                                svc.mark_root_error(rid).await;
                                schedule_coalesced_scan(&mut scheduled_scans, rid);
                            }
                        }
                    }
                    _ = dirty_tick.tick() => {
                        let Some(svc) = weak_self.upgrade() else { break; };
                        if svc.shutting_down.load(Ordering::SeqCst) { break; }

                        let drained: Vec<ProjectRootId> = svc.dirty_roots.lock().drain().collect();
                        for rid in drained {
                            schedule_coalesced_scan(&mut scheduled_scans, rid);
                        }

                        let now = Instant::now();
                        let due_roots: Vec<ProjectRootId> = scheduled_scans
                            .iter()
                            .filter_map(|(rid, (_, due))| (*due <= now).then_some(*rid))
                            .collect();

                        for rid in due_roots {
                            scheduled_scans.remove(&rid);
                            let enabled = {
                                let coord = svc.coordinator.read().await;
                                coord
                                    .catalog
                                    .roots
                                    .iter()
                                    .find(|r| r.id == rid)
                                    .is_some_and(|r| r.watch_enabled)
                            };
                            if enabled {
                                let _ = svc.scan_and_reconcile_root(rid, false).await;
                            }
                        }

                        // Retentativas com backoff exponencial (5s -> 60s) para raízes Missing
                        let retry_ids: Vec<ProjectRootId> = missing_retries
                            .iter()
                            .filter_map(|(rid, (next_at, _))| (*next_at <= now).then_some(*rid))
                            .collect();

                        for rid in retry_ids {
                            let root_opt = {
                                let coord = svc.coordinator.read().await;
                                let st = coord.watch_status.get(&rid).copied();
                                let row = coord.catalog.roots.iter().find(|r| r.id == rid).cloned();
                                (row, st)
                            };
                            match root_opt {
                                (Some(row), Some(WatchStatus::Missing)) if row.watch_enabled => {
                                    if Path::new(&row.path).is_dir() {
                                        missing_retries.remove(&rid);
                                        let _ = svc.scan_and_reconcile_root(rid, false).await;
                                    } else if let Some((next_at, backoff)) = missing_retries.get_mut(&rid) {
                                        *backoff = (*backoff * 2).min(Duration::from_secs(60));
                                        *next_at = Instant::now() + *backoff;
                                    }
                                }
                                _ => {
                                    missing_retries.remove(&rid);
                                }
                            }
                        }
                    }
                    _ = periodic_tick.tick() => {
                        let Some(svc) = weak_self.upgrade() else { break; };
                        if svc.shutting_down.load(Ordering::SeqCst) { break; }

                        let enabled_roots: Vec<ProjectRootId> = {
                            let coord = svc.coordinator.read().await;
                            coord
                                .catalog
                                .roots
                                .iter()
                                .filter(|r| r.watch_enabled)
                                .map(|r| r.id)
                                .collect()
                        };

                        for rid in enabled_roots {
                            if svc.shutting_down.load(Ordering::SeqCst) { break; }
                            let _ = svc.scan_and_reconcile_root(rid, true).await;
                        }
                        let _ = svc.refresh_missing().await;
                    }
                }
            }
        });

        self.bg_tasks.lock().push(worker_task);
    }

    fn emit_discovered(&self, epoch: &str, revision: u32, payload: ProjectDiscoveredPayload) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        self.sink.discovered(ProjectEvent {
            epoch: epoch.to_string(),
            revision,
            payload,
        });
    }

    fn emit_removed(&self, epoch: &str, revision: u32, payload: ProjectRemovedPayload) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        self.sink.removed(ProjectEvent {
            epoch: epoch.to_string(),
            revision,
            payload,
        });
    }

    fn emit_changed(&self, epoch: &str, revision: u32, payload: ProjectChangedPayload) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        self.sink.changed(ProjectEvent {
            epoch: epoch.to_string(),
            revision,
            payload,
        });
    }

    fn emit_root_changed(&self, epoch: &str, revision: u32, payload: ProjectRootChangedPayload) {
        if self.shutting_down.load(Ordering::SeqCst) {
            return;
        }
        self.sink.root_changed(ProjectEvent {
            epoch: epoch.to_string(),
            revision,
            payload,
        });
    }
}

fn schedule_coalesced_scan(
    scheduled: &mut HashMap<ProjectRootId, (Instant, Instant)>,
    rid: ProjectRootId,
) {
    let now = Instant::now();
    let settle_due = now + RootWatcher::SETTLE;
    scheduled
        .entry(rid)
        .and_modify(|(first_seen, due_at)| {
            let max_due = *first_seen + MAX_COALESCE_DELAY;
            *due_at = settle_due.min(max_due);
        })
        .or_insert((now, settle_due));
}

fn build_snapshot(coord: &CoordinatorState) -> ProjectSnapshot {
    let mut projects = coord.catalog.projects.clone();
    projects.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut roots: Vec<ProjectRoot> = coord
        .catalog
        .roots
        .iter()
        .map(|r| {
            let status = coord
                .watch_status
                .get(&r.id)
                .copied()
                .unwrap_or(if r.watch_enabled {
                    WatchStatus::Watching
                } else {
                    WatchStatus::Disabled
                });
            to_project_root(r, status)
        })
        .collect();
    roots.sort_by(|a, b| {
        a.created_at
            .cmp(&b.created_at)
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut discovered: Vec<DiscoveredFolder> = coord.pending.values().cloned().collect();
    discovered.sort_by(|a, b| a.root_id.cmp(&b.root_id).then_with(|| a.path.cmp(&b.path)));

    ProjectSnapshot {
        epoch: coord.epoch.clone(),
        revision: coord.revision,
        projects,
        roots,
        discovered,
    }
}

fn to_project_root(row: &ProjectRootRow, watch_status: WatchStatus) -> ProjectRoot {
    ProjectRoot {
        id: row.id,
        path: row.path.clone(),
        discovery_mode: row.discovery_mode,
        watch_enabled: row.watch_enabled,
        watch_status,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn resolve_primary_root_id(
    catalog: &CatalogStateRows,
    canonical_path: &Path,
    canonical_utf8: &str,
) -> Option<ProjectRootId> {
    let parent_utf8 = canonical_path.parent().and_then(Path::to_str);
    // As raízes em catalog.roots já estão ordenadas por (created_at, id)
    for root in &catalog.roots {
        let is_direct_parent = parent_utf8 == Some(root.path.as_str());
        let is_observed_alias = catalog
            .entries
            .iter()
            .any(|e| e.root_id == root.id && e.canonical_path == canonical_utf8);
        if is_direct_parent || is_observed_alias {
            return Some(root.id);
        }
    }
    None
}

fn find_eligible_representative_root(
    catalog: &CatalogStateRows,
    canonical_utf8: &str,
) -> Option<ProjectRootId> {
    for root in &catalog.roots {
        let observes = catalog
            .entries
            .iter()
            .any(|e| e.root_id == root.id && e.canonical_path == canonical_utf8);
        let dismissed = catalog
            .dismissed
            .iter()
            .any(|d| d.root_id == root.id && d.path == canonical_utf8);
        if observes && !dismissed {
            return Some(root.id);
        }
    }
    None
}

fn collect_dismissals_for_path(
    catalog: &CatalogStateRows,
    canonical_utf8: &str,
    fallback_root_id: Option<ProjectRootId>,
    dismissed_at: i64,
) -> Vec<DismissedRow> {
    let mut root_ids = HashSet::new();
    if let Some(rid) = fallback_root_id
        && catalog.roots.iter().any(|r| r.id == rid)
    {
        root_ids.insert(rid);
    }

    let parent_utf8 = Path::new(canonical_utf8).parent().and_then(Path::to_str);
    for root in &catalog.roots {
        if parent_utf8 == Some(root.path.as_str()) {
            root_ids.insert(root.id);
        }
    }
    for entry in &catalog.entries {
        if entry.canonical_path == canonical_utf8 {
            root_ids.insert(entry.root_id);
        }
    }

    root_ids
        .into_iter()
        .map(|root_id| DismissedRow {
            root_id,
            path: canonical_utf8.to_string(),
            dismissed_at,
        })
        .collect()
}

fn validate_and_dedup_discovery_paths(paths: Vec<String>) -> MtResult<Vec<String>> {
    if paths.is_empty() || paths.len() > MAX_DISCOVERY_BATCH_PATHS {
        return Err(MtError::new(
            codes::PROJECT_INVALID_ARGUMENT,
            format!("O lote de caminhos deve conter entre 1 e {MAX_DISCOVERY_BATCH_PATHS} itens."),
        ));
    }

    let mut seen = HashSet::with_capacity(paths.len());
    let mut out = Vec::with_capacity(paths.len());
    for raw in paths {
        let expanded = expand_home(&raw)?;
        let utf8 = expanded
            .to_str()
            .ok_or_else(|| {
                MtError::new(
                    codes::PROJECT_PATH_NOT_UTF8,
                    "O caminho informado não é UTF-8 válido.",
                )
            })?
            .to_string();
        if seen.insert(utf8.clone()) {
            out.push(utf8);
        }
    }
    Ok(out)
}

#[allow(clippy::cast_possible_truncation)]
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}
