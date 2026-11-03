//! Comandos IPC Tauri para máquina de estados das abas e painéis (`tab_state_*`, `tab_*`).

use std::sync::Arc;

use mt_core::{
    MtError,
    ids::{PaneId, TabId},
    shell::{ContinuousPolicy, FocusSnapshot, SeenRequest, TabStateSnapshot},
};
use mt_shell::ShellService;
use tauri::State;

/// Lista os snapshots de estado de todas as abas registradas (`tab_state_list`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn tab_state_list(
    state: State<'_, Arc<ShellService>>,
) -> Result<Vec<TabStateSnapshot>, MtError> {
    Ok(state.tab_state_list())
}

/// Obtém o snapshot de estado atual de uma aba pelo seu `TabId` (`tab_state_get`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn tab_state_get(
    state: State<'_, Arc<ShellService>>,
    tab_id: TabId,
) -> Result<TabStateSnapshot, MtError> {
    state.tab_state_get(tab_id)
}

/// Marca conclusões não vistas (`CompletedUnseen` / `ErrorUnseen`) e alertas de atenção como vistos (`tab_mark_seen`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn tab_mark_seen(
    state: State<'_, Arc<ShellService>>,
    request: SeenRequest,
) -> Result<TabStateSnapshot, MtError> {
    state.tab_mark_seen(&request)
}

/// Atualiza o foco global da janela e da aba/painel ativo, retornando as abas cujos estados mudaram (`tab_set_focus`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn tab_set_focus(
    state: State<'_, Arc<ShellService>>,
    focus: FocusSnapshot,
) -> Result<Vec<TabStateSnapshot>, MtError> {
    Ok(state.tab_set_focus(focus))
}

/// Define a política de execução contínua (`ContinuousPolicy`) para uma aba ou painel específico (`tab_set_continuous`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub async fn tab_set_continuous(
    state: State<'_, Arc<ShellService>>,
    tab_id: TabId,
    pane_id: Option<PaneId>,
    policy: ContinuousPolicy,
) -> Result<TabStateSnapshot, MtError> {
    state.tab_set_continuous(tab_id, pane_id, policy)
}
