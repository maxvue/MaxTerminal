//! Os 20 comandos IPC Tauri (`github_*`) para a Aba Fixa de Issues do GitHub (Fase 16).

use mt_core::MtError;
use mt_core::ids::ProjectId;
use mt_github::model::{
    BugCyclePreview, BugCycleRunState, CanonicalLabelSpec, CreateIssueInput,
    CreateIssueWorktreeRequest, GithubAuthStatus, IssueCounts, IssueDetail, IssueFilter,
    IssueListPage, IssueStateFilter, IssueSummary, IssueWorktreeResolution, LabelEnsureReport,
    LabelUpdatePatch, OpenIssueTabReceipt, OpenIssueTabRequest, RepoLabel, RepoResolution,
    SendIssueToMaxCodeReceipt, SendIssueToMaxCodeRequest, StartBugCycleRequest,
};
use tauri::State;

use crate::github::AppGithubState;

/// 1. Resolve se um projeto é um repositório GitHub.com (`github_repo_resolve`).
#[tauri::command]
#[specta::specta]
pub async fn github_repo_resolve(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
) -> Result<RepoResolution, MtError> {
    state
        .service
        .resolve_project_repo(project_id)
        .await
        .map_err(MtError::from)
}

/// 2. Retorna o status consolidado de autenticação GitHub (`github_auth_status`).
#[tauri::command]
#[specta::specta]
pub async fn github_auth_status(
    state: State<'_, AppGithubState>,
    force_probe: Option<bool>,
) -> Result<GithubAuthStatus, MtError> {
    state
        .service
        .auth_status(force_probe.unwrap_or(false))
        .await
        .map_err(MtError::from)
}

/// 3. Configura um Personal Access Token para o fallback REST (`github_auth_set_token`).
#[tauri::command]
#[specta::specta]
pub async fn github_auth_set_token(
    state: State<'_, AppGithubState>,
    token: String,
) -> Result<GithubAuthStatus, MtError> {
    state
        .service
        .set_auth_token(&token)
        .await
        .map_err(MtError::from)
}

/// 4. Remove o Personal Access Token configurado (`github_auth_clear_token`).
#[tauri::command]
#[specta::specta]
pub async fn github_auth_clear_token(
    state: State<'_, AppGithubState>,
) -> Result<GithubAuthStatus, MtError> {
    state
        .service
        .clear_auth_token()
        .await
        .map_err(MtError::from)
}

/// 5. Lista issues de um projeto aplicando `IssueFilter` (`github_issues_list`).
#[tauri::command]
#[specta::specta]
pub async fn github_issues_list(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
    filter: Option<IssueFilter>,
) -> Result<IssueListPage, MtError> {
    state
        .service
        .list_issues(project_id, filter.unwrap_or_default())
        .await
        .map_err(MtError::from)
}

/// 6. Obtém o detalhe completo de uma issue, comentários e `## PLANO DE IMPLEMENTAÇÃO` (`github_issue_detail`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_detail(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
    issue_number: u64,
    force_refresh: Option<bool>,
) -> Result<IssueDetail, MtError> {
    state
        .service
        .issue_detail(project_id, issue_number, force_refresh.unwrap_or(false))
        .await
        .map_err(MtError::from)
}

/// 7. Obtém os contadores agregados de issues do projeto para o badge 🟣 (`github_issue_counts`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_counts(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
) -> Result<IssueCounts, MtError> {
    state
        .service
        .issue_counts(project_id)
        .await
        .map_err(MtError::from)
}

/// 8. Força sincronização imediata de issues e labels com o GitHub (`github_refresh`).
#[tauri::command]
#[specta::specta]
pub async fn github_refresh(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
    state_filter: Option<IssueStateFilter>,
    force_ignore_etag: Option<bool>,
) -> Result<IssueListPage, MtError> {
    let page = state
        .service
        .refresh_issues(project_id, state_filter, force_ignore_etag.unwrap_or(false))
        .await
        .map_err(MtError::from)?;
    crate::search::adapters::sync_projections_and_invalidate(
        &state.db,
        &state.app,
        "github_issues_refreshed",
    );
    Ok(page)
}

/// 9. Retorna o catálogo das 24 labels canônicas do workflow `MaxCode` (`github_labels_catalog`).
#[tauri::command]
#[specta::specta]
pub async fn github_labels_catalog(
    state: State<'_, AppGithubState>,
) -> Result<Vec<CanonicalLabelSpec>, MtError> {
    Ok(state.service.labels_catalog())
}

