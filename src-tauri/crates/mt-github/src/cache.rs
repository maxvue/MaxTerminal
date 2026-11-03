//! Persistência SQLite (`0010_16_github.sql`) com sanitização obrigatória de segredos.

use std::collections::BTreeMap;

use mt_core::ids::ProjectId;
use mt_db::Db;
use rusqlite::{OptionalExtension, Transaction, params};

use crate::client::RemoteIssueRecord;
use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::labels::{
    classify_label_group, extract_canonical_facets, find_canonical_label,
    find_missing_canonical_labels, to_repo_label,
};
use crate::model::{
    BugCycleRunState, BugCycleSkippedItem, CanonicalLabelGroup, ExtractedPlan, GithubRepoIdentity,
    GithubTransportKind, IssueComment, IssueCounts, IssueDetail, IssueFilter, IssueListPage,
    IssueSortOrder, IssueState, IssueStateFilter, PlanSource, RateLimitSnapshot, RepoLabel,
    RepoResolution,
};
use crate::plan::{
    PlanCandidateInput, extract_last_plan_section_from_markdown, resolve_issue_plan,
};

/// Repositório SQLite para o domínio GitHub Issues.
#[derive(Clone)]
pub struct GithubCacheRepo {
    db: Db,
}

impl GithubCacheRepo {
    /// Cria uma nova instância conectada ao `Db` (modo WAL).
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Retorna referência ao `Db` subjacente.
    #[must_use]
    pub const fn db(&self) -> &Db {
        &self.db
    }

