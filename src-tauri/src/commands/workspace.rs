//! Comandos IPC Tauri de hidratação do Workspace e disparo de comando inicial (Fase 07).

use mt_core::error::MtError;
use mt_core::ids::{PaneId, TabId};
use mt_core::workspace::{TabView, WorkspaceSnapshot};
use tauri::State;

use crate::workspace::manager::WorkspaceManager;

/// Retorna o snapshot atômico de hidratação do workspace (`workspace_snapshot`).
#[tauri::command]
#[specta::specta]
pub async fn workspace_snapshot(
    state: State<'_, WorkspaceManager>,
) -> Result<WorkspaceSnapshot, MtError> {
    state.workspace_snapshot().await
}

/// Dispara de forma idempotente o comando inicial de uma aba/painel (`workspace_initial_command_run`).
#[tauri::command]
#[specta::specta]
pub async fn workspace_initial_command_run(
    state: State<'_, WorkspaceManager>,
    tab_id: TabId,
    pane_id: PaneId,
    generation: i64,
    request_id: String,
) -> Result<TabView, MtError> {
    state
        .workspace_initial_command_run(tab_id, pane_id, generation, request_id)
        .await
}
