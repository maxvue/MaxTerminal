//! Adaptadores concretos entre `mt-github` e os serviços do crate principal (`maxterminal`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mt_agents::isolation::{AgentTabOpenRequest, WorktreeIsolation};
use mt_bridge::{
    BridgeService, BugsCycleFilters, BugsCycleStartPayload, IssueCommandKind, IssueCommandPayload,
    IssueStage,
};
use mt_core::ids::{PaneId, ProjectId, TabId};
use mt_core::workspace::{CommandOverride, EnvValue, TabOpenOverrides, TabOpenRequest};
use mt_git::{GitService, WorktreeCreateRequest, WorktreeLocation, WorktreeManager};
use mt_github::error::{GithubError, GithubErrorKind, GithubResult};
use mt_github::model::{
    BugCycleRunState, CreateIssueWorktreeRequest, IssueTabTargetKind, IssueWorktreeResolution,
    MaxCodeIssueAction, OpenIssueTabReceipt, OpenIssueTabRequest, SendIssueToMaxCodeReceipt,
};
use mt_github::ports::{
    GitRemoteEntry, GitRepoPort, MaxCodeBridgePort, ProjectLookupPort, WorkspaceTabPort,
};
use mt_github::worktree::{default_issue_branch_name, resolve_worktree_from_inventory};
use mt_projects::ProjectService;
use tokio::process::Command;
use uuid::Uuid;

use crate::agents::AppAgentState;
use crate::workspace::WorkspaceManager;

/// Adaptador de `ProjectLookupPort` conectado ao `ProjectService` (Fase 06).
pub struct TauriGithubProjectPort {
    projects: Arc<ProjectService>,
}

impl TauriGithubProjectPort {
    /// Cria um novo adaptador de projetos.
    #[must_use]
    pub const fn new(projects: Arc<ProjectService>) -> Self {
        Self { projects }
    }
}

#[async_trait]
impl ProjectLookupPort for TauriGithubProjectPort {
    async fn project_root_path(&self, project_id: ProjectId) -> GithubResult<PathBuf> {
        let list = self.projects.list().await.map_err(|e| {
            GithubError::new(
                GithubErrorKind::Database,
                "Falha ao listar projetos cadastrados.",
            )
            .with_details(e.to_string())
        })?;
        let found = list
            .into_iter()
            .find(|p| p.id == project_id)
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::NotFound,
                    format!("Projeto '{project_id}' não encontrado."),
                )
            })?;
        Ok(PathBuf::from(found.path))
    }
}

/// Adaptador de `GitRepoPort` conectado ao `GitService` e `WorktreeManager` (Fase 08).
pub struct TauriGithubGitPort {
    git_service: GitService,
    worktree_manager: Arc<dyn WorktreeManager>,
}

impl TauriGithubGitPort {
    /// Cria um novo adaptador Git para `mt-github`.
    #[must_use]
    pub fn new(git_service: GitService, worktree_manager: Arc<dyn WorktreeManager>) -> Self {
        Self {
            git_service,
            worktree_manager,
        }
    }

    async fn run_git_query(
        &self,
        cwd: &Path,
        args: &[&str],
        timeout: Duration,
    ) -> Option<std::process::Output> {
        let mut cmd = Command::new("git");
        cmd.current_dir(cwd)
            .arg("--no-optional-locks")
            .args(["-c", "core.quotepath=off"])
            .args(["-c", "color.ui=never"])
            .args(["-c", "gc.auto=0"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        tokio::time::timeout(timeout, cmd.output())
            .await
            .ok()
            .and_then(Result::ok)
    }

    async fn list_local_branches(&self, project_path: &Path) -> Vec<String> {
        let Some(out) = self
            .run_git_query(
                project_path,
                &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
                Duration::from_secs(3),
            )
            .await
        else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(ToString::to_string)
            .collect()
    }
}

#[async_trait]
impl GitRepoPort for TauriGithubGitPort {
    async fn list_remotes(&self, project_path: &Path) -> GithubResult<Option<Vec<GitRemoteEntry>>> {
        if !project_path.exists() {
            return Ok(None);
        }

        let Some(check_out) = self
            .run_git_query(
                project_path,
                &["rev-parse", "--is-inside-work-tree"],
                Duration::from_secs(3),
            )
            .await
        else {
            return Ok(None);
        };
        if !check_out.status.success() {
            return Ok(None);
        }

        let out = self
            .run_git_query(project_path, &["remote", "-v"], Duration::from_secs(3))
            .await
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "Falha ao inspecionar remotes Git do projeto.",
                )
            })?;

