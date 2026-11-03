//! Observador de ciclo de vida (`IndicatorsLifecycleObserver`) que sincroniza `WorkspaceManager`
//! com `GitService` e `PortsService` (Fase 08).

use std::path::PathBuf;
use std::sync::Arc;

use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_core::shell::{
    CommandFinished, CommandStarted, ShellCwdChangedEvent, ShellIntegrationModeEvent,
    TabStateRemovedEvent, TabStateSnapshot,
};
use mt_git::{GitService, RefreshReason};
use mt_ports::PortsService;
use mt_pty::ExitInfo;
use mt_shell::ShellEventSink;

use crate::workspace::WorkspaceManager;
use crate::workspace::observer::WorkspaceLifecycleObserver;

/// Observador registrado no [`WorkspaceManager`] que vincula/desvincula painéis nos serviços de
/// indicadores contextuais (`GitService` e `PortsService`).
pub struct IndicatorsLifecycleObserver {
    workspace: WorkspaceManager,
    git_service: GitService,
    ports_service: PortsService,
}

impl std::fmt::Debug for IndicatorsLifecycleObserver {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IndicatorsLifecycleObserver")
            .finish_non_exhaustive()
    }
}

impl IndicatorsLifecycleObserver {
    /// Cria uma nova instância do observador de indicadores.
    #[must_use]
    pub fn new(
        workspace: WorkspaceManager,
        git_service: GitService,
        ports_service: PortsService,
    ) -> Arc<Self> {
        Arc::new(Self {
            workspace,
            git_service,
            ports_service,
        })
    }
}

impl WorkspaceLifecycleObserver for IndicatorsLifecycleObserver {
    fn on_pane_spawned(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        _pty_id: PtyId,
        _session_id: SessionId,
    ) {
        self.ports_service.notify_activity();
        if let Some((_tid, _pty, cwd)) = self.workspace.pane_info(pane_id) {
            let git = self.git_service.clone();
            let path = PathBuf::from(cwd);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = git
                        .bind_pane_cwd(tab_id, pane_id, path, RefreshReason::Initial)
                        .await;
                });
            }
        }
    }

    fn on_pane_exited(&self, _tab_id: TabId, pane_id: PaneId, _pty_id: PtyId, _exit: &ExitInfo) {
        self.ports_service.remove_pane(pane_id);
        self.git_service.unbind_pane(pane_id);
    }

    fn on_session_archived(&self, _tab_id: TabId, _tab_session_id: TabSessionId) {}

    fn on_tab_closed(&self, _project_id: ProjectId, tab_id: TabId) {
        self.ports_service.remove_tab(tab_id);
        self.git_service.unbind_tab(tab_id);
    }
}

impl ShellEventSink for IndicatorsLifecycleObserver {
    fn on_tab_state_changed(&self, _snapshot: &TabStateSnapshot) {}
    fn on_tab_state_removed(&self, _event: &TabStateRemovedEvent) {}
    fn on_command_started(&self, _event: &CommandStarted) {}

    fn on_command_finished(&self, event: &CommandFinished) {
        if let Some(pane_id) = event.binding.pane_id {
            let git = self.git_service.clone();
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = git.notify_command_finished(pane_id).await;
                });
            }
        }
    }

    fn on_cwd_changed(&self, event: &ShellCwdChangedEvent) {
        if let (Some(tab_id), Some(pane_id)) = (event.binding.tab_id, event.binding.pane_id) {
            let git = self.git_service.clone();
            let path = PathBuf::from(&event.cwd);
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move {
                    let _ = git
                        .bind_pane_cwd(tab_id, pane_id, path, RefreshReason::CwdChanged)
                        .await;
                });
            }
        }
    }

    fn on_integration_mode_changed(&self, _event: &ShellIntegrationModeEvent) {}
}
