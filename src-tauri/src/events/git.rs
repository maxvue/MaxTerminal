//! Eventos Tauri do subsistema Git e Worktrees (`git://status-changed` — Fase 08).

use std::sync::Arc;

use mt_git::{GitEventSink, GitStatusChanged};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_specta::Event;

/// Evento Tauri emitido quando o status Git de um repositório/worktree muda (`git://status-changed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "git://status-changed")]
#[serde(transparent)]
pub struct GitStatusChangedTauriEvent(pub GitStatusChanged);

/// Adaptador que encaminha eventos do [`mt_git::GitService`] para o barramento Tauri.
#[derive(Clone)]
pub struct TauriGitEventSink {
    app: AppHandle,
}

impl std::fmt::Debug for TauriGitEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TauriGitEventSink").finish_non_exhaustive()
    }
}

impl TauriGitEventSink {
    /// Cria um novo [`TauriGitEventSink`] associado ao [`AppHandle`].
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl GitEventSink for TauriGitEventSink {
    fn emit_status_changed(&self, payload: GitStatusChanged) {
        let _ = GitStatusChangedTauriEvent(payload).emit(&self.app);
    }
}
