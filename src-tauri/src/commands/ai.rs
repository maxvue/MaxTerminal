//! Comandos IPC Tauri do subsistema `mt-ai` (Fase 12 — Integração `MaxAiManager`).

use std::sync::Arc;

use mt_ai::catalog::HarnessCatalog;
use mt_ai::client::http::GatewayHealth;
use mt_ai::config::AiSettings;
use mt_ai::context::AiContext;
use mt_ai::diagnose::{DiagnoseRequest, DiagnosisOutcome};
use mt_ai::ids::AiRequestId;
use mt_ai::rag::{RagSearchRequest, RagSearchResponse, RagSlug};
use mt_ai::repo::{AiHistoryFilter, AiHistoryPage, CacheClearScope};
use mt_ai::risk::RiskAssessment;
use mt_ai::structured::{StructuredRequest, StructuredResponse};
use mt_ai::suggest::{AiStreamEvent, SuggestRequest};
use mt_core::error::MtError;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId};
use tauri::{State, ipc::Channel};
use tauri_specta::Event;

use crate::ai_state::AppAiState;
use crate::events::ai::{AiCatalogUpdatedTauriEvent, AiSettingsChangedTauriEvent};

/// Executa sonda imediata de conectividade com o gateway `MaxAiManager` (`ai_health`).
#[tauri::command]
#[specta::specta]
pub async fn ai_health(state: State<'_, AppAiState>) -> Result<GatewayHealth, MtError> {
    Ok(state.service.health().await)
}

/// Retorna as configurações de IA (`ai_settings_get`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn ai_settings_get(state: State<'_, AppAiState>) -> Result<AiSettings, MtError> {
    Ok(state.service.get_settings())
}

/// Atualiza configurações de IA e emite `ai://settings-changed` (`ai_settings_update`).
#[tauri::command]
#[specta::specta]
pub async fn ai_settings_update(
    state: State<'_, AppAiState>,
    settings: AiSettings,
) -> Result<AiSettings, MtError> {
    let saved = state.service.update_settings(settings).await?;
    let _ = AiSettingsChangedTauriEvent(saved.clone()).emit(&state.app);
    Ok(saved)
}

/// Obtém o catálogo de harnesses, modelos e níveis de raciocínio (`ai_catalog_get`).
#[tauri::command]
#[specta::specta]
pub async fn ai_catalog_get(state: State<'_, AppAiState>) -> Result<HarnessCatalog, MtError> {
    Ok(state.service.catalog_get().await)
}

/// Força a reconsulta do catálogo ao gateway e emite `ai://catalog-updated` (`ai_catalog_refresh`).
#[tauri::command]
#[specta::specta]
pub async fn ai_catalog_refresh(state: State<'_, AppAiState>) -> Result<HarnessCatalog, MtError> {
    let cat = state.service.catalog_refresh().await?;
    let _ = AiCatalogUpdatedTauriEvent(cat.clone()).emit(&state.app);
    Ok(cat)
}

/// Retorna o preview exato e sanitizado do contexto que será enviado ao gateway (`ai_context_preview`).
#[tauri::command]
#[specta::specta]
pub async fn ai_context_preview(
    state: State<'_, AppAiState>,
    pane_id: PaneId,
) -> Result<AiContext, MtError> {
    state.service.context_preview(pane_id).await
}

/// Inicia a tradução NL → Shell com streaming incremental via `tauri::ipc::Channel<AiStreamEvent>` (`ai_suggest_command`).
/// Executa `prepare_suggest` sincronamente antes do spawn e retorna `AiRequestId` imediatamente (`CA-09`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn ai_suggest_command(
    state: State<'_, AppAiState>,
    req: SuggestRequest,
    on_event: Channel<AiStreamEvent>,
) -> Result<AiRequestId, MtError> {
    let ticket = state.service.prepare_suggest(&req)?;
    let req_id = req.request_id;
    let svc = Arc::clone(&state.service);
    tauri::async_runtime::spawn(async move {
        let _ = svc
            .run_suggest_stream(req, ticket, move |evt| on_event.send(evt).is_ok())
            .await;
    });
    Ok(req_id)
}

/// Cancela imediatamente uma requisição de IA em andamento (`ai_cancel`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn ai_cancel(
    state: State<'_, AppAiState>,
    request_id: AiRequestId,
) -> Result<bool, MtError> {
    Ok(state.service.cancel(request_id))
}

/// Reavalia deterministicamente o risco de um comando shell (`ai_classify_risk`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn ai_classify_risk(
    state: State<'_, AppAiState>,
    command: String,
) -> Result<RiskAssessment, MtError> {
    Ok(state.service.classify_risk(&command))
}

/// Gera ou recupera do cache o diagnóstico rápido inline para um bloco falho (`ai_diagnose_block`).
#[tauri::command]
#[specta::specta]
pub async fn ai_diagnose_block(
    state: State<'_, AppAiState>,
    req: DiagnoseRequest,
) -> Result<DiagnosisOutcome, MtError> {
    state.service.diagnose_block(req).await
}

/// Marca o diagnóstico rápido inline de um bloco como dispensado (`ai_diagnosis_dismiss`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn ai_diagnosis_dismiss(
    state: State<'_, AppAiState>,
    block_id: CommandBlockId,
) -> Result<(), MtError> {
    state.service.dismiss_diagnosis(block_id);
    Ok(())
}

/// Lista os slugs RAG disponíveis no `MaxAiManager` (`ai_rag_list_slugs`).
#[tauri::command]
#[specta::specta]
pub async fn ai_rag_list_slugs(state: State<'_, AppAiState>) -> Result<Vec<RagSlug>, MtError> {
    state.service.rag_list_slugs().await
}

/// Executa consulta RAG em `/v1/rags/{slug}/search` (`ai_rag_search`).
#[tauri::command]
#[specta::specta]
pub async fn ai_rag_search(
    state: State<'_, AppAiState>,
    req: RagSearchRequest,
) -> Result<RagSearchResponse, MtError> {
    state.service.rag_search(req).await
}

/// Executa geração estruturada JSON validada por schema (`ai_generate_structured`).
#[tauri::command]
#[specta::specta]
pub async fn ai_generate_structured(
    state: State<'_, AppAiState>,
    req: StructuredRequest,
) -> Result<StructuredResponse, MtError> {
    state.service.generate_structured(req).await
}

/// Lista paginadamente o histórico de interações de IA (`ai_history_list`).
#[tauri::command]
#[specta::specta]
pub async fn ai_history_list(
    state: State<'_, AppAiState>,
    filter: AiHistoryFilter,
) -> Result<AiHistoryPage, MtError> {
    state.service.history_list(filter).await
}

/// Remove registros do histórico de IA e retorna a quantidade removida (`ai_history_clear`).
#[tauri::command]
#[specta::specta]
pub async fn ai_history_clear(
    state: State<'_, AppAiState>,
    project_id: Option<ProjectId>,
) -> Result<u32, MtError> {
    state.service.history_clear(project_id).await
}

/// Limpa os caches de IA conforme o escopo informado e retorna a quantidade removida (`ai_cache_clear`).
#[tauri::command]
#[specta::specta]
pub async fn ai_cache_clear(
    state: State<'_, AppAiState>,
    scope: CacheClearScope,
) -> Result<u32, MtError> {
    state.service.cache_clear(scope).await
}
