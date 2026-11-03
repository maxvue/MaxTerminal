//! Ações por issue: envio 1-clique para o `MaxCode` e abertura/reutilização de aba no `MaxTerminal`.

use std::sync::Arc;

use crate::cache::GithubCacheRepo;
use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::model::{
    CreateIssueWorktreeRequest, GithubRepoIdentity, IssueTabDirectoryStrategy,
    IssueWorktreeResolution, OpenIssueTabReceipt, OpenIssueTabRequest, SendIssueToMaxCodeReceipt,
    SendIssueToMaxCodeRequest,
};
use crate::ports::{GitRepoPort, MaxCodeBridgePort, ProjectLookupPort, WorkspaceTabPort};

/// Executor das ações de integração por issue (MaxCode e abertura de abas com worktree).
pub struct IssueActionsCoordinator {
    cache: GithubCacheRepo,
    projects: Arc<dyn ProjectLookupPort>,
    git: Arc<dyn GitRepoPort>,
    workspace: Arc<dyn WorkspaceTabPort>,
    bridge: Arc<dyn MaxCodeBridgePort>,
}

impl IssueActionsCoordinator {
    /// Cria um novo `IssueActionsCoordinator`.
    #[must_use]
    pub fn new(
        cache: GithubCacheRepo,
        projects: Arc<dyn ProjectLookupPort>,
        git: Arc<dyn GitRepoPort>,
        workspace: Arc<dyn WorkspaceTabPort>,
        bridge: Arc<dyn MaxCodeBridgePort>,
    ) -> Self {
        Self {
            cache,
            projects,
            git,
            workspace,
            bridge,
        }
    }

    /// Envia uma issue ao `MaxCode` com 1 clique (`Open`, `BugPlan` ou `BugFix`).
    pub async fn send_to_maxcode(
        &self,
        repo: &GithubRepoIdentity,
        req: &SendIssueToMaxCodeRequest,
    ) -> GithubResult<SendIssueToMaxCodeReceipt> {
        let project_path = self.projects.project_root_path(req.project_id).await?;
        let detail = self
            .cache
            .get_issue_detail(&repo.repo_key, req.issue_number, false)
            .await?
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::NotFound,
                    format!("Issue #{} não encontrada no cache local.", req.issue_number),
                )
            })?;

        self.bridge
            .send_issue_action(
                req.project_id,
                &project_path.to_string_lossy(),
                &repo.repo_key,
                req.issue_number,
                &detail.summary.title,
                &detail.summary.html_url,
                detail.summary.has_plan,
                req.action,
            )
            .await
    }

    /// Resolve a worktree ou branch existente para uma issue sem criar nada em disco.
    pub async fn resolve_worktree(
        &self,
        repo: &GithubRepoIdentity,
        project_id: mt_core::ids::ProjectId,
        issue_number: u64,
    ) -> GithubResult<IssueWorktreeResolution> {
        let project_path = self.projects.project_root_path(project_id).await?;
        let issue_title = self
            .cache
            .get_issue_detail(&repo.repo_key, issue_number, false)
            .await?
            .map_or_else(|| format!("issue-{issue_number}"), |d| d.summary.title);

        self.git
            .resolve_issue_worktree(
                &project_path,
                issue_number,
                &issue_title,
                repo.default_branch.as_deref(),
            )
            .await
    }

    /// Cria ou reutiliza uma worktree dedicada para a issue.
    pub async fn create_worktree(
        &self,
        repo: &GithubRepoIdentity,
        req: &CreateIssueWorktreeRequest,
    ) -> GithubResult<IssueWorktreeResolution> {
        let project_path = self.projects.project_root_path(req.project_id).await?;
        let issue_title = self
            .cache
            .get_issue_detail(&repo.repo_key, req.issue_number, false)
            .await?
            .map_or_else(
                || format!("issue-{}", req.issue_number),
                |d| d.summary.title,
            );

        let (path, branch) = self
            .git
            .ensure_issue_worktree(
                &project_path,
                req,
                &issue_title,
                repo.default_branch.as_deref(),
            )
            .await?;

        Ok(IssueWorktreeResolution::ExistingWorktree { path, branch })
    }

    /// Abre uma aba de Terminal ou Agente CLI para a issue (reutilizando aba já aberta na mesma worktree quando existir).
    pub async fn open_issue_tab(
        &self,
        repo: &GithubRepoIdentity,
        req: &OpenIssueTabRequest,
    ) -> GithubResult<OpenIssueTabReceipt> {
        let project_path = self.projects.project_root_path(req.project_id).await?;
        let detail = self
            .cache
            .get_issue_detail(&repo.repo_key, req.issue_number, false)
            .await?;

        let issue_title = detail.as_ref().map_or_else(
            || format!("Issue #{}", req.issue_number),
            |d| d.summary.title.clone(),
        );
        let issue_url = detail.as_ref().map_or_else(
            || format!("{}/issues/{}", repo.html_url, req.issue_number),
            |d| d.summary.html_url.clone(),
        );

        let (resolved_cwd, resolved_branch) = match &req.directory {
            IssueTabDirectoryStrategy::UseProjectRoot => (
                project_path.to_string_lossy().into_owned(),
                repo.default_branch.clone(),
            ),
            IssueTabDirectoryStrategy::UseOrCreateIssueWorktree {
                branch_name,
                base_ref,
            } => {
                let wt_req = CreateIssueWorktreeRequest {
                    project_id: req.project_id,
                    issue_number: req.issue_number,
                    branch_name: branch_name.clone(),
                    base_ref: base_ref.clone(),
                };
                let (wt_path, branch) = self
                    .git
                    .ensure_issue_worktree(
                        &project_path,
                        &wt_req,
                        &issue_title,
                        repo.default_branch.as_deref(),
                    )
                    .await?;
                (wt_path, Some(branch))
            }
        };

        // Se já houver uma aba aberta nessa mesma worktree (quando não é a raiz do projeto), focamos a existente
        if resolved_cwd != project_path.to_string_lossy() {
            if let Some((tab_id, pane_id)) = self
                .workspace
                .find_tab_by_cwd(req.project_id, &resolved_cwd)
                .await?
            {
                self.workspace
                    .focus_tab(req.project_id, tab_id, pane_id)
                    .await?;
                return Ok(OpenIssueTabReceipt {
                    tab_id,
                    pane_id,
                    cwd: resolved_cwd,
                    branch: resolved_branch,
                    reused_existing_tab: true,
                });
            }
        }

        self.workspace
            .open_issue_tab(
                req,
                &resolved_cwd,
                resolved_branch.as_deref(),
                &issue_title,
                &issue_url,
            )
            .await
    }
}
