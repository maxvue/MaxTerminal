//! Eventos tipados do domínio `os://` (Fase 09) e adaptador [`TauriOsEventSink`].

use std::sync::Arc;

use mt_os::{
    FocusTabPayload, LinkContext, OpenPreferencesPayload, OsEnvironment, OsEvent, OsEventSink,
    OsSettings, ShortcutStatus, ShutdownFailedPayload, ShutdownStartedPayload, StatusCounters,
    WindowVisibilityPayload,
};
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

/// Evento emitido para focar uma aba/painel específico (`os://focus-tab`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://focus-tab")]
pub struct OsFocusTabTauriEvent(pub OsEvent<FocusTabPayload>);

/// Evento emitido quando a visibilidade ou foco da janela muda (`os://window-visibility`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://window-visibility")]
pub struct OsWindowVisibilityTauriEvent(pub OsEvent<WindowVisibilityPayload>);

/// Evento emitido após atualização bem-sucedida de `OsSettings` (`os://settings-changed`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://settings-changed")]
pub struct OsSettingsChangedTauriEvent(pub OsEvent<OsSettings>);

/// Evento emitido quando o status do atalho global muda (`os://shortcut-status`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://shortcut-status")]
pub struct OsShortcutStatusTauriEvent(pub OsEvent<ShortcutStatus>);

/// Evento emitido quando os contadores globais da bandeja mudam (`os://counters`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://counters")]
pub struct OsCountersTauriEvent(pub OsEvent<StatusCounters>);

/// Evento emitido quando o usuário solicita abrir Preferências pela bandeja (`os://open-preferences`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://open-preferences")]
pub struct OsOpenPreferencesTauriEvent(pub OsEvent<OpenPreferencesPayload>);

/// Evento emitido ao iniciar o ciclo de encerramento (`os://shutdown-started`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://shutdown-started")]
pub struct OsShutdownStartedTauriEvent(pub OsEvent<ShutdownStartedPayload>);

/// Evento emitido se o encerramento falhar por sobreviventes remanescentes (`os://shutdown-failed`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://shutdown-failed")]
pub struct OsShutdownFailedTauriEvent(pub OsEvent<ShutdownFailedPayload>);

/// Evento emitido quando o ambiente gráfico/D-Bus detectado muda (`os://environment-changed`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://environment-changed")]
pub struct OsEnvironmentChangedTauriEvent(pub OsEvent<OsEnvironment>);

/// Evento emitido quando o contexto de resolução de links de um painel muda (`os://link-context-changed`).
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "os://link-context-changed")]
pub struct OsLinkContextChangedTauriEvent(pub OsEvent<LinkContext>);

/// Implementação de [`OsEventSink`] acoplada ao [`AppHandle`] do Tauri.
pub struct TauriOsEventSink<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriOsEventSink<R> {
    /// Cria um novo [`TauriOsEventSink`] compartilhado.
    #[must_use]
    pub fn new(app: AppHandle<R>) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl<R: Runtime> OsEventSink for TauriOsEventSink<R> {
    fn emit_focus_tab(&self, event: OsEvent<FocusTabPayload>) {
        let _ = OsFocusTabTauriEvent(event).emit(&self.app);
    }

    fn emit_window_visibility(&self, event: OsEvent<WindowVisibilityPayload>) {
        let _ = OsWindowVisibilityTauriEvent(event).emit(&self.app);
    }

    fn emit_settings_changed(&self, event: OsEvent<OsSettings>) {
        let _ = OsSettingsChangedTauriEvent(event).emit(&self.app);
    }

    fn emit_shortcut_status(&self, event: OsEvent<ShortcutStatus>) {
        let _ = OsShortcutStatusTauriEvent(event).emit(&self.app);
    }

    fn emit_counters(&self, event: OsEvent<StatusCounters>) {
        let _ = OsCountersTauriEvent(event).emit(&self.app);
    }

    fn emit_open_preferences(&self, event: OsEvent<OpenPreferencesPayload>) {
        let _ = OsOpenPreferencesTauriEvent(event).emit(&self.app);
    }

    fn emit_shutdown_started(&self, event: OsEvent<ShutdownStartedPayload>) {
        let _ = OsShutdownStartedTauriEvent(event).emit(&self.app);
    }

    fn emit_shutdown_failed(&self, event: OsEvent<ShutdownFailedPayload>) {
        let _ = OsShutdownFailedTauriEvent(event).emit(&self.app);
    }

    fn emit_environment_changed(&self, event: OsEvent<OsEnvironment>) {
        let _ = OsEnvironmentChangedTauriEvent(event).emit(&self.app);
    }

    fn emit_link_context_changed(&self, event: OsEvent<LinkContext>) {
        let _ = OsLinkContextChangedTauriEvent(event).emit(&self.app);
    }
}
