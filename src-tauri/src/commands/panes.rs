//! Comandos IPC Tauri para Split Panes e Layout de Painéis (Fase 07).

use mt_core::error::MtError;
use mt_core::ids::{PaneId, TabId};
use mt_core::workspace::{LayoutSizePatch, PaneCloseOutcome, SplitDirection, TabView};
use tauri::State;

use crate::workspace::manager::WorkspaceManager;

/// Divide um painel existente horizontal ou verticalmente (`pane_split`).
#[tauri::command]
#[specta::specta]
pub async fn pane_split(
    state: State<'_, WorkspaceManager>,
    pane_id: PaneId,
    direction: SplitDirection,
    expected_layout_revision: i64,
    request_id: String,
) -> Result<TabView, MtError> {
    state
        .pane_split(pane_id, direction, expected_layout_revision, request_id)
        .await
}

/// Fecha um painel individual ou a aba caso seja o último painel (`pane_close`).
#[tauri::command]
#[specta::specta]
pub async fn pane_close(
    state: State<'_, WorkspaceManager>,
    pane_id: PaneId,
    request_id: String,
) -> Result<PaneCloseOutcome, MtError> {
    state.pane_close(pane_id, request_id).await
}

/// Reinicia o PTY de um painel que já encerrou (`pane_restart`).
#[tauri::command]
#[specta::specta]
pub async fn pane_restart(
    state: State<'_, WorkspaceManager>,
    pane_id: PaneId,
    cwd_fallback_to_project: bool,
    request_id: String,
) -> Result<TabView, MtError> {
    state
        .pane_restart(pane_id, cwd_fallback_to_project, request_id)
        .await
}

/// Atualiza as proporções dos splitters na árvore de layout de uma aba (`pane_layout_update`).
#[tauri::command]
#[specta::specta]
pub async fn pane_layout_update(
    state: State<'_, WorkspaceManager>,
    tab_id: TabId,
    expected_layout_revision: i64,
    patches: Vec<LayoutSizePatch>,
) -> Result<TabView, MtError> {
    state
        .pane_layout_update(tab_id, expected_layout_revision, patches)
        .await
}
