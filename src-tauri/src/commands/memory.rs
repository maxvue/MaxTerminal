//! Comandos IPC Tauri da Memória Contextual de Comandos (Fase 13 — `memory_*`).

use mt_agents::detect::current_unix_ms;
use mt_agents::memory::{MemoryEntry, MemoryQuery};
use mt_core::error::MtError;
use mt_core::ids::ProjectId;
use tauri::State;
use tauri_specta::Event;

use crate::agents::AppAgentState;
use crate::events::agents::{MemoryUpdatedPayload, MemoryUpdatedTauriEvent};

/// Consulta sugestões de comandos recorrentes ordenadas por `pinned` e `score` (`memory_suggestions`).
#[tauri::command]
#[specta::specta]
pub async fn memory_suggestions(
    state: State<'_, AppAgentState>,
    query: MemoryQuery,
) -> Result<Vec<MemoryEntry>, MtError> {
    state
        .service
        .memory
        .suggestions(&query, current_unix_ms())
        .await
}

/// Lista as entradas da memória contextual de um projeto (`memory_list`).
#[tauri::command]
#[specta::specta]
pub async fn memory_list(
    state: State<'_, AppAgentState>,
    project_id: ProjectId,
    include_hidden: bool,
    limit: u16,
) -> Result<Vec<MemoryEntry>, MtError> {
    state
        .service
        .memory
        .list(project_id, include_hidden, limit, current_unix_ms())
        .await
}

/// Fixa ou desafixa um comando na memória contextual do projeto (`memory_pin`).
#[tauri::command]
#[specta::specta]
pub async fn memory_pin(
    state: State<'_, AppAgentState>,
    project_id: ProjectId,
    command: String,
    pinned: bool,
) -> Result<MemoryEntry, MtError> {
    let entry = state
        .service
        .memory
        .set_pinned(project_id, &command, pinned, current_unix_ms())
        .await?;
    let _ = MemoryUpdatedTauriEvent(MemoryUpdatedPayload { project_id }).emit(&state.app);
    Ok(entry)
}

/// Oculta ou reexibe um comando nas sugestões automáticas do projeto (`memory_hide`).
#[tauri::command]
#[specta::specta]
pub async fn memory_hide(
    state: State<'_, AppAgentState>,
    project_id: ProjectId,
    command: String,
    hidden: bool,
) -> Result<MemoryEntry, MtError> {
    let entry = state
        .service
        .memory
        .set_hidden(project_id, &command, hidden, current_unix_ms())
        .await?;
    let _ = MemoryUpdatedTauriEvent(MemoryUpdatedPayload { project_id }).emit(&state.app);
    Ok(entry)
}

/// Remove permanentemente um comando da memória contextual do projeto e registra tombstone (`memory_forget`).
#[tauri::command]
#[specta::specta]
pub async fn memory_forget(
    state: State<'_, AppAgentState>,
    project_id: ProjectId,
    command: String,
) -> Result<bool, MtError> {
    let removed = state
        .service
        .memory
        .forget(project_id, &command, current_unix_ms())
        .await?;
    if removed {
        let _ = MemoryUpdatedTauriEvent(MemoryUpdatedPayload { project_id }).emit(&state.app);
    }
    Ok(removed)
}

/// Reconstrói deterministicamente a memória contextual a partir de `command_blocks` preservando preferências (`memory_rebuild`).
#[tauri::command]
#[specta::specta]
pub async fn memory_rebuild(
    state: State<'_, AppAgentState>,
    project_id: Option<ProjectId>,
) -> Result<u32, MtError> {
    let count = state
        .service
        .memory
        .rebuild(project_id, current_unix_ms())
        .await?;
    if let Some(pid) = project_id {
        let _ = MemoryUpdatedTauriEvent(MemoryUpdatedPayload { project_id: pid }).emit(&state.app);
    }
    Ok(count)
}
