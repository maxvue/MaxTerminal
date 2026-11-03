//! Testes de resolução de worktree/branch e ciclo em lote "Corrigir todos os Bugs" (`mt-github::bulk` e `worktree`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use mt_core::ids::ProjectId;
use mt_db::{
    Db, DbConfig, ProjectMutation, StoredNewProject, StoredProject, sanitizer::SanitizedText,
};
use mt_github::bulk::BugCycleOrchestrator;
use mt_github::cache::GithubCacheRepo;
use mt_github::client::RemoteIssueRecord;
use mt_github::error::{GithubErrorKind, GithubResult};
use mt_github::labels::to_repo_label;
use mt_github::model::{
    BugCycleRunState, GithubRepoIdentity, IssueStateFilter, IssueWorktreeResolution,
    MaxCodeIssueAction, SendIssueToMaxCodeReceipt, StartBugCycleRequest,
};
use mt_github::ports::{MaxCodeBridgePort, NoopGithubEventSink, ProjectLookupPort};
use mt_github::worktree::{default_issue_branch_name, resolve_worktree_from_inventory};
use tempfile::TempDir;

struct DummyProjectPort(PathBuf);

#[async_trait]
impl ProjectLookupPort for DummyProjectPort {
    async fn project_root_path(&self, _project_id: ProjectId) -> GithubResult<PathBuf> {
        Ok(self.0.clone())
    }
}

struct DummyBridgePort;

#[async_trait]
impl MaxCodeBridgePort for DummyBridgePort {
    async fn send_issue_action(
        &self,
        _project_id: ProjectId,
        _project_path: &str,
        _repo_key: &str,
        issue_number: u64,
        _issue_title: &str,
        _issue_url: &str,
        _has_plan: bool,
        action: MaxCodeIssueAction,
    ) -> GithubResult<SendIssueToMaxCodeReceipt> {
        Ok(SendIssueToMaxCodeReceipt {
            issue_number,
            action,
            delivery_channel: String::from("loopback_ws"),
            request_id: String::from("req-1"),
            message: String::from("Enviado com sucesso."),
        })
    }

    async fn start_bugs_cycle(
        &self,
        _cycle: &BugCycleRunState,
        _project_path: &str,
    ) -> GithubResult<()> {
        Ok(())
    }
}

#[test]
fn worktree_naming_and_inventory_resolution() {
    assert_eq!(
        default_issue_branch_name(42, "Corrigir vazamento de PTY ao fechar aba"),
        "fix/issue-42-corrigir-vazamento-de-pty-ao-fechar-ab"
    );

    let root = Path::new("/tmp/proj");
    let existing = vec![(
        String::from("/tmp/proj/.worktrees/wt-issue-42"),
        Some(String::from("fix/issue-42-pty")),
    )];
    let res = resolve_worktree_from_inventory(root, 42, "Bug PTY", Some("main"), &existing, &[]);
    assert_eq!(
        res,
        IssueWorktreeResolution::ExistingWorktree {
            path: String::from("/tmp/proj/.worktrees/wt-issue-42"),
            branch: String::from("fix/issue-42-pty"),
        }
    );
}

#[tokio::test]
async fn bug_cycle_filters_candidates_and_enforces_single_active_cycle() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("bulk.db"))).unwrap();
    let cache = GithubCacheRepo::new(db.clone());

    let proj_id = ProjectId::new();
    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj_id,
                title: SanitizedText::new("MaxTerminal"),
                icon: String::from("logos:tauri"),
                color: String::from("#14B8A6"),
                path: tmp.path().to_string_lossy().into_owned(),
                root_id: None,
                stack: String::from("tauri"),
                identity_source: String::from("auto"),
                sort_order: 0,
                missing: false,
                created_at: 1_000,
                updated_at: 1_000,
            },
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap();

    let repo = GithubRepoIdentity {
        repo_key: String::from("github.com/johnattas/maxterminal"),
        host: String::from("github.com"),
        owner: String::from("johnattas"),
        repo: String::from("maxterminal"),
        remote_name: String::from("origin"),
        html_url: String::from("https://github.com/johnattas/maxterminal"),
        default_branch: Some(String::from("main")),
    };

    let issues = vec![
        RemoteIssueRecord {
            number: 10,
            node_id: None,
            title: String::from("Issue aguardando plano"),
            body: String::new(),
            state: String::from("open"),
            state_reason: None,
            author_login: String::from("john"),
            author_avatar_url: None,
            assignees: Vec::new(),
            milestone_title: None,
            labels: vec![to_repo_label("Waiting Plan", Some("0E8A16"), None)],
            comments_count: 0,
            html_url: String::from("https://github.com/johnattas/maxterminal/issues/10"),
            created_at_ms: 1_000,
            updated_at_ms: 1_000,
            closed_at_ms: None,
        },
        RemoteIssueRecord {
            number: 11,
            node_id: None,
            title: String::from("Issue duplicada ignorada"),
            body: String::new(),
            state: String::from("open"),
            state_reason: None,
            author_login: String::from("john"),
            author_avatar_url: None,
            assignees: Vec::new(),
            milestone_title: None,
            labels: vec![to_repo_label("Duplicated", Some("CFD3D7"), None)],
            comments_count: 0,
            html_url: String::from("https://github.com/johnattas/maxterminal/issues/11"),
            created_at_ms: 1_100,
            updated_at_ms: 1_100,
            closed_at_ms: None,
        },
    ];

    cache
        .upsert_issues_batch(&repo, &issues, IssueStateFilter::Open, 2_000)
        .await
        .unwrap();

    let orchestrator = BugCycleOrchestrator::new(
        cache,
        Arc::new(DummyProjectPort(tmp.path().to_path_buf())),
        Arc::new(DummyBridgePort),
        Arc::new(NoopGithubEventSink),
    );

    let preview = orchestrator.preview(proj_id, &repo, true).await.unwrap();
    assert_eq!(preview.candidates.len(), 1);
    assert_eq!(preview.candidates[0].issue_number, 10);
    assert_eq!(preview.skipped.len(), 1);
    assert_eq!(preview.skipped[0].issue_number, 11);

    let started = orchestrator
        .start(
            &repo,
            &StartBugCycleRequest {
                project_id: proj_id,
                issue_numbers: vec![10],
                include_replan_and_rejected: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(started.status, "running");
    assert_eq!(started.total_issues, 1);

    // Tentar iniciar outro ciclo ativo simultaneamente deve retornar Conflict
    let second_err = orchestrator
        .start(
            &repo,
            &StartBugCycleRequest {
                project_id: proj_id,
                issue_numbers: vec![10],
                include_replan_and_rejected: true,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(second_err.kind, GithubErrorKind::Conflict);

    db.shutdown().await.unwrap();
}
