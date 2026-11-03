//! Adaptadores do crate binário que implementam [`TabStatusProvider`], [`TabCwdProvider`],
//! [`ShutdownHooks`], [`WindowHost`] e o observador de ciclo de vida para [`OsIntegration`] (Fase 09).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use mt_core::error::{MtError, MtResult};
use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_core::shell::{
    CommandFinished, CommandStarted, ShellCwdChangedEvent, ShellIntegrationModeEvent,
    TabStateRemovedEvent, TabStateSnapshot, TabStatus,
};
use mt_db::Db;
use mt_git::GitService;
use mt_os::{
    ActivitySnapshot, CommandFinishedInfo, DropDownGeometry, LinkContext, OsEvent, OsEventSink,
    OsIntegration, QuitSource, ShutdownHooks, ShutdownPhase, ShutdownReport, TabCwdProvider,
    TabStatusEntry, TabStatusProvider, WindowHost, WindowVisibilityPayload, codes, os_error,
};
use mt_ports::PortsService;
use mt_projects::ProjectService;
use mt_pty::{ExitInfo, PtyManager};
use mt_shell::{ShellEventSink, ShellService};
use parking_lot::RwLock;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime};

use crate::workspace::WorkspaceManager;
use crate::workspace::observer::WorkspaceLifecycleObserver;

/// Estado compartilhado de épocas de CWD por painel (`(TabId, PtyId, u32, PathBuf)`).
#[derive(Default)]
pub struct PaneCwdTracker {
    entries: RwLock<HashMap<PaneId, (TabId, PtyId, u32, PathBuf)>>,
}

impl PaneCwdTracker {
    /// Cria um novo rastreador de épocas de CWD por painel.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registra ou atualiza o CWD/PTY de um painel e retorna o novo [`LinkContext`].
    pub fn upsert(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        pty_id: PtyId,
        cwd: PathBuf,
    ) -> LinkContext {
        let mut guard = self.entries.write();
        let next_epoch = match guard.get(&pane_id) {
            Some((_, prev_pty, prev_epoch, prev_cwd))
                if *prev_pty == pty_id && *prev_cwd == cwd =>
            {
                *prev_epoch
            }
            Some((_, _, prev_epoch, _)) => prev_epoch.saturating_add(1),
            None => 1,
        };
        guard.insert(pane_id, (tab_id, pty_id, next_epoch, cwd));
        LinkContext {
            tab_id,
            pane_id,
            pty_id,
            cwd_epoch: next_epoch,
        }
    }

    /// Remove um painel fechado do rastreador.
    pub fn remove_pane(&self, pane_id: &PaneId) {
        self.entries.write().remove(pane_id);
    }

    /// Remove todos os painéis pertencentes a uma aba fechada.
    pub fn remove_tab(&self, tab_id: &TabId) {
        self.entries.write().retain(|_, (tid, ..)| tid != tab_id);
    }
}

/// Implementação de [`TabStatusProvider`] conectada ao [`ShellService`], [`WorkspaceManager`] e [`PtyManager`].
pub struct WorkspaceTabStatusProvider {
    workspace: WorkspaceManager,
    shell: Arc<ShellService>,
    pty: Arc<PtyManager>,
}

impl WorkspaceTabStatusProvider {
    /// Cria um novo [`WorkspaceTabStatusProvider`].
    #[must_use]
    pub fn new(
        workspace: WorkspaceManager,
        shell: Arc<ShellService>,
        pty: Arc<PtyManager>,
    ) -> Arc<Self> {
        Arc::new(Self {
            workspace,
            shell,
            pty,
        })
    }
}

impl TabStatusProvider for WorkspaceTabStatusProvider {
    fn snapshot(&self) -> Vec<TabStatusEntry> {
        let states = self.shell.tab_state_list();
        states
            .into_iter()
            .map(|snap| {
                let seen = match snap.status {
                    TabStatus::CompletedUnseen | TabStatus::ErrorUnseen => {
                        snap.last_completion.as_ref().is_some_and(|c| c.seen)
                    }
                    _ => true,
                };
                let title = self
                    .workspace
                    .tab_meta(snap.tab_id)
                    .map_or_else(|| snap.tab_id.to_string(), |(_, title, _)| title);
                TabStatusEntry {
                    tab_id: snap.tab_id,
                    status: snap.status,
                    seen,
                    title,
                }
            })
            .collect()
    }