        if !out.status.success() {
            return Ok(Some(Vec::new()));
        }

        let mut seen = BTreeMap::<String, String>::new();
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[0].trim();
                let url = parts[1].trim();
                let is_fetch = parts.get(2).is_none_or(|kind| *kind == "(fetch)");
                if is_fetch || !seen.contains_key(name) {
                    seen.insert(name.to_string(), url.to_string());
                }
            }
        }

        let entries = seen
            .into_iter()
            .map(|(name, url)| GitRemoteEntry { name, url })
            .collect();
        Ok(Some(entries))
    }

    async fn detect_default_branch(&self, project_path: &Path) -> GithubResult<Option<String>> {
        if let Some(out) = self
            .run_git_query(
                project_path,
                &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
                Duration::from_secs(2),
            )
            .await
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(branch) = text.strip_prefix("origin/")
                && !branch.is_empty()
            {
                return Ok(Some(branch.to_string()));
            }
        }

        let branches = self.list_local_branches(project_path).await;
        for candidate in ["main", "master", "dev", "develop"] {
            if branches.iter().any(|b| b == candidate) {
                return Ok(Some(candidate.to_string()));
            }
        }
        Ok(branches.into_iter().next())
    }

    async fn resolve_issue_worktree(
        &self,
        project_path: &Path,
        issue_number: u64,
        issue_title: &str,
        default_branch: Option<&str>,
    ) -> GithubResult<IssueWorktreeResolution> {
        let wt_list = self
            .worktree_manager
            .list(project_path)
            .await
            .unwrap_or_default();

        let existing_wt: Vec<(String, Option<String>)> = wt_list
            .into_iter()
            .map(|w| (w.path.to_string_lossy().to_string(), w.branch))
            .collect();
        let local_branches = self.list_local_branches(project_path).await;

        Ok(resolve_worktree_from_inventory(
            project_path,
            issue_number,
            issue_title,
            default_branch,
            &existing_wt,
            &local_branches,
        ))
    }

    async fn ensure_issue_worktree(
        &self,
        project_path: &Path,
        req: &CreateIssueWorktreeRequest,
        issue_title: &str,
        default_branch: Option<&str>,
    ) -> GithubResult<(String, String)> {
        let resolution = self
            .resolve_issue_worktree(project_path, req.issue_number, issue_title, default_branch)
            .await?;

        if let IssueWorktreeResolution::ExistingWorktree { path, branch } = resolution {
            return Ok((path, branch));
        }

        let branch_to_use = req
            .branch_name
            .clone()
            .filter(|b| !b.trim().is_empty())
            .unwrap_or_else(|| match &resolution {
                IssueWorktreeResolution::ExistingBranch { branch, .. } => branch.clone(),
                _ => default_issue_branch_name(req.issue_number, issue_title),
            });

        let wt_name = format!("issue-{}", req.issue_number);
        let location = if project_path.join(".max-code-worktrees").is_dir() {
            WorktreeLocation::MaxCode
        } else {
            WorktreeLocation::MaxTerminal
        };

        let created = self
            .worktree_manager
            .create(WorktreeCreateRequest {
                project_root: project_path.to_path_buf(),
                name: wt_name,
                branch: Some(branch_to_use.clone()),
                base_ref: req
                    .base_ref
                    .clone()
                    .or_else(|| default_branch.map(ToString::to_string)),
                location,
            })
            .await
            .map_err(|e| {
                GithubError::new(
                    GithubErrorKind::WorktreeFailed,
                    "Não foi possível criar a worktree dedicada para a issue.",
                )
                .with_details(e.to_string())
            })?;

        let _ = self
            .git_service
            .refresh_path(&created.path, mt_git::RefreshReason::Manual)
            .await;

        Ok((
            created.path.to_string_lossy().to_string(),
            created.branch.unwrap_or(branch_to_use),
        ))
    }
}

/// Adaptador de `WorkspaceTabPort` conectado ao `WorkspaceManager` (Fase 07) e `AppAgentState` (Fase 13).
pub struct TauriGithubWorkspacePort {
    workspace: WorkspaceManager,
    agents: AppAgentState,
}

