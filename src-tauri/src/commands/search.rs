//! Comandos IPC Tauri do domínio `search_*` (Fase 17).

use mt_core::MtError;
use mt_db::search::{
    CancelOutcome, RebuildJobHandle, SearchIndexProgressEvent, SearchIndexStat,
    SearchInvalidatedEvent, SearchOwner, SearchRequest, SearchResponse, SearchSourceKind,
};
use tauri::State;
use tauri_specta::Event;

use crate::events::search::{SearchIndexProgressTauriEvent, SearchInvalidatedTauriEvent};
use crate::search::AppSearchState;

/// Executa uma busca global multi-fonte FTS5 ou pagina uma seção sobre um snapshot existente.
#[tauri::command]
#[specta::specta]
pub async fn search_query(
    state: State<'_, AppSearchState>,
    request: SearchRequest,
) -> Result<SearchResponse, MtError> {
    let owner = SearchOwner::new("main");
    state
        .engine
        .search(owner, request)
        .await
        .map_err(MtError::from)
}

/// Cancela cooperativamente uma consulta de busca em andamento para a janela principal.
#[tauri::command]
#[specta::specta]
pub async fn search_cancel(
    state: State<'_, AppSearchState>,
    request_id: String,
) -> Result<CancelOutcome, MtError> {
    let owner = SearchOwner::new("main");
    Ok(state.engine.cancel(&owner, &request_id))
}

/// Libera explicitamente um snapshot materializado associado à janela principal (ex.: ao fechar a paleta).
#[tauri::command]
#[specta::specta]
pub async fn search_release(
    state: State<'_, AppSearchState>,
    snapshot_id: String,
) -> Result<(), MtError> {
    let owner = SearchOwner::new("main");
    state.engine.release(&owner, &snapshot_id);
    Ok(())
}

/// Retorna a contagem de documentos indexados e estado de prontidão de cada uma das 9 fontes.
#[tauri::command]
#[specta::specta]
pub async fn search_index_stats(
    state: State<'_, AppSearchState>,
) -> Result<Vec<SearchIndexStat>, MtError> {
    state.engine.index_stats().await.map_err(MtError::from)
}

/// Dispara a reconstrução dos índices FTS5 das fontes informadas (ou todas quando `None`/vazio),
/// emitindo progresso via `search://index-progress` e invalidando snapshots anteriores.
#[tauri::command]
#[specta::specta]
pub async fn search_index_rebuild(
    state: State<'_, AppSearchState>,
    sources: Option<Vec<SearchSourceKind>>,
) -> Result<RebuildJobHandle, MtError> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<SearchIndexProgressEvent>();
    let app_opt = state.app_handle.clone();
    if let Some(app) = app_opt.clone() {
        tokio::spawn(async move {
            while let Some(ev) = rx.recv().await {
                let _ = SearchIndexProgressTauriEvent(ev).emit(&app);
            }
        });
    }

    let handle = state
        .engine
        .rebuild_indexes(sources.unwrap_or_default(), Some(tx))
        .await
        .map_err(MtError::from)?;

    if let Some(app) = app_opt {
        let _ = SearchInvalidatedTauriEvent(SearchInvalidatedEvent {
            reason: String::from("index_rebuilt"),
            invalidated_snapshot_ids: Vec::new(),
        })
        .emit(&app);
    }

    Ok(handle)
}
