//! Eventos tipados do domínio `search://` (`search://index-progress` e `search://invalidated`).

use mt_db::search::model::{SearchIndexProgressEvent, SearchInvalidatedEvent};
use serde::{Deserialize, Serialize};
use tauri_specta::Event;

/// Evento emitido durante reconstrução assíncrona dos índices FTS5 (`search://index-progress`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "search://index-progress")]
pub struct SearchIndexProgressTauriEvent(pub SearchIndexProgressEvent);

/// Evento emitido quando snapshots materializados de busca são invalidados (`search://invalidated`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "search://invalidated")]
pub struct SearchInvalidatedTauriEvent(pub SearchInvalidatedEvent);
