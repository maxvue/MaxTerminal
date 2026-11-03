//! Comandos IPC Tauri para integração de shell e histórico de blocos de comando (`shell_*`, `command_block_*`).

use std::sync::Arc;

use mt_core::{
    MtError,
    ids::{CommandBlockId, PtyId},
    shell::{
        CommandBlockDto, CommandBlockHit, CommandBlockListQuery, CommandBlockSearchQuery,
        ErrorContext, IntegrationInfo, Page,
    },
};
use mt_shell::ShellService;
use tauri::State;

/// Retorna diagnósticos da integração de shell ativa para um PTY (`shell_integration_info`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn shell_integration_info(
    state: State<'_, Arc<ShellService>>,
    pty_id: PtyId,
) -> Result<IntegrationInfo, MtError> {
    state.integration_info(pty_id)
}

/// Lista blocos de comando paginados com filtros por sessão, aba, PTY ou status (`command_block_list`).
#[tauri::command]
#[specta::specta]
pub async fn command_block_list(
    state: State<'_, Arc<ShellService>>,
    query: CommandBlockListQuery,
) -> Result<Page<CommandBlockDto>, MtError> {
    state.command_block_list(query).await
}

/// Obtém os detalhes completos de um bloco de comando pelo ID (`command_block_get`).
#[tauri::command]
#[specta::specta]
pub async fn command_block_get(
    state: State<'_, Arc<ShellService>>,
    block_id: CommandBlockId,
) -> Result<CommandBlockDto, MtError> {
    state.command_block_get(block_id).await
}

/// Constrói o contexto de erro sanitizado e truncado para inspeção no `MaxCode` (`command_block_error_context`).
#[tauri::command]
#[specta::specta]
pub async fn command_block_error_context(
    state: State<'_, Arc<ShellService>>,
    block_id: CommandBlockId,
) -> Result<ErrorContext, MtError> {
    state.command_block_error_context(block_id).await
}

/// Executa busca textual FTS5 sobre o histórico de blocos de comando (`command_block_search`).
#[tauri::command]
#[specta::specta]
pub async fn command_block_search(
    state: State<'_, Arc<ShellService>>,
    query: CommandBlockSearchQuery,
) -> Result<Page<CommandBlockHit>, MtError> {
    state.command_block_search(query).await
}
