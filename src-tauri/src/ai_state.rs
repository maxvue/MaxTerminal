//! Estado Tauri gerenciado para o subsistema de IA (`mt-ai` — Fase 12).

use std::sync::Arc;

use mt_ai::AiService;
use tauri::AppHandle;

/// Estado compartilhado do `AiService` registrado em `app.manage(...)`.
#[derive(Clone)]
pub struct AppAiState {
    /// Instância central do serviço de IA (`mt-ai`).
    pub service: Arc<AiService>,
    /// Handle da aplicação Tauri para emissão de eventos tipados.
    pub app: AppHandle,
}

impl AppAiState {
    /// Cria o estado encapsulando o `AiService` e o `AppHandle`.
    #[must_use]
    pub fn new(service: Arc<AiService>, app: AppHandle) -> Self {
        Self { service, app }
    }
}
