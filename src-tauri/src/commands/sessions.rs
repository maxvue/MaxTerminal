//! Comandos IPC Tauri para Sessões de Aba, Histórico, Scrollback e Busca FTS5 (Fase 07).

use mt_core::error::MtError;
use mt_core::ids::{SessionId, TabId, TabSessionId};
use mt_core::workspace::{
    CursorPage, ScrollbackChunk, SessionDetail, SessionSearchHit, SessionSummary, TabView,
};
use tauri::State;

use crate::workspace::manager::WorkspaceManager;

/// Arquiva a sessão atual da aba e inicia uma nova sessão limpa (`session_new`).
#[tauri::command]
#[specta::specta]
pub async fn session_new(
    state: State<'_, WorkspaceManager>,
    tab_id: TabId,
    request_id: String,
) -> Result<TabView, MtError> {
    state.session_new(tab_id, request_id).await
}

/// Lista paginadamente as sessões de uma aba (`session_list`).
#[tauri::command]
#[specta::specta]
pub async fn session_list(
    state: State<'_, WorkspaceManager>,
    tab_id: TabId,
    cursor: Option<String>,
    limit: u32,
) -> Result<CursorPage<SessionSummary>, MtError> {
    state.session_list(tab_id, cursor, limit).await
}

/// Obtém o detalhe completo de uma sessão de aba (`session_get`).
#[tauri::command]
#[specta::specta]
pub async fn session_get(
    state: State<'_, WorkspaceManager>,
    tab_session_id: TabSessionId,
) -> Result<SessionDetail, MtError> {
    state.session_get(tab_session_id).await
}

/// Lista paginadamente os chunks de scrollback de um painel na sessão (`session_scrollback_get`).
#[tauri::command]
#[specta::specta]
pub async fn session_scrollback_get(
    state: State<'_, WorkspaceManager>,
    tab_session_id: TabSessionId,
    session_id: SessionId,
    cursor: Option<String>,
    limit: u32,
) -> Result<CursorPage<ScrollbackChunk>, MtError> {
    state
        .session_scrollback_get(tab_session_id, session_id, cursor, limit)
        .await
}

/// Realiza busca textual FTS5 em comandos e scrollback das sessões de uma aba (`session_search`).
#[tauri::command]
#[specta::specta]
pub async fn session_search(
    state: State<'_, WorkspaceManager>,
    tab_id: TabId,
    query: String,
    limit: u32,
) -> Result<Vec<SessionSearchHit>, MtError> {
    state.session_search(tab_id, query, limit).await
}