    fn activity(&self) -> ActivitySnapshot {
        let states = self.shell.tab_state_list();
        let mut active_panes = 0u32;
        for tab in &states {
            for pane in &tab.panes {
                if matches!(pane.status, TabStatus::Running | TabStatus::Continuous) {
                    active_panes = active_panes.saturating_add(1);
                }
            }
        }
        let supervised_busy = active_panes > 0;
        let _ = &self.pty;
        ActivitySnapshot {
            active_panes,
            pending_spawns: 0,
            supervised_busy,
            incomplete: false,
        }
    }

    fn project_of(&self, tab_id: &TabId) -> Option<ProjectId> {
        self.workspace
            .tab_project_and_cwd(*tab_id)
            .map(|(proj_id, _)| proj_id)
    }
}

/// Implementação de [`TabCwdProvider`] apoiada no [`PaneCwdTracker`] e [`WorkspaceManager`].
pub struct WorkspaceTabCwdProvider {
    workspace: WorkspaceManager,
    tracker: Arc<PaneCwdTracker>,
}

impl WorkspaceTabCwdProvider {
    /// Cria um novo [`WorkspaceTabCwdProvider`].
    #[must_use]
    pub fn new(workspace: WorkspaceManager, tracker: Arc<PaneCwdTracker>) -> Arc<Self> {
        Arc::new(Self { workspace, tracker })
    }
}

impl TabCwdProvider for WorkspaceTabCwdProvider {
    fn cwd(&self, context: &LinkContext) -> MtResult<Option<PathBuf>> {
        if let Some((tab_id, pty_id, epoch, cwd)) =
            self.tracker.entries.read().get(&context.pane_id).cloned()
        {
            if tab_id != context.tab_id {
                return Err(os_error(
                    codes::TAB_NOT_FOUND,
                    "Painel não pertence à aba informada.",
                ));
            }
            if pty_id != context.pty_id || epoch != context.cwd_epoch {
                return Err(os_error(
                    codes::LINK_CONTEXT_STALE,
                    "O diretório de trabalho ou PTY do painel mudou durante a resolução do link.",
                ));
            }
            return Ok(Some(cwd));
        }

        // Fallback direto para o WorkspaceManager se o painel acabou de ser criado com cwd_epoch == 1
        if let Some((tab_id, pty_id_opt, cwd_str)) = self.workspace.pane_info(context.pane_id) {
            if tab_id != context.tab_id {
                return Err(os_error(
                    codes::TAB_NOT_FOUND,
                    "Painel não pertence à aba informada.",
                ));
            }
            if pty_id_opt != Some(context.pty_id) || context.cwd_epoch != 1 {
                return Err(os_error(
                    codes::LINK_CONTEXT_STALE,
                    "Contexto de PTY ou época de CWD obsoleto.",
                ));
            }
            return Ok(Some(PathBuf::from(cwd_str)));
        }

        Err(os_error(
            codes::TAB_NOT_FOUND,
            "Aba ou painel não encontrado para resolução de link.",
        ))
    }
}

/// Implementação de [`WindowHost`] sobre a janela `"main"` do Tauri.
pub struct TauriWindowHost<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
    event_sink: Arc<dyn OsEventSink>,
}

impl<R: Runtime> TauriWindowHost<R> {
    /// Cria um novo [`TauriWindowHost`].
    #[must_use]
    pub fn new(app: AppHandle<R>, event_sink: Arc<dyn OsEventSink>) -> Arc<Self> {
        Arc::new(Self { app, event_sink })
    }
}

impl<R: Runtime> WindowHost for TauriWindowHost<R> {
    fn snapshot(&self) -> WindowVisibilityPayload {
        if let Some(win) = self.app.get_webview_window("main") {
            WindowVisibilityPayload {
                visible: win.is_visible().unwrap_or(false),
                focused: win.is_focused().unwrap_or(false),
                minimized: win.is_minimized().unwrap_or(false),
            }
        } else {
            WindowVisibilityPayload::default()
        }
    }

    fn current_monitor_rect(&self) -> Option<((i32, i32), (u32, u32))> {
        let win = self.app.get_webview_window("main")?;
        let monitor = win
            .current_monitor()
            .ok()
            .flatten()
            .or_else(|| win.primary_monitor().ok().flatten())?;
        let pos = monitor.position();
        let size = monitor.size();
        Some(((pos.x, pos.y), (size.width, size.height)))
    }

