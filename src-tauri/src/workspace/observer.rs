//! Observadores de ciclo de vida de painéis, abas e sessões (`WorkspaceLifecycleObserver`)
//! para integração desacoplada das Fases 08, 09 e 12.

use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_pty::ExitInfo;

/// Trait de observação de eventos estruturais do [`super::WorkspaceManager`].
pub trait WorkspaceLifecycleObserver: Send + Sync + 'static {
    /// Invocado quando um painel recebe um novo PTY ativo (abertura, split, restart, nova sessão ou restore).
    fn on_pane_spawned(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        pty_id: PtyId,
        session_id: SessionId,
    ) {
        let _ = (tab_id, pane_id, pty_id, session_id);
    }

    /// Invocado quando o PTY ativo de um painel encerra.
    fn on_pane_exited(&self, tab_id: TabId, pane_id: PaneId, pty_id: PtyId, exit: &ExitInfo) {
        let _ = (tab_id, pane_id, pty_id, exit);
    }

    /// Invocado quando uma `tab_session` é arquivada (`session_new` ou `tab_close`).
    fn on_session_archived(&self, tab_id: TabId, tab_session_id: TabSessionId) {
        let _ = (tab_id, tab_session_id);
    }

    /// Invocado quando uma aba inteira é fechada definitivamente.
    fn on_tab_closed(&self, project_id: ProjectId, tab_id: TabId) {
        let _ = (project_id, tab_id);
    }
}
