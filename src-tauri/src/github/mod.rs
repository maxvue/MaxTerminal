//! Composição e estado gerenciado pelo Tauri para o subsistema de GitHub Issues (`AppGithubState` — Fase 16).

pub mod adapters;

use std::sync::Arc;

use mt_bridge::BridgeService;
use mt_core::paths::AppPaths;
use mt_db::Db;
use mt_git::{GitService, WorktreeManager};
use mt_github::GithubService;
use mt_github::auth::TokenStore;
use mt_github::cache::GithubCacheRepo;
use mt_github::client::fallback::FallbackGithubClient;
use mt_github::client::gh_cli::{GhCliClient, TokioGhProcessRunner};
use mt_github::client::rest::RestClient;
use mt_github::error::GithubResult;
use mt_github::rate_limit::RateLimitTracker;
use mt_projects::ProjectService;
use tauri::AppHandle;

use crate::agents::AppAgentState;
use crate::events::github::TauriGithubEventSink;
use crate::github::adapters::{
    TauriGithubBridgePort, TauriGithubGitPort, TauriGithubProjectPort, TauriGithubWorkspacePort,
};
use crate::workspace::WorkspaceManager;

/// Estado gerenciado pelo Tauri para a Fase 16 (`mt-github`).
#[derive(Clone)]
pub struct AppGithubState {
    /// Instância principal do `GithubService`.
    pub service: Arc<GithubService>,
    /// Banco de dados SQLite para sincronização incremental de busca.
    pub db: Db,
    /// Handle da aplicação Tauri.
    pub app: AppHandle,
}

impl AppGithubState {
    /// Inicializa o subsistema `mt-github` conectando todas as portas do aplicativo.
    #[allow(clippy::too_many_arguments)]
    pub fn init(
        app: AppHandle,
        paths: &AppPaths,
        db: Db,
        projects: Arc<ProjectService>,
        git_service: GitService,
        worktree_manager: Arc<dyn WorktreeManager>,
        workspace: WorkspaceManager,
        agents: AppAgentState,
        bridge: Arc<BridgeService>,
    ) -> GithubResult<Self> {
        let token_path = paths.data_dir.join("github_token.bin");
        let token_store = TokenStore::with_storage_file(token_path);
        let rate_limit = RateLimitTracker::new();
        let rest_client = RestClient::new(token_store, rate_limit)?;
        let gh_client = GhCliClient::new(Arc::new(TokioGhProcessRunner::default()));
        let fallback_client = Arc::new(FallbackGithubClient::new(gh_client, rest_client));

        let cache = GithubCacheRepo::new(db.clone());
        let project_port = Arc::new(TauriGithubProjectPort::new(projects));
        let git_port = Arc::new(TauriGithubGitPort::new(git_service, worktree_manager));
        let workspace_port = Arc::new(TauriGithubWorkspacePort::new(workspace, agents));
        let bridge_port = Arc::new(TauriGithubBridgePort::new(bridge));
        let event_sink = TauriGithubEventSink::new(app.clone());

        let service = Arc::new(GithubService::new(
            cache,
            fallback_client,
            project_port,
            git_port,
            workspace_port,
            bridge_port,
            event_sink,
        ));

        Ok(Self { service, db, app })
    }
}
