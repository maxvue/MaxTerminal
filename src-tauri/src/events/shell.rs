//! Eventos tipados dos domínios `tab://` e `shell://` e implementação de [`ShellEventSink`] acoplada ao `AppHandle`.

use std::sync::Arc;

use mt_core::shell::{
    CommandFinished, CommandStarted, ShellCwdChangedEvent, ShellIntegrationModeEvent,
    TabStateRemovedEvent, TabStateSnapshot,
};
use mt_shell::ShellEventSink;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

/// Evento emitido quando o snapshot de estado de uma aba muda (`tab://state-changed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "tab://state-changed")]
pub struct TabStateChangedEvent(pub TabStateSnapshot);

/// Evento emitido quando uma aba é removida do registro (`tab://state-removed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "tab://state-removed")]
pub struct TabStateRemovedTauriEvent(pub TabStateRemovedEvent);

/// Evento emitido quando um bloco de comando inicia (`shell://command-started`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "shell://command-started")]
pub struct ShellCommandStartedEvent(pub CommandStarted);

/// Evento emitido quando um bloco de comando termina (`shell://command-finished`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "shell://command-finished")]
pub struct ShellCommandFinishedEvent(pub CommandFinished);

/// Evento emitido quando o diretório de trabalho de um painel muda (`shell://cwd-changed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "shell://cwd-changed")]
pub struct ShellCwdChangedTauriEvent(pub ShellCwdChangedEvent);

/// Evento emitido quando o modo de integração de um painel muda (`shell://integration-mode`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "shell://integration-mode")]
pub struct ShellIntegrationModeTauriEvent(pub ShellIntegrationModeEvent);

/// Implementação de [`ShellEventSink`] que publica eventos Tauri para a janela principal
/// e replica notificações para observadores internos (Fases 08 e 09).
pub struct TauriShellEventSink<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
    extra_sinks: RwLock<Vec<Arc<dyn ShellEventSink>>>,
}

impl<R: Runtime> TauriShellEventSink<R> {
    /// Cria um novo sink vinculado ao [`AppHandle`] da aplicação.
    #[must_use]
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            extra_sinks: RwLock::new(Vec::new()),
        }
    }

    /// Registra um listener interno adicional de [`ShellEventSink`].
    pub fn add_listener(&self, listener: Arc<dyn ShellEventSink>) {
        self.extra_sinks.write().push(listener);
    }
}

impl<R: Runtime> ShellEventSink for TauriShellEventSink<R> {
    fn on_tab_state_changed(&self, snapshot: &TabStateSnapshot) {
        if let Err(err) = TabStateChangedEvent(snapshot.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_shell",
                error = %err,
                "Falha ao emitir evento tab://state-changed"
            );
        }
        let listeners = self.extra_sinks.read().clone();
        for l in listeners {
            l.on_tab_state_changed(snapshot);
        }
    }

    fn on_tab_state_removed(&self, event: &TabStateRemovedEvent) {
        if let Err(err) = TabStateRemovedTauriEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_shell",
                error = %err,
                "Falha ao emitir evento tab://state-removed"
            );
        }
        let listeners = self.extra_sinks.read().clone();
        for l in listeners {
            l.on_tab_state_removed(event);
        }
    }

    fn on_command_started(&self, event: &CommandStarted) {
        if let Err(err) = ShellCommandStartedEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_shell",
                error = %err,
                "Falha ao emitir evento shell://command-started"
            );
        }
        let listeners = self.extra_sinks.read().clone();
        for l in listeners {
            l.on_command_started(event);
        }
    }

    fn on_command_finished(&self, event: &CommandFinished) {
        if let Err(err) = ShellCommandFinishedEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_shell",
                error = %err,
                "Falha ao emitir evento shell://command-finished"
            );
        }
        let listeners = self.extra_sinks.read().clone();
        for l in listeners {
            l.on_command_finished(event);
        }
    }

    fn on_cwd_changed(&self, event: &ShellCwdChangedEvent) {
        if let Err(err) = ShellCwdChangedTauriEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_shell",
                error = %err,
                "Falha ao emitir evento shell://cwd-changed"
            );
        }
        let listeners = self.extra_sinks.read().clone();
        for l in listeners {
            l.on_cwd_changed(event);
        }
    }

    fn on_integration_mode_changed(&self, event: &ShellIntegrationModeEvent) {
        if let Err(err) = ShellIntegrationModeTauriEvent(event.clone()).emit(&self.app) {
            tracing::warn!(
                target: "mt_shell",
                error = %err,
                "Falha ao emitir evento shell://integration-mode"
            );
        }
        let listeners = self.extra_sinks.read().clone();
        for l in listeners {
            l.on_integration_mode_changed(event);
        }
    }
}
