//! Eventos Tauri do domínio GitHub Issues (`github:*` — Fase 16).

use std::sync::Arc;

use mt_github::events::{
    GithubAuthChangedPayload, GithubBugCycleProgressEventPayload, GithubIssueUpdatedPayload,
    GithubIssuesChangedPayload, GithubLabelsSyncedPayload, GithubRateLimitPayload,
};
use mt_github::ports::GithubEventSink;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

/// Evento `github:issues-changed` emitido quando a listagem ou contadores de issues mudam.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "github:issues-changed")]
pub struct GithubIssuesChangedTauriEvent(pub GithubIssuesChangedPayload);

/// Evento `github:issue-updated` emitido quando uma issue específica ou suas labels são atualizadas.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "github:issue-updated")]
pub struct GithubIssueUpdatedTauriEvent(pub GithubIssueUpdatedPayload);

/// Evento `github:labels-synced` emitido quando o catálogo de labels do repositório é sincronizado.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "github:labels-synced")]
pub struct GithubLabelsSyncedTauriEvent(pub GithubLabelsSyncedPayload);

/// Evento `github:auth-changed` emitido quando o estado de autenticação GitHub muda.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "github:auth-changed")]
pub struct GithubAuthChangedTauriEvent(pub GithubAuthChangedPayload);

/// Evento `github:bug-cycle-progress` emitido durante o ciclo automatizado "Corrigir todos os Bugs".
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "github:bug-cycle-progress")]
pub struct GithubBugCycleProgressTauriEvent(pub GithubBugCycleProgressEventPayload);

/// Evento `github:rate-limit` emitido quando o snapshot de cota do GitHub é atualizado.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "github:rate-limit")]
pub struct GithubRateLimitTauriEvent(pub GithubRateLimitPayload);

/// Adaptador que encaminha eventos de `GithubService` para o barramento Tauri.
pub struct TauriGithubEventSink {
    app: AppHandle,
}

impl TauriGithubEventSink {
    /// Cria uma nova instância de [`TauriGithubEventSink`].
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl GithubEventSink for TauriGithubEventSink {
    fn emit_issues_changed(&self, payload: GithubIssuesChangedPayload) {
        let _ = GithubIssuesChangedTauriEvent(payload).emit(&self.app);
    }

    fn emit_issue_updated(&self, payload: GithubIssueUpdatedPayload) {
        let _ = GithubIssueUpdatedTauriEvent(payload).emit(&self.app);
    }

    fn emit_labels_synced(&self, payload: GithubLabelsSyncedPayload) {
        let _ = GithubLabelsSyncedTauriEvent(payload).emit(&self.app);
    }

    fn emit_auth_changed(&self, payload: GithubAuthChangedPayload) {
        let _ = GithubAuthChangedTauriEvent(payload).emit(&self.app);
    }

    fn emit_bug_cycle_progress(&self, payload: GithubBugCycleProgressEventPayload) {
        let _ = GithubBugCycleProgressTauriEvent(payload).emit(&self.app);
    }

    fn emit_rate_limit(&self, payload: GithubRateLimitPayload) {
        let _ = GithubRateLimitTauriEvent(payload).emit(&self.app);
    }
}
