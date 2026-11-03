//! Comandos IPC Tauri para inspeção Git e gerenciamento de Worktrees (Fase 08).

use std::path::PathBuf;
use std::sync::Arc;

use mt_core::MtResult;
use mt_git::{
    GitContext, GitService, RefreshReason, WorktreeCreateRequest, WorktreeEntry, WorktreeManager,
};
use tauri::State;

/// Retorna o contexto Git para `path` (usando cache por `RepoKey` quando disponível).
#[tauri::command]
#[specta::specta]
pub async fn git_status_for_path(
    git: State<'_, GitService>,
    path: PathBuf,
) -> MtResult<GitContext> {
    Ok(git.status_for_path(&path).await?)
}

/// Força a reavaliação imediata do status Git para `path`.
#[tauri::command]
#[specta::specta]
pub async fn git_status_refresh(git: State<'_, GitService>, path: PathBuf) -> MtResult<GitContext> {
    Ok(git.refresh_path(&path, RefreshReason::Manual).await?)
}

/// Lista todas as worktrees associadas ao repositório de `project_root`.
#[tauri::command]
#[specta::specta]
pub async fn git_worktree_list(
    worktrees: State<'_, Arc<dyn WorktreeManager>>,
    project_root: PathBuf,
) -> MtResult<Vec<WorktreeEntry>> {
    Ok(worktrees.list(&project_root).await?)
}

/// Cria uma nova worktree gerenciada em `.worktrees/` ou `.max-code-worktrees/`.
#[tauri::command]
#[specta::specta]
pub async fn git_worktree_create(
    git: State<'_, GitService>,
    worktrees: State<'_, Arc<dyn WorktreeManager>>,
    req: WorktreeCreateRequest,
) -> MtResult<WorktreeEntry> {
    let project_root = req.project_root.clone();
    let entry = worktrees.create(req).await?;
    let _ = git
        .refresh_path(&project_root, RefreshReason::WorktreeMutated)
        .await;
    Ok(entry)
}

/// Remove uma worktree gerenciada, validando uso ativo e alterações locais.
#[tauri::command]
#[specta::specta]
pub async fn git_worktree_remove(
    git: State<'_, GitService>,
    worktrees: State<'_, Arc<dyn WorktreeManager>>,
    worktree_path: PathBuf,
    force: bool,
) -> MtResult<()> {
    worktrees.remove(&worktree_path, force).await?;
    if let Some(parent) = worktree_path.parent().and_then(|p| p.parent()) {
        let _ = git
            .refresh_path(parent, RefreshReason::WorktreeMutated)
            .await;
    }
    Ok(())
}
