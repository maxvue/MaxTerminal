//! Testes do `GhCliClient` com `GhProcessRunner` determinístico (`mt-github::client::gh_cli`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mt_github::client::RemoteFetchOutcome;
use mt_github::client::gh_cli::{GhCliClient, GhCommandOutput, GhProcessRunner};
use mt_github::error::GithubResult;
use mt_github::model::{GithubRepoIdentity, IssueStateFilter};

struct FakeGhRunner;

#[async_trait]
impl GhProcessRunner for FakeGhRunner {
    async fn run_gh(&self, args: &[String], _timeout: Duration) -> GithubResult<GhCommandOutput> {
        if args.first().map(String::as_str) == Some("--version") {
            return Ok(GhCommandOutput {
                exit_code: 0,
                stdout: String::from("gh version 2.65.0 (2026-01-15)\n"),
                stderr: String::new(),
            });
        }
        if args.first().map(String::as_str) == Some("auth") {
            return Ok(GhCommandOutput {
                exit_code: 0,
                stdout: String::from(
                    "github.com\n  ✓ Logged in to github.com account johnattas (keyring)\n  - Token scopes: 'gist', 'read:org', 'repo'\n",
                ),
                stderr: String::new(),
            });
        }
        if args.first().map(String::as_str) == Some("api") {
            return Ok(GhCommandOutput {
                exit_code: 0,
                stdout: String::from(
                    r#"[{"number": 7, "title": "Issue via gh CLI", "body": "Descrição", "state": "open", "user": {"login": "johnattas"}, "labels": [{"name": "Planned", "color": "1D76DB"}], "comments": 0, "html_url": "https://github.com/johnattas/maxterminal/issues/7", "created_at": "2026-03-01T00:00:00Z", "updated_at": "2026-03-02T00:00:00Z"}]"#,
                ),
                stderr: String::new(),
            });
        }
        Ok(GhCommandOutput {
            exit_code: 1,
            stdout: String::new(),
            stderr: String::from("comando desconhecido"),
        })
    }
}

#[tokio::test]
async fn gh_cli_client_probes_auth_and_fetches_issues() {
    let client = GhCliClient::new(Arc::new(FakeGhRunner));
    let status = client.probe_status().await;
    assert!(status.installed);
    assert!(status.authenticated);
    assert_eq!(status.account_login.as_deref(), Some("johnattas"));
    assert!(status.scopes.contains(&String::from("repo")));

    let repo = GithubRepoIdentity {
        repo_key: String::from("github.com/johnattas/maxterminal"),
        host: String::from("github.com"),
        owner: String::from("johnattas"),
        repo: String::from("maxterminal"),
        remote_name: String::from("origin"),
        html_url: String::from("https://github.com/johnattas/maxterminal"),
        default_branch: Some(String::from("main")),
    };

    let outcome = client
        .fetch_issues(&repo, IssueStateFilter::Open, 50)
        .await
        .unwrap();
    match outcome {
        RemoteFetchOutcome::Fresh { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0].number, 7);
            assert_eq!(data[0].title, "Issue via gh CLI");
        }
        other => panic!("Esperava Fresh, obteve {other:?}"),
    }
}