impl TauriGithubWorkspacePort {
    /// Cria um novo adaptador de abas do workspace.
    #[must_use]
    pub const fn new(workspace: WorkspaceManager, agents: AppAgentState) -> Self {
        Self { workspace, agents }
    }
}

#[async_trait]
impl WorkspaceTabPort for TauriGithubWorkspacePort {
    async fn find_tab_by_cwd(
        &self,
        project_id: ProjectId,
        cwd: &str,
    ) -> GithubResult<Option<(TabId, PaneId)>> {
        let tabs = self.workspace.tab_list(project_id).await.map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Falha ao listar abas do projeto.",
            )
            .with_details(e.to_string())
        })?;
        for tab in tabs {
            for pane in &tab.panes {
                if pane.cwd == cwd {
                    return Ok(Some((tab.id, pane.id)));
                }
            }
        }
        Ok(None)
    }

    async fn focus_tab(
        &self,
        _project_id: ProjectId,
        tab_id: TabId,
        pane_id: PaneId,
    ) -> GithubResult<()> {
        self.workspace
            .tab_focus(tab_id, Some(pane_id))
            .await
            .map_err(|e| {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "Falha ao focar a aba existente da issue.",
                )
                .with_details(e.to_string())
            })?;
        Ok(())
    }

    async fn open_issue_tab(
        &self,
        req: &OpenIssueTabRequest,
        resolved_cwd: &str,
        resolved_branch: Option<&str>,
        issue_title: &str,
        issue_html_url: &str,
    ) -> GithubResult<OpenIssueTabReceipt> {
        let tab_title = format!("#{} {}", req.issue_number, issue_title);

        match &req.target {
            IssueTabTargetKind::Terminal => {
                let mut extra_env = BTreeMap::new();
                extra_env.insert(
                    String::from("MAXTERMINAL_ISSUE_NUMBER"),
                    EnvValue::Literal {
                        value: req.issue_number.to_string(),
                    },
                );
                extra_env.insert(
                    String::from("MAXTERMINAL_ISSUE_URL"),
                    EnvValue::Literal {
                        value: issue_html_url.to_string(),
                    },
                );
                if let Some(br) = resolved_branch {
                    extra_env.insert(
                        String::from("MAXTERMINAL_ISSUE_BRANCH"),
                        EnvValue::Literal {
                            value: br.to_string(),
                        },
                    );
                }

                let view = self
                    .workspace
                    .tab_open(TabOpenRequest {
                        request_id: Uuid::now_v7().to_string(),
                        project_id: req.project_id,
                        template_id: None,
                        overrides: Some(TabOpenOverrides {
                            title: Some(tab_title),
                            cwd: Some(resolved_cwd.to_string()),
                            initial_command: CommandOverride::Inherit,
                            extra_env,
                            continuous: Some(false),
                            cwd_fallback_to_project: true,
                        }),
                        focus: true,
                    })
                    .await
                    .map_err(|e| {
                        GithubError::new(
                            GithubErrorKind::Internal,
                            "Falha ao abrir aba de terminal para a issue.",
                        )
                        .with_details(e.to_string())
                    })?;

                Ok(OpenIssueTabReceipt {
                    tab_id: view.id,
                    pane_id: view.active_pane_id,
                    cwd: resolved_cwd.to_string(),
                    branch: resolved_branch.map(ToString::to_string),
                    reused_existing_tab: false,
                })
            }
            IssueTabTargetKind::Agent {
                agent,
                inject_issue_context: _,
            } => {
                let isolation = if Path::new(resolved_cwd)
                    .to_string_lossy()
                    .contains("worktrees")
                {
                    WorktreeIsolation::Existing {
                        path: resolved_cwd.to_string(),
                    }
                } else {
                    WorktreeIsolation::None
                };

                let opened = self
                    .agents
                    .open_agent_tab(AgentTabOpenRequest {
                        project_id: req.project_id,
                        agent: *agent,
                        template_id: None,
                        isolation,
                        model: None,
                        title: Some(tab_title),
                        focus: true,
                    })
                    .await
                    .map_err(|e| {
                        GithubError::new(
                            GithubErrorKind::Internal,
                            "Falha ao abrir aba de Agente CLI para a issue.",
                        )
                        .with_details(e.to_string())
                    })?;

                Ok(OpenIssueTabReceipt {
                    tab_id: opened.tab.id,
                    pane_id: opened.tab.active_pane_id,
                    cwd: resolved_cwd.to_string(),
                    branch: resolved_branch.map(ToString::to_string),
                    reused_existing_tab: false,
                })
            }
        }
    }
}