    /// Persiste a resolução de repositório GitHub de um projeto (`github_repos` + `github_project_repos` + `github_sync_state`).
    pub async fn save_repo_resolution(
        &self,
        project_id: ProjectId,
        raw_remote_url: Option<String>,
        resolution: &RepoResolution,
        now_ms: i64,
    ) -> GithubResult<()> {
        let res_clone = resolution.clone();
        let proj_str = project_id.to_string();
        let raw_sanitized = raw_remote_url.map(|u| mt_db::sanitizer::sanitize(&u).into_owned());
        self.db
            .write(move |tx| {
                match &res_clone {
                    RepoResolution::GitHub { identity } => {
                        tx.execute(
                            "INSERT INTO github_repos (
                                repo_key, host, owner, repo, html_url, default_branch, open_issues_count, created_at_ms, updated_at_ms
                            ) VALUES (?1, 'github.com', ?2, ?3, ?4, ?5, 0, ?6, ?6)
                            ON CONFLICT(repo_key) DO UPDATE SET
                                owner = excluded.owner,
                                repo = excluded.repo,
                                html_url = excluded.html_url,
                                default_branch = COALESCE(excluded.default_branch, github_repos.default_branch),
                                updated_at_ms = excluded.updated_at_ms;",
                            params![
                                identity.repo_key,
                                identity.owner,
                                identity.repo,
                                identity.html_url,
                                identity.default_branch,
                                now_ms
                            ],
                        )?;

                        tx.execute(
                            "INSERT INTO github_sync_state (repo_key, missing_labels_count)
                             VALUES (?1, 0)
                             ON CONFLICT(repo_key) DO NOTHING;",
                            params![identity.repo_key],
                        )?;

                        tx.execute(
                            "INSERT INTO github_project_repos (
                                project_id, repo_key, remote_name, raw_remote_url, state, error_message, resolved_at_ms
                            ) VALUES (?1, ?2, ?3, ?4, 'git_hub', NULL, ?5)
                            ON CONFLICT(project_id) DO UPDATE SET
                                repo_key = excluded.repo_key,
                                remote_name = excluded.remote_name,
                                raw_remote_url = excluded.raw_remote_url,
                                state = excluded.state,
                                error_message = NULL,
                                resolved_at_ms = excluded.resolved_at_ms;",
                            params![
                                proj_str,
                                identity.repo_key,
                                identity.remote_name,
                                raw_sanitized,
                                now_ms
                            ],
                        )?;
                    }
                    RepoResolution::NonGitHubRemote { remote_name, host } => {
                        tx.execute(
                            "INSERT INTO github_project_repos (
                                project_id, repo_key, remote_name, raw_remote_url, state, error_message, resolved_at_ms
                            ) VALUES (?1, NULL, ?2, ?3, 'non_git_hub_remote', ?4, ?5)
                            ON CONFLICT(project_id) DO UPDATE SET
                                repo_key = NULL,
                                remote_name = excluded.remote_name,
                                raw_remote_url = excluded.raw_remote_url,
                                state = excluded.state,
                                error_message = excluded.error_message,
                                resolved_at_ms = excluded.resolved_at_ms;",
                            params![proj_str, remote_name, raw_sanitized, host, now_ms],
                        )?;
                    }
                    RepoResolution::NoRemotes => {
                        tx.execute(
                            "INSERT INTO github_project_repos (
                                project_id, repo_key, remote_name, raw_remote_url, state, error_message, resolved_at_ms
                            ) VALUES (?1, NULL, NULL, NULL, 'no_remotes', NULL, ?2)
                            ON CONFLICT(project_id) DO UPDATE SET
                                repo_key = NULL,
                                remote_name = NULL,
                                raw_remote_url = NULL,
                                state = 'no_remotes',
                                error_message = NULL,
                                resolved_at_ms = excluded.resolved_at_ms;",
                            params![proj_str, now_ms],
                        )?;
                    }
                    RepoResolution::NotGitRepo => {
                        tx.execute(
                            "INSERT INTO github_project_repos (
                                project_id, repo_key, remote_name, raw_remote_url, state, error_message, resolved_at_ms
                            ) VALUES (?1, NULL, NULL, NULL, 'not_git_repo', NULL, ?2)
                            ON CONFLICT(project_id) DO UPDATE SET
                                repo_key = NULL,
                                remote_name = NULL,
                                raw_remote_url = NULL,
                                state = 'not_git_repo',
                                error_message = NULL,
                                resolved_at_ms = excluded.resolved_at_ms;",
                            params![proj_str, now_ms],
                        )?;
                    }
                    RepoResolution::Error { message } => {
                        let msg = mt_db::sanitizer::sanitize(message).into_owned();
                        tx.execute(
                            "INSERT INTO github_project_repos (
                                project_id, repo_key, remote_name, raw_remote_url, state, error_message, resolved_at_ms
                            ) VALUES (?1, NULL, NULL, NULL, 'error', ?2, ?3)
                            ON CONFLICT(project_id) DO UPDATE SET
                                repo_key = NULL,
                                state = 'error',
                                error_message = excluded.error_message,
                                resolved_at_ms = excluded.resolved_at_ms;",
                            params![proj_str, msg, now_ms],
                        )?;
                    }
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Recupera a identidade do repositório GitHub vinculado a um projeto, se já resolvido.
    pub async fn get_project_repo_identity(
        &self,
        project_id: ProjectId,
    ) -> GithubResult<Option<GithubRepoIdentity>> {
        let proj_str = project_id.to_string();
        let row = self
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT r.repo_key, r.host, r.owner, r.repo, COALESCE(p.remote_name, 'origin'), r.html_url, r.default_branch
                     FROM github_project_repos p
                     JOIN github_repos r ON r.repo_key = p.repo_key
                     WHERE p.project_id = ?1 AND p.state = 'git_hub';",
                    params![proj_str],
                    |row| {
                        Ok(GithubRepoIdentity {
                            repo_key: row.get(0)?,
                            host: row.get(1)?,
                            owner: row.get(2)?,
                            repo: row.get(3)?,
                            remote_name: row.get(4)?,
                            html_url: row.get(5)?,
                            default_branch: row.get(6)?,
                        })
                    },
                )
                .optional()
                .map_err(mt_db::DbError::from)
            })
            .await?;
        Ok(row)
    }

    /// Lê o `ETag` salvo em `github_http_cache` para uma `cache_key`.
    pub async fn get_cached_etag(&self, cache_key: &str) -> GithubResult<Option<String>> {
        let key_owned = cache_key.to_string();
        let etag = self
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT etag FROM github_http_cache WHERE cache_key = ?1;",
                    params![key_owned],
                    |row| row.get::<_, Option<String>>(0),
                )
                .optional()
                .map(Option::flatten)
                .map_err(mt_db::DbError::from)
            })
            .await?;
        Ok(etag)
    }

    /// Grava ou atualiza uma entrada em `github_http_cache`.
    pub async fn save_http_cache(
        &self,
        cache_key: &str,
        repo_key: &str,
        etag: Option<&str>,
        status_code: u16,
        now_ms: i64,
    ) -> GithubResult<()> {
        let ck = cache_key.to_string();
        let rk = repo_key.to_string();
        let et = etag.map(ToString::to_string);
        self.db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO github_http_cache (cache_key, repo_key, etag, last_modified, status_code, updated_at_ms)
                     VALUES (?1, ?2, ?3, NULL, ?4, ?5)
                     ON CONFLICT(cache_key) DO UPDATE SET
                         etag = COALESCE(excluded.etag, github_http_cache.etag),
                         status_code = excluded.status_code,
                         updated_at_ms = excluded.updated_at_ms;",
                    params![ck, rk, et, i64::from(status_code), now_ms],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Atualiza `github_sync_state` com o resultado da última sincronização e snapshot de rate limit.
    pub async fn update_sync_state(
        &self,
        repo_key: &str,
        transport: GithubTransportKind,
        rate_limit: Option<&RateLimitSnapshot>,
        error: Option<(&str, &str)>,
        now_ms: i64,
    ) -> GithubResult<()> {
        let rk = repo_key.to_string();
        let tr_str = match transport {
            GithubTransportKind::GhCli => "gh_cli",
            GithubTransportKind::Rest => "rest",
            GithubTransportKind::Cache => "cache",
        };
        let rl = rate_limit.cloned();
        let err_owned =
            error.map(|(c, m)| (c.to_string(), mt_db::sanitizer::sanitize(m).into_owned()));

        self.db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO github_sync_state (repo_key, missing_labels_count) VALUES (?1, 0)
                     ON CONFLICT(repo_key) DO NOTHING;",
                    params![rk],
                )?;

                if let Some((code, msg)) = err_owned {
                    tx.execute(
                        "UPDATE github_sync_state SET
                            transport_used = ?2,
                            last_sync_at_ms = ?3,
                            last_error_code = ?4,
                            last_error_message = ?5
                         WHERE repo_key = ?1;",
                        params![rk, tr_str, now_ms, code, msg],
                    )?;
                } else {
                    tx.execute(
                        "UPDATE github_sync_state SET
                            transport_used = ?2,
                            last_sync_at_ms = ?3,
                            last_success_at_ms = ?3,
                            last_error_code = NULL,
                            last_error_message = NULL
                         WHERE repo_key = ?1;",
                        params![rk, tr_str, now_ms],
                    )?;
                }

                if let Some(r) = rl {
                    tx.execute(
                        "UPDATE github_sync_state SET
                            rate_limit_limit = ?2,
                            rate_limit_remaining = ?3,
                            rate_limit_used = ?4,
                            rate_limit_reset_ms = ?5,
                            retry_after_until_ms = ?6
                         WHERE repo_key = ?1;",
                        params![
                            rk,
                            i64::from(r.limit),
                            i64::from(r.remaining),
                            i64::from(r.used),
                            r.reset_at_ms,
                            r.retry_after_until_ms
                        ],
                    )?;
                }
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Sincroniza a lista de issues de um repositório no SQLite (sanitizando títulos e descrições e extraindo planos do corpo).
    pub async fn upsert_issues_batch(
        &self,
        repo: &GithubRepoIdentity,
        issues: &[RemoteIssueRecord],
        state_scope: IssueStateFilter,
        now_ms: i64,
    ) -> GithubResult<u32> {
        let repo_clone = repo.clone();
        let records = issues.to_vec();

        let open_count = self
            .db
            .write(move |tx| {
                ensure_repo_row_tx(tx, &repo_clone, now_ms)?;

                for rec in &records {
                    upsert_single_issue_tx(tx, &repo_clone.repo_key, rec, now_ms)?;
                }

                // Se fizemos sync de issues abertas, reconciliamos issues que fecharam remotamente
                if state_scope == IssueStateFilter::Open && !records.is_empty() {
                    let keep_numbers: Vec<i64> = records
                        .iter()
                        .filter(|r| r.state.eq_ignore_ascii_case("open"))
                        .filter_map(|r| i64::try_from(r.number).ok())
                        .collect();
                    let mut existing_open_stmt = tx.prepare(
                        "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'open';",
                    )?;
                    let current_open: Vec<i64> = existing_open_stmt
                        .query_map(params![repo_clone.repo_key], |r| r.get(0))?
                        .collect::<Result<Vec<_>, _>>()?;
                    for num in current_open {
                        if !keep_numbers.contains(&num) {
                            tx.execute(
                                "UPDATE github_issues SET state = 'closed', synced_at_ms = ?3 WHERE repo_key = ?1 AND issue_number = ?2;",
                                params![repo_clone.repo_key, num, now_ms],
                            )?;
                        }
                    }
                }

                let open_cnt = recompute_open_issues_count_tx(tx, &repo_clone.repo_key, now_ms)?;
                Ok(open_cnt)
            })
            .await?;

        Ok(open_count)
    }

    /// Persiste o detalhe completo de uma issue + comentários e recalcula o `ExtractedPlan` vencedor.
    pub async fn upsert_issue_detail(
        &self,
        repo: &GithubRepoIdentity,
        issue: &RemoteIssueRecord,
        comments: &[IssueComment],
        comments_complete: bool,
        now_ms: i64,
    ) -> GithubResult<IssueDetail> {
        let repo_clone = repo.clone();
        let rec = issue.clone();
        let comms = comments.to_vec();

        self.db
            .write(move |tx| {
                ensure_repo_row_tx(tx, &repo_clone, now_ms)?;
                upsert_single_issue_tx(tx, &repo_clone.repo_key, &rec, now_ms)?;

                let issue_num_i64 = i64::try_from(rec.number).unwrap_or(0);
                tx.execute(
                    "DELETE FROM github_issue_comments WHERE repo_key = ?1 AND issue_number = ?2;",
                    params![repo_clone.repo_key, issue_num_i64],
                )?;

                for c in &comms {
                    let body_sanitized = mt_db::sanitizer::sanitize(&c.body_markdown).into_owned();
                    tx.execute(
                        "INSERT INTO github_issue_comments (
                            repo_key, issue_number, comment_id, author_login, author_avatar,
                            body_sanitized, html_url, created_at_ms, updated_at_ms, synced_at_ms
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
                        params![
                            repo_clone.repo_key,
                            issue_num_i64,
                            c.comment_id,
                            c.author_login,
                            c.author_avatar_url,
                            body_sanitized,
                            c.html_url,
                            c.created_at_ms,
                            c.updated_at_ms,
                            now_ms
                        ],
                    )?;
                }

                let sanitized_issue_body = mt_db::sanitizer::sanitize(&rec.body).into_owned();
                let body_cand = PlanCandidateInput {
                    source: PlanSource::Body,
                    comment_id: None,
                    author_login: &rec.author_login,
                    created_at_ms: rec.created_at_ms,
                    updated_at_ms: rec.updated_at_ms,
                    markdown: &sanitized_issue_body,
                };
                let sanitized_comment_bodies: Vec<String> = comms
                    .iter()
                    .map(|c| mt_db::sanitizer::sanitize(&c.body_markdown).into_owned())
                    .collect();
                let comment_cands: Vec<PlanCandidateInput<'_>> = comms
                    .iter()
                    .zip(sanitized_comment_bodies.iter())
                    .map(|(c, body_md)| PlanCandidateInput {
                        source: PlanSource::Comment,
                        comment_id: Some(c.comment_id),
                        author_login: &c.author_login,
                        created_at_ms: c.created_at_ms,
                        updated_at_ms: c.updated_at_ms,
                        markdown: body_md,
                    })
                    .collect();

                let resolved_plan =
                    resolve_issue_plan(&body_cand, &comment_cands, comments_complete);

                match &resolved_plan {
                    Some(plan) => {
                        tx.execute(
                            "UPDATE github_issues SET
                                has_plan = 1,
                                plan_source = ?3,
                                plan_comment_id = ?4,
                                plan_markdown = ?5,
                                plan_updated_at_ms = ?6,
                                detail_synced_at_ms = ?7
                             WHERE repo_key = ?1 AND issue_number = ?2;",
                            params![
                                repo_clone.repo_key,
                                issue_num_i64,
                                plan.source.as_str(),
                                plan.comment_id,
                                plan.markdown,
                                plan.updated_at_ms,
                                now_ms
                            ],
                        )?;
                    }
                    None => {
                        tx.execute(
                            "UPDATE github_issues SET
                                has_plan = 0,
                                plan_source = NULL,
                                plan_comment_id = NULL,
                                plan_markdown = NULL,
                                plan_updated_at_ms = NULL,
                                detail_synced_at_ms = ?3
                             WHERE repo_key = ?1 AND issue_number = ?2;",
                            params![repo_clone.repo_key, issue_num_i64, now_ms],
                        )?;
                    }
                }

                let _ = recompute_open_issues_count_tx(tx, &repo_clone.repo_key, now_ms)?;
                Ok(())
            })
            .await?;

        self.get_issue_detail(&repo.repo_key, issue.number, comments_complete)
            .await?
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::NotFound,
                    format!(
                        "Issue #{} não encontrada no cache após gravação.",
                        issue.number
                    ),
                )
            })
    }

    /// Atualiza apenas as labels de uma issue no cache local e retorna o `IssueSummary` atualizado.
    pub async fn update_issue_labels_in_cache(
        &self,
        repo_key: &str,
        issue_number: u64,
        labels: &[RepoLabel],
        now_ms: i64,
    ) -> GithubResult<crate::model::IssueSummary> {
        let rk = repo_key.to_string();
        let lbs = labels.to_vec();
        let num_i64 = i64::try_from(issue_number).unwrap_or(0);

        self.db
            .write(move |tx| {
                tx.execute(
                    "DELETE FROM github_issue_labels WHERE repo_key = ?1 AND issue_number = ?2;",
                    params![rk, num_i64],
                )?;
                for lb in &lbs {
                    tx.execute(
                        "INSERT INTO github_issue_labels (repo_key, issue_number, label_lower, label_name)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT(repo_key, issue_number, label_lower) DO UPDATE SET label_name = excluded.label_name;",
                        params![rk, num_i64, lb.name_lower, lb.name],
                    )?;
                }
                tx.execute(
                    "UPDATE github_issues SET updated_at_ms = ?3, synced_at_ms = ?3 WHERE repo_key = ?1 AND issue_number = ?2;",
                    params![rk, num_i64, now_ms],
                )?;
                Ok(())
            })
            .await?;

        let rk2 = repo_key.to_string();
        let summary = self
            .db
            .read(move |conn| {
                read_issue_summary_conn(conn, &rk2, num_i64).map_err(mt_db::DbError::from)
            })
            .await?;

        summary.ok_or_else(|| {
            GithubError::new(
                GithubErrorKind::NotFound,
                format!("Issue #{issue_number} não encontrada no cache."),
            )
        })
    }

    /// Salva o catálogo de labels do repositório no SQLite e atualiza `missing_labels_count`.
    pub async fn upsert_repo_labels(
        &self,
        repo: &GithubRepoIdentity,
        labels: &[RepoLabel],
        now_ms: i64,
    ) -> GithubResult<u32> {
        let repo_clone = repo.clone();
        let lbs = labels.to_vec();
        let missing_count = u32::try_from(find_missing_canonical_labels(&lbs).len()).unwrap_or(0);

        self.db
            .write(move |tx| {
                ensure_repo_row_tx(tx, &repo_clone, now_ms)?;
                tx.execute(
                    "DELETE FROM github_labels WHERE repo_key = ?1;",
                    params![repo_clone.repo_key],
                )?;
                for lb in &lbs {
                    let group_str = lb.canonical_group.map(|g| match g {
                        CanonicalLabelGroup::State => "state",
                        CanonicalLabelGroup::Closure => "closure",
                        CanonicalLabelGroup::Category => "category",
                    });
                    tx.execute(
                        "INSERT INTO github_labels (
                            repo_key, name_lower, name, color_hex, description, canonical_group, synced_at_ms
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7);",
                        params![
                            repo_clone.repo_key,
                            lb.name_lower,
                            lb.name,
                            lb.color_hex,
                            lb.description,
                            group_str,
                            now_ms
                        ],
                    )?;
                }
                tx.execute(
                    "UPDATE github_sync_state SET
                        labels_checked_at_ms = ?2,
                        missing_labels_count = ?3
                     WHERE repo_key = ?1;",
                    params![repo_clone.repo_key, now_ms, i64::from(missing_count)],
                )?;
                Ok(())
            })
            .await?;

        Ok(missing_count)
    }

    /// Lista as labels persistidas de um repositório.
    pub async fn list_repo_labels(&self, repo_key: &str) -> GithubResult<Vec<RepoLabel>> {
        let rk = repo_key.to_string();
        let list = self
            .db
            .read(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT name, color_hex, description FROM github_labels WHERE repo_key = ?1 ORDER BY name_lower ASC;",
                )?;
                let rows = stmt
                    .query_map(params![rk], |row| {
                        let name: String = row.get(0)?;
                        let color: String = row.get(1)?;
                        let desc: String = row.get(2)?;
                        Ok(to_repo_label(&name, Some(&color), Some(&desc)))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await?;
        Ok(list)
    }

    /// Lista issues do cache SQLite aplicando `IssueFilter` completo.
    pub async fn list_issues(
        &self,
        repo_key: &str,
        filter: &IssueFilter,
        stale: bool,
    ) -> GithubResult<IssueListPage> {
        let rk = repo_key.to_string();
        let f = filter.clone();

        let page = self
            .db
            .read(move |conn| {
                let all_numbers: Vec<i64> = {
                    let mut stmt = match (f.state, f.sort) {
                        (IssueStateFilter::Open, IssueSortOrder::UpdatedDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'open' ORDER BY updated_at_ms DESC, issue_number DESC;",
                        )?,
                        (IssueStateFilter::Open, IssueSortOrder::CreatedDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'open' ORDER BY created_at_ms DESC, issue_number DESC;",
                        )?,
                        (IssueStateFilter::Open, IssueSortOrder::CreatedAsc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'open' ORDER BY created_at_ms ASC, issue_number ASC;",
                        )?,
                        (IssueStateFilter::Open, IssueSortOrder::CommentsDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'open' ORDER BY comments_count DESC, updated_at_ms DESC;",
                        )?,
                        (IssueStateFilter::Closed, IssueSortOrder::UpdatedDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'closed' ORDER BY updated_at_ms DESC, issue_number DESC;",
                        )?,
                        (IssueStateFilter::Closed, IssueSortOrder::CreatedDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'closed' ORDER BY created_at_ms DESC, issue_number DESC;",
                        )?,
                        (IssueStateFilter::Closed, IssueSortOrder::CreatedAsc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'closed' ORDER BY created_at_ms ASC, issue_number ASC;",
                        )?,
                        (IssueStateFilter::Closed, IssueSortOrder::CommentsDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 AND state = 'closed' ORDER BY comments_count DESC, updated_at_ms DESC;",
                        )?,
                        (IssueStateFilter::All, IssueSortOrder::UpdatedDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 ORDER BY updated_at_ms DESC, issue_number DESC;",
                        )?,
                        (IssueStateFilter::All, IssueSortOrder::CreatedDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 ORDER BY created_at_ms DESC, issue_number DESC;",
                        )?,
                        (IssueStateFilter::All, IssueSortOrder::CreatedAsc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 ORDER BY created_at_ms ASC, issue_number ASC;",
                        )?,
                        (IssueStateFilter::All, IssueSortOrder::CommentsDesc) => conn.prepare(
                            "SELECT issue_number FROM github_issues WHERE repo_key = ?1 ORDER BY comments_count DESC, updated_at_ms DESC;",
                        )?,
                    };
                    stmt.query_map(params![rk], |r| r.get::<_, i64>(0))?
                        .collect::<Result<Vec<_>, _>>()?
                };

                let mut matching = Vec::new();
                for num in all_numbers {
                    if let Some((summary, body_text)) = read_issue_summary_and_body_conn(conn, &rk, num)? {
                        if matches_issue_filter(&summary, &body_text, &f) {
                            matching.push(summary);
                        }
                    }
                }

                let total_matching = u32::try_from(matching.len()).unwrap_or(0);
                let offset = usize::try_from(f.offset.unwrap_or(0)).unwrap_or(0);
                let limit = usize::try_from(f.limit.unwrap_or(50).clamp(1, 200)).unwrap_or(50);

                let items: Vec<_> = matching.into_iter().skip(offset).take(limit).collect();

                let open_issues_count: u32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM github_issues WHERE repo_key = ?1 AND state = 'open';",
                        params![rk],
                        |r| r.get::<_, i64>(0),
                    )
                    .ok()
                    .and_then(|c| u32::try_from(c).ok())
                    .unwrap_or(0);

                let (transport_used, last_success_at_ms, missing_canonical_labels_count, rate_limit) =
                    read_sync_meta_conn(conn, &rk)?;

                Ok(IssueListPage {
                    repo_key: rk,
                    items,
                    total_matching,
                    open_issues_count,
                    stale,
                    transport_used,
                    last_success_at_ms,
                    missing_canonical_labels_count,
                    rate_limit,
                })
            })
            .await?;

        Ok(page)
    }

    /// Obtém o detalhe completo de uma issue a partir do cache SQLite.
    pub async fn get_issue_detail(
        &self,
        repo_key: &str,
        issue_number: u64,
        comments_complete: bool,
    ) -> GithubResult<Option<IssueDetail>> {
        let rk = repo_key.to_string();
        let num_i64 = i64::try_from(issue_number).unwrap_or(0);

        let detail = self
            .db
            .read(move |conn| {
                let Some((
                    summary,
                    body_markdown,
                    plan_source_str,
                    plan_comment_id,
                    plan_markdown,
                    plan_updated_at_ms,
                )) = read_issue_full_row_conn(conn, &rk, num_i64)?
                else {
                    return Ok(None);
                };

                let mut stmt = conn.prepare(
                    "SELECT comment_id, author_login, author_avatar, body_sanitized, html_url, created_at_ms, updated_at_ms
                     FROM github_issue_comments
                     WHERE repo_key = ?1 AND issue_number = ?2
                     ORDER BY created_at_ms ASC, comment_id ASC;",
                )?;

                let mut comments: Vec<IssueComment> = stmt
                    .query_map(params![rk, num_i64], |r| {
                        let cid: i64 = r.get(0)?;
                        Ok(IssueComment {
                            comment_id: cid,
                            issue_number,
                            author_login: r.get(1)?,
                            author_avatar_url: r.get(2)?,
                            body_markdown: r.get(3)?,
                            html_url: r.get(4)?,
                            created_at_ms: r.get(5)?,
                            updated_at_ms: r.get(6)?,
                            contains_active_plan: plan_comment_id == Some(cid),
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;

                let plan = match (plan_source_str.as_deref().and_then(PlanSource::from_db_str), plan_markdown) {
                    (Some(source), Some(markdown)) => {
                        let author_login = if source == PlanSource::Comment {
                            comments
                                .iter()
                                .find(|c| Some(c.comment_id) == plan_comment_id)
                                .map_or_else(|| summary.author_login.clone(), |c| c.author_login.clone())
                        } else {
                            summary.author_login.clone()
                        };
                        Some(ExtractedPlan {
                            source,
                            comment_id: plan_comment_id,
                            author_login,
                            updated_at_ms: plan_updated_at_ms.unwrap_or(summary.updated_at_ms),
                            markdown,
                            provisional: !comments_complete,
                        })
                    }
                    _ => None,
                };

                if let Some(ref p) = plan {
                    if p.source == PlanSource::Comment {
                        for c in &mut comments {
                            c.contains_active_plan = Some(c.comment_id) == p.comment_id;
                        }
                    }
                }

                Ok(Some(IssueDetail {
                    summary,
                    body_markdown,
                    comments,
                    comments_complete,
                    plan,
                }))
            })
            .await?;

        Ok(detail)
    }

    /// Calcula os contadores agregados de issues de um projeto/repositório.
    pub async fn get_issue_counts(
        &self,
        project_id: ProjectId,
        repo_key: Option<&str>,
    ) -> GithubResult<IssueCounts> {
        let Some(rk_ref) = repo_key else {
            return Ok(IssueCounts {
                project_id,
                repo_key: None,
                open_count: 0,
                closed_count: 0,
                by_state_label: BTreeMap::new(),
                by_category_label: BTreeMap::new(),
                with_plan_count: 0,
            });
        };
        let rk = rk_ref.to_string();

        let counts = self
            .db
            .read(move |conn| {
                let open_count: u32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM github_issues WHERE repo_key = ?1 AND state = 'open';",
                        params![rk],
                        |r| r.get::<_, i64>(0),
                    )
                    .ok()
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(0);

                let closed_count: u32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM github_issues WHERE repo_key = ?1 AND state = 'closed';",
                        params![rk],
                        |r| r.get::<_, i64>(0),
                    )
                    .ok()
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(0);

                let with_plan_count: u32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM github_issues WHERE repo_key = ?1 AND state = 'open' AND has_plan = 1;",
                        params![rk],
                        |r| r.get::<_, i64>(0),
                    )
                    .ok()
                    .and_then(|n| u32::try_from(n).ok())
                    .unwrap_or(0);

                let mut by_state_label: BTreeMap<String, u32> = BTreeMap::new();
                let mut by_category_label: BTreeMap<String, u32> = BTreeMap::new();

                let mut stmt = conn.prepare(
                    "SELECT il.label_name, COUNT(*)
                     FROM github_issue_labels il
                     JOIN github_issues i ON i.repo_key = il.repo_key AND i.issue_number = il.issue_number
                     WHERE il.repo_key = ?1 AND i.state = 'open'
                     GROUP BY il.label_lower;",
                )?;
                let rows = stmt.query_map(params![rk], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
                })?;

                for row in rows {
                    let (label_name, cnt_i64) = row?;
                    let cnt = u32::try_from(cnt_i64).unwrap_or(0);
                    if let Some(spec) = find_canonical_label(&label_name) {
                        match spec.group {
                            CanonicalLabelGroup::State => {
                                *by_state_label.entry(spec.name.clone()).or_insert(0) += cnt;
                            }
                            CanonicalLabelGroup::Category => {
                                *by_category_label.entry(spec.name.clone()).or_insert(0) += cnt;
                            }
                            CanonicalLabelGroup::Closure => {}
                        }
                    }
                }

                Ok(IssueCounts {
                    project_id,
                    repo_key: Some(rk),
                    open_count,
                    closed_count,
                    by_state_label,
                    by_category_label,
                    with_plan_count,
                })
            })
            .await?;

        Ok(counts)
    }

    /// Verifica se uma `client_mutation_id` já foi processada e retorna o JSON da resposta anterior.
    pub async fn get_mutation_response(
        &self,
        client_mutation_id: &str,
    ) -> GithubResult<Option<String>> {
        let id = client_mutation_id.to_string();
        let res = self
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT response_json FROM github_mutations WHERE client_mutation_id = ?1;",
                    params![id],
                    |r| r.get::<_, String>(0),
                )
                .optional()
                .map_err(mt_db::DbError::from)
            })
            .await?;
        Ok(res)
    }

    /// Registra uma `client_mutation_id` concluída para garantir idempotência em retries.
    pub async fn record_mutation_response(
        &self,
        client_mutation_id: &str,
        repo_key: &str,
        kind: &str,
        response_json: &str,
        now_ms: i64,
    ) -> GithubResult<()> {
        let id = client_mutation_id.to_string();
        let rk = repo_key.to_string();
        let kd = kind.to_string();
        let json = response_json.to_string();
        self.db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO github_mutations (client_mutation_id, repo_key, kind, response_json, created_at_ms)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(client_mutation_id) DO UPDATE SET response_json = excluded.response_json;",
                    params![id, rk, kd, json, now_ms],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

    /// Obtém o ciclo ativo (`queued`, `running`, `paused`) ou o último ciclo registrado para um repositório.
    pub async fn get_latest_bug_cycle(
        &self,
        repo_key: &str,
    ) -> GithubResult<Option<BugCycleRunState>> {
        let rk = repo_key.to_string();
        let cycle = self
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT cycle_id, project_id, repo_key, status, total_issues, completed_issues,
                            failed_issues, current_issue, current_stage, issue_numbers_json,
                            skipped_json, error_message, started_at_ms, updated_at_ms, finished_at_ms
                     FROM github_bug_cycles
                     WHERE repo_key = ?1
                     ORDER BY CASE WHEN status IN ('queued', 'running', 'paused') THEN 0 ELSE 1 END ASC,
                              started_at_ms DESC
                     LIMIT 1;",
                    params![rk],
                    map_bug_cycle_row,
                )
                .optional()
                .map_err(mt_db::DbError::from)
            })
            .await?;
        Ok(cycle)
    }

    /// Insere um novo ciclo em lote (`github_bug_cycles`), respeitando o índice único parcial `idx_github_one_active_cycle`.
    pub async fn insert_bug_cycle(&self, state: &BugCycleRunState) -> GithubResult<()> {
        let st = state.clone();
        self.db
            .write(move |tx| {
                let nums_json =
                    serde_json::to_string(&st.issue_numbers).unwrap_or_else(|_| String::from("[]"));
                let skipped_json =
                    serde_json::to_string(&st.skipped).unwrap_or_else(|_| String::from("[]"));
                let curr_issue = st.current_issue.and_then(|n| i64::try_from(n).ok());
                let proj_str = st.project_id.to_string();

                tx.execute(
                    "INSERT INTO github_bug_cycles (
                        cycle_id, project_id, repo_key, status, total_issues, completed_issues,
                        failed_issues, current_issue, current_stage, issue_numbers_json,
                        skipped_json, error_message, started_at_ms, updated_at_ms, finished_at_ms
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15);",
                    params![
                        st.cycle_id,
                        proj_str,
                        st.repo_key,
                        st.status,
                        i64::from(st.total_issues),
                        i64::from(st.completed_issues),
                        i64::from(st.failed_issues),
                        curr_issue,
                        st.current_stage,
                        nums_json,
                        skipped_json,
                        st.error_message,
                        st.started_at_ms,
                        st.updated_at_ms,
                        st.finished_at_ms
                    ],
                )
                .map_err(|e| {
                    if e.to_string().contains("UNIQUE constraint failed") {
                        mt_db::DbError::ConstraintViolation {
                            constraint: String::from("github_bug_cycles.one_active_cycle"),
                        }
                    } else {
                        mt_db::DbError::from(e)
                    }
                })?;
                Ok(())
            })
            .await
            .map_err(|err| match err {
                mt_db::DbError::ConstraintViolation { .. } => GithubError::new(
                    GithubErrorKind::Conflict,
                    "Já existe um ciclo automatizado de correção de bugs em andamento para este repositório.",
                ),
                other => GithubError::from(other),
            })
    }

    /// Atualiza um ciclo em lote existente.
    pub async fn update_bug_cycle(&self, state: &BugCycleRunState) -> GithubResult<()> {
        let st = state.clone();
        self.db
            .write(move |tx| {
                let curr_issue = st.current_issue.and_then(|n| i64::try_from(n).ok());
                tx.execute(
                    "UPDATE github_bug_cycles SET
                        status = ?2,
                        completed_issues = ?3,
                        failed_issues = ?4,
                        current_issue = ?5,
                        current_stage = ?6,
                        error_message = ?7,
                        updated_at_ms = ?8,
                        finished_at_ms = ?9
                     WHERE cycle_id = ?1;",
                    params![
                        st.cycle_id,
                        st.status,
                        i64::from(st.completed_issues),
                        i64::from(st.failed_issues),
                        curr_issue,
                        st.current_stage,
                        st.error_message,
                        st.updated_at_ms,
                        st.finished_at_ms
                    ],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}

fn ensure_repo_row_tx(
    tx: &Transaction<'_>,
    repo: &GithubRepoIdentity,
    now_ms: i64,
) -> Result<(), rusqlite::Error> {
    tx.execute(
        "INSERT INTO github_repos (
            repo_key, host, owner, repo, html_url, default_branch, open_issues_count, created_at_ms, updated_at_ms
        ) VALUES (?1, 'github.com', ?2, ?3, ?4, ?5, 0, ?6, ?6)
        ON CONFLICT(repo_key) DO UPDATE SET
            owner = excluded.owner,
            repo = excluded.repo,
            html_url = excluded.html_url,
            default_branch = COALESCE(excluded.default_branch, github_repos.default_branch),
            updated_at_ms = excluded.updated_at_ms;",
        params![
            repo.repo_key,
            repo.owner,
            repo.repo,
            repo.html_url,
            repo.default_branch,
            now_ms
        ],
    )?;
    tx.execute(
        "INSERT INTO github_sync_state (repo_key, missing_labels_count)
         VALUES (?1, 0)
         ON CONFLICT(repo_key) DO NOTHING;",
        params![repo.repo_key],
    )?;
    Ok(())
}

fn upsert_single_issue_tx(
    tx: &Transaction<'_>,
    repo_key: &str,
    rec: &RemoteIssueRecord,
    now_ms: i64,
) -> Result<(), rusqlite::Error> {
    let issue_num_i64 = i64::try_from(rec.number).unwrap_or(0);
    let sanitized_title = mt_db::sanitizer::sanitize(&rec.title).into_owned();
    let sanitized_body = mt_db::sanitizer::sanitize(&rec.body).into_owned();
    let state_norm = if rec.state.eq_ignore_ascii_case("closed") {
        "closed"
    } else {
        "open"
    };
    let assignees_json =
        serde_json::to_string(&rec.assignees).unwrap_or_else(|_| String::from("[]"));

    let body_plan = extract_last_plan_section_from_markdown(&sanitized_body);
    let has_body_plan = body_plan.is_some();

    // Preserva plano já extraído de comentário se existir e for mais recente
    let existing_plan: Option<(
        i64,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
    )> = tx
        .query_row(
            "SELECT has_plan, plan_source, plan_comment_id, plan_markdown, plan_updated_at_ms
             FROM github_issues WHERE repo_key = ?1 AND issue_number = ?2;",
            params![repo_key, issue_num_i64],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?;

    let (has_plan_i64, plan_source, plan_comment_id, plan_markdown, plan_updated_at_ms) =
        match existing_plan {
            Some((1, Some(ref src), cid, Some(md), upd)) if src == "comment" => {
                (1_i64, Some(String::from("comment")), cid, Some(md), upd)
            }
            _ => {
                if has_body_plan {
                    (
                        1_i64,
                        Some(String::from("body")),
                        None,
                        body_plan,
                        Some(rec.updated_at_ms),
                    )
                } else {
                    (0_i64, None, None, None, None)
                }
            }
        };

    tx.execute(
        "INSERT INTO github_issues (
            repo_key, issue_number, node_id, title, body_sanitized, state, state_reason,
            author_login, author_avatar_url, assignees_json, milestone_title, comments_count,
            html_url, has_plan, plan_source, plan_comment_id, plan_markdown, plan_updated_at_ms,
            created_at_ms, updated_at_ms, closed_at_ms, synced_at_ms
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7,
            ?8, ?9, ?10, ?11, ?12,
            ?13, ?14, ?15, ?16, ?17, ?18,
            ?19, ?20, ?21, ?22
        )
        ON CONFLICT(repo_key, issue_number) DO UPDATE SET
            node_id = COALESCE(excluded.node_id, github_issues.node_id),
            title = excluded.title,
            body_sanitized = excluded.body_sanitized,
            state = excluded.state,
            state_reason = excluded.state_reason,
            author_login = excluded.author_login,
            author_avatar_url = excluded.author_avatar_url,
            assignees_json = excluded.assignees_json,
            milestone_title = excluded.milestone_title,
            comments_count = excluded.comments_count,
            html_url = excluded.html_url,
            has_plan = excluded.has_plan,
            plan_source = excluded.plan_source,
            plan_comment_id = excluded.plan_comment_id,
            plan_markdown = excluded.plan_markdown,
            plan_updated_at_ms = excluded.plan_updated_at_ms,
            updated_at_ms = excluded.updated_at_ms,
            closed_at_ms = excluded.closed_at_ms,
            synced_at_ms = excluded.synced_at_ms;",
        params![
            repo_key,
            issue_num_i64,
            rec.node_id,
            sanitized_title,
            sanitized_body,
            state_norm,
            rec.state_reason,
            rec.author_login,
            rec.author_avatar_url,
            assignees_json,
            rec.milestone_title,
            i64::from(rec.comments_count),
            rec.html_url,
            has_plan_i64,
            plan_source,
            plan_comment_id,
            plan_markdown,
            plan_updated_at_ms,
            rec.created_at_ms,
            rec.updated_at_ms,
            rec.closed_at_ms,
            now_ms
        ],
    )?;

    tx.execute(
        "DELETE FROM github_issue_labels WHERE repo_key = ?1 AND issue_number = ?2;",
        params![repo_key, issue_num_i64],
    )?;

    for lb in &rec.labels {
        tx.execute(
            "INSERT INTO github_issue_labels (repo_key, issue_number, label_lower, label_name)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(repo_key, issue_number, label_lower) DO UPDATE SET label_name = excluded.label_name;",
            params![repo_key, issue_num_i64, lb.name_lower, lb.name],
        )?;
    }

    Ok(())
}

fn recompute_open_issues_count_tx(
    tx: &Transaction<'_>,
    repo_key: &str,
    now_ms: i64,
) -> Result<u32, rusqlite::Error> {
    let count_i64: i64 = tx.query_row(
        "SELECT COUNT(*) FROM github_issues WHERE repo_key = ?1 AND state = 'open';",
        params![repo_key],
        |r| r.get(0),
    )?;
    tx.execute(
        "UPDATE github_repos SET open_issues_count = ?2, updated_at_ms = ?3 WHERE repo_key = ?1;",
        params![repo_key, count_i64, now_ms],
    )?;
    Ok(u32::try_from(count_i64).unwrap_or(0))
}

fn read_issue_labels_conn(
    conn: &rusqlite::Connection,
    repo_key: &str,
    issue_num_i64: i64,
) -> Result<Vec<RepoLabel>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT il.label_name, l.color_hex, l.description
         FROM github_issue_labels il
         LEFT JOIN github_labels l ON l.repo_key = il.repo_key AND l.name_lower = il.label_lower
         WHERE il.repo_key = ?1 AND il.issue_number = ?2
         ORDER BY il.label_lower ASC;",
    )?;
    let rows = stmt
        .query_map(params![repo_key, issue_num_i64], |r| {
            let name: String = r.get(0)?;
            let color: Option<String> = r.get(1)?;
            let desc: Option<String> = r.get(2)?;
            Ok(to_repo_label(&name, color.as_deref(), desc.as_deref()))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn read_issue_summary_conn(
    conn: &rusqlite::Connection,
    repo_key: &str,
    issue_num_i64: i64,
) -> Result<Option<crate::model::IssueSummary>, rusqlite::Error> {
    Ok(read_issue_summary_and_body_conn(conn, repo_key, issue_num_i64)?.map(|(s, _)| s))
}

#[allow(clippy::type_complexity)]
fn read_issue_full_row_conn(
    conn: &rusqlite::Connection,
    repo_key: &str,
    issue_num_i64: i64,
) -> Result<
    Option<(
        crate::model::IssueSummary,
        String,
        Option<String>,
        Option<i64>,
        Option<String>,
        Option<i64>,
    )>,
    rusqlite::Error,
> {
    let row = conn
        .query_row(
            "SELECT issue_number, title, body_sanitized, state, state_reason, author_login,
                    author_avatar_url, assignees_json, milestone_title, comments_count,
                    html_url, has_plan, plan_source, plan_comment_id, plan_markdown,
                    plan_updated_at_ms, created_at_ms, updated_at_ms, closed_at_ms
             FROM github_issues
             WHERE repo_key = ?1 AND issue_number = ?2;",
            params![repo_key, issue_num_i64],
            |r| {
                let num_i64: i64 = r.get(0)?;
                let title: String = r.get(1)?;
                let body: String = r.get(2)?;
                let state_s: String = r.get(3)?;
                let state_reason: Option<String> = r.get(4)?;
                let author_login: String = r.get(5)?;
                let author_avatar_url: Option<String> = r.get(6)?;
                let assignees_json: String = r.get(7)?;
                let milestone_title: Option<String> = r.get(8)?;
                let comments_count_i64: i64 = r.get(9)?;
                let html_url: String = r.get(10)?;
                let has_plan_i64: i64 = r.get(11)?;
                let plan_source: Option<String> = r.get(12)?;
                let plan_comment_id: Option<i64> = r.get(13)?;
                let plan_markdown: Option<String> = r.get(14)?;
                let plan_updated_at_ms: Option<i64> = r.get(15)?;
                let created_at_ms: i64 = r.get(16)?;
                let updated_at_ms: i64 = r.get(17)?;
                let closed_at_ms: Option<i64> = r.get(18)?;
                Ok((
                    num_i64,
                    title,
                    body,
                    state_s,
                    state_reason,
                    author_login,
                    author_avatar_url,
                    assignees_json,
                    milestone_title,
                    comments_count_i64,
                    html_url,
                    has_plan_i64,
                    plan_source,
                    plan_comment_id,
                    plan_markdown,
                    plan_updated_at_ms,
                    created_at_ms,
                    updated_at_ms,
                    closed_at_ms,
                ))
            },
        )
        .optional()?;

    let Some((
        num_i64,
        title,
        body,
        state_s,
        state_reason,
        author_login,
        author_avatar_url,
        assignees_json,
        milestone_title,
        comments_count_i64,
        html_url,
        has_plan_i64,
        plan_source,
        plan_comment_id,
        plan_markdown,
        plan_updated_at_ms,
        created_at_ms,
        updated_at_ms,
        closed_at_ms,
    )) = row
    else {
        return Ok(None);
    };

    let labels = read_issue_labels_conn(conn, repo_key, num_i64)?;
    let (active_state_label, active_closure_label, category_labels) =
        extract_canonical_facets(&labels);
    let assignees: Vec<String> = serde_json::from_str(&assignees_json).unwrap_or_default();

    let summary = crate::model::IssueSummary {
        repo_key: repo_key.to_string(),
        number: u64::try_from(num_i64).unwrap_or(0),
        title,
        state: IssueState::from_db_str(&state_s),
        state_reason,
        author_login,
        author_avatar_url,
        assignees,
        milestone_title,
        labels,
        active_state_label,
        active_closure_label,
        category_labels,
        comments_count: u32::try_from(comments_count_i64).unwrap_or(0),
        has_plan: has_plan_i64 != 0,
        html_url,
        created_at_ms,
        updated_at_ms,
        closed_at_ms,
    };

    Ok(Some((
        summary,
        body,
        plan_source,
        plan_comment_id,
        plan_markdown,
        plan_updated_at_ms,
    )))
}

fn read_issue_summary_and_body_conn(
    conn: &rusqlite::Connection,
    repo_key: &str,
    issue_num_i64: i64,
) -> Result<Option<(crate::model::IssueSummary, String)>, rusqlite::Error> {
    Ok(read_issue_full_row_conn(conn, repo_key, issue_num_i64)?
        .map(|(summary, body, _, _, _, _)| (summary, body)))
}

fn matches_issue_filter(
    summary: &crate::model::IssueSummary,
    body_text: &str,
    filter: &IssueFilter,
) -> bool {
    if let Some(expected_plan) = filter.has_plan {
        if summary.has_plan != expected_plan {
            return false;
        }
    }

    if let Some(ref author) = filter.author {
        let trimmed = author.trim();
        if !trimmed.is_empty() && !summary.author_login.eq_ignore_ascii_case(trimmed) {
            return false;
        }
    }

    if let Some(ref assignee) = filter.assignee {
        let trimmed = assignee.trim();
        if !trimmed.is_empty()
            && !summary
                .assignees
                .iter()
                .any(|a| a.eq_ignore_ascii_case(trimmed))
        {
            return false;
        }
    }

    if !filter.state_labels.is_empty() {
        let Some(ref active) = summary.active_state_label else {
            return false;
        };
        if !filter
            .state_labels
            .iter()
            .any(|l| l.trim().eq_ignore_ascii_case(active))
        {
            return false;
        }
    }

    if !filter.closure_labels.is_empty() {
        let Some(ref active) = summary.active_closure_label else {
            return false;
        };
        if !filter
            .closure_labels
            .iter()
            .any(|l| l.trim().eq_ignore_ascii_case(active))
        {
            return false;
        }
    }

    if !filter.category_labels.is_empty() {
        let has_any = filter.category_labels.iter().any(|wanted| {
            summary
                .category_labels
                .iter()
                .any(|cat| cat.eq_ignore_ascii_case(wanted.trim()))
        });
        if !has_any {
            return false;
        }
    }

    if !filter.custom_labels.is_empty() {
        let has_any = filter.custom_labels.iter().any(|wanted| {
            summary
                .labels
                .iter()
                .any(|lb| lb.name.eq_ignore_ascii_case(wanted.trim()))
        });
        if !has_any {
            return false;
        }
    }

    if let Some(ref search) = filter.search_text {
        let q = search.trim().to_ascii_lowercase();
        if !q.is_empty() {
            let num_query = q.strip_prefix('#').unwrap_or(&q);
            let num_str = summary.number.to_string();
            let title_match = summary.title.to_ascii_lowercase().contains(&q);
            let body_match = body_text.to_ascii_lowercase().contains(&q);
            let num_match = num_str.contains(num_query);
            if !title_match && !body_match && !num_match {
                return false;
            }
        }
    }

    let _ = classify_label_group;
    true
}

fn read_sync_meta_conn(
    conn: &rusqlite::Connection,
    repo_key: &str,
) -> Result<
    (
        GithubTransportKind,
        Option<i64>,
        u32,
        Option<RateLimitSnapshot>,
    ),
    rusqlite::Error,
> {
    let row = conn
        .query_row(
            "SELECT transport_used, last_success_at_ms, missing_labels_count,
                    rate_limit_limit, rate_limit_remaining, rate_limit_used,
                    rate_limit_reset_ms, retry_after_until_ms
             FROM github_sync_state WHERE repo_key = ?1;",
            params![repo_key],
            |r| {
                let tr: Option<String> = r.get(0)?;
                let succ: Option<i64> = r.get(1)?;
                let miss: i64 = r.get(2)?;
                let lim: Option<i64> = r.get(3)?;
                let rem: Option<i64> = r.get(4)?;
                let used: Option<i64> = r.get(5)?;
                let reset: Option<i64> = r.get(6)?;
                let retry: Option<i64> = r.get(7)?;
                Ok((tr, succ, miss, lim, rem, used, reset, retry))
            },
        )
        .optional()?;

    let Some((tr, succ, miss, lim, rem, used, reset, retry)) = row else {
        return Ok((GithubTransportKind::Cache, None, 0, None));
    };

    let transport = match tr.as_deref() {
        Some("gh_cli") => GithubTransportKind::GhCli,
        Some("rest") => GithubTransportKind::Rest,
        _ => GithubTransportKind::Cache,
    };

    let rl = match (lim, rem, reset) {
        (Some(l), Some(r), Some(rst)) => {
            let limit = u32::try_from(l.max(0)).unwrap_or(0);
            let remaining = u32::try_from(r.max(0)).unwrap_or(0);
            let used_u32 = used
                .and_then(|u| u32::try_from(u.max(0)).ok())
                .unwrap_or_else(|| limit.saturating_sub(remaining));
            Some(RateLimitSnapshot {
                limit,
                remaining,
                used: used_u32,
                reset_at_ms: rst,
                retry_after_until_ms: retry,
                low_remaining: limit > 0 && remaining.saturating_mul(10) < limit,
            })
        }
        _ => None,
    };

    Ok((transport, succ, u32::try_from(miss.max(0)).unwrap_or(0), rl))
}

fn map_bug_cycle_row(row: &rusqlite::Row<'_>) -> Result<BugCycleRunState, rusqlite::Error> {
    let cycle_id: String = row.get(0)?;
    let project_id_str: String = row.get(1)?;
    let repo_key: String = row.get(2)?;
    let status: String = row.get(3)?;
    let total_i64: i64 = row.get(4)?;
    let completed_i64: i64 = row.get(5)?;
    let failed_i64: i64 = row.get(6)?;
    let current_issue_i64: Option<i64> = row.get(7)?;
    let current_stage: Option<String> = row.get(8)?;
    let nums_json: String = row.get(9)?;
    let skipped_json: String = row.get(10)?;
    let error_message: Option<String> = row.get(11)?;
    let started_at_ms: i64 = row.get(12)?;
    let updated_at_ms: i64 = row.get(13)?;
    let finished_at_ms: Option<i64> = row.get(14)?;

    let issue_numbers: Vec<u64> = serde_json::from_str(&nums_json).unwrap_or_default();
    let skipped: Vec<BugCycleSkippedItem> = serde_json::from_str(&skipped_json).unwrap_or_default();

    Ok(BugCycleRunState {
        cycle_id,
        project_id: project_id_str.parse::<ProjectId>().unwrap_or_default(),
        repo_key,
        status,
        total_issues: u32::try_from(total_i64.max(0)).unwrap_or(0),
        completed_issues: u32::try_from(completed_i64.max(0)).unwrap_or(0),
        failed_issues: u32::try_from(failed_i64.max(0)).unwrap_or(0),
        current_issue: current_issue_i64.and_then(|n| u64::try_from(n).ok()),
        current_stage,
        issue_numbers,
        skipped,
        error_message,
        started_at_ms,
        updated_at_ms,
        finished_at_ms,
    })
}
