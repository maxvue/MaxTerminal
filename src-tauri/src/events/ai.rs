//! Eventos Tauri emitidos pelo subsistema `mt-ai` (Fase 12).

use std::sync::Arc;
use std::time::Duration;

use mt_ai::AiService;
use mt_ai::catalog::HarnessCatalog;
use mt_ai::client::http::{GatewayHealth, GatewayStatus};
use mt_ai::config::AiSettings;
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::AppHandle;
use tauri_specta::Event;

/// Evento `ai://status-changed` emitido quando o estado de conectividade do gateway muda.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "ai://status-changed")]
pub struct AiStatusChangedTauriEvent(pub GatewayHealth);

/// Evento `ai://catalog-updated` emitido quando o catálogo de harnesses/modelos é atualizado.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "ai://catalog-updated")]
pub struct AiCatalogUpdatedTauriEvent(pub HarnessCatalog);

/// Evento `ai://settings-changed` emitido quando as preferências de IA são salvas.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "ai://settings-changed")]
pub struct AiSettingsChangedTauriEvent(pub AiSettings);

/// Inicia as tarefas em background do subsistema de IA:
/// 1. Forwarder de mudanças de `GatewayHealth` para o evento Tauri `ai://status-changed`.
/// 2. Health check inicial + atualização oportunista de catálogo + sonda periódica a cada 30s.
pub fn spawn_ai_background_tasks(app: AppHandle, service: Arc<AiService>) {
    let mut rx = service.subscribe_status();
    let app_for_status = app.clone();
    tauri::async_runtime::spawn(async move {
        while rx.changed().await.is_ok() {
            let snapshot = rx.borrow().clone();
            let _ = AiStatusChangedTauriEvent(snapshot).emit(&app_for_status);
        }
    });

    let app_for_probe = app;
    tauri::async_runtime::spawn(async move {
        let initial = service.health().await;
        if initial.status == GatewayStatus::Online
            && let Ok(cat) = service.catalog_refresh().await
        {
            let _ = AiCatalogUpdatedTauriEvent(cat).emit(&app_for_probe);
        }

        let mut ticker = tokio::time::interval(Duration::from_secs(30));
        ticker.tick().await;
        loop {
            ticker.tick().await;
            let _ = service.health().await;
        }
    });
}
