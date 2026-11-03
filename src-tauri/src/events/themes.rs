//! Eventos tipados Tauri para `theme://changed` e `theme://library-changed`.

use std::sync::Arc;

use mt_themes::{ThemeChangedEvent, ThemeEventSink, ThemeLibraryChangedEvent};
use serde::Serialize;
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

/// Evento Tauri `theme://changed`.
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "theme://changed")]
pub struct ThemeChangedTauriEvent(pub ThemeChangedEvent);

/// Evento Tauri `theme://library-changed`.
#[derive(Debug, Clone, Serialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "theme://library-changed")]
pub struct ThemeLibraryChangedTauriEvent(pub ThemeLibraryChangedEvent);

/// Adaptador que emite eventos do `ThemeService` para o runtime Tauri.
pub struct TauriThemeEventSink<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriThemeEventSink<R> {
    /// Constrói um novo `TauriThemeEventSink` compartilhado.
    #[must_use]
    pub fn new(app: AppHandle<R>) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl<R: Runtime> ThemeEventSink for TauriThemeEventSink<R> {
    fn emit_theme_changed(&self, payload: ThemeChangedEvent) {
        if let Err(err) = ThemeChangedTauriEvent(payload).emit(&self.app) {
            tracing::warn!(
                target: "mt_themes",
                error = %err,
                "Falha ao emitir evento theme://changed"
            );
        }
    }

    fn emit_library_changed(&self, payload: ThemeLibraryChangedEvent) {
        if let Err(err) = ThemeLibraryChangedTauriEvent(payload).emit(&self.app) {
            tracing::warn!(
                target: "mt_themes",
                error = %err,
                "Falha ao emitir evento theme://library-changed"
            );
        }
    }
}
