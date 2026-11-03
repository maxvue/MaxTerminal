//! Eventos tipados Tauri para o subsistema `mt-bridge` (`bridge://*` — Fase 11).

use std::sync::Arc;

use mt_bridge::{
    BridgeError, BridgeEventSink, BridgeProjectSyncedEvent, BridgeStatusSnapshot,
    BridgeTerminalOpenedEvent, BugCycleProgressPayload,
};
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

/// Evento Tauri `bridge://status-changed`.
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "bridge://status-changed")]
pub struct BridgeStatusChangedTauriEvent(pub BridgeStatusSnapshot);

/// Evento Tauri `bridge://project-synced`.
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "bridge://project-synced")]
pub struct BridgeProjectSyncedTauriEvent(pub BridgeProjectSyncedEvent);

/// Evento Tauri `bridge://terminal-opened`.
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "bridge://terminal-opened")]
pub struct BridgeTerminalOpenedTauriEvent(pub BridgeTerminalOpenedEvent);

/// Evento Tauri `bridge://bug-cycle-progress`.
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "bridge://bug-cycle-progress")]
pub struct BridgeBugCycleProgressTauriEvent(pub BugCycleProgressPayload);

/// Evento Tauri `bridge://error`.
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "bridge://error")]
pub struct BridgeErrorTauriEvent(pub BridgeError);

/// Adaptador que encaminha eventos do `BridgeService` para o frontend via `tauri_specta::Event`.
pub struct TauriBridgeEventSink<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriBridgeEventSink<R> {
    /// Cria uma nova instância compartilhada de `TauriBridgeEventSink`.
    #[must_use]
    pub fn new(app: AppHandle<R>) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl<R: Runtime> BridgeEventSink for TauriBridgeEventSink<R> {
    fn emit_status_changed(&self, status: &BridgeStatusSnapshot) {
        if let Err(err) = BridgeStatusChangedTauriEvent(status.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_bridge",
                error = %err,
                "Falha ao emitir evento bridge://status-changed"
            );
        }
    }

    fn emit_project_synced(&self, event: &BridgeProjectSyncedEvent) {
        if let Err(err) = BridgeProjectSyncedTauriEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_bridge",
                error = %err,
                "Falha ao emitir evento bridge://project-synced"
            );
        }
    }

    fn emit_terminal_opened(&self, event: &BridgeTerminalOpenedEvent) {
        if let Err(err) = BridgeTerminalOpenedTauriEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_bridge",
                error = %err,
                "Falha ao emitir evento bridge://terminal-opened"
            );
        }
    }

    fn emit_bug_cycle_progress(&self, event: &BugCycleProgressPayload) {
        if let Err(err) = BridgeBugCycleProgressTauriEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_bridge",
                error = %err,
                "Falha ao emitir evento bridge://bug-cycle-progress"
            );
        }
    }

    fn emit_error(&self, error: &BridgeError) {
        if let Err(err) = BridgeErrorTauriEvent(error.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_bridge",
                error = %err,
                "Falha ao emitir evento bridge://error"
            );
        }
    }
}
