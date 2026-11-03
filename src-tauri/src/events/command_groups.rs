//! Eventos Tauri e assinante de `Channel<GroupOutputFrame>` para Grupos de Comandos (`>>` — Fase 15).

use std::sync::Arc;

use mt_commands::groups::events::{
    GroupChangedEvent, GroupEventSink, GroupOutputFrame, GroupRunEvent, OutputFrameSubscriber,
};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, ipc::Channel};
use tauri_specta::Event;

/// Evento `command-group://changed` emitido em mutações CRUD de grupos de comandos.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "command-group://changed")]
pub struct CommandGroupChangedTauriEvent(pub GroupChangedEvent);

/// Evento `command-group://run-started` emitido quando uma execução de grupo inicia.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "command-group://run-started")]
pub struct CommandGroupRunStartedTauriEvent(pub GroupRunEvent);

/// Evento `command-group://run-item-changed` emitido quando um item da execução muda de estado.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "command-group://run-item-changed")]
pub struct CommandGroupRunItemChangedTauriEvent(pub GroupRunEvent);

/// Evento `command-group://run-finished` emitido quando uma execução de grupo atinge estado terminal.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "command-group://run-finished")]
pub struct CommandGroupRunFinishedTauriEvent(pub GroupRunEvent);

/// Adaptador que encaminha eventos de `CommandGroupService` para o barramento global do Tauri.
pub struct TauriGroupEventSink {
    app: AppHandle,
}

impl TauriGroupEventSink {
    /// Cria uma nova instância de [`TauriGroupEventSink`].
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl GroupEventSink for TauriGroupEventSink {
    fn emit_group_changed(&self, event: GroupChangedEvent) {
        let _ = CommandGroupChangedTauriEvent(event).emit(&self.app);
    }

    fn emit_run_started(&self, event: GroupRunEvent) {
        let _ = CommandGroupRunStartedTauriEvent(event).emit(&self.app);
    }

    fn emit_run_item_changed(&self, event: GroupRunEvent) {
        let _ = CommandGroupRunItemChangedTauriEvent(event).emit(&self.app);
    }

    fn emit_run_finished(&self, event: GroupRunEvent) {
        let _ = CommandGroupRunFinishedTauriEvent(event).emit(&self.app);
    }
}

/// Assinante de stream de saída que envia [`GroupOutputFrame`] via `tauri::ipc::Channel<GroupOutputFrame>`.
pub struct ChannelOutputSubscriber {
    ch: Channel<GroupOutputFrame>,
}

impl ChannelOutputSubscriber {
    /// Cria um novo assinante de saída vinculado ao `Channel` informado.
    #[must_use]
    pub fn new(ch: Channel<GroupOutputFrame>) -> Arc<Self> {
        Arc::new(Self { ch })
    }
}

impl OutputFrameSubscriber for ChannelOutputSubscriber {
    fn send_frame(&self, frame: GroupOutputFrame) -> bool {
        self.ch.send(frame).is_ok()
    }
}
