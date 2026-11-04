//! Orquestrador central de Abas, Templates, Split Panes e Sessões (`WorkspaceManager` — Fase 07).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use async_trait::async_trait;
use mt_core::error::{MtError, MtResult, codes};
use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId, TemplateId};
use mt_core::shell::{
    ContinuousPolicy, IntegrationMode, SeenRequest, ShellIntegrationRequest, ShellKind,
};
use mt_core::workspace::{
    CommandOverride, CursorPage, InitialDispatchState, LaunchSnapshot, LayoutSizePatch,
    PaneCloseOutcome, PaneExitInfo, PaneLayoutNode, PaneRuntimeState, PaneView, ScrollbackChunk,
    SessionDetail, SessionEndReason, SessionSearchHit, SessionSummary, SplitDirection,
    TabLifecycle, TabOpenRequest, TabOrigin, TabTemplate, TabTemplateInput, TabView,
    WorkspaceRestoreReport, WorkspaceSnapshot,
};
use mt_db::repos::{
    archive_tab_session_tx, attach_pane_session_tx, bump_pane_generation_tx, close_pane_session_tx,
    close_tab_tx, create_tab_with_initial_pane_tx, enforce_tab_sessions_retention_tx,
    prepare_operation_tx, remove_pane_from_layout_tx, reopen_workspace_tab_tx, split_pane_tx,
    start_tab_session_tx, update_operation_stage_tx,
};
use mt_db::sanitizer::{SanitizedText, sanitize_with};
use mt_db::{
    Db, NewSession, NewTabTxInput, OperationStage, PrepareOperationOutcome, SessionEnd,
    compute_migration_checksum, insert_session_tx,
};
use mt_projects::ProjectRemovalHook;
use mt_pty::{
    ExitInfo, ExitReason, KillMode, PtyEvent, PtyEventListener, PtyManager, PtyOwner, PtySize,
    SpawnSpec,
};
use mt_shell::ShellService;
use parking_lot::{Mutex, RwLock};
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::sync::Mutex as AsyncMutex;

use crate::credentials::service::VaultService;
use crate::events::workspace::{
    PaneExitedEvent, PaneExitedPayload, PanePtyReplacedEvent, PanePtyReplacedPayload,
    SessionArchivedEvent, SessionStartedEvent, TabClosedEvent, TabClosedPayload,
    TabLayoutChangedEvent, TabOpenedEvent, TabReorderedEvent, TabReorderedPayload,
    TemplateChangedEvent, TemplateChangedPayload, WorkspaceEvent, WorkspaceRestoredEvent,
};
use crate::workspace::environment::EnvironmentResolver;
use crate::workspace::errors::{
    initial_command_stale, map_db_error, pane_not_exited, pane_not_found, tab_open_failed,
};
use crate::workspace::observer::WorkspaceLifecycleObserver;
use crate::workspace::recorder::SessionRecorder;
use crate::workspace::restore::{read_retention_settings, run_boot_reconciliation_and_restore};
use crate::workspace::spawn_hooks::{SpawnHookChain, WorkspaceSpawnGuard};

fn hash_operation_args<T: serde::Serialize>(args: &T) -> String {
    let serialized = serde_json::to_string(args).unwrap_or_default();
    compute_migration_checksum(&serialized)
}

#[derive(Debug, Clone)]
struct LivePaneEntry {
    pane_id: PaneId,
    session_id: SessionId,
    pty_id: Option<PtyId>,
    generation: i64,
    cwd: String,
    state: PaneRuntimeState,
    exit: Option<PaneExitInfo>,
    error: Option<MtError>,
    initial_dispatch: InitialDispatchState,
}

#[derive(Debug, Clone)]
struct LiveTabEntry {
    id: TabId,
    project_id: ProjectId,
    template_id: Option<TemplateId>,
    title: String,
    icon: String,
    position: i64,
    origin: TabOrigin,
    continuous: bool,
    lifecycle: TabLifecycle,
    revision: i64,
    layout_revision: i64,
    layout: PaneLayoutNode,
    active_pane_id: PaneId,
    primary_pane_id: PaneId,
    tab_session_id: TabSessionId,
    launch_snapshot: LaunchSnapshot,
    panes: BTreeMap<PaneId, LivePaneEntry>,
}

impl LiveTabEntry {
    fn to_view(&self) -> TabView {
        let mut panes_vec: Vec<PaneView> = self
            .panes
            .values()
            .map(|p| PaneView {
                id: p.pane_id,
                pty_id: p.pty_id,
                generation: p.generation,
                cwd: p.cwd.clone(),
                state: p.state,
                exit: p.exit.clone(),
                error: p.error.clone(),
            })
            .collect();
        panes_vec.sort_by_key(|p| p.id.to_string());

        let initial_dispatch = self
            .panes
            .get(&self.primary_pane_id)
            .map_or(InitialDispatchState::None, |p| p.initial_dispatch);

        TabView {
            id: self.id,
            project_id: self.project_id,
            template_id: self.template_id,
            title: self.title.clone(),
            icon: self.icon.clone(),
            position: self.position,
            origin: self.origin,
            continuous: self.continuous,
            lifecycle: self.lifecycle,
            revision: self.revision,
            layout_revision: self.layout_revision,
            layout: self.layout.clone(),
            panes: panes_vec,
            active_pane_id: self.active_pane_id,
            primary_pane_id: self.primary_pane_id,
            initial_dispatch,
            tab_session_id: self.tab_session_id,
        }
    }
}

struct WorkspaceManagerInner {
    boot_id: String,
    event_seq: AtomicU64,
    restored: AtomicBool,
    restore_report: RwLock<Option<WorkspaceRestoreReport>>,
    db: Db,
    pty_manager: Arc<PtyManager>,
    shell_service: Arc<ShellService>,
    recorder: Arc<SessionRecorder>,
    spawn_hooks: SpawnHookChain,
    observers: RwLock<Vec<Arc<dyn WorkspaceLifecycleObserver>>>,
    spawn_guards: Mutex<HashMap<PtyId, Vec<Box<dyn WorkspaceSpawnGuard>>>>,
    app_handle: RwLock<Option<AppHandle>>,
    tabs: RwLock<HashMap<TabId, LiveTabEntry>>,
    pane_to_tab: RwLock<HashMap<PaneId, TabId>>,
    pty_to_pane: RwLock<HashMap<PtyId, (TabId, PaneId, SessionId, i64)>>,
    tab_locks: Mutex<HashMap<TabId, Arc<AsyncMutex<()>>>>,
    restore_lock: AsyncMutex<()>,
    vault_service: Arc<VaultService>,
}

/// Gerenciador central de Abas, Templates, Split Panes e Sessões (Fase 07).
#[derive(Clone)]
pub struct WorkspaceManager {
    inner: Arc<WorkspaceManagerInner>,
}

impl std::fmt::Debug for WorkspaceManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceManager")
            .field("boot_id", &self.inner.boot_id)
            .field("open_tabs", &self.inner.tabs.read().len())
            .finish_non_exhaustive()
    }
}

struct WorkspacePtyListener {
    manager: WorkspaceManager,
}

impl PtyEventListener for WorkspacePtyListener {
    fn on_event(&self, event: &PtyEvent) {
        if let PtyEvent::Exited { pty_id, exit, .. } = event {
            self.manager.handle_pty_exit(*pty_id, exit);
        }
    }
}

impl WorkspaceManager {
    /// Cria uma nova instância do `WorkspaceManager`.
    #[must_use]
    pub fn new(
        db: Db,
        pty_manager: Arc<PtyManager>,
        shell_service: Arc<ShellService>,
        recorder: Arc<SessionRecorder>,
        spawn_hooks: SpawnHookChain,
        vault_service: Arc<VaultService>,
    ) -> Self {
        let boot_id = uuid::Uuid::new_v4().to_string();
        Self {
            inner: Arc::new(WorkspaceManagerInner {
                boot_id,
                event_seq: AtomicU64::new(0),
                restored: AtomicBool::new(false),
                restore_report: RwLock::new(None),
                db,
                pty_manager,
                shell_service,
                recorder,
                spawn_hooks,
                observers: RwLock::new(Vec::new()),
                spawn_guards: Mutex::new(HashMap::new()),
                app_handle: RwLock::new(None),
                tabs: RwLock::new(HashMap::new()),
                pane_to_tab: RwLock::new(HashMap::new()),
                pty_to_pane: RwLock::new(HashMap::new()),
                tab_locks: Mutex::new(HashMap::new()),
                restore_lock: AsyncMutex::new(()),
                vault_service,
            }),
        }
    }

    /// Retorna uma referência ao serviço do cofre de credenciais.
    #[must_use]
    pub fn vault(&self) -> &Arc<VaultService> {
        &self.inner.vault_service
    }

    /// Registra o listener de eventos de PTY do `WorkspaceManager` no `PtyManager`.
    pub fn install_pty_listener(&self) {
        let listener = Arc::new(WorkspacePtyListener {
            manager: self.clone(),
        });
        self.inner.pty_manager.add_listener(listener);
    }

    /// Registra um observador de ciclo de vida de abas/painéis/sessões (Fases 08, 09, 12).
    pub fn register_observer(&self, observer: Arc<dyn WorkspaceLifecycleObserver>) {
        self.inner.observers.write().push(observer);
    }

    /// Associa o `AppHandle` para emissão de eventos Tauri.
    pub fn attach_app_handle(&self, handle: AppHandle) {
        *self.inner.app_handle.write() = Some(handle);
    }

    /// Retorna o `boot_id` desta instância.
    #[must_use]
    pub fn boot_id(&self) -> &str {
        &self.inner.boot_id
    }

    /// Retorna o `SessionRecorder` associado.
    #[must_use]
    pub fn recorder(&self) -> &Arc<SessionRecorder> {
        &self.inner.recorder
    }

    /// Retorna a cadeia de hooks de spawn.
    #[must_use]
    pub fn spawn_hooks(&self) -> &SpawnHookChain {
        &self.inner.spawn_hooks
    }

    /// Lista todos os painéis com PTY ativo no momento na forma `(tab_id, pane_id, pty_id, cwd)`.
    #[must_use]
    pub fn active_pane_targets(&self) -> Vec<(TabId, PaneId, PtyId, String)> {
        let tabs = self.inner.tabs.read();
        let mut out = Vec::new();
        for tab in tabs.values() {
            for pane in tab.panes.values() {
                if let Some(pty_id) = pane.pty_id {
                    out.push((tab.id, pane.pane_id, pty_id, pane.cwd.clone()));
                }
            }
        }
        out
    }

    /// Retorna `(tab_id, pty_id, cwd)` para um `pane_id` aberto.
    #[must_use]
    pub fn pane_info(&self, pane_id: PaneId) -> Option<(TabId, Option<PtyId>, String)> {
        let tab_id = *self.inner.pane_to_tab.read().get(&pane_id)?;
        let tabs = self.inner.tabs.read();
        let tab = tabs.get(&tab_id)?;
        let pane = tab.panes.get(&pane_id)?;
        Some((tab_id, pane.pty_id, pane.cwd.clone()))
    }

    /// Retorna `(project_id, tab_id, tab_session_id, session_id, pty_id, cwd)` para um `pane_id` aberto.
    #[must_use]
    pub fn pane_full_info(
        &self,
        pane_id: PaneId,
    ) -> Option<(
        ProjectId,
        TabId,
        TabSessionId,
        SessionId,
        Option<PtyId>,
        String,
    )> {
        let tab_id = *self.inner.pane_to_tab.read().get(&pane_id)?;
        let tabs = self.inner.tabs.read();
        let tab = tabs.get(&tab_id)?;
        let pane = tab.panes.get(&pane_id)?;
        Some((
            tab.project_id,
            tab_id,
            tab.tab_session_id,
            pane.session_id,
            pane.pty_id,
            pane.cwd.clone(),
        ))
    }