/// Adaptador de `MaxCodeBridgePort` conectado ao `BridgeService` (Fase 11).
pub struct TauriGithubBridgePort {
    bridge: Arc<BridgeService>,
}

impl TauriGithubBridgePort {
    /// Cria um novo adaptador da ponte MaxCode.
    #[must_use]
    pub const fn new(bridge: Arc<BridgeService>) -> Self {
        Self { bridge }
    }
}

#[async_trait]
impl MaxCodeBridgePort for TauriGithubBridgePort {
    async fn send_issue_action(
        &self,
        _project_id: ProjectId,
        project_path: &str,
        repo_key: &str,
        issue_number: u64,
        issue_title: &str,
        issue_url: &str,
        _has_plan: bool,
        action: MaxCodeIssueAction,
    ) -> GithubResult<SendIssueToMaxCodeReceipt> {
        let (kind, stage) = match action {
            MaxCodeIssueAction::Open => (IssueCommandKind::Open, IssueStage::Inspect),
            MaxCodeIssueAction::BugPlan => (IssueCommandKind::Plan, IssueStage::BugPlan),
            MaxCodeIssueAction::BugFix => (IssueCommandKind::Fix, IssueStage::BugFix),
        };

        let request_uuid = Uuid::new_v4();
        let request_id = request_uuid.to_string();
        let payload = IssueCommandPayload {
            kind,
            project_path: project_path.to_string(),
            repo_full_name: repo_key
                .strip_prefix("github.com/")
                .unwrap_or(repo_key)
                .to_string(),
            issue_number,
            issue_title: issue_title.to_string(),
            issue_url: Some(issue_url.to_string()),
            labels: Vec::new(),
            stage,
            focus_editor: true,
        };

        match self.bridge.send_issue_command(payload.clone()).await {
            Ok(outcome) => {
                let channel = match outcome {
                    mt_bridge::IssueActionDispatchOutcome::Loopback { .. } => "loopback_ws",
                    mt_bridge::IssueActionDispatchOutcome::UriFallback { .. } => "uri_scheme",
                };
                Ok(SendIssueToMaxCodeReceipt {
                    issue_number,
                    action,
                    delivery_channel: channel.to_string(),
                    request_id,
                    message: format!("Issue #{issue_number} enviada para o MaxCode."),
                })
            }
            Err(_err) => {
                // Fallback determinístico por URI Scheme caso o loopback não tenha respondido
                if let Ok(uri) = mt_bridge::build_vscode_issue_action_uri(request_uuid, &payload) {
                    let _ = std::process::Command::new("xdg-open").arg(&uri).spawn();
                }
                Ok(SendIssueToMaxCodeReceipt {
                    issue_number,
                    action,
                    delivery_channel: String::from("uri_scheme"),
                    request_id,
                    message: format!(
                        "Issue #{issue_number} encaminhada para o MaxCode via URI Scheme."
                    ),
                })
            }
        }
    }

    async fn start_bugs_cycle(
        &self,
        cycle: &BugCycleRunState,
        project_path: &str,
    ) -> GithubResult<()> {
        let cycle_uuid = Uuid::parse_str(&cycle.cycle_id).unwrap_or_else(|_| Uuid::new_v4());
        let payload = BugsCycleStartPayload {
            cycle_id: cycle_uuid,
            project_path: project_path.to_string(),
            repo_full_name: cycle
                .repo_key
                .strip_prefix("github.com/")
                .unwrap_or(&cycle.repo_key)
                .to_string(),
            issue_numbers: cycle.issue_numbers.clone(),
            filters: BugsCycleFilters {
                include_labels: Vec::new(),
                exclude_labels: Vec::new(),
                explicit_issue_numbers: cycle.issue_numbers.clone(),
                max_issues: None,
            },
            dry_run: false,
        };

        let _ = self.bridge.bugs_cycle_start(payload).await;
        Ok(())
    }
}
