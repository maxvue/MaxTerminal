//! Comandos IPC Tauri (`bridge_*`) para a integração bidirecional com a extensão `MaxCode` (Fase 11).

use std::sync::Arc;

use mt_bridge::{
    BridgeService, BridgeSettings, BridgeSettingsPatch, BridgeStatusSnapshot, BugsCycleAckPayload,
    BugsCycleStartPayload, InspectDispatchResult, IssueActionDispatchOutcome, IssueCommandPayload,
    IssueProtocolSupport, OpenFilePayload,
};
use mt_core::MtError;
use tauri::State;

/// Retorna o snapshot atual de conexão e telemetria da ponte com o `MaxCode` (`bridge_status`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_status(
    svc: State<'_, Arc<BridgeService>>,
) -> Result<BridgeStatusSnapshot, MtError> {
    Ok(svc.status())
}

/// Retorna as configurações atuais da ponte (`bridge_get_settings`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_get_settings(
    svc: State<'_, Arc<BridgeService>>,
) -> Result<BridgeSettings, MtError> {
    Ok(svc.get_settings())
}

/// Atualiza as configurações da ponte e aplica hot-reload no servidor loopback (`bridge_update_settings`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_update_settings(
    svc: State<'_, Arc<BridgeService>>,
    patch: BridgeSettingsPatch,
) -> Result<BridgeSettings, MtError> {
    svc.update_settings(patch).await.map_err(MtError::from)
}

/// Dispara a inspeção de um bloco de comando com falha em Plan Mode no `MaxCode` (`bridge_inspect_command_error`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_inspect_command_error(
    svc: State<'_, Arc<BridgeService>>,
    block_id: String,
) -> Result<InspectDispatchResult, MtError> {
    svc.inspect_command_error(&block_id)
        .await
        .map_err(MtError::from)
}

/// Notifica o `MaxCode` sobre o projeto ativo selecionado no `MaxTerminal` (`bridge_notify_project_selected`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_notify_project_selected(
    svc: State<'_, Arc<BridgeService>>,
    project_path: String,
    project_name: Option<String>,
    git_branch: Option<String>,
) -> Result<bool, MtError> {
    svc.notify_project_selected(&project_path, project_name, git_branch)
        .await
        .map_err(MtError::from)
}

/// Solicita ao `MaxCode` a abertura de um arquivo em linha e coluna (`bridge_open_file_in_maxcode`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_open_file_in_maxcode(
    svc: State<'_, Arc<BridgeService>>,
    payload: OpenFilePayload,
) -> Result<bool, MtError> {
    svc.open_file_in_maxcode(payload)
        .await
        .map_err(MtError::from)
}

/// Força releitura do arquivo de descoberta `maxcode.endpoint.json` e reconexão imediata (`bridge_reconnect_now`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_reconnect_now(
    svc: State<'_, Arc<BridgeService>>,
) -> Result<BridgeStatusSnapshot, MtError> {
    svc.reconnect_now().await.map_err(MtError::from)
}

/// Retorna as capabilities do protocolo de Issues suportadas pela instância conectada do `MaxCode` (`bridge_issue_protocol_support`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_issue_protocol_support(
    svc: State<'_, Arc<BridgeService>>,
) -> Result<IssueProtocolSupport, MtError> {
    Ok(svc.issue_protocol_support())
}

/// Envia uma ação unitária de Issue (`Abrir`, `Analisar (BUG PLAN)`, `Corrigir (BUG FIX)`) ao `MaxCode` (`bridge_send_issue_command`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_send_issue_command(
    svc: State<'_, Arc<BridgeService>>,
    payload: IssueCommandPayload,
) -> Result<IssueActionDispatchOutcome, MtError> {
    svc.send_issue_command(payload).await.map_err(MtError::from)
}

/// Calcula a prévia (`dry_run = true`) do ciclo automatizado de bugs em lote (`bridge_bugs_cycle_preview`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_bugs_cycle_preview(
    svc: State<'_, Arc<BridgeService>>,
    payload: BugsCycleStartPayload,
) -> Result<BugsCycleAckPayload, MtError> {
    svc.bugs_cycle_preview(payload).await.map_err(MtError::from)
}

/// Inicia (`dry_run = false`) o ciclo automatizado de bugs em lote no `MaxCode` (`bridge_bugs_cycle_start`).
#[tauri::command]
#[specta::specta]
pub async fn bridge_bugs_cycle_start(
    svc: State<'_, Arc<BridgeService>>,
    payload: BugsCycleStartPayload,
) -> Result<BugsCycleAckPayload, MtError> {
    svc.bugs_cycle_start(payload).await.map_err(MtError::from)
}