    /// Retorna `(tab_id, pty_id, session_id, generation, initial_dispatch)` para um `pane_id` ativo.
    #[must_use]
    pub fn pane_runtime_meta(
        &self,
        pane_id: PaneId,
    ) -> Option<(TabId, PtyId, SessionId, u64, InitialDispatchState)> {
        let tab_id = *self.inner.pane_to_tab.read().get(&pane_id)?;
        let tabs = self.inner.tabs.read();
        let tab = tabs.get(&tab_id)?;
        let pane = tab.panes.get(&pane_id)?;
        let pty_id = pane.pty_id?;
        let generation = u64::try_from(pane.generation).unwrap_or(1);
        Some((
            tab_id,
            pty_id,
            pane.session_id,
            generation,
            pane.initial_dispatch,
        ))
    }

    /// Retorna todos os `PaneId` abertos de uma aba.
    #[must_use]
    pub fn tab_pane_ids(&self, tab_id: TabId) -> Vec<PaneId> {
        let tabs = self.inner.tabs.read();
        tabs.get(&tab_id)
            .map(|t| t.panes.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Retorna `(project_id, active_pane_cwd)` para uma aba aberta.
    #[must_use]
    pub fn tab_project_and_cwd(&self, tab_id: TabId) -> Option<(ProjectId, String)> {
        let tabs = self.inner.tabs.read();
        let tab = tabs.get(&tab_id)?;
        let cwd = tab
            .panes
            .get(&tab.active_pane_id)
            .map_or_else(String::new, |p| p.cwd.clone());
        Some((tab.project_id, cwd))
    }

    /// Retorna `(project_id, title, continuous)` para uma aba aberta.
    #[must_use]
    pub fn tab_meta(&self, tab_id: TabId) -> Option<(ProjectId, String, bool)> {
        let tabs = self.inner.tabs.read();
        let tab = tabs.get(&tab_id)?;
        Some((tab.project_id, tab.title.clone(), tab.continuous))
    }

    fn release_pty_guards(&self, pty_id: PtyId) {
        if let Some(guards) = self.inner.spawn_guards.lock().remove(&pty_id) {
            for g in guards {
                g.release();
            }
        }
    }

    fn next_envelope<T>(&self, payload: T) -> WorkspaceEvent<T> {
        let seq = self.inner.event_seq.fetch_add(1, Ordering::SeqCst) + 1;
        WorkspaceEvent {
            boot_id: self.inner.boot_id.clone(),
            event_seq: seq,
            payload,
        }
    }

    fn emit_tab_opened(&self, view: TabView) {
        let env = self.next_envelope(view);
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = TabOpenedEvent(env).emit(app);
        }
    }

    fn emit_tab_closed(&self, tab_id: TabId, project_id: ProjectId, revision: i64) {
        let env = self.next_envelope(TabClosedPayload {
            tab_id,
            project_id,
            revision,
        });
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = TabClosedEvent(env).emit(app);
        }
        let observers = self.inner.observers.read().clone();
        for obs in observers {
            obs.on_tab_closed(project_id, tab_id);
        }
    }

