//! Comandos IPC Tauri para gerenciamento de Templates de Aba (Fase 07).

use mt_core::error::MtError;
use mt_core::ids::{ProjectId, TemplateId};
use mt_core::workspace::{TabTemplate, TabTemplateInput, TemplateConnectionInput};
use tauri::State;

use crate::workspace::manager::WorkspaceManager;

/// Lista todos os templates configurados em um projeto (`template_list`).
#[tauri::command]
#[specta::specta]
pub async fn template_list(
    state: State<'_, WorkspaceManager>,
    project_id: ProjectId,
) -> Result<Vec<TabTemplate>, MtError> {
    state.template_list(project_id).await
}

/// Obtém um template específico pelo identificador (`template_get`).
#[tauri::command]
#[specta::specta]
pub async fn template_get(
    state: State<'_, WorkspaceManager>,
    template_id: TemplateId,
) -> Result<TabTemplate, MtError> {
    state.template_get(template_id).await
}

/// Cria um novo template de aba no projeto (`template_create`).
#[tauri::command]
#[specta::specta]
pub async fn template_create(
    state: State<'_, WorkspaceManager>,
    project_id: ProjectId,
    input: TabTemplateInput,
) -> Result<TabTemplate, MtError> {
    state.template_create(project_id, input).await
}

/// Atualiza um template existente validando `expected_revision` (`template_update`).
#[tauri::command]
#[specta::specta]
pub async fn template_update(
    state: State<'_, WorkspaceManager>,
    template_id: TemplateId,
    expected_revision: i64,
    input: TabTemplateInput,
) -> Result<TabTemplate, MtError> {
    state
        .template_update(template_id, expected_revision, input)
        .await
}

/// Remove um template não-padrão de um projeto (`template_delete`).
#[tauri::command]
#[specta::specta]
pub async fn template_delete(
    state: State<'_, WorkspaceManager>,
    template_id: TemplateId,
    expected_revision: i64,
) -> Result<(), MtError> {
    state.template_delete(template_id, expected_revision).await
}

/// Define um template como padrão do projeto (`template_set_default`).
#[tauri::command]
#[specta::specta]
pub async fn template_set_default(
    state: State<'_, WorkspaceManager>,
    template_id: TemplateId,
) -> Result<Vec<TabTemplate>, MtError> {
    state.template_set_default(template_id).await
}

/// Testa uma conexão SSH e a pasta remota antes de salvar o template.
#[tauri::command]
#[specta::specta]
pub async fn template_test_connection(
    state: State<'_, WorkspaceManager>,
    project_id: ProjectId,
    template_id: Option<TemplateId>,
    connection: TemplateConnectionInput,
    cwd: Option<String>,
) -> Result<String, MtError> {
    state
        .template_test_connection(project_id, template_id, connection, cwd)
        .await
}
