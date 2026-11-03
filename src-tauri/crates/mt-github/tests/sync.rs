//! Testes do `GithubSyncCoordinator` e `ensure_canonical_labels` (`mt-github::sync`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use mt_core::ids::ProjectId;
use mt_db::{Db, DbConfig};
use mt_github::cache::GithubCacheRepo;
use mt_github::client::{
    GithubRemoteClient, RemoteFetchOutcome, RemoteIssueDetailBundle, RemoteIssueRecord,
};
use mt_github::error::GithubResult;
use mt_github::labels::to_repo_label;
use mt_github::model::{
    GhCliStatus, GithubRepoIdentity, GithubTransportKind, IssueStateFilter, RepoLabel,
};
use mt_github::ports::NoopGithubEventSink;
use mt_github::sync::GithubSyncCoordinator;
use tempfile::TempDir;

struct MockRemote {
    created_labels: AtomicUsize,
}

#[async_trait]
impl GithubRemoteClient for MockRemote {
    async fn check_gh_status(&self) -> GhCliStatus {
        GhCliStatus {
            installed: true,
            version: Some(String::from("2.65.0")),
            authenticated: true,
            account_login: Some(String::from("johnattas")),
            scopes: vec![String::from("repo")],
            message: None,
        }
    }

    async fn fetch_issues(
        &self,
        _repo: &GithubRepoIdentity,
        _state: IssueStateFilter,
        _per_page: u32,
        _max_pages: u32,
        _cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RemoteIssueRecord>>> {
        Ok(RemoteFetchOutcome::Fresh {
            data: Vec::new(),
            etag: Some(String::from("\"v1\"")),
            transport: GithubTransportKind::GhCli,
            rate_limit: None,
        })
    }

    async fn fetch_issue_detail(
        &self,
        _repo: &GithubRepoIdentity,
        _issue_number: u64,
    ) -> GithubResult<RemoteIssueDetailBundle> {
        unreachable!()
    }

    async fn fetch_repo_labels(
        &self,
        _repo: &GithubRepoIdentity,
        _cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RepoLabel>>> {
        Ok(RemoteFetchOutcome::Fresh {
            data: vec![to_repo_label("Waiting Plan", Some("0E8A16"), None)],
            etag: None,
            transport: GithubTransportKind::GhCli,
            rate_limit: None,
        })
    }

    async fn create_repo_label(
        &self,
        _repo: &GithubRepoIdentity,
        name: &str,
        color_hex: &str,
        description: &str,
    ) -> GithubResult<RepoLabel> {
        self.created_labels.fetch_add(1, Ordering::SeqCst);
        Ok(to_repo_label(name, Some(color_hex), Some(description)))
    }

    async fn create_issue(
        &self,
        _repo: &GithubRepoIdentity,
        _title: &str,
        _body: &str,
        _labels: &[String],
        _assignees: &[String],
    ) -> GithubResult<RemoteIssueRecord> {
        unreachable!()
    }

    async fn replace_issue_labels(
        &self,
        _repo: &GithubRepoIdentity,
        _issue_number: u64,
        _labels: &[String],
    ) -> GithubResult<Vec<RepoLabel>> {
        unreachable!()
    }
}

#[tokio::test]
async fn ensure_canonical_labels_creates_only_missing_23_labels() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("sync.db"))).unwrap();
    let cache = GithubCacheRepo::new(db.clone());
    let mock = Arc::new(MockRemote {
        created_labels: AtomicUsize::new(0),
    });
    let coord = GithubSyncCoordinator::new(cache, mock.clone(), Arc::new(NoopGithubEventSink));

    let repo = GithubRepoIdentity {
        repo_key: String::from("github.com/johnattas/maxterminal"),
        host: String::from("github.com"),
        owner: String::from("johnattas"),
        repo: String::from("maxterminal"),
        remote_name: String::from("origin"),
        html_url: String::from("https://github.com/johnattas/maxterminal"),
        default_branch: Some(String::from("main")),
    };

    let dry = coord
        .ensure_canonical_labels(ProjectId::new(), &repo, true)
        .await
        .unwrap();
    assert_eq!(dry.already_present, vec![String::from("Waiting Plan")]);
    assert_eq!(dry.missing.len(), 23);
    assert!(dry.created.is_empty());

    let applied = coord
        .ensure_canonical_labels(ProjectId::new(), &repo, false)
        .await
        .unwrap();
    assert_eq!(applied.created.len(), 23);
    assert!(applied.missing.is_empty());
    assert_eq!(mock.created_labels.load(Ordering::SeqCst), 23);

    db.shutdown().await.unwrap();
}
