//! Adaptadores de sincronização incremental de projeções `search_metadata` e invalidação de snapshots.

use mt_core::ids::{ProjectId, SessionId, TemplateId};
use mt_db::Db;
use mt_db::error::DbResult;
use mt_db::search::projection::{
    backfill_all_projections, sync_project_if_ready, sync_session_if_ready, sync_template_if_ready,
};

use crate::events::search::SearchInvalidatedTauriEvent;
use crate::search::AppSearchState;
use tauri_specta::Event;

/// Sincroniza na transação/conexão do banco um projeto alterado.
pub async fn sync_project_projection(db: &Db, project_id: ProjectId) -> DbResult<()> {
    db.write_sized("search_sync_project", 512, move |tx| {
        sync_project_if_ready(tx, project_id)
    })
    .await
}

/// Sincroniza na transação/conexão do banco um template alterado.
pub async fn sync_template_projection(db: &Db, template_id: TemplateId) -> DbResult<()> {
    db.write_sized("search_sync_template", 512, move |tx| {
        sync_template_if_ready(tx, template_id)
    })
    .await
}

/// Sincroniza na transação/conexão do banco uma sessão alterada.
pub async fn sync_session_projection(db: &Db, session_id: SessionId) -> DbResult<()> {
    db.write_sized("search_sync_session", 512, move |tx| {
        sync_session_if_ready(tx, session_id)
    })
    .await
}

/// Executa o backfill completo de projeções (grupos, runs, issues, comentários, projetos, templates e sessões).
pub async fn refresh_all_projections(db: &Db) -> DbResult<u64> {
    db.write_sized("search_refresh_all_projections", 4096, |tx| {
        backfill_all_projections(tx)
    })
    .await
}

/// Invalida todos os snapshots do motor de busca e emite `search://invalidated` para o frontend.
pub fn invalidate_and_notify(state: &AppSearchState, reason: &'static str) {
    let invalidated = state.engine.invalidate_all(reason);
    if let Some(ref app) = state.app_handle {
        let ids: Vec<String> = invalidated.into_iter().map(|(_, id)| id).collect();
        let _ = SearchInvalidatedTauriEvent(mt_db::search::model::SearchInvalidatedEvent {
            reason: reason.to_string(),
            invalidated_snapshot_ids: ids,
        })
        .emit(app);
    }
}

/// Atualiza em background as projeções FTS5 (`search_metadata`) e invalida o cache do `AppSearchState`.
pub fn sync_projections_and_invalidate(db: &Db, app: &tauri::AppHandle, reason: &'static str) {
    use tauri::Manager;
    let db_clone = db.clone();
    let app_clone = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = refresh_all_projections(&db_clone).await;
        if let Some(search_state) = app_clone.try_state::<AppSearchState>() {
            invalidate_and_notify(&search_state, reason);
        }
    });
}
