//! Orquestrador de fallback automático (`gh` CLI -> REST API -> Cache).

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::client::gh_cli::GhCliClient;
use crate::client::rest::RestClient;
use crate::client::{
    GithubRemoteClient, RemoteFetchOutcome, RemoteIssueDetailBundle, RemoteIssueRecord,
};
use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::model::{
    GhCliStatus, GithubAuthStatus, GithubRepoIdentity, GithubTransportKind, IssueStateFilter,
    RepoLabel,
};

/// Cliente combinado que prioriza o `gh` CLI quando autenticado e faz fallback transparente para a REST API.
pub struct FallbackGithubClient {
    gh: GhCliClient,
    rest: RestClient,
    last_gh_status: RwLock<Option<GhCliStatus>>,
}

impl FallbackGithubClient {
    /// Cria um novo `FallbackGithubClient`.
    #[must_use]
    pub const fn new(gh: GhCliClient, rest: RestClient) -> Self {
        Self {
            gh,
            rest,
            last_gh_status: RwLock::new(None),
        }
    }

    /// Retorna referência ao `RestClient` subjacente.
    #[must_use]
    pub const fn rest(&self) -> &RestClient {
        &self.rest
    }

    /// Retorna referência ao `GhCliClient` subjacente.
    #[must_use]
    pub const fn gh(&self) -> &GhCliClient {
        &self.gh
    }

    /// Inspeciona e consolida o `GithubAuthStatus` completo.
    pub async fn auth_status(&self, force_probe: bool) -> GithubAuthStatus {
        let gh_status = if force_probe || self.last_gh_status.read().is_none() {
            let probed = self.gh.probe_status().await;
            *self.last_gh_status.write() = Some(probed.clone());
            probed
        } else {
            self.last_gh_status
                .read()
                .clone()
                .unwrap_or_else(|| GhCliStatus {
                    installed: false,
                    version: None,
                    authenticated: false,
                    account_login: None,
                    scopes: Vec::new(),
                    message: None,
                })
        };

        let rest_token = self.rest.token_store().status();
        let preferred_transport = if gh_status.installed && gh_status.authenticated {
            GithubTransportKind::GhCli
        } else {
            GithubTransportKind::Rest
        };

        GithubAuthStatus {
            gh_cli: gh_status,
            rest_token,
            preferred_transport,
            rate_limit: self.rest.rate_limit().snapshot(),
        }
    }

    async fn should_try_gh(&self) -> bool {
        if let Some(ref cached) = *self.last_gh_status.read() {
            return cached.installed && cached.authenticated;
        }
        let probed = self.gh.probe_status().await;
        let ok = probed.installed && probed.authenticated;
        *self.last_gh_status.write() = Some(probed);
        ok
    }

    fn invalidate_gh_on_auth_error(&self, err: &GithubError) {
        if matches!(
            err.kind,
            GithubErrorKind::AuthRequired | GithubErrorKind::AuthInvalid
        ) {
            if let Some(ref mut cached) = *self.last_gh_status.write() {
                cached.authenticated = false;
            }
        }
    }
}

#[async_trait]
impl GithubRemoteClient for FallbackGithubClient {
    async fn check_gh_status(&self) -> GhCliStatus {
        let probed = self.gh.probe_status().await;
        *self.last_gh_status.write() = Some(probed.clone());
        probed
    }

    async fn fetch_issues(
        &self,
        repo: &GithubRepoIdentity,
        state: IssueStateFilter,
        per_page: u32,
        max_pages: u32,
        cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RemoteIssueRecord>>> {
        // Quando já temos ETag em cache e estamos usando REST, priorizamos condicional 304
        if cached_etag.is_none() && self.should_try_gh().await {
            match self.gh.fetch_issues(repo, state, per_page).await {
                Ok(outcome) => return Ok(outcome),
                Err(err) => {
                    self.invalidate_gh_on_auth_error(&err);
                }
            }
        }
        self.rest
            .fetch_issues(repo, state, per_page, max_pages, cached_etag)
            .await
    }

    async fn fetch_issue_detail(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
    ) -> GithubResult<RemoteIssueDetailBundle> {
        if self.should_try_gh().await {
            match self.gh.fetch_issue_detail(repo, issue_number).await {
                Ok(bundle) => return Ok(bundle),
                Err(err) => {
                    self.invalidate_gh_on_auth_error(&err);
                }
            }
        }
        self.rest.fetch_issue_detail(repo, issue_number).await
    }

    async fn fetch_repo_labels(
        &self,
        repo: &GithubRepoIdentity,
        cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RepoLabel>>> {
        if cached_etag.is_none() && self.should_try_gh().await {
            match self.gh.fetch_repo_labels(repo).await {
                Ok(outcome) => return Ok(outcome),
                Err(err) => {
                    self.invalidate_gh_on_auth_error(&err);
                }
            }
        }
        self.rest.fetch_repo_labels(repo, cached_etag).await
    }

    async fn create_repo_label(
        &self,
        repo: &GithubRepoIdentity,
        name: &str,
        color_hex: &str,
        description: &str,
    ) -> GithubResult<RepoLabel> {
        if self.should_try_gh().await {
            match self
                .gh
                .create_repo_label(repo, name, color_hex, description)
                .await
            {
                Ok(label) => return Ok(label),
                Err(err) => self.invalidate_gh_on_auth_error(&err),
            }
        }
        self.rest
            .create_repo_label(repo, name, color_hex, description)
            .await
    }

    async fn create_issue(
        &self,
        repo: &GithubRepoIdentity,
        title: &str,
        body: &str,
        labels: &[String],
        assignees: &[String],
    ) -> GithubResult<RemoteIssueRecord> {
        if self.should_try_gh().await {
            match self
                .gh
                .create_issue(repo, title, body, labels, assignees)
                .await
            {
                Ok(issue) => return Ok(issue),
                Err(err) => self.invalidate_gh_on_auth_error(&err),
            }
        }
        self.rest
            .create_issue(repo, title, body, labels, assignees)
            .await
    }

    async fn replace_issue_labels(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
        labels: &[String],
    ) -> GithubResult<Vec<RepoLabel>> {
        if self.should_try_gh().await {
            match self
                .gh
                .replace_issue_labels(repo, issue_number, labels)
                .await
            {
                Ok(updated) => return Ok(updated),
                Err(err) => self.invalidate_gh_on_auth_error(&err),
            }
        }
        self.rest
            .replace_issue_labels(repo, issue_number, labels)
            .await
    }
}
