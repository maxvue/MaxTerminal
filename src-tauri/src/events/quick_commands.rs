//! Eventos Tauri emitidos pelo subsistema `mt-commands` (Fase 14).

use std::sync::Arc;

use mt_commands::artisan::model::{
    CatalogUpdatedPayload, CommandCreatedPayload, DiscoveryFailedPayload,
};
use mt_commands::ports::CommandsEventSink;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

/// Evento `artisan://catalog-updated` emitido quando o catálogo Artisan é atualizado.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "artisan://catalog-updated")]
pub struct ArtisanCatalogUpdatedTauriEvent(pub CatalogUpdatedPayload);

/// Evento `artisan://discovery-failed` emitido quando a descoberta em segundo plano falha.
#[derive(Debug, Clone, Serialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "artisan://discovery-failed")]
pub struct ArtisanDiscoveryFailedTauriEvent(pub DiscoveryFailedPayload);

/// Evento `artisan://command-created` emitido quando um comando Artisan é criado no disco.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "artisan://command-created")]
pub struct ArtisanCommandCreatedTauriEvent(pub CommandCreatedPayload);

/// Adaptador que encaminha eventos de `mt-commands` para o barramento Tauri.
pub struct TauriCommandsEventSink {
    app: AppHandle,
}

impl TauriCommandsEventSink {
    /// Cria uma nova instância de [`TauriCommandsEventSink`].
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl CommandsEventSink for TauriCommandsEventSink {
    fn catalog_updated(&self, payload: CatalogUpdatedPayload) {
        let _ = ArtisanCatalogUpdatedTauriEvent(payload).emit(&self.app);
    }

    fn discovery_failed(&self, payload: DiscoveryFailedPayload) {
        let _ = ArtisanDiscoveryFailedTauriEvent(payload).emit(&self.app);
    }

    fn command_created(&self, payload: CommandCreatedPayload) {
        let _ = ArtisanCommandCreatedTauriEvent(payload).emit(&self.app);
    }
}
