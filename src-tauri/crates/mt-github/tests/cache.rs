//! Testes de persistência SQLite (`GithubCacheRepo`), sanitização de segredos e filtros (`mt-github::cache`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec
)]

use mt_core::ids::ProjectId;
use mt_db::{Db, DbConfig};
use mt_github::cache::GithubCacheRepo;
use mt_github::client::RemoteIssueRecord;
use mt_github::labels::to_repo_label;
use mt_github::model::{
    GithubRepoIdentity, IssueComment, IssueFilter, IssueStateFilter, RepoResolution,
};
use tempfile::TempDir;

#[tokio::test]
async fn cache_sanitizes_secrets_and_filters_by_canonical_labels_and_plan() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("github.db"))).unwrap();
    let cache = GithubCacheRepo::new(db.clone());

    let proj_id = ProjectId::new();
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
            number: 1,
            node_id: None,
            title: String::from("Erro com Authorization: Bearer ghp_secretToken1234567890"),
            body: String::from(
                "## PLANO DE IMPLEMENTAÇÃO\nCorrigir vazamento de APP_KEY=base64:segredoSuperSecreto123=",
            ),
            state: String::from("open"),
            state_reason: None,
            author_login: String::from("johnattas"),
            author_avatar_url: None,
            assignees: vec![String::from("johnattas")],
            milestone_title: None,
            labels: vec![
                to_repo_label("Planned", Some("1D76DB"), None),
                to_repo_label("Seguranca", Some("F9D0C4"), None),
            ],
            comments_count: 1,
            html_url: String::from("https://github.com/johnattas/maxterminal/issues/1"),
            created_at_ms: 1_000,
            updated_at_ms: 2_000,
            closed_at_ms: None,
        },
        RemoteIssueRecord {
            number: 2,
            node_id: None,
            title: String::from("Ajustar layout da barra de abas"),
            body: String::from("Sem plano ainda."),
            state: String::from("open"),
            state_reason: None,
            author_login: String::from("maria"),
            author_avatar_url: None,
            assignees: Vec::new(),
            milestone_title: None,
            labels: vec![to_repo_label("Waiting Plan", Some("0E8A16"), None)],
            comments_count: 0,
            html_url: String::from("https://github.com/johnattas/maxterminal/issues/2"),
            created_at_ms: 1_500,
            updated_at_ms: 1_500,
            closed_at_ms: None,
        },
    ];

    let open_cnt = cache
        .upsert_issues_batch(&repo, &issues, IssueStateFilter::Open, 3_000)
        .await
        .unwrap();
    assert_eq!(open_cnt, 2);

    // Verifica que o segredo nunca foi persistido em texto claro
    let detail1 = cache
        .upsert_issue_detail(
            &repo,
            &issues[0],
            &[IssueComment {
                comment_id: 500,
                issue_number: 1,
                author_login: String::from("bot"),
                author_avatar_url: None,
                body_markdown: String::from(
                    "## PLANO DE IMPLEMENTAÇÃO\nPlano atualizado no comentário 500.",
                ),
                html_url: String::from(
                    "https://github.com/johnattas/maxterminal/issues/1#issuecomment-500",
                ),
                created_at_ms: 4_000,
                updated_at_ms: 4_500,
                contains_active_plan: false,
            }],
            true,
            5_000,
        )
        .await
        .unwrap();

    assert!(!detail1.summary.title.contains("ghp_secretToken1234567890"));
    assert!(!detail1.body_markdown.contains("segredoSuperSecreto123"));
    assert!(detail1.plan.is_some());
    assert_eq!(detail1.plan.as_ref().unwrap().comment_id, Some(500));

    // Filtro por has_plan = true
    let page_with_plan = cache
        .list_issues(
            &repo.repo_key,
            &IssueFilter {
                has_plan: Some(true),
                ..Default::default()
            },
            false,
        )
        .await
        .unwrap();
    assert_eq!(page_with_plan.items.len(), 1);
    assert_eq!(page_with_plan.items[0].number, 1);

    // Contadores por projeto
    let counts = cache
        .get_issue_counts(proj_id, Some(&repo.repo_key))
        .await
        .unwrap();
    assert_eq!(counts.open_count, 2);
    assert_eq!(counts.with_plan_count, 1);
    assert_eq!(counts.by_state_label.get("Planned"), Some(&1));
    assert_eq!(counts.by_state_label.get("Waiting Plan"), Some(&1));

    let _ = RepoResolution::NoRemotes;
    db.shutdown().await.unwrap();
}