    fn emit_tab_reordered(&self, project_id: ProjectId, ordered_tab_ids: Vec<TabId>) {
        let env = self.next_envelope(TabReorderedPayload {
            project_id,
            ordered_tab_ids,
        });
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = TabReorderedEvent(env).emit(app);
        }
    }

    fn emit_tab_layout_changed(&self, view: TabView) {
        let env = self.next_envelope(view);
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = TabLayoutChangedEvent(env).emit(app);
        }
    }

    fn emit_pane_pty_replaced(&self, payload: PanePtyReplacedPayload) {
        let env = self.next_envelope(payload);
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = PanePtyReplacedEvent(env).emit(app);
        }
    }

    fn emit_pane_exited(&self, payload: PaneExitedPayload) {
        let env = self.next_envelope(payload);
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = PaneExitedEvent(env).emit(app);
        }
    }

    fn emit_session_started(&self, summary: SessionSummary) {
        let env = self.next_envelope(summary);
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = SessionStartedEvent(env).emit(app);
        }
    }

    fn emit_session_archived(&self, summary: SessionSummary) {
        let tab_id = summary.tab_id;
        let tab_session_id = summary.id;
        let env = self.next_envelope(summary);
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = SessionArchivedEvent(env).emit(app);
        }
        let observers = self.inner.observers.read().clone();
        for obs in observers {
            obs.on_session_archived(tab_id, tab_session_id);
        }
    }

    fn emit_template_changed(&self, project_id: ProjectId, template_ids: Vec<TemplateId>) {
        let env = self.next_envelope(TemplateChangedPayload {
            project_id,
            template_ids,
        });
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = TemplateChangedEvent(env).emit(app);
        }
    }

    fn emit_workspace_restored(&self, report: WorkspaceRestoreReport) {
        let env = self.next_envelope(report);
        if let Some(app) = self.inner.app_handle.read().as_ref() {
            let _ = WorkspaceRestoredEvent(env).emit(app);
        }
    }

    fn tab_lock(&self, tab_id: TabId) -> Arc<AsyncMutex<()>> {
        let mut locks = self.inner.tab_locks.lock();
        Arc::clone(
            locks
                .entry(tab_id)
                .or_insert_with(|| Arc::new(AsyncMutex::new(()))),
        )
    }

    /// Executa (uma única vez por boot) a reconciliação e restauração inicial do workspace.
    pub async fn ensure_restored(&self) -> MtResult<WorkspaceRestoreReport> {
        if self.inner.restored.load(Ordering::Acquire)
            && let Some(rep) = self.inner.restore_report.read().clone()
        {
            return Ok(rep);
        }

        let _guard = self.inner.restore_lock.lock().await;
        if self.inner.restored.load(Ordering::Acquire)
            && let Some(rep) = self.inner.restore_report.read().clone()
        {
            return Ok(rep);
        }

        let report =
            run_boot_reconciliation_and_restore(&self.inner.db, self, &self.inner.boot_id).await?;
        *self.inner.restore_report.write() = Some(report.clone());
        self.inner.restored.store(true, Ordering::Release);
        self.emit_workspace_restored(report.clone());
        Ok(report)
    }

    /// Retorna o snapshot atômico de hidratação do workspace (`workspace_snapshot`).
    pub async fn workspace_snapshot(&self) -> MtResult<WorkspaceSnapshot> {
        let restore_report = self.ensure_restored().await.ok();
        let event_seq = self.inner.event_seq.load(Ordering::SeqCst);
        let mut tabs: Vec<TabView> = self
            .inner
            .tabs
            .read()
            .values()
            .map(LiveTabEntry::to_view)
            .collect();
        tabs.sort_by(|a, b| {
            a.project_id
                .to_string()
                .cmp(&b.project_id.to_string())
                .then(a.position.cmp(&b.position))
        });

        Ok(WorkspaceSnapshot {
            boot_id: self.inner.boot_id.clone(),
            event_seq,
            restored: self.inner.restored.load(Ordering::Acquire),
            tabs,
            restore_report,
        })
    }

    // -------------------------------------------------------------------------
    // Templates CRUD
    // -------------------------------------------------------------------------

    /// Lista todos os templates configurados em um projeto.
    pub async fn template_list(&self, project_id: ProjectId) -> MtResult<Vec<TabTemplate>> {
        self.inner
            .db
            .templates()
            .list_by_project(project_id)
            .await
            .map_err(map_db_error)
    }

    /// Obtém um template específico pelo identificador.
    pub async fn template_get(&self, template_id: TemplateId) -> MtResult<TabTemplate> {
        let tpl = self
            .inner
            .db
            .templates()
            .get(template_id)
            .await
            .map_err(map_db_error)?;
        tpl.ok_or_else(|| {
            MtError::new(
                codes::TEMPLATE_NOT_FOUND,
                format!("Template '{template_id}' não encontrado."),
            )
        })
    }

    /// Testa a configuração SSH sem salvar o template ou alterar credenciais.
    pub async fn template_test_connection(
        &self,
        project_id: ProjectId,
        template_id: Option<TemplateId>,
        connection: mt_core::workspace::TemplateConnectionInput,
        cwd: Option<String>,
    ) -> MtResult<String> {
        use mt_core::workspace::{
            PasswordCredentialOperation, SshAuth, SshAuthInput, TemplateConnection,
            TemplateConnectionInput,
        };

        let TemplateConnectionInput::Ssh { host, user, auth } = connection else {
            return Err(MtError::invalid_argument(
                "connection",
                "Selecione uma conexão SSH para testar.",
            ));
        };
        let host = mt_core::workspace::template::validate_ssh_host(&host)?;
        let user = mt_core::workspace::template::validate_ssh_user(&user)?;
        let (auth, password) = match auth {
            SshAuthInput::Key { identity_file } => (
                SshAuth::Key {
                    identity_file: mt_core::workspace::template::validate_ssh_identity_file(
                        identity_file.as_deref(),
                    )?,
                },
                None,
            ),
            SshAuthInput::Password { operation } => {
                let password = match operation {
                    PasswordCredentialOperation::Set { password } if !password.is_empty() => {
                        zeroize::Zeroizing::new(password)
                    }
                    PasswordCredentialOperation::Keep => {
                        let template_id = template_id.ok_or_else(|| {
                            MtError::invalid_argument(
                                "templateId",
                                "Informe a senha SSH para testar um novo template.",
                            )
                        })?;
                        let existing = self.template_get(template_id).await?;
                        if existing.project_id != project_id {
                            return Err(MtError::new(
                                codes::TEMPLATE_INVALID,
                                "O template informado pertence a outro projeto.",
                            ));
                        }
                        let TemplateConnection::Ssh {
                            auth: SshAuth::Password { credential_id },
                            ..
                        } = existing.config.connection
                        else {
                            return Err(MtError::new(
                                codes::TEMPLATE_INVALID,
                                "O template não possui senha SSH salva.",
                            ));
                        };
                        self.inner
                            .vault_service
                            .retrieve_credential(project_id, &credential_id)
                            .await?
                    }
                    _ => {
                        return Err(MtError::invalid_argument(
                            "password",
                            "Informe uma senha SSH válida para testar a conexão.",
                        ));
                    }
                };
                (
                    SshAuth::Password {
                        credential_id: String::new(),
                    },
                    Some(password),
                )
            }
        };
        crate::remote::ssh::test_connection(
            &TemplateConnection::Ssh { host, user, auth },
            password,
            cwd.as_deref(),
        )
        .await
    }

    /// Cria um novo template de aba no projeto validando segredos e ambiente.
    pub async fn template_create(
        &self,
        project_id: ProjectId,
        mut input: TabTemplateInput,
    ) -> MtResult<TabTemplate> {
        mt_core::workspace::validate_template_input(&input)?;
        EnvironmentResolver::validate_environment_available(&input.environment)?;

        if matches!(input.connection, mt_core::workspace::TemplateConnectionInput::Local)
            && let Some(ref custom_cwd) = input.cwd
            && let Some(proj) = self.load_project_path(project_id).await?
        {
            let _ = EnvironmentResolver::resolve_cwd(&proj, Some(custom_cwd), false)?;
        }

        // Se a conexão for SSH com senha informada (Set), armazena no cofre e sanitiza o payload
        if let mt_core::workspace::TemplateConnectionInput::Ssh {
            ref host,
            ref user,
            ref auth,
        } = input.connection
        {
            if let mt_core::workspace::SshAuthInput::Password {
                operation: mt_core::workspace::PasswordCredentialOperation::Set { password },
            } = auth
            {
                let credential_id = self
                    .inner
                    .vault_service
                    .store_credential(project_id, &password)
                    .await?;
                input.connection = mt_core::workspace::TemplateConnectionInput::Ssh {
                    host: host.clone(),
                    user: user.clone(),
                    auth: mt_core::workspace::SshAuthInput::Password {
                        operation: mt_core::workspace::PasswordCredentialOperation::KeepExisting {
                            credential_id,
                        },
                    },
                };
            }
        }

        let now_ms = mt_db::time::now_unix_ms();
        let created = self
            .inner
            .db
            .templates()
            .create(project_id, input, now_ms)
            .await
            .map_err(map_db_error)?;
        self.emit_template_changed(project_id, vec![created.id]);
        Ok(created)
    }

    /// Atualiza um template existente validando `expected_revision`, segredos e ambiente.
    pub async fn template_update(
        &self,
        template_id: TemplateId,
        expected_revision: i64,
        mut input: TabTemplateInput,
    ) -> MtResult<TabTemplate> {
        mt_core::workspace::validate_template_input(&input)?;
        EnvironmentResolver::validate_environment_available(&input.environment)?;

        let existing = self.template_get(template_id).await?;
        if matches!(input.connection, mt_core::workspace::TemplateConnectionInput::Local)
            && let Some(ref custom_cwd) = input.cwd
            && let Some(proj) = self.load_project_path(existing.project_id).await?
        {
            let _ = EnvironmentResolver::resolve_cwd(&proj, Some(custom_cwd), false)?;
        }

        // Se a conexão for SSH com nova senha (Set), atualiza ou armazena no cofre
        if let mt_core::workspace::TemplateConnectionInput::Ssh {
            ref host,
            ref user,
            ref auth,
        } = input.connection
        {
            if let mt_core::workspace::SshAuthInput::Password {
                operation: mt_core::workspace::PasswordCredentialOperation::Set { password },
            } = auth
            {
                let existing_cred_id = if let mt_core::workspace::TemplateConnection::Ssh {
                    auth: mt_core::workspace::SshAuth::Password { credential_id },
                    ..
                } = &existing.config.connection
                {
                    Some(credential_id.as_str())
                } else {
                    None
                };

                let credential_id = self
                    .inner
                    .vault_service
                    .update_or_store_credential(existing.project_id, existing_cred_id, &password)
                    .await?;

                input.connection = mt_core::workspace::TemplateConnectionInput::Ssh {
                    host: host.clone(),
                    user: user.clone(),
                    auth: mt_core::workspace::SshAuthInput::Password {
                        operation: mt_core::workspace::PasswordCredentialOperation::KeepExisting {
                            credential_id,
                        },
                    },
                };
            }
        }

        let now_ms = mt_db::time::now_unix_ms();
        let updated = self
            .inner
            .db
            .templates()
            .update(template_id, expected_revision, input, now_ms)
            .await
            .map_err(map_db_error)?;
        self.emit_template_changed(updated.project_id, vec![updated.id]);
        Ok(updated)
    }

    /// Remove um template não-padrão de um projeto.
    pub async fn template_delete(
        &self,
        template_id: TemplateId,
        expected_revision: i64,
    ) -> MtResult<()> {
        let existing = self.template_get(template_id).await?;
        let project_id = self
            .inner
            .db
            .templates()
            .delete(template_id, expected_revision)
            .await
            .map_err(map_db_error)?;

        if let mt_core::workspace::TemplateConnection::Ssh {
            auth: mt_core::workspace::SshAuth::Password { ref credential_id },
            ..
        } = existing.config.connection
        {
            let _ = self.inner.vault_service.delete_credential(credential_id).await;
        }

        self.emit_template_changed(project_id, vec![template_id]);
        Ok(())
    }

    /// Define um template como padrão do projeto.
    pub async fn template_set_default(
        &self,
        template_id: TemplateId,
    ) -> MtResult<Vec<TabTemplate>> {
        let now_ms = mt_db::time::now_unix_ms();
        let list = self
            .inner
            .db
            .templates()
            .set_default(template_id, now_ms)
            .await
            .map_err(map_db_error)?;
        if let Some(first) = list.first() {
            let ids = list.iter().map(|t| t.id).collect();
            self.emit_template_changed(first.project_id, ids);
        }
        Ok(list)
    }

    // -------------------------------------------------------------------------
    // Abas
    // -------------------------------------------------------------------------

    /// Lista todas as abas abertas de um projeto ordenadas por `position`.
    pub async fn tab_list(&self, project_id: ProjectId) -> MtResult<Vec<TabView>> {
        let _ = self.ensure_restored().await;
        let mut list: Vec<TabView> = self
            .inner
            .tabs
            .read()
            .values()
            .filter(|t| t.project_id == project_id && t.lifecycle == TabLifecycle::Ready)
            .map(LiveTabEntry::to_view)
            .collect();
        list.sort_by_key(|t| t.position);
        Ok(list)
    }

    /// Abre uma nova aba em um projeto a partir do template informado ou do template padrão.
    pub async fn tab_open(&self, request: TabOpenRequest) -> MtResult<TabView> {
        let _ = self.ensure_restored().await;
        self.open_tab_internal(request, TabOrigin::Manual, false)
            .await
    }

    pub(crate) async fn open_workspace_restore_tab(
        &self,
        request: TabOpenRequest,
    ) -> MtResult<TabView> {
        self.open_tab_internal(request, TabOrigin::Workspace, true)
            .await
    }

    #[allow(clippy::too_many_lines)]
    async fn open_tab_internal(
        &self,
        request: TabOpenRequest,
        origin: TabOrigin,
        allow_fallback_cwd: bool,
    ) -> MtResult<TabView> {
        let args_hash = hash_operation_args(&request);
        let boot_id = self.inner.boot_id.clone();
        let req_id = request.request_id.clone();

        // Verifica idempotência em workspace_operations
        let prep = self
            .inner
            .db
            .write_sized("workspace_op_prepare_tab_open", 512, move |tx| {
                prepare_operation_tx(
                    tx,
                    &req_id,
                    "tab_open",
                    &args_hash,
                    None,
                    &boot_id,
                    mt_db::time::now_unix_ms(),
                )
            })
            .await
            .map_err(map_db_error)?;

        if let PrepareOperationOutcome::ExistingSameArgs(row) = prep
            && let Some(tid) = row.tab_id
            && let Some(entry) = self.inner.tabs.read().get(&tid)
        {
            return Ok(entry.to_view());
        }

        let project_path = self
            .load_project_path(request.project_id)
            .await?
            .ok_or_else(|| {
                MtError::new(
                    codes::PROJECT_NOT_FOUND,
                    format!("Projeto '{}' não encontrado.", request.project_id),
                )
            })?;

        let template = match request.template_id {
            Some(tid) => self.template_get(tid).await?,
            None => self
                .inner
                .db
                .templates()
                .get_default(request.project_id)
                .await
                .map_err(map_db_error)?
                .ok_or_else(|| {
                    MtError::new(
                        codes::TEMPLATE_NOT_FOUND,
                        format!(
                            "Projeto '{}' não possui template padrão configurado.",
                            request.project_id
                        ),
                    )
                })?,
        };

        if template.project_id != request.project_id {
            return Err(MtError::new(
                codes::TEMPLATE_INVALID,
                "O template informado pertence a outro projeto.",
            ));
        }

        let overrides = request.overrides.clone().unwrap_or_default();
        let mut snapshot = LaunchSnapshot {
            template_id: Some(template.id),
            title: template.config.name.clone(),
            icon: template.config.icon.clone(),
            resolved_cwd: project_path.to_string_lossy().into_owned(),
            configured_cwd: template.config.cwd.clone(),
            initial_command: template.config.initial_command.clone(),
            continuous: template.config.continuous,
            environment: template.config.environment.clone(),
            env_vars: template.config.env_vars.clone(),
            connection: template.config.connection.clone(),
        };

        if let Some(ref title_ov) = overrides.title {
            let trimmed = title_ov.trim();
            if !trimmed.is_empty() {
                snapshot.title = trimmed.to_string();
            }
        }
        if let Some(ref cwd_ov) = overrides.cwd {
            snapshot.configured_cwd = Some(cwd_ov.clone());
        }
        if let Some(cont_ov) = overrides.continuous {
            snapshot.continuous = cont_ov;
        }
        match &overrides.initial_command {
            CommandOverride::Inherit => {}
            CommandOverride::Clear => {
                snapshot.initial_command = None;
            }
            CommandOverride::Replace { command } => {
                mt_core::workspace::validate_command_text(command)?;
                let (_, report) = sanitize_with(command);
                if report.total_redactions > 0 {
                    return Err(MtError::new(
                        codes::TEMPLATE_SECRET_LITERAL,
                        "Segredo detectado no comando inicial informado.",
                    ));
                }
                snapshot.initial_command = Some(command.clone());
            }
        }
        if !overrides.extra_env.is_empty() {
            mt_core::workspace::validate_env_map(&overrides.extra_env)?;
            for (k, v) in &overrides.extra_env {
                if let mt_core::workspace::EnvValue::Literal { value } = v {
                    let (_, report) = sanitize_with(value);
                    if report.total_redactions > 0 {
                        return Err(MtError::new(
                            codes::TEMPLATE_SECRET_LITERAL,
                            format!(
                                "Segredo literal detectado na variável '{k}'; utilize variáveis herdadas."
                            ),
                        ));
                    }
                }
                snapshot.env_vars.insert(k.clone(), v.clone());
            }
        }

        let resolved_env = EnvironmentResolver::resolve_snapshot(
            &project_path,
            &snapshot,
            None,
            allow_fallback_cwd
                || overrides.cwd_fallback_to_project
                || origin == TabOrigin::Workspace,
        )?;
        let initial_cwd_str = resolved_env.cwd.to_string_lossy().into_owned();
        snapshot.resolved_cwd = initial_cwd_str.clone();

        let existing_workspace_tab = if origin == TabOrigin::Workspace {
            self.inner
                .db
                .tabs()
                .find_workspace_tab_for_template(template.id)
                .await
                .map_err(map_db_error)?
        } else {
            None
        };

        let now_ms = mt_db::time::now_unix_ms();
        let tab_session_id = TabSessionId::new();
        let has_initial_cmd = snapshot
            .initial_command
            .as_ref()
            .is_some_and(|c| !c.trim().is_empty());
        let initial_dispatch_state = if has_initial_cmd {
            InitialDispatchState::Pending
        } else {
            InitialDispatchState::None
        };

        let snapshot_for_tx = snapshot.clone();
        let cwd_for_tx = initial_cwd_str.clone();
        let req_id_stage = request.request_id.clone();

        let (tab_row, pane_rows, session_summary, pane_sessions_map) = self
            .inner
            .db
            .write_sized("tab_open_begin_tx", 4096, move |tx| {
                let sanitized_title = SanitizedText::new(&snapshot_for_tx.title);
                let sanitized_cwd = SanitizedText::new(&cwd_for_tx);
                let (tab_row, panes) = if let Some(prev_tab) = existing_workspace_tab {
                    let fallback_pane_id = PaneId::new();
                    let (reopened_tab, reopened_panes, _) = reopen_workspace_tab_tx(
                        tx,
                        prev_tab.id,
                        &sanitized_title,
                        &snapshot_for_tx.icon,
                        snapshot_for_tx.continuous,
                        &snapshot_for_tx,
                        fallback_pane_id,
                        &sanitized_cwd,
                        now_ms,
                    )?;
                    (reopened_tab, reopened_panes)
                } else {
                    let new_tab_id = TabId::new();
                    let initial_pane_id = PaneId::new();
                    let (created_tab, created_pane) = create_tab_with_initial_pane_tx(
                        tx,
                        &NewTabTxInput {
                            id: new_tab_id,
                            project_id: request.project_id,
                            template_id: Some(template.id),
                            origin,
                            title: sanitized_title,
                            icon: snapshot_for_tx.icon.clone(),
                            continuous: snapshot_for_tx.continuous,
                            launch_snapshot: snapshot_for_tx,
                            initial_pane_id,
                            initial_cwd: sanitized_cwd,
                            opened_at: now_ms,
                        },
                    )?;
                    (created_tab, vec![created_pane])
                };

                let summary = start_tab_session_tx(tx, tab_session_id, tab_row.id, now_ms)?;
                let mut pane_sessions = HashMap::new();
                for pane in &panes {
                    let sid = SessionId::new();
                    let disp = if pane.id == tab_row.primary_pane_id {
                        initial_dispatch_state
                    } else {
                        InitialDispatchState::None
                    };
                    insert_session_tx(
                        tx,
                        &NewSession {
                            id: sid,
                            tab_id: Some(tab_row.id),
                            pane_id: Some(pane.id),
                            project_id: Some(tab_row.project_id),
                            tab_session_id: Some(tab_session_id),
                            pty_id: PtyId::new(),
                            shell_kind: ShellKind::Sh,
                            shell_executable: SanitizedText::new("shell"),
                            initial_cwd: SanitizedText::new(&pane.cwd),
                            integration_mode: IntegrationMode::Pending,
                            started_at: now_ms,
                        },
                    )?;
                    attach_pane_session_tx(
                        tx,
                        sid,
                        tab_session_id,
                        pane.id,
                        pane.generation,
                        now_ms,
                        disp,
                    )?;
                    pane_sessions.insert(pane.id, (sid, disp));
                }

                update_operation_stage_tx(
                    tx,
                    &req_id_stage,
                    OperationStage::Committed,
                    Some(tab_row.id),
                    None,
                    now_ms,
                )?;

                Ok((tab_row, panes, summary, pane_sessions))
            })
            .await
            .map_err(map_db_error)?;

        // Spawna um PTY para cada painel da aba
        let mut live_panes = BTreeMap::new();
        let mut spawned_ptys = Vec::new();

        for pane_row in &pane_rows {
            let (session_id, disp_state) = pane_sessions_map
                .get(&pane_row.id)
                .copied()
                .unwrap_or_else(|| (SessionId::new(), InitialDispatchState::None));

            let pane_cwd = PathBuf::from(&pane_row.cwd);
            let effective_cwd = if matches!(
                snapshot.connection,
                mt_core::workspace::TemplateConnection::Ssh { .. }
            ) || pane_cwd.is_dir() {
                pane_cwd
            } else {
                resolved_env.cwd.clone()
            };

            match self
                .spawn_pane_pty(
                    tab_row.id,
                    tab_row.project_id,
                    pane_row.id,
                    session_id,
                    tab_session_id,
                    pane_row.generation,
                    tab_row.continuous,
                    &effective_cwd,
                    &resolved_env.env,
                    &snapshot.connection,
                )
                .await
            {
                Ok(pty_id) => {
                    spawned_ptys.push((pane_row.id, session_id, pty_id, pane_row.generation));
                    live_panes.insert(
                        pane_row.id,
                        LivePaneEntry {
                            pane_id: pane_row.id,
                            session_id,
                            pty_id: Some(pty_id),
                            generation: pane_row.generation,
                            cwd: effective_cwd.to_string_lossy().into_owned(),
                            state: PaneRuntimeState::Live,
                            exit: None,
                            error: None,
                            initial_dispatch: disp_state,
                        },
                    );
                }
                Err(spawn_err) => {
                    // Compensação: encerra PTYs já abertos e arquiva a tab_session / fecha a aba
                    for (_, _, pty_id, _) in spawned_ptys {
                        self.inner.recorder.unregister_pty(pty_id);
                        self.release_pty_guards(pty_id);
                        let _ = self.inner.pty_manager.kill(pty_id, KillMode::Force);
                        let _ = self.inner.shell_service.finalize_pty_session(
                            pty_id,
                            None,
                            Some(String::from("SIGKILL")),
                            mt_db::time::now_unix_ms(),
                        );
                    }
                    let _ = self.inner.shell_service.remove_tab(tab_row.id);

                    let fail_tab_id = tab_row.id;
                    let fail_req_id = request.request_id.clone();
                    let all_sids: Vec<SessionId> =
                        pane_sessions_map.values().map(|(sid, _)| *sid).collect();
                    let _ = self
                        .inner
                        .db
                        .write_sized("tab_open_rollback_tx", 1024, move |tx| {
                            let now = mt_db::time::now_unix_ms();
                            for sid in all_sids {
                                let _ = close_pane_session_tx(
                                    tx,
                                    sid,
                                    &SessionEnd {
                                        ended_at: now,
                                        exit_code: None,
                                        exit_signal: Some(String::from("SPAWN_FAILED")),
                                        last_cwd: None,
                                        integration_mode: IntegrationMode::Disabled,
                                        command_count: 0,
                                        error_count: 0,
                                    },
                                );
                            }
                            let _ = archive_tab_session_tx(
                                tx,
                                tab_session_id,
                                SessionEndReason::SpawnFailed,
                                now,
                            );
                            let _ = close_tab_tx(tx, fail_tab_id, now);
                            let _ = update_operation_stage_tx(
                                tx,
                                &fail_req_id,
                                OperationStage::Interrupted,
                                Some(fail_tab_id),
                                None,
                                now,
                            );
                            Ok(())
                        })
                        .await;
                    return Err(spawn_err);
                }
            }
        }

        let live_tab = LiveTabEntry {
            id: tab_row.id,
            project_id: tab_row.project_id,
            template_id: tab_row.template_id,
            title: tab_row.title.clone(),
            icon: tab_row.icon.clone(),
            position: tab_row.position,
            origin: tab_row.origin,
            continuous: tab_row.continuous,
            lifecycle: TabLifecycle::Ready,
            revision: tab_row.revision,
            layout_revision: tab_row.layout_revision,
            layout: tab_row.layout.clone(),
            active_pane_id: tab_row.active_pane_id,
            primary_pane_id: tab_row.primary_pane_id,
            tab_session_id,
            launch_snapshot: tab_row.launch_snapshot,
            panes: live_panes,
        };

        let view = live_tab.to_view();
        {
            let mut tabs = self.inner.tabs.write();
            let mut p2t = self.inner.pane_to_tab.write();
            let mut pty_map = self.inner.pty_to_pane.write();
            for pane_id in live_tab.panes.keys() {
                p2t.insert(*pane_id, live_tab.id);
            }
            for (pane_id, sid, pty_id, generation_num) in spawned_ptys {
                pty_map.insert(pty_id, (live_tab.id, pane_id, sid, generation_num));
            }
            tabs.insert(live_tab.id, live_tab);
        }

        let result_json = serde_json::to_string(&view).ok();
        let req_id_commit = request.request_id.clone();
        let committed_tab_id = tab_row.id;
        let _ = self
            .inner
            .db
            .write_sized("tab_open_commit_op", 1024, move |tx| {
                update_operation_stage_tx(
                    tx,
                    &req_id_commit,
                    OperationStage::Completed,
                    Some(committed_tab_id),
                    result_json.as_deref(),
                    mt_db::time::now_unix_ms(),
                )
            })
            .await;

        self.emit_session_started(session_summary);
        self.emit_tab_opened(view.clone());
        Ok(view)
    }

    #[allow(clippy::too_many_arguments)]
    async fn spawn_pane_pty(
        &self,
        tab_id: TabId,
        project_id: ProjectId,
        pane_id: PaneId,
        session_id: SessionId,
        tab_session_id: TabSessionId,
        _generation: i64,
        continuous: bool,
        cwd: &std::path::Path,
        base_env: &BTreeMap<String, String>,
        connection: &mt_core::workspace::TemplateConnection,
    ) -> MtResult<PtyId> {
        match connection {
            mt_core::workspace::TemplateConnection::Local => {
                let mut env = base_env.clone();
                let guards = self
                    .inner
                    .spawn_hooks
                    .apply(project_id, tab_id, pane_id, cwd, &mut env)?;

                let spec = SpawnSpec {
                    shell: None,
                    args: Vec::new(),
                    login: true,
                    cwd: Some(cwd.to_string_lossy().into_owned()),
                    env,
                    env_remove: Vec::new(),
                    size: PtySize::default(),
                    owner: PtyOwner {
                        tab_id: Some(tab_id),
                        pane_id: Some(pane_id),
                    },
                };

                let integration_req = ShellIntegrationRequest {
                    enabled: true,
                    continuous_policy: if continuous {
                        ContinuousPolicy::Always
                    } else {
                        ContinuousPolicy::Auto
                    },
                    project_id: Some(project_id),
                    tab_session_id: Some(tab_session_id),
                };

                let summary = match self
                    .inner
                    .shell_service
                    .spawn_pty_with_session(spec, session_id, integration_req)
                {
                    Ok(s) => s,
                    Err(err) => {
                        for g in guards {
                            g.release();
                        }
                        return Err(tab_open_failed(format!(
                            "Falha ao abrir PTY do painel: {err}"
                        )));
                    }
                };

                let pty_id = summary.id;
                if !guards.is_empty() {
                    self.inner.spawn_guards.lock().insert(pty_id, guards);
                }

                self.inner
                    .recorder
                    .register_pty(tab_id, pane_id, session_id, tab_session_id, pty_id);

                let observers = self.inner.observers.read().clone();
                for obs in observers {
                    obs.on_pane_spawned(tab_id, pane_id, pty_id, session_id);
                }

                Ok(pty_id)
            }
            mt_core::workspace::TemplateConnection::Ssh { host: _, user: _, auth } => {
                let password_opt = match auth {
                    mt_core::workspace::SshAuth::Key { .. } => None,
                    mt_core::workspace::SshAuth::Password { credential_id } => {
                        let pass = self
                            .inner
                            .vault_service
                            .retrieve_credential(project_id, credential_id)
                            .await?;
                        Some(pass)
                    }
                };

                let ssh_session = crate::remote::ssh::build_ssh_spawn(
                    connection,
                    password_opt,
                    tab_id,
                    pane_id,
                    PtySize::default(),
                )?;

                let mut spec = ssh_session.spec.clone();
                crate::remote::ssh::configure_remote_shell(&mut spec, cwd.to_str())?;
                let integration_req = ShellIntegrationRequest {
                    enabled: false,
                    continuous_policy: ContinuousPolicy::Never,
                    project_id: Some(project_id),
                    tab_session_id: Some(tab_session_id),
                };

                let summary = match self
                    .inner
                    .shell_service
                    .spawn_pty_with_session(spec, session_id, integration_req)
                {
                    Ok(s) => s,
                    Err(err) => {
                        return Err(MtError::new(
                            codes::SSH_CONNECTION_FAILED,
                            format!("Falha ao iniciar processo OpenSSH para o painel: {err}"),
                        ));
                    }
                };

                let pty_id = summary.id;
                ssh_session.keep_alive_in_background(std::time::Duration::from_secs(20));

                self.inner
                    .recorder
                    .register_pty(tab_id, pane_id, session_id, tab_session_id, pty_id);

                let observers = self.inner.observers.read().clone();
                for obs in observers {
                    obs.on_pane_spawned(tab_id, pane_id, pty_id, session_id);
                }

                Ok(pty_id)
            }
        }
    }

    /// Fecha uma aba inteira (`tab_close`) encerrando seus PTYs e arquivando sua `tab_session`.
    pub async fn tab_close(&self, tab_id: TabId, request_id: String) -> MtResult<()> {
        let _ = self.ensure_restored().await;
        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let args_hash = hash_operation_args(&(tab_id, &request_id));
        let boot_id = self.inner.boot_id.clone();
        let req_id_prep = request_id.clone();
        let prep = self
            .inner
            .db
            .write_sized("workspace_op_prepare_tab_close", 512, move |tx| {
                prepare_operation_tx(
                    tx,
                    &req_id_prep,
                    "tab_close",
                    &args_hash,
                    Some(tab_id),
                    &boot_id,
                    mt_db::time::now_unix_ms(),
                )
            })
            .await
            .map_err(map_db_error)?;

        if let PrepareOperationOutcome::ExistingSameArgs(row) = prep
            && row.stage == OperationStage::Completed
        {
            return Ok(());
        }

        let live_tab = {
            let mut tabs = self.inner.tabs.write();
            let Some(mut entry) = tabs.remove(&tab_id) else {
                return Err(MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não está aberta."),
                ));
            };
            entry.lifecycle = TabLifecycle::Closing;
            let mut p2t = self.inner.pane_to_tab.write();
            let mut pty_map = self.inner.pty_to_pane.write();
            for p in entry.panes.values() {
                p2t.remove(&p.pane_id);
                if let Some(pty_id) = p.pty_id {
                    pty_map.remove(&pty_id);
                }
            }
            entry
        };

        self.terminate_and_archive_tab_entry(&live_tab, SessionEndReason::TabClosed)
            .await?;

        let req_id_commit = request_id.clone();
        let _ = self
            .inner
            .db
            .write_sized("tab_close_commit_op", 512, move |tx| {
                update_operation_stage_tx(
                    tx,
                    &req_id_commit,
                    OperationStage::Completed,
                    Some(tab_id),
                    Some("null"),
                    mt_db::time::now_unix_ms(),
                )
            })
            .await;

        Ok(())
    }

    async fn terminate_and_archive_tab_entry(
        &self,
        live_tab: &LiveTabEntry,
        reason: SessionEndReason,
    ) -> MtResult<()> {
        let now_ms = mt_db::time::now_unix_ms();
        let mut finalized_ends: HashMap<SessionId, SessionEnd> = HashMap::new();

        for pane in live_tab.panes.values() {
            if let Some(pty_id) = pane.pty_id {
                let _ = self.inner.pty_manager.kill(pty_id, KillMode::Graceful);
                if let Some((sid, end)) =
                    self.inner
                        .shell_service
                        .finalize_pty_session(pty_id, Some(0), None, now_ms)
                {
                    finalized_ends.insert(sid, end);
                }
                self.release_pty_guards(pty_id);
            }
        }
        let _ = self.inner.shell_service.remove_tab(live_tab.id);

        let _ = self
            .inner
            .recorder
            .flush_tab_session(live_tab.tab_session_id)
            .await;

        let tab_id = live_tab.id;
        let tsid = live_tab.tab_session_id;
        let pane_sids: Vec<SessionId> = live_tab.panes.values().map(|p| p.session_id).collect();
        let (keep_per_tab, max_age_days) = read_retention_settings(&self.inner.db);

        let (archived_summary, new_rev) = self
            .inner
            .db
            .write_sized("tab_close_finalize_tx", 2048, move |tx| {
                for sid in pane_sids {
                    let end = finalized_ends.remove(&sid).unwrap_or(SessionEnd {
                        ended_at: now_ms,
                        exit_code: Some(0),
                        exit_signal: None,
                        last_cwd: None,
                        integration_mode: IntegrationMode::Disabled,
                        command_count: 0,
                        error_count: 0,
                    });
                    close_pane_session_tx(tx, sid, &end)?;
                }

                let summary = archive_tab_session_tx(tx, tsid, reason, now_ms)?;
                let closed_tab = if reason == SessionEndReason::AppExit {
                    None
                } else {
                    close_tab_tx(tx, tab_id, now_ms)?
                };
                let rev = closed_tab.map_or(1, |t| t.revision);

                let _ = enforce_tab_sessions_retention_tx(tx, keep_per_tab, max_age_days, now_ms)?;
                Ok((summary, rev))
            })
            .await
            .map_err(map_db_error)?;

        self.emit_session_archived(archived_summary);
        if reason != SessionEndReason::AppExit {
            self.emit_tab_closed(live_tab.id, live_tab.project_id, new_rev);
        }
        Ok(())
    }

    /// Reordena as abas abertas de um projeto (`tab_reorder`).
    pub async fn tab_reorder(
        &self,
        project_id: ProjectId,
        ordered_tab_ids: Vec<TabId>,
    ) -> MtResult<Vec<TabId>> {
        let _ = self.ensure_restored().await;
        let ordered = self
            .inner
            .db
            .tabs()
            .reorder_open_tabs(project_id, ordered_tab_ids)
            .await
            .map_err(map_db_error)?;

        {
            let mut tabs = self.inner.tabs.write();
            for (idx, tid) in ordered.iter().enumerate() {
                if let Some(entry) = tabs.get_mut(tid) {
                    entry.position = i64::try_from(idx).unwrap_or(0);
                    entry.revision += 1;
                }
            }
        }

        self.emit_tab_reordered(project_id, ordered.clone());
        Ok(ordered)
    }

    /// Atualiza o foco de uma aba e opcionalmente de seu painel ativo (`tab_focus`),
    /// acionando `shell_service.tab_mark_seen`.
    pub async fn tab_focus(&self, tab_id: TabId, pane_id: Option<PaneId>) -> MtResult<TabView> {
        let _ = self.ensure_restored().await;
        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let target_pane = {
            let tabs = self.inner.tabs.read();
            let entry = tabs.get(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            let resolved = pane_id.unwrap_or(entry.active_pane_id);
            if !entry.panes.contains_key(&resolved) {
                return Err(pane_not_found());
            }
            resolved
        };

        let now_ms = mt_db::time::now_unix_ms();
        let updated_row = self
            .inner
            .db
            .tabs()
            .update_focus(tab_id, Some(target_pane), now_ms)
            .await
            .map_err(map_db_error)?;

        let _ = self.inner.shell_service.tab_mark_seen(&SeenRequest {
            tab_id,
            pane_id: Some(target_pane),
            up_to_revision: None,
        });

        let view = {
            let mut tabs = self.inner.tabs.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            entry.active_pane_id = target_pane;
            entry.revision = updated_row.revision;
            entry.to_view()
        };

        self.emit_tab_layout_changed(view.clone());
        Ok(view)
    }

    // -------------------------------------------------------------------------
    // Split Panes
    // -------------------------------------------------------------------------

    /// Divide um painel existente horizontal ou verticalmente (`pane_split`).
    #[allow(clippy::too_many_lines)]
    pub async fn pane_split(
        &self,
        pane_id: PaneId,
        direction: SplitDirection,
        expected_layout_revision: i64,
        request_id: String,
    ) -> MtResult<TabView> {
        let _ = self.ensure_restored().await;
        let tab_id = self
            .inner
            .pane_to_tab
            .read()
            .get(&pane_id)
            .copied()
            .ok_or_else(pane_not_found)?;

        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let args_hash = hash_operation_args(&(pane_id, direction, expected_layout_revision));
        let boot_id = self.inner.boot_id.clone();
        let req_id_prep = request_id.clone();
        let prep = self
            .inner
            .db
            .write_sized("workspace_op_prepare_pane_split", 512, move |tx| {
                prepare_operation_tx(
                    tx,
                    &req_id_prep,
                    "pane_split",
                    &args_hash,
                    Some(tab_id),
                    &boot_id,
                    mt_db::time::now_unix_ms(),
                )
            })
            .await
            .map_err(map_db_error)?;

        if let PrepareOperationOutcome::ExistingSameArgs(row) = prep
            && let Some(tid) = row.tab_id
            && let Some(entry) = self.inner.tabs.read().get(&tid)
        {
            return Ok(entry.to_view());
        }

        let (project_id, snapshot, source_cwd, tab_session_id, continuous) = {
            let tabs = self.inner.tabs.read();
            let entry = tabs.get(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            let src_pane = entry.panes.get(&pane_id).ok_or_else(pane_not_found)?;
            let runtime_cwd = match entry.launch_snapshot.connection {
                mt_core::workspace::TemplateConnection::Local => src_pane
                    .pty_id
                    .and_then(|pty_id| self.inner.shell_service.current_cwd(pty_id))
                    .unwrap_or_else(|| src_pane.cwd.clone()),
                mt_core::workspace::TemplateConnection::Ssh { .. } => src_pane.cwd.clone(),
            };
            (
                entry.project_id,
                entry.launch_snapshot.clone(),
                runtime_cwd,
                entry.tab_session_id,
                entry.continuous,
            )
        };

        let project_path = self.load_project_path(project_id).await?.ok_or_else(|| {
            MtError::new(
                codes::PROJECT_NOT_FOUND,
                format!("Projeto '{project_id}' não encontrado."),
            )
        })?;

        let resolved_env = EnvironmentResolver::resolve_snapshot(
            &project_path,
            &snapshot,
            Some(&source_cwd),
            true,
        )?;
        let new_cwd_str = resolved_env.cwd.to_string_lossy().into_owned();
        let new_pane_id = PaneId::new();
        let new_node_id = format!("split-{}", uuid::Uuid::new_v4());
        let new_session_id = SessionId::new();
        let now_ms = mt_db::time::now_unix_ms();
        let cwd_for_tx = new_cwd_str.clone();

        let (updated_tab_row, created_pane_row) = self
            .inner
            .db
            .write_sized("pane_split_tx", 2048, move |tx| {
                let sanitized_cwd = SanitizedText::new(&cwd_for_tx);
                let (t_row, p_row) = split_pane_tx(
                    tx,
                    tab_id,
                    pane_id,
                    new_pane_id,
                    new_node_id,
                    direction,
                    expected_layout_revision,
                    &sanitized_cwd,
                    now_ms,
                )?;
                insert_session_tx(
                    tx,
                    &NewSession {
                        id: new_session_id,
                        tab_id: Some(tab_id),
                        pane_id: Some(new_pane_id),
                        project_id: Some(project_id),
                        tab_session_id: Some(tab_session_id),
                        pty_id: PtyId::new(),
                        shell_kind: ShellKind::Sh,
                        shell_executable: SanitizedText::new("shell"),
                        initial_cwd: sanitized_cwd,
                        integration_mode: IntegrationMode::Pending,
                        started_at: now_ms,
                    },
                )?;
                attach_pane_session_tx(
                    tx,
                    new_session_id,
                    tab_session_id,
                    new_pane_id,
                    p_row.generation,
                    now_ms,
                    InitialDispatchState::None,
                )?;
                Ok((t_row, p_row))
            })
            .await
            .map_err(map_db_error)?;

        let pty_id = match self
            .spawn_pane_pty(
                tab_id,
                project_id,
                new_pane_id,
                new_session_id,
                tab_session_id,
                created_pane_row.generation,
                continuous,
                &resolved_env.cwd,
                &resolved_env.env,
                &snapshot.connection,
            )
            .await
        {
            Ok(id) => id,
            Err(spawn_err) => {
                // Compensação em caso de falha de spawn do novo painel
                let _ = self
                    .inner
                    .db
                    .write_sized("pane_split_rollback_tx", 1024, move |tx| {
                        let now = mt_db::time::now_unix_ms();
                        let _ = close_pane_session_tx(
                            tx,
                            new_session_id,
                            &SessionEnd {
                                ended_at: now,
                                exit_code: None,
                                exit_signal: Some(String::from("SPAWN_FAILED")),
                                last_cwd: None,
                                integration_mode: IntegrationMode::Disabled,
                                command_count: 0,
                                error_count: 0,
                            },
                        );
                        let _ = remove_pane_from_layout_tx(tx, tab_id, new_pane_id, now);
                        Ok(())
                    })
                    .await;
                return Err(spawn_err);
            }
        };

        let view = {
            let mut tabs = self.inner.tabs.write();
            let mut p2t = self.inner.pane_to_tab.write();
            let mut pty_map = self.inner.pty_to_pane.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            entry.layout = updated_tab_row.layout;
            entry.layout_revision = updated_tab_row.layout_revision;
            entry.revision = updated_tab_row.revision;
            entry.active_pane_id = updated_tab_row.active_pane_id;
            entry.panes.insert(
                new_pane_id,
                LivePaneEntry {
                    pane_id: new_pane_id,
                    session_id: new_session_id,
                    pty_id: Some(pty_id),
                    generation: created_pane_row.generation,
                    cwd: new_cwd_str,
                    state: PaneRuntimeState::Live,
                    exit: None,
                    error: None,
                    initial_dispatch: InitialDispatchState::None,
                },
            );
            p2t.insert(new_pane_id, tab_id);
            pty_map.insert(
                pty_id,
                (
                    tab_id,
                    new_pane_id,
                    new_session_id,
                    created_pane_row.generation,
                ),
            );
            entry.to_view()
        };

        let result_json = serde_json::to_string(&view).ok();
        let req_id_commit = request_id.clone();
        let _ = self
            .inner
            .db
            .write_sized("pane_split_commit_op", 1024, move |tx| {
                update_operation_stage_tx(
                    tx,
                    &req_id_commit,
                    OperationStage::Completed,
                    Some(tab_id),
                    result_json.as_deref(),
                    mt_db::time::now_unix_ms(),
                )
            })
            .await;

        self.emit_tab_layout_changed(view.clone());
        Ok(view)
    }

    /// Fecha um painel individual; se for o último painel da aba, fecha a aba inteira (`pane_close`).
    #[allow(clippy::too_many_lines)]
    pub async fn pane_close(
        &self,
        pane_id: PaneId,
        request_id: String,
    ) -> MtResult<PaneCloseOutcome> {
        let _ = self.ensure_restored().await;
        let tab_id = self
            .inner
            .pane_to_tab
            .read()
            .get(&pane_id)
            .copied()
            .ok_or_else(pane_not_found)?;

        let pane_count = {
            let tabs = self.inner.tabs.read();
            let entry = tabs.get(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            entry.panes.len()
        };

        if pane_count <= 1 {
            self.tab_close(tab_id, request_id).await?;
            return Ok(PaneCloseOutcome::TabClosed { tab_id });
        }

        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let removed_pane = {
            let mut tabs = self.inner.tabs.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            let p = entry.panes.remove(&pane_id).ok_or_else(pane_not_found)?;
            self.inner.pane_to_tab.write().remove(&pane_id);
            if let Some(pty_id) = p.pty_id {
                self.inner.pty_to_pane.write().remove(&pty_id);
            }
            p
        };

        let now_ms = mt_db::time::now_unix_ms();
        let mut finalized_end = None;
        if let Some(pty_id) = removed_pane.pty_id {
            let _ = self.inner.pty_manager.kill(pty_id, KillMode::Graceful);
            if let Some((_, end)) =
                self.inner
                    .shell_service
                    .finalize_pty_session(pty_id, Some(0), None, now_ms)
            {
                finalized_end = Some(end);
            }
            let _ = self.inner.shell_service.remove_pane(tab_id, pane_id);
            self.release_pty_guards(pty_id);
            let _ = self.inner.recorder.flush_pty(pty_id).await;
        }

        let sid = removed_pane.session_id;
        let req_id_commit = request_id.clone();

        let updated_tab_opt = self
            .inner
            .db
            .write_sized("pane_close_tx", 1536, move |tx| {
                let end = finalized_end.unwrap_or(SessionEnd {
                    ended_at: now_ms,
                    exit_code: Some(0),
                    exit_signal: None,
                    last_cwd: None,
                    integration_mode: IntegrationMode::Disabled,
                    command_count: 0,
                    error_count: 0,
                });
                close_pane_session_tx(tx, sid, &end)?;
                let res = remove_pane_from_layout_tx(tx, tab_id, pane_id, now_ms)?;
                let _ = update_operation_stage_tx(
                    tx,
                    &req_id_commit,
                    OperationStage::Completed,
                    Some(tab_id),
                    None,
                    now_ms,
                );
                Ok(res)
            })
            .await
            .map_err(map_db_error)?;

        let Some(updated_tab_row) = updated_tab_opt else {
            return Ok(PaneCloseOutcome::TabClosed { tab_id });
        };

        let view = {
            let mut tabs = self.inner.tabs.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            entry.layout = updated_tab_row.layout;
            entry.layout_revision = updated_tab_row.layout_revision;
            entry.revision = updated_tab_row.revision;
            entry.active_pane_id = updated_tab_row.active_pane_id;
            entry.primary_pane_id = updated_tab_row.primary_pane_id;
            entry.to_view()
        };

        self.emit_tab_layout_changed(view.clone());
        Ok(PaneCloseOutcome::PaneClosed {
            tab: Box::new(view),
        })
    }

    /// Reinicia o PTY de um painel que já encerrou (`pane_restart`), incrementando sua `generation`
    /// dentro da mesma `tab_session`.
    #[allow(clippy::too_many_lines)]
    pub async fn pane_restart(
        &self,
        pane_id: PaneId,
        cwd_fallback_to_project: bool,
        _request_id: String,
    ) -> MtResult<TabView> {
        let _ = self.ensure_restored().await;
        let tab_id = self
            .inner
            .pane_to_tab
            .read()
            .get(&pane_id)
            .copied()
            .ok_or_else(pane_not_found)?;

        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let (project_id, snapshot, old_pane, tab_session_id, continuous) = {
            let tabs = self.inner.tabs.read();
            let entry = tabs.get(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            let p = entry.panes.get(&pane_id).ok_or_else(pane_not_found)?;
            if p.state == PaneRuntimeState::Live || p.state == PaneRuntimeState::Starting {
                return Err(pane_not_exited());
            }
            (
                entry.project_id,
                entry.launch_snapshot.clone(),
                p.clone(),
                entry.tab_session_id,
                entry.continuous,
            )
        };

        let project_path = self.load_project_path(project_id).await?.ok_or_else(|| {
            MtError::new(
                codes::PROJECT_NOT_FOUND,
                format!("Projeto '{project_id}' não encontrado."),
            )
        })?;

        let resolved_env = EnvironmentResolver::resolve_snapshot(
            &project_path,
            &snapshot,
            Some(&old_pane.cwd),
            cwd_fallback_to_project,
        )?;
        let effective_cwd_str = resolved_env.cwd.to_string_lossy().into_owned();
        let new_session_id = SessionId::new();
        let now_ms = mt_db::time::now_unix_ms();
        let cwd_for_tx = effective_cwd_str.clone();

        let new_generation = self
            .inner
            .db
            .write_sized("pane_restart_tx", 1024, move |tx| {
                let sanitized_cwd = SanitizedText::new(&cwd_for_tx);
                let bumped = bump_pane_generation_tx(tx, pane_id, Some(&sanitized_cwd))?;
                insert_session_tx(
                    tx,
                    &NewSession {
                        id: new_session_id,
                        tab_id: Some(tab_id),
                        pane_id: Some(pane_id),
                        project_id: Some(project_id),
                        tab_session_id: Some(tab_session_id),
                        pty_id: PtyId::new(),
                        shell_kind: ShellKind::Sh,
                        shell_executable: SanitizedText::new("shell"),
                        initial_cwd: sanitized_cwd,
                        integration_mode: IntegrationMode::Pending,
                        started_at: now_ms,
                    },
                )?;
                attach_pane_session_tx(
                    tx,
                    new_session_id,
                    tab_session_id,
                    pane_id,
                    bumped.generation,
                    now_ms,
                    InitialDispatchState::None,
                )?;
                Ok(bumped.generation)
            })
            .await
            .map_err(map_db_error)?;

        let new_pty_id = self
            .spawn_pane_pty(
                tab_id,
                project_id,
                pane_id,
                new_session_id,
                tab_session_id,
                new_generation,
                continuous,
                &resolved_env.cwd,
                &resolved_env.env,
                &snapshot.connection,
            )
            .await?;

        let view = {
            let mut tabs = self.inner.tabs.write();
            let mut pty_map = self.inner.pty_to_pane.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            if let Some(p) = entry.panes.get_mut(&pane_id) {
                p.session_id = new_session_id;
                p.pty_id = Some(new_pty_id);
                p.generation = new_generation;
                p.cwd = effective_cwd_str;
                p.state = PaneRuntimeState::Live;
                p.exit = None;
                p.error = None;
            }
            pty_map.insert(
                new_pty_id,
                (tab_id, pane_id, new_session_id, new_generation),
            );
            entry.to_view()
        };

        self.emit_pane_pty_replaced(PanePtyReplacedPayload {
            tab_id,
            pane_id,
            session_id: new_session_id,
            generation: new_generation,
            old_pty_id: old_pane.pty_id,
            new_pty_id,
        });
        self.emit_tab_layout_changed(view.clone());
        Ok(view)
    }

    /// Atualiza as proporções dos splitters de uma aba (`pane_layout_update`).
    pub async fn pane_layout_update(
        &self,
        tab_id: TabId,
        expected_layout_revision: i64,
        patches: Vec<LayoutSizePatch>,
    ) -> MtResult<TabView> {
        let _ = self.ensure_restored().await;
        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let updated_row = self
            .inner
            .db
            .tabs()
            .update_layout_with_revision(tab_id, expected_layout_revision, patches)
            .await
            .map_err(map_db_error)?;

        let view = {
            let mut tabs = self.inner.tabs.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            entry.layout = updated_row.layout;
            entry.layout_revision = updated_row.layout_revision;
            entry.revision = updated_row.revision;
            entry.to_view()
        };

        self.emit_tab_layout_changed(view.clone());
        Ok(view)
    }

    // -------------------------------------------------------------------------
    // Sessões de Aba ("Iniciar nova Sessão", Histórico, Scrollback e FTS5)
    // -------------------------------------------------------------------------

    /// Arquiva a `tab_session` ativa e reinicia PTYs limpos em todos os painéis da aba (`session_new`).
    #[allow(clippy::too_many_lines)]
    pub async fn session_new(&self, tab_id: TabId, request_id: String) -> MtResult<TabView> {
        let _ = self.ensure_restored().await;
        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let args_hash = hash_operation_args(&(tab_id, &request_id));
        let boot_id = self.inner.boot_id.clone();
        let req_id_prep = request_id.clone();
        let prep = self
            .inner
            .db
            .write_sized("workspace_op_prepare_session_new", 512, move |tx| {
                prepare_operation_tx(
                    tx,
                    &req_id_prep,
                    "session_new",
                    &args_hash,
                    Some(tab_id),
                    &boot_id,
                    mt_db::time::now_unix_ms(),
                )
            })
            .await
            .map_err(map_db_error)?;

        if let PrepareOperationOutcome::ExistingSameArgs(row) = prep
            && let Some(tid) = row.tab_id
            && let Some(entry) = self.inner.tabs.read().get(&tid)
        {
            return Ok(entry.to_view());
        }

        let (project_id, snapshot, old_tab_session_id, primary_pane_id, continuous, old_panes) = {
            let tabs = self.inner.tabs.read();
            let entry = tabs.get(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            (
                entry.project_id,
                entry.launch_snapshot.clone(),
                entry.tab_session_id,
                entry.primary_pane_id,
                entry.continuous,
                entry.panes.values().cloned().collect::<Vec<_>>(),
            )
        };

        let project_path = self.load_project_path(project_id).await?.ok_or_else(|| {
            MtError::new(
                codes::PROJECT_NOT_FOUND,
                format!("Projeto '{project_id}' não encontrado."),
            )
        })?;

        let now_ms = mt_db::time::now_unix_ms();
        let mut finalized_ends: HashMap<SessionId, SessionEnd> = HashMap::new();

        // 1. Encerra todos os PTYs atuais da aba e drena o recorder com barreira
        for p in &old_panes {
            if let Some(pty_id) = p.pty_id {
                self.inner.pty_to_pane.write().remove(&pty_id);
                let _ = self.inner.pty_manager.kill(pty_id, KillMode::Graceful);
                if let Some((sid, end)) =
                    self.inner
                        .shell_service
                        .finalize_pty_session(pty_id, Some(0), None, now_ms)
                {
                    finalized_ends.insert(sid, end);
                }
                self.release_pty_guards(pty_id);
            }
        }
        self.inner.shell_service.clear_tab_panes(tab_id);
        let _ = self
            .inner
            .recorder
            .flush_tab_session(old_tab_session_id)
            .await;

        let new_tab_session_id = TabSessionId::new();
        let has_initial_cmd = snapshot
            .initial_command
            .as_ref()
            .is_some_and(|c| !c.trim().is_empty());
        let primary_dispatch = if has_initial_cmd {
            InitialDispatchState::Pending
        } else {
            InitialDispatchState::None
        };
        let (keep_per_tab, max_age_days) = read_retention_settings(&self.inner.db);
        let old_pane_tuples: Vec<(PaneId, SessionId, String)> = old_panes
            .iter()
            .map(|p| (p.pane_id, p.session_id, p.cwd.clone()))
            .collect();

        // 2. Transação atômica: fecha sessões antigas, arquiva tab_session anterior, cria nova tab_session e novas pane_sessions
        let (archived_summary, started_summary, new_pane_specs, new_tab_rev) = self
            .inner
            .db
            .write_sized("session_new_tx", 4096, move |tx| {
                for (_, old_sid, _) in &old_pane_tuples {
                    let end = finalized_ends.remove(old_sid).unwrap_or(SessionEnd {
                        ended_at: now_ms,
                        exit_code: Some(0),
                        exit_signal: None,
                        last_cwd: None,
                        integration_mode: IntegrationMode::Disabled,
                        command_count: 0,
                        error_count: 0,
                    });
                    close_pane_session_tx(tx, *old_sid, &end)?;
                }

                let archived = archive_tab_session_tx(
                    tx,
                    old_tab_session_id,
                    SessionEndReason::NewSession,
                    now_ms,
                )?;
                let started = start_tab_session_tx(tx, new_tab_session_id, tab_id, now_ms)?;

                let mut specs = Vec::with_capacity(old_pane_tuples.len());
                for (pid, _, cwd_str) in &old_pane_tuples {
                    let sanitized_cwd = SanitizedText::new(cwd_str);
                    let bumped = bump_pane_generation_tx(tx, *pid, Some(&sanitized_cwd))?;
                    let new_sid = SessionId::new();
                    let disp = if *pid == primary_pane_id {
                        primary_dispatch
                    } else {
                        InitialDispatchState::None
                    };
                    insert_session_tx(
                        tx,
                        &NewSession {
                            id: new_sid,
                            tab_id: Some(tab_id),
                            pane_id: Some(*pid),
                            project_id: Some(project_id),
                            tab_session_id: Some(new_tab_session_id),
                            pty_id: PtyId::new(),
                            shell_kind: ShellKind::Sh,
                            shell_executable: SanitizedText::new("shell"),
                            initial_cwd: sanitized_cwd,
                            integration_mode: IntegrationMode::Pending,
                            started_at: now_ms,
                        },
                    )?;
                    attach_pane_session_tx(
                        tx,
                        new_sid,
                        new_tab_session_id,
                        *pid,
                        bumped.generation,
                        now_ms,
                        disp,
                    )?;
                    specs.push((*pid, new_sid, bumped.generation, disp, cwd_str.clone()));
                }

                let _ = enforce_tab_sessions_retention_tx(tx, keep_per_tab, max_age_days, now_ms)?;

                let rev: i64 = tx
                    .query_row(
                        "SELECT revision FROM tabs WHERE id = ?1;",
                        [tab_id.to_string()],
                        |r| r.get(0),
                    )
                    .unwrap_or(1);

                Ok((archived, started, specs, rev))
            })
            .await
            .map_err(map_db_error)?;

        // 3. Spawna os novos PTYs para cada painel preservando o layout de splits
        let mut replacement_events = Vec::new();
        let mut updated_panes = BTreeMap::new();

        for (pid, new_sid, next_gen, disp, prev_cwd_str) in new_pane_specs {
            let old_pty_id = old_panes
                .iter()
                .find(|p| p.pane_id == pid)
                .and_then(|p| p.pty_id);
            let resolved_env = EnvironmentResolver::resolve_snapshot(
                &project_path,
                &snapshot,
                Some(&prev_cwd_str),
                true,
            )?;
            let eff_cwd_str = resolved_env.cwd.to_string_lossy().into_owned();

            let new_pty_id = self
                .spawn_pane_pty(
                    tab_id,
                    project_id,
                    pid,
                    new_sid,
                    new_tab_session_id,
                    next_gen,
                    continuous,
                    &resolved_env.cwd,
                    &resolved_env.env,
                    &snapshot.connection,
                )
                .await?;

            self.inner
                .pty_to_pane
                .write()
                .insert(new_pty_id, (tab_id, pid, new_sid, next_gen));

            replacement_events.push(PanePtyReplacedPayload {
                tab_id,
                pane_id: pid,
                session_id: new_sid,
                generation: next_gen,
                old_pty_id,
                new_pty_id,
            });

            updated_panes.insert(
                pid,
                LivePaneEntry {
                    pane_id: pid,
                    session_id: new_sid,
                    pty_id: Some(new_pty_id),
                    generation: next_gen,
                    cwd: eff_cwd_str,
                    state: PaneRuntimeState::Live,
                    exit: None,
                    error: None,
                    initial_dispatch: disp,
                },
            );
        }

        let view = {
            let mut tabs = self.inner.tabs.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            entry.tab_session_id = new_tab_session_id;
            entry.revision = new_tab_rev;
            entry.panes = updated_panes;
            entry.to_view()
        };

        let result_json = serde_json::to_string(&view).ok();
        let req_id_commit = request_id.clone();
        let _ = self
            .inner
            .db
            .write_sized("session_new_commit_op", 1024, move |tx| {
                update_operation_stage_tx(
                    tx,
                    &req_id_commit,
                    OperationStage::Completed,
                    Some(tab_id),
                    result_json.as_deref(),
                    mt_db::time::now_unix_ms(),
                )
            })
            .await;

        self.emit_session_archived(archived_summary);
        self.emit_session_started(started_summary);
        for ev in replacement_events {
            self.emit_pane_pty_replaced(ev);
        }
        self.emit_tab_layout_changed(view.clone());
        Ok(view)
    }

    /// Lista paginadamente o histórico de sessões de uma aba (`session_list`).
    pub async fn session_list(
        &self,
        tab_id: TabId,
        cursor: Option<String>,
        limit: u32,
    ) -> MtResult<CursorPage<SessionSummary>> {
        self.inner
            .db
            .tab_sessions()
            .list_by_tab(tab_id, cursor, limit)
            .await
            .map_err(map_db_error)
    }

    /// Obtém o detalhe completo de uma sessão de aba (`session_get`).
    pub async fn session_get(&self, tab_session_id: TabSessionId) -> MtResult<SessionDetail> {
        let _ = self.inner.recorder.flush_all().await;
        let detail = self
            .inner
            .db
            .tab_sessions()
            .get_detail(tab_session_id)
            .await
            .map_err(map_db_error)?;
        detail.ok_or_else(|| {
            MtError::new(
                codes::SESSION_NOT_FOUND,
                format!("Sessão '{tab_session_id}' não encontrada."),
            )
        })
    }

    /// Lista paginadamente os chunks de scrollback de um painel na sessão (`session_scrollback_get`).
    pub async fn session_scrollback_get(
        &self,
        tab_session_id: TabSessionId,
        session_id: SessionId,
        cursor: Option<String>,
        limit: u32,
    ) -> MtResult<CursorPage<ScrollbackChunk>> {
        let _ = self.inner.recorder.flush_all().await;
        self.inner
            .db
            .scrollback()
            .list_chunks(tab_session_id, session_id, cursor, limit)
            .await
            .map_err(map_db_error)
    }

    /// Realiza busca textual FTS5 em comandos e scrollback das sessões de uma aba (`session_search`).
    pub async fn session_search(
        &self,
        tab_id: TabId,
        query: String,
        limit: u32,
    ) -> MtResult<Vec<SessionSearchHit>> {
        let _ = self.inner.recorder.flush_all().await;
        self.inner
            .db
            .scrollback()
            .search_in_tab(tab_id, query, limit)
            .await
            .map_err(map_db_error)
    }

    /// Despacha de forma idempotente o `initial_command` do template após o handshake do frontend (`workspace_initial_command_run`).
    pub async fn workspace_initial_command_run(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        generation: i64,
        _request_id: String,
    ) -> MtResult<TabView> {
        let _ = self.ensure_restored().await;
        let lock = self.tab_lock(tab_id);
        let _guard = lock.lock().await;

        let (pty_id, session_id, command_opt, current_disp) = {
            let tabs = self.inner.tabs.read();
            let entry = tabs.get(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            let pane = entry.panes.get(&pane_id).ok_or_else(pane_not_found)?;
            if pane.generation != generation {
                return Err(initial_command_stale());
            }
            (
                pane.pty_id,
                pane.session_id,
                entry.launch_snapshot.initial_command.clone(),
                pane.initial_dispatch,
            )
        };

        if current_disp == InitialDispatchState::Sent {
            let tabs = self.inner.tabs.read();
            if let Some(entry) = tabs.get(&tab_id) {
                return Ok(entry.to_view());
            }
        }

        let Some(cmd) = command_opt.filter(|c| !c.trim().is_empty()) else {
            return Err(initial_command_stale());
        };

        let Some(pty_id) = pty_id else {
            return Err(initial_command_stale());
        };

        let claimed = self
            .inner
            .db
            .tab_sessions()
            .claim_initial_dispatch(session_id)
            .await
            .map_err(map_db_error)?;

        if claimed {
            let mut payload = cmd.clone();
            if !payload.ends_with('\n') {
                payload.push('\n');
            }
            self.inner
                .recorder
                .record_system_marker(session_id, format!("$ {}", cmd.trim()));
            self.inner
                .pty_manager
                .write(pty_id, payload.as_bytes())
                .map_err(|e| tab_open_failed(format!("Falha ao enviar comando inicial: {e}")))?;
            let _ = self
                .inner
                .db
                .tab_sessions()
                .mark_initial_dispatch_sent(session_id)
                .await;
        }

        let view = {
            let mut tabs = self.inner.tabs.write();
            let entry = tabs.get_mut(&tab_id).ok_or_else(|| {
                MtError::new(
                    codes::TAB_NOT_FOUND,
                    format!("Aba '{tab_id}' não encontrada."),
                )
            })?;
            if let Some(pane) = entry.panes.get_mut(&pane_id) {
                pane.initial_dispatch = InitialDispatchState::Sent;
            }
            entry.to_view()
        };

        self.emit_tab_layout_changed(view.clone());
        Ok(view)
    }

    // -------------------------------------------------------------------------
    // Callbacks do Observer de PTY
    // -------------------------------------------------------------------------

    /// Processa o encerramento de um PTY associado a um painel da aba.
    pub fn handle_pty_exit(&self, pty_id: PtyId, exit: &ExitInfo) {
        let mapping = self.inner.pty_to_pane.write().remove(&pty_id);
        let Some((tab_id, pane_id, session_id, generation)) = mapping else {
            return;
        };

        self.release_pty_guards(pty_id);

        let reason_str = match exit.reason {
            ExitReason::Exited => "exited",
            ExitReason::Signaled => "signaled",
            ExitReason::KilledByUser => "killedByUser",
            ExitReason::Shutdown => "shutdown",
            ExitReason::ReaderFailed => "readerFailed",
        }
        .to_string();

        let at_ms = i64::try_from(exit.at_ms.0).unwrap_or_else(|_| mt_db::time::now_unix_ms());
        let exit_info = PaneExitInfo {
            code: exit.code,
            signal: exit.signal,
            signal_name: exit.signal_name.clone(),
            reason: reason_str,
            at_ms,
        };

        let view_opt = {
            let mut tabs = self.inner.tabs.write();
            if let Some(entry) = tabs.get_mut(&tab_id) {
                if let Some(pane) = entry.panes.get_mut(&pane_id)
                    && pane.pty_id == Some(pty_id)
                {
                    pane.pty_id = None;
                    pane.state = PaneRuntimeState::Exited;
                    pane.exit = Some(exit_info.clone());
                }
                Some(entry.to_view())
            } else {
                None
            }
        };

        let db = self.inner.db.clone();
        let recorder = Arc::clone(&self.inner.recorder);
        let code = exit.code;
        let sig_name = exit.signal_name.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let _ = recorder.flush_pty(pty_id).await;
                let _ = db
                    .write_sized("pane_exit_close_session", 512, move |tx| {
                        close_pane_session_tx(
                            tx,
                            session_id,
                            &SessionEnd {
                                ended_at: at_ms,
                                exit_code: code,
                                exit_signal: sig_name,
                                last_cwd: None,
                                integration_mode: IntegrationMode::Disabled,
                                command_count: 0,
                                error_count: 0,
                            },
                        )
                    })
                    .await;
            });
        }

        let observers = self.inner.observers.read().clone();
        for obs in observers {
            obs.on_pane_exited(tab_id, pane_id, pty_id, exit);
        }

        self.emit_pane_exited(PaneExitedPayload {
            tab_id,
            pane_id,
            session_id,
            generation,
            pty_id,
            exit: exit_info,
        });
        if let Some(view) = view_opt {
            self.emit_tab_layout_changed(view);
        }
    }

    /// Restaura uma aba que estava aberta antes do encerramento da aplicação (`restore_open_tab_entry`).
    pub async fn restore_open_tab_entry(
        &self,
        stored_tab: mt_db::repos::StoredTabRow,
    ) -> MtResult<TabView> {
        let now_ms = mt_db::time::now_unix_ms();
        let tab_id = stored_tab.id;
        let project_id = stored_tab.project_id;
        let tab_session_id = TabSessionId::new();

        let open_panes = self
            .inner
            .db
            .panes()
            .list_open_by_tab(tab_id)
            .await
            .map_err(map_db_error)?;

        let panes_to_restore = if open_panes.is_empty() {
            vec![mt_db::repos::StoredPaneRow {
                id: stored_tab.primary_pane_id,
                tab_id,
                cwd: stored_tab.launch_snapshot.resolved_cwd.clone(),
                generation: 1,
                created_at: now_ms,
                closed_at: None,
            }]
        } else {
            open_panes
        };

        let panes_copy = panes_to_restore.clone();
        let project_id_copy = project_id;
        let stored_tab_id = tab_id;

        let (_summary, pane_sessions) = self
            .inner
            .db
            .write_sized("tab_restore_begin_tx", 2048, move |tx| {
                let summary = start_tab_session_tx(tx, tab_session_id, stored_tab_id, now_ms)?;
                let mut map = HashMap::new();
                for pane in &panes_copy {
                    let sid = SessionId::new();
                    insert_session_tx(
                        tx,
                        &NewSession {
                            id: sid,
                            tab_id: Some(stored_tab_id),
                            pane_id: Some(pane.id),
                            project_id: Some(project_id_copy),
                            tab_session_id: Some(tab_session_id),
                            pty_id: PtyId::new(),
                            shell_kind: ShellKind::Sh,
                            shell_executable: SanitizedText::new("shell"),
                            initial_cwd: SanitizedText::new(&pane.cwd),
                            integration_mode: IntegrationMode::Pending,
                            started_at: now_ms,
                        },
                    )?;
                    attach_pane_session_tx(
                        tx,
                        sid,
                        tab_session_id,
                        pane.id,
                        pane.generation,
                        now_ms,
                        InitialDispatchState::None,
                    )?;
                    map.insert(pane.id, sid);
                }
                Ok((summary, map))
            })
            .await
            .map_err(map_db_error)?;

        let mut live_panes = BTreeMap::new();
        let mut p2t_entries = Vec::new();
        let mut pty_map_entries = Vec::new();

        let base_env: BTreeMap<String, String> = stored_tab
            .launch_snapshot
            .env_vars
            .iter()
            .filter_map(|(k, v)| match v {
                mt_core::workspace::EnvValue::Literal { value } => Some((k.clone(), value.clone())),
                mt_core::workspace::EnvValue::Inherited { source } => {
                    std::env::var(source).ok().map(|val| (k.clone(), val))
                }
            })
            .collect();

        for pane_row in panes_to_restore {
            let pane_id = pane_row.id;
            let session_id = pane_sessions
                .get(&pane_id)
                .copied()
                .unwrap_or_else(SessionId::new);

            p2t_entries.push((pane_id, tab_id));

            let pane_cwd = PathBuf::from(&pane_row.cwd);
            let effective_cwd = if matches!(
                stored_tab.launch_snapshot.connection,
                mt_core::workspace::TemplateConnection::Ssh { .. }
            ) || pane_cwd.is_dir() {
                pane_cwd
            } else {
                PathBuf::from(&stored_tab.launch_snapshot.resolved_cwd)
            };

            match self
                .spawn_pane_pty(
                    tab_id,
                    project_id,
                    pane_id,
                    session_id,
                    tab_session_id,
                    pane_row.generation,
                    stored_tab.continuous,
                    &effective_cwd,
                    &base_env,
                    &stored_tab.launch_snapshot.connection,
                )
                .await
            {
                Ok(pty_id) => {
                    pty_map_entries.push((
                        pty_id,
                        (tab_id, pane_id, session_id, pane_row.generation),
                    ));
                    live_panes.insert(
                        pane_id,
                        LivePaneEntry {
                            pane_id,
                            session_id,
                            pty_id: Some(pty_id),
                            generation: pane_row.generation,
                            cwd: effective_cwd.to_string_lossy().into_owned(),
                            state: PaneRuntimeState::Live,
                            exit: None,
                            error: None,
                            initial_dispatch: InitialDispatchState::None,
                        },
                    );
                }
                Err(err) => {
                    // Preserva a aba viva mesmo se o PTY falhar no boot (ex: VAULT_LOCKED ou offline)
                    live_panes.insert(
                        pane_id,
                        LivePaneEntry {
                            pane_id,
                            session_id,
                            pty_id: None,
                            generation: pane_row.generation,
                            cwd: effective_cwd.to_string_lossy().into_owned(),
                            state: PaneRuntimeState::Failed,
                            exit: None,
                            error: Some(err),
                            initial_dispatch: InitialDispatchState::None,
                        },
                    );
                }
            }
        }

        {
            let mut p2t = self.inner.pane_to_tab.write();
            for (p_id, t_id) in p2t_entries {
                p2t.insert(p_id, t_id);
            }
        }
        {
            let mut pty_map = self.inner.pty_to_pane.write();
            for (pty_id, entry) in pty_map_entries {
                pty_map.insert(pty_id, entry);
            }
        }

        let live_tab = LiveTabEntry {
            id: stored_tab.id,
            project_id: stored_tab.project_id,
            template_id: stored_tab.template_id,
            title: stored_tab.title,
            icon: stored_tab.icon,
            position: stored_tab.position,
            origin: stored_tab.origin,
            continuous: stored_tab.continuous,
            lifecycle: TabLifecycle::Ready,
            revision: stored_tab.revision,
            layout_revision: stored_tab.layout_revision,
            layout: stored_tab.layout,
            active_pane_id: stored_tab.active_pane_id,
            primary_pane_id: stored_tab.primary_pane_id,
            tab_session_id,
            launch_snapshot: stored_tab.launch_snapshot,
            panes: live_panes,
        };

        let view = live_tab.to_view();
        self.inner.tabs.write().insert(tab_id, live_tab);
        self.emit_tab_opened(view.clone());
        Ok(view)
    }

    /// Drena e arquiva todas as abas abertas durante o encerramento gracioso da aplicação (`RunEvent::Exit`).
    pub async fn shutdown(&self) {
        let open_entries: Vec<LiveTabEntry> = {
            let mut tabs = self.inner.tabs.write();
            let list: Vec<LiveTabEntry> = tabs.drain().map(|(_, v)| v).collect();
            self.inner.pane_to_tab.write().clear();
            self.inner.pty_to_pane.write().clear();
            list
        };

        for entry in open_entries {
            let _ = self
                .terminate_and_archive_tab_entry(&entry, SessionEndReason::AppExit)
                .await;
        }
        let _ = self.inner.recorder.flush_all().await;
    }

    async fn load_project_path(&self, project_id: ProjectId) -> MtResult<Option<PathBuf>> {
        let catalog = self
            .inner
            .db
            .projects()
            .load_catalog()
            .await
            .map_err(map_db_error)?;
        Ok(catalog
            .projects
            .into_iter()
            .find(|p| p.id == project_id)
            .map(|p| PathBuf::from(p.path)))
    }
}

#[async_trait]
impl ProjectRemovalHook for WorkspaceManager {
    async fn before_remove(&self, project_id: ProjectId) -> MtResult<()> {
        let project_tabs: Vec<LiveTabEntry> = {
            let mut tabs = self.inner.tabs.write();
            let matching_ids: Vec<TabId> = tabs
                .values()
                .filter(|t| t.project_id == project_id)
                .map(|t| t.id)
                .collect();

            let mut removed = Vec::with_capacity(matching_ids.len());
            let mut p2t = self.inner.pane_to_tab.write();
            let mut pty_map = self.inner.pty_to_pane.write();
            for tid in matching_ids {
                if let Some(entry) = tabs.remove(&tid) {
                    for p in entry.panes.values() {
                        p2t.remove(&p.pane_id);
                        if let Some(pty_id) = p.pty_id {
                            pty_map.remove(&pty_id);
                        }
                    }
                    removed.push(entry);
                }
            }
            removed
        };

        for entry in project_tabs {
            self.terminate_and_archive_tab_entry(&entry, SessionEndReason::ProjectRemoved)
                .await?;
        }
        Ok(())
    }
}
