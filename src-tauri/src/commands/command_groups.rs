//! Comandos IPC Tauri para Grupos de Comandos (`>>` — Fase 15).

use mt_commands::groups::events::{GroupOutputFrame, OutputAttachReceipt};
use mt_commands::groups::model::{
    CommandGroup, CommandGroupCatalog, CommandGroupInput, DeleteGroupReceipt, GroupRun, GroupRunId,
    GroupRunItemId, GroupRunListPage, GroupRunRequest, GroupScope, OutputSubscriptionId,
    ReorderGroupEntry,
};
use mt_commands::groups::suggest::{
    GroupSuggestRequest, GroupSuggestResponse, RecurringCommandsResponse,
};
use mt_core::error::MtError;
use mt_core::ids::{CommandGroupId, ProjectId};
use tauri::{State, ipc::Channel};

use crate::command_groups::AppCommandGroupState;
use crate::events::command_groups::ChannelOutputSubscriber;

/// Lista os grupos locais do projeto e globais ordenados canonicamente (`command_group_list`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_list(
    state: State<'_, AppCommandGroupState>,
    project_id: Option<ProjectId>,
) -> Result<CommandGroupCatalog, MtError> {
    state.service.list(project_id).await
}

/// Obtém o detalhe de um grupo de comandos pelo ID (`command_group_get`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_get(
    state: State<'_, AppCommandGroupState>,
    group_id: CommandGroupId,
) -> Result<CommandGroup, MtError> {
    state.service.get(group_id).await
}

/// Cria um novo grupo de comandos local ou global (`command_group_create`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_create(
    state: State<'_, AppCommandGroupState>,
    input: CommandGroupInput,
) -> Result<CommandGroup, MtError> {
    let created = state.service.create(input).await?;
    crate::search::adapters::sync_projections_and_invalidate(
        &state.db,
        &state.app,
        "command_group_created",
    );
    Ok(created)
}

/// Atualiza um grupo de comandos com controle de concorrência otimista (`command_group_update`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_update(
    state: State<'_, AppCommandGroupState>,
    group_id: CommandGroupId,
    expected_revision: u64,
    input: CommandGroupInput,
) -> Result<CommandGroup, MtError> {
    let updated = state
        .service
        .update(group_id, expected_revision, input)
        .await?;
    crate::search::adapters::sync_projections_and_invalidate(
        &state.db,
        &state.app,
        "command_group_updated",
    );
    Ok(updated)
}

/// Remove um grupo de comandos com controle de revisão (`command_group_delete`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_delete(
    state: State<'_, AppCommandGroupState>,
    group_id: CommandGroupId,
    expected_revision: u64,
) -> Result<DeleteGroupReceipt, MtError> {
    let receipt = state.service.delete(group_id, expected_revision).await?;
    crate::search::adapters::sync_projections_and_invalidate(
        &state.db,
        &state.app,
        "command_group_deleted",
    );
    Ok(receipt)
}

/// Reordena canonicamente um escopo de grupos (`command_group_reorder`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_reorder(
    state: State<'_, AppCommandGroupState>,
    scope: GroupScope,
    project_id: Option<ProjectId>,
    entries: Vec<ReorderGroupEntry>,
) -> Result<CommandGroupCatalog, MtError> {
    state.service.reorder(scope, project_id, entries).await
}

/// Inicia a execução de um grupo de comandos com idempotência estrita (`command_group_run`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_run(
    state: State<'_, AppCommandGroupState>,
    request: GroupRunRequest,
) -> Result<GroupRun, MtError> {
    let run = state.service.run(request).await?;
    crate::search::adapters::sync_projections_and_invalidate(
        &state.db,
        &state.app,
        "command_group_run_started",
    );
    Ok(run)
}

/// Cancela graciosamente uma execução de grupo ou um item individual (`command_group_run_cancel`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_run_cancel(
    state: State<'_, AppCommandGroupState>,
    run_id: GroupRunId,
    item_id: Option<GroupRunItemId>,
) -> Result<GroupRun, MtError> {
    state.service.cancel_run(run_id, item_id).await
}

/// Lista o histórico paginado de execuções de grupos de um projeto (`command_group_run_list`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_run_list(
    state: State<'_, AppCommandGroupState>,
    project_id: ProjectId,
    cursor: Option<String>,
    limit: u32,
    active_only: Option<bool>,
) -> Result<GroupRunListPage, MtError> {
    state
        .service
        .list_runs(project_id, cursor, limit, active_only)
        .await
}

/// Obtém o detalhe completo de uma execução de grupo e seus itens (`command_group_run_get`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_run_get(
    state: State<'_, AppCommandGroupState>,
    run_id: GroupRunId,
) -> Result<GroupRun, MtError> {
    state.service.get_run(run_id).await
}

/// Anexa um stream `Channel<GroupOutputFrame>` (`Replay + Live`) à saída de um item (`command_group_run_item_attach`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_run_item_attach(
    state: State<'_, AppCommandGroupState>,
    run_id: GroupRunId,
    item_id: GroupRunItemId,
    after_seq: Option<u64>,
    on_frame: Channel<GroupOutputFrame>,
) -> Result<OutputAttachReceipt, MtError> {
    let subscriber = ChannelOutputSubscriber::new(on_frame);
    state
        .service
        .attach_item_output(run_id, item_id, after_seq, subscriber)
        .await
}

/// Remove uma assinatura ativa de saída sem encerrar o item (`command_group_run_item_detach`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_run_item_detach(
    state: State<'_, AppCommandGroupState>,
    subscription_id: OutputSubscriptionId,
) -> Result<(), MtError> {
    state.service.detach_item_output(subscription_id).await
}

/// Gera sugestão assistida de grupo via `MaxAiManager` e memória contextual (`command_group_suggest`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_suggest(
    state: State<'_, AppCommandGroupState>,
    request: GroupSuggestRequest,
) -> Result<GroupSuggestResponse, MtError> {
    state.service.suggest(request).await
}

/// Lista comandos recorrentes sanitizados da memória contextual do projeto (`command_group_recurring_commands`).
#[tauri::command]
#[specta::specta]
pub async fn command_group_recurring_commands(
    state: State<'_, AppCommandGroupState>,
    project_id: ProjectId,
    limit: u32,
) -> Result<RecurringCommandsResponse, MtError> {
    state.service.recurring_commands(project_id, limit).await
}
