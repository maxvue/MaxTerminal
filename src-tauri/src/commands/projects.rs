//! Comandos IPC Tauri para gerenciamento de projetos, Pastas de Projetos e auto-descoberta (`project_*`).

use std::sync::Arc;

use mt_core::MtError;
use mt_core::ids::ProjectId;
use mt_projects::{
    DiscoveredFolder, NewProject, NewProjectRoot, Project, ProjectIdentity, ProjectPatch,
    ProjectRoot, ProjectRootId, ProjectRootPatch, ProjectService, ProjectSnapshot,
    RootImportReport,
};
use tauri::State;

/// Retorna uma visão consistente e versionada de todo o catálogo (`project_snapshot`).
#[tauri::command]
#[specta::specta]
pub async fn project_snapshot(
    state: State<'_, Arc<ProjectService>>,
) -> Result<ProjectSnapshot, MtError> {
    state.snapshot().await
}

/// Lista todos os projetos cadastrados ordenados por `sort_order` (`project_list`).
#[tauri::command]
#[specta::specta]
pub async fn project_list(state: State<'_, Arc<ProjectService>>) -> Result<Vec<Project>, MtError> {
    state.list().await
}

/// Cadastra um novo projeto individual (`project_add`).
#[tauri::command]
#[specta::specta]
pub async fn project_add(
    state: State<'_, Arc<ProjectService>>,
    input: NewProject,
) -> Result<Project, MtError> {
    state.add(input).await
}

/// Atualiza metadados de um projeto existente (`project_update`).
#[tauri::command]
#[specta::specta]
pub async fn project_update(
    state: State<'_, Arc<ProjectService>>,
    id: ProjectId,
    patch: ProjectPatch,
) -> Result<Project, MtError> {
    state.update(id, patch).await
}

/// Remove um projeto do cadastro após executar os hooks de remoção (`project_remove`).
#[tauri::command]
#[specta::specta]
pub async fn project_remove(
    state: State<'_, Arc<ProjectService>>,
    id: ProjectId,
) -> Result<(), MtError> {
    state.remove(id).await
}

/// Reordena a lista completa de projetos (`project_reorder`).
#[tauri::command]
#[specta::specta]
pub async fn project_reorder(
    state: State<'_, Arc<ProjectService>>,
    ids: Vec<ProjectId>,
) -> Result<(), MtError> {
    state.reorder(ids).await
}

/// Re-detecta a stack tecnológica de um projeto cadastrado (`project_redetect`).
#[tauri::command]
#[specta::specta]
pub async fn project_redetect(
    state: State<'_, Arc<ProjectService>>,
    id: ProjectId,
) -> Result<Project, MtError> {
    state.redetect(id).await
}

/// Calcula a identidade visual e stack de um caminho sem persistir no banco (`project_preview_identity`).
#[tauri::command]
#[specta::specta]
pub async fn project_preview_identity(
    state: State<'_, Arc<ProjectService>>,
    path: String,
) -> Result<ProjectIdentity, MtError> {
    state.preview_identity(path).await
}

/// Verifica a presença das pastas de todos os projetos no disco e retorna apenas os alterados (`project_refresh_missing`).
#[tauri::command]
#[specta::specta]
pub async fn project_refresh_missing(
    state: State<'_, Arc<ProjectService>>,
) -> Result<Vec<Project>, MtError> {
    state.refresh_missing().await
}

/// Lista todas as Pastas de Projetos cadastradas (`project_root_list`).
#[tauri::command]
#[specta::specta]
pub async fn project_root_list(
    state: State<'_, Arc<ProjectService>>,
) -> Result<Vec<ProjectRoot>, MtError> {
    state.list_roots().await
}

/// Cadastra uma nova Pasta de Projetos e executa sua varredura inicial (`project_root_add`).
#[tauri::command]
#[specta::specta]
pub async fn project_root_add(
    state: State<'_, Arc<ProjectService>>,
    input: NewProjectRoot,
) -> Result<RootImportReport, MtError> {
    state.add_root(input).await
}

/// Atualiza configurações de uma Pasta de Projetos (`project_root_update`).
#[tauri::command]
#[specta::specta]
pub async fn project_root_update(
    state: State<'_, Arc<ProjectService>>,
    id: ProjectRootId,
    patch: ProjectRootPatch,
) -> Result<ProjectRoot, MtError> {
    state.update_root(id, patch).await
}

/// Remove uma Pasta de Projetos preservando os projetos já importados (`project_root_remove`).
#[tauri::command]
#[specta::specta]
pub async fn project_root_remove(
    state: State<'_, Arc<ProjectService>>,
    id: ProjectRootId,
) -> Result<(), MtError> {
    state.remove_root(id).await
}

/// Executa uma nova varredura manual em uma Pasta de Projetos (`project_root_rescan`).
#[tauri::command]
#[specta::specta]
pub async fn project_root_rescan(
    state: State<'_, Arc<ProjectService>>,
    id: ProjectRootId,
) -> Result<RootImportReport, MtError> {
    state.rescan_root(id).await
}

/// Lista todas as subpastas descobertas aguardando aceite ou dispensa (`project_discovery_list`).
#[tauri::command]
#[specta::specta]
pub async fn project_discovery_list(
    state: State<'_, Arc<ProjectService>>,
) -> Result<Vec<DiscoveredFolder>, MtError> {
    state.list_discovered().await
}

/// Aceita um lote de subpastas descobertas convertendo-as em projetos (`project_discovery_accept`).
#[tauri::command]
#[specta::specta]
pub async fn project_discovery_accept(
    state: State<'_, Arc<ProjectService>>,
    paths: Vec<String>,
) -> Result<Vec<Project>, MtError> {
    state.accept_discovered(paths).await
}

/// Dispensa um lote de subpastas descobertas (`project_discovery_dismiss`).
#[tauri::command]
#[specta::specta]
pub async fn project_discovery_dismiss(
    state: State<'_, Arc<ProjectService>>,
    paths: Vec<String>,
) -> Result<(), MtError> {
    state.dismiss_discovered(paths).await
}