/// 10. Lista as labels existentes no repositório GitHub do projeto (`github_repo_labels`).
#[tauri::command]
#[specta::specta]
pub async fn github_repo_labels(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
    force_refresh: Option<bool>,
) -> Result<Vec<RepoLabel>, MtError> {
    state
        .service
        .repo_labels(project_id, force_refresh.unwrap_or(false))
        .await
        .map_err(MtError::from)
}

/// 11. Inspeciona ou cria no repositório as 24 labels canônicas ausentes (`github_labels_ensure`).
#[tauri::command]
#[specta::specta]
pub async fn github_labels_ensure(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
    dry_run: Option<bool>,
) -> Result<LabelEnsureReport, MtError> {
    state
        .service
        .ensure_labels(project_id, dry_run.unwrap_or(false))
        .await
        .map_err(MtError::from)
}

/// 12. Cria uma nova issue no repositório do projeto (`github_issue_create`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_create(
    state: State<'_, AppGithubState>,
    input: CreateIssueInput,
) -> Result<IssueSummary, MtError> {
    let created = state
        .service
        .create_issue(input)
        .await
        .map_err(MtError::from)?;
    crate::search::adapters::sync_projections_and_invalidate(
        &state.db,
        &state.app,
        "github_issue_created",
    );
    Ok(created)
}

/// 13. Atualiza atomicamente as labels de uma issue seguindo as regras MaxCode (`github_issue_labels_update`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_labels_update(
    state: State<'_, AppGithubState>,
    patch: LabelUpdatePatch,
) -> Result<IssueSummary, MtError> {
    let updated = state
        .service
        .update_issue_labels(patch)
        .await
        .map_err(MtError::from)?;
    crate::search::adapters::sync_projections_and_invalidate(
        &state.db,
        &state.app,
        "github_issue_labels_updated",
    );
    Ok(updated)
}

/// 14. Envia uma issue com 1 clique para o `MaxCode` (`Abrir`, `BUG PLAN` ou `BUG FIX`) (`github_issue_send_maxcode`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_send_maxcode(
    state: State<'_, AppGithubState>,
    request: SendIssueToMaxCodeRequest,
) -> Result<SendIssueToMaxCodeReceipt, MtError> {
    state
        .service
        .send_issue_to_maxcode(request)
        .await
        .map_err(MtError::from)
}

/// 15. Resolve se já existe worktree ou branch local associada à issue (`github_issue_worktree_resolve`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_worktree_resolve(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
    issue_number: u64,
) -> Result<IssueWorktreeResolution, MtError> {
    state
        .service
        .resolve_issue_worktree(project_id, issue_number)
        .await
        .map_err(MtError::from)
}

/// 16. Cria explicitamente uma worktree dedicada para a issue (`github_issue_worktree_create`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_worktree_create(
    state: State<'_, AppGithubState>,
    request: CreateIssueWorktreeRequest,
) -> Result<IssueWorktreeResolution, MtError> {
    state
        .service
        .create_issue_worktree(request)
        .await
        .map_err(MtError::from)
}

/// 17. Abre uma aba de Terminal ou Agente CLI posicionada na worktree/branch da issue (`github_issue_open_tab`).
#[tauri::command]
#[specta::specta]
pub async fn github_issue_open_tab(
    state: State<'_, AppGithubState>,
    request: OpenIssueTabRequest,
) -> Result<OpenIssueTabReceipt, MtError> {
    state
        .service
        .open_issue_tab(request)
        .await
        .map_err(MtError::from)
}

/// 18. Calcula a prévia determinística do ciclo em lote **"Corrigir todos os Bugs"** (`github_bug_cycle_preview`).
#[tauri::command]
#[specta::specta]
pub async fn github_bug_cycle_preview(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
    include_replan_and_rejected: Option<bool>,
) -> Result<BugCyclePreview, MtError> {
    state
        .service
        .bug_cycle_preview(project_id, include_replan_and_rejected.unwrap_or(true))
        .await
        .map_err(MtError::from)
}

/// 19. Inicia o ciclo automatizado em lote **"Corrigir todos os Bugs"** (`github_bug_cycle_start`).
#[tauri::command]
#[specta::specta]
pub async fn github_bug_cycle_start(
    state: State<'_, AppGithubState>,
    request: StartBugCycleRequest,
) -> Result<BugCycleRunState, MtError> {
    state
        .service
        .bug_cycle_start(request)
        .await
        .map_err(MtError::from)
}

/// 20. Consulta o estado atual do ciclo automatizado de bugs do projeto (`github_bug_cycle_status`).
#[tauri::command]
#[specta::specta]
pub async fn github_bug_cycle_status(
    state: State<'_, AppGithubState>,
    project_id: ProjectId,
) -> Result<Option<BugCycleRunState>, MtError> {
    state
        .service
        .bug_cycle_status(project_id)
        .await
        .map_err(MtError::from)
}
