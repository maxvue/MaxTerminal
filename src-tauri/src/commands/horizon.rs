//! Comandos IPC Tauri para gerenciamento e monitoramento do Laravel Horizon.

use std::sync::Arc;

use mt_commands::horizon::{HorizonClearPreview, HorizonClearResult, HorizonSnapshot};
use mt_core::MtResult;
use mt_core::ids::ProjectId;
use tauri::State;

use crate::horizon::HorizonRuntimeService;

/// Obtém o snapshot das métricas, configuração e status do Laravel Horizon para o projeto.
#[tauri::command]
#[specta::specta]
pub async fn horizon_snapshot(
    project_id: ProjectId,
    state: State<'_, Arc<HorizonRuntimeService>>,
) -> MtResult<HorizonSnapshot> {
    state.snapshot(project_id).await
}

/// Inicia o daemon do Laravel Horizon em um PTY supervisionado e exclusivo.
#[tauri::command]
#[specta::specta]
pub async fn horizon_start(
    project_id: ProjectId,
    expected_context_revision: String,
    request_id: String,
    state: State<'_, Arc<HorizonRuntimeService>>,
) -> MtResult<HorizonSnapshot> {
    state
        .start(project_id, &expected_context_revision, &request_id)
        .await
}

/// Encerra graciosamente o processo master do Horizon supervisionado pelo MaxTerminal.
#[tauri::command]
#[specta::specta]
pub async fn horizon_stop(
    project_id: ProjectId,
    expected_generation: u64,
    request_id: String,
    state: State<'_, Arc<HorizonRuntimeService>>,
) -> MtResult<HorizonSnapshot> {
    state
        .stop(project_id, expected_generation, &request_id)
        .await
}

/// Solicita a prévia e o token temporário de confirmação para limpeza de tarefas pendentes.
#[tauri::command]
#[specta::specta]
pub async fn horizon_clear_pending_preview(
    project_id: ProjectId,
    state: State<'_, Arc<HorizonRuntimeService>>,
) -> MtResult<HorizonClearPreview> {
    state.clear_pending_preview(project_id).await
}

/// Executa a limpeza confirmada de tarefas pendentes (ready + delayed), preservando tarefas reservadas.
#[tauri::command]
#[specta::specta]
pub async fn horizon_clear_pending(
    project_id: ProjectId,
    token: String,
    request_id: String,
    state: State<'_, Arc<HorizonRuntimeService>>,
) -> MtResult<HorizonClearResult> {
    state.clear_pending(project_id, &token, &request_id).await
}
