//! Eventos Tauri do detector de portas ativas (`ports://opened`, `ports://updated`, `ports://closed` — Fase 08).

use std::sync::Arc;

use mt_ports::{PortClosed, PortOpened, PortUpdated, PortsEventSink};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_specta::Event;

/// Evento Tauri emitido quando uma porta passa pela histerese de abertura (`ports://opened`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "ports://opened")]
#[serde(transparent)]
pub struct PortOpenedTauriEvent(pub PortOpened);

/// Evento Tauri emitido quando uma porta ativa muda de PID ou endereços (`ports://updated`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "ports://updated")]
#[serde(transparent)]
pub struct PortUpdatedTauriEvent(pub PortUpdated);

/// Evento Tauri emitido quando uma porta deixa de estar em escuta (`ports://closed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[tauri_specta(event_name = "ports://closed")]
#[serde(transparent)]
pub struct PortClosedTauriEvent(pub PortClosed);

/// Adaptador que encaminha eventos do [`mt_ports::PortsService`] para o barramento Tauri.
#[derive(Clone)]
pub struct TauriPortsEventSink {
    app: AppHandle,
}

impl std::fmt::Debug for TauriPortsEventSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TauriPortsEventSink")
            .finish_non_exhaustive()
    }
}

impl TauriPortsEventSink {
    /// Cria um novo [`TauriPortsEventSink`] associado ao [`AppHandle`].
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl PortsEventSink for TauriPortsEventSink {
    fn emit_port_opened(&self, payload: PortOpened) {
        let _ = PortOpenedTauriEvent(payload).emit(&self.app);
    }

    fn emit_port_updated(&self, payload: PortUpdated) {
        let _ = PortUpdatedTauriEvent(payload).emit(&self.app);
    }

    fn emit_port_closed(&self, payload: PortClosed) {
        let _ = PortClosedTauriEvent(payload).emit(&self.app);
    }
}