    fn apply_drop_down_geometry(
        &self,
        geometry: DropDownGeometry,
        always_on_top: bool,
    ) -> MtResult<()> {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = win.set_position(PhysicalPosition::new(geometry.x, geometry.y));
            let _ = win.set_size(PhysicalSize::new(geometry.width, geometry.height));
            let _ = win.set_always_on_top(always_on_top);
        }
        Ok(())
    }

    fn restore_normal_mode(&self) -> MtResult<()> {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = win.set_always_on_top(false);
        }
        Ok(())
    }

    fn show_and_focus(&self) -> MtResult<()> {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = win.unminimize();
            let _ = win.show();
            let _ = win.set_focus();
        }
        Ok(())
    }

    fn focus_only(&self) -> MtResult<()> {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = win.unminimize();
            let _ = win.set_focus();
        }
        Ok(())
    }

    fn hide_window(&self, can_hide_to_tray_or_shortcut: bool) -> MtResult<()> {
        if let Some(win) = self.app.get_webview_window("main") {
            if can_hide_to_tray_or_shortcut {
                let _ = win.hide();
            } else {
                let _ = win.minimize();
            }
        }
        Ok(())
    }

    fn minimize_window(&self) -> MtResult<()> {
        if let Some(win) = self.app.get_webview_window("main") {
            let _ = win.minimize();
        }
        Ok(())
    }

    fn on_visibility_changed(&self, payload: WindowVisibilityPayload) {
        self.event_sink.emit_window_visibility(OsEvent {
            revision: 1,
            payload,
        });
    }
}

/// Implementação de [`ShutdownHooks`] para o ciclo de desligamento coordenado no binário.
pub struct AppShutdownHooks<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
    pty: Arc<PtyManager>,
    db: Arc<Db>,
    shell: Arc<ShellService>,
    projects: Arc<ProjectService>,
    workspace: WorkspaceManager,
    git: GitService,
    ports: PortsService,
    status_provider: Arc<dyn TabStatusProvider>,
    event_sink: Arc<dyn OsEventSink>,
    shutdown_complete: Arc<AtomicBool>,
}

impl<R: Runtime> AppShutdownHooks<R> {
    /// Cria uma nova instância de [`AppShutdownHooks`].
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        app: AppHandle<R>,
        pty: Arc<PtyManager>,
        db: Arc<Db>,
        shell: Arc<ShellService>,
        projects: Arc<ProjectService>,
        workspace: WorkspaceManager,
        git: GitService,
        ports: PortsService,
        status_provider: Arc<dyn TabStatusProvider>,
        event_sink: Arc<dyn OsEventSink>,
        shutdown_complete: Arc<AtomicBool>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app,
            pty,
            db,
            shell,
            projects,
            workspace,
            git,
            ports,
            status_provider,
            event_sink,
            shutdown_complete,
        })
    }
}

#[async_trait]
impl<R: Runtime> ShutdownHooks for AppShutdownHooks<R> {
    fn active_count(&self) -> u32 {
        self.status_provider.activity().active_panes
    }

    async fn confirm_quit(&self, _active_count: u32) -> bool {
        true
    }

    async fn freeze_spawns(&self) -> MtResult<()> {
        self.ports.shutdown();
        self.git.shutdown();
        Ok(())
    }

    async fn shutdown_workspace(&self) -> MtResult<()> {
        if let Some(cg) = self
            .app
            .try_state::<crate::command_groups::AppCommandGroupState>()
        {
            cg.service.shutdown().await;
        }
        self.projects.shutdown().await;
        self.workspace.shutdown().await;
        self.shell.shutdown();
        Ok(())
    }

    async fn shutdown_ptys(&self, grace: Duration) -> ShutdownReport {
        let pty = Arc::clone(&self.pty);
        match tokio::task::spawn_blocking(move || pty.shutdown(grace)).await {
            Ok(rep) => ShutdownReport {
                terminated: u32::try_from(rep.completed.len()).unwrap_or(0),
                killed: 0,
                survivors: (0..rep.pending.len())
                    .map(|i| u32::try_from(i + 1).unwrap_or(1))
                    .collect(),
            },
            Err(_) => ShutdownReport::default(),
        }
    }

