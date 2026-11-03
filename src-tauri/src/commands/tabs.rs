//! Comandos IPC Tauri para gerenciamento de Abas (`tabs` — Fase 07).

use mt_core::error::MtError;
use mt_core::ids::{PaneId, ProjectId, TabId};
use mt_core::workspace::{TabOpenRequest, TabView};
use tauri::State;

use crate::workspace::manager::WorkspaceManager;

/// Lista as abas abertas de um projeto ordenadas por `position` (`tab_list`).
#[tauri::command]
#[specta::specta]
pub async fn tab_list(
    state: State<'_, WorkspaceManager>,
    project_id: ProjectId,
) -> Result<Vec<TabView>, MtError> {
    state.tab_list(project_id).await
}

/// Abre uma nova aba no projeto a partir do template informado ou do template padrão (`tab_open`).
#[tauri::command]
#[specta::specta]
pub async fn tab_open(
    state: State<'_, WorkspaceManager>,
    request: TabOpenRequest,
) -> Result<TabView, MtError> {
    state.tab_open(request).await
}

/// Fecha uma aba aberta, encerrando seus PTYs e arquivando sua sessão (`tab_close`).
#[tauri::command]
#[specta::specta]
pub async fn tab_close(
    state: State<'_, WorkspaceManager>,
    tab_id: TabId,
    request_id: String,
) -> Result<(), MtError> {
    state.tab_close(tab_id, request_id).await
}

/// Reordena as abas abertas de um projeto (`tab_reorder`).
#[tauri::command]
#[specta::specta]
pub async fn tab_reorder(
    state: State<'_, WorkspaceManager>,
    project_id: ProjectId,
    ordered_tab_ids: Vec<TabId>,
) -> Result<Vec<TabId>, MtError> {
    state.tab_reorder(project_id, ordered_tab_ids).await
}

/// Atualiza o foco de uma aba e de seu painel ativo (`tab_focus`).
#[tauri::command]
#[specta::specta]
pub async fn tab_focus(
    state: State<'_, WorkspaceManager>,
    tab_id: TabId,
    pane_id: Option<PaneId>,
) -> Result<TabView, MtError> {
    state.tab_focus(tab_id, pane_id).await
}
