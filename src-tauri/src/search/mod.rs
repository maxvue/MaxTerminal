//! Subsistema de Busca Global unificada (`AppSearchState` e adaptadores de sincronização).

pub mod adapters;

use std::sync::Arc;

use mt_db::Db;
use mt_db::search::{SearchConfig, SearchEngine};
use tauri::AppHandle;

/// Estado Tauri gerenciado para o motor de busca global (`SearchEngine`).
#[derive(Clone)]
pub struct AppSearchState {
    /// Instância compartilhada do motor de busca (`mt-db::search::SearchEngine`).
    pub engine: Arc<SearchEngine>,
    /// Handle opcional da aplicação Tauri para emissão de eventos `search://*`.
    pub app_handle: Option<AppHandle>,
}

impl AppSearchState {
    /// Inicializa o `AppSearchState` acoplado ao banco `Db` e opcionalmente ao `AppHandle`.
    #[must_use]
    pub fn new(db: Db, app_handle: Option<AppHandle>) -> Self {
        let engine = Arc::new(SearchEngine::new(db, SearchConfig::default()));
        Self { engine, app_handle }
    }
}