    async fn flush_db(&self) -> MtResult<()> {
        self.db.shutdown().await.map_err(|e| {
            os_error(
                codes::SHUTDOWN_TIMEOUT,
                format!("Falha ao drenar banco de dados no desligamento: {e}"),
            )
        })
    }

    fn on_shutdown_started(&self, source: QuitSource) {
        self.event_sink.emit_shutdown_started(OsEvent {
            revision: 1,
            payload: mt_os::ShutdownStartedPayload { source },
        });
    }

    fn on_shutdown_failed(&self, error: &MtError) {
        self.event_sink.emit_shutdown_failed(OsEvent {
            revision: 1,
            payload: mt_os::ShutdownFailedPayload {
                error: error.clone(),
                phase: ShutdownPhase::Failed,
            },
        });
    }

    fn exit_application(&self) {
        self.shutdown_complete.store(true, Ordering::Release);
        self.app.exit(0);
    }
}

/// Observador que conecta eventos do [`WorkspaceManager`] e [`ShellService`] ao [`OsIntegration`].
pub struct OsLifecycleObserver {
    os: Arc<OsIntegration>,
    workspace: WorkspaceManager,
    tracker: Arc<PaneCwdTracker>,
}

impl OsLifecycleObserver {
    /// Cria um novo [`OsLifecycleObserver`].
    #[must_use]
    pub fn new(
        os: Arc<OsIntegration>,
        workspace: WorkspaceManager,
        tracker: Arc<PaneCwdTracker>,
    ) -> Arc<Self> {
        Arc::new(Self {
            os,
            workspace,
            tracker,
        })
    }
}

impl WorkspaceLifecycleObserver for OsLifecycleObserver {
    fn on_pane_spawned(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        pty_id: PtyId,
        _session_id: SessionId,
    ) {
        let cwd = self
            .workspace
            .pane_info(pane_id)
            .map_or_else(|| PathBuf::from("/"), |(_, _, c)| PathBuf::from(c));
        let ctx = self.tracker.upsert(tab_id, pane_id, pty_id, cwd);
        self.os.update_link_context(ctx);
    }

    fn on_pane_exited(&self, _tab_id: TabId, pane_id: PaneId, _pty_id: PtyId, _exit: &ExitInfo) {
        self.tracker.remove_pane(&pane_id);
        self.os.remove_pane_context(&pane_id);
    }

    fn on_session_archived(&self, _tab_id: TabId, _tab_session_id: TabSessionId) {}

    fn on_tab_closed(&self, _project_id: ProjectId, tab_id: TabId) {
        self.tracker.remove_tab(&tab_id);
        self.os.on_tab_closed(&tab_id);
    }
}

impl ShellEventSink for OsLifecycleObserver {
    fn on_tab_state_changed(&self, _snapshot: &TabStateSnapshot) {
        self.os.on_tab_state_changed();
    }

    fn on_tab_state_removed(&self, event: &TabStateRemovedEvent) {
        self.os.on_tab_closed(&event.tab_id);
    }

    fn on_command_started(&self, _event: &CommandStarted) {}

    fn on_command_finished(&self, event: &CommandFinished) {
        let Some(tab_id) = event.binding.tab_id else {
            return;
        };
        let duration_ms = u64::try_from(event.duration_ms.max(0)).unwrap_or(0);
        let is_continuous = self
            .workspace
            .tab_meta(tab_id)
            .is_some_and(|(_, _, continuous)| continuous);
        let info = CommandFinishedInfo {
            block_id: event.block_id,
            tab_id,
            pane_id: event.binding.pane_id,
            command: event.command_text.clone().unwrap_or_default(),
            exit_code: event.exit_code,
            duration_ms,
            end_reason: event.end_reason,
            is_continuous,
        };
        let os = Arc::clone(&self.os);
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                os.on_command_finished(info).await;
            });
        }
    }

    fn on_cwd_changed(&self, event: &ShellCwdChangedEvent) {
        if let (Some(tab_id), Some(pane_id)) = (event.binding.tab_id, event.binding.pane_id) {
            let ctx = self.tracker.upsert(
                tab_id,
                pane_id,
                event.binding.pty_id,
                PathBuf::from(&event.cwd),
            );
            self.os.update_link_context(ctx);
        }
    }

    fn on_integration_mode_changed(&self, _event: &ShellIntegrationModeEvent) {}
}
