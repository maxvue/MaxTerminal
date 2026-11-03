//! Coordenador de sincronização single-flight por `repo_key`, cache ETag e stale-while-revalidate.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mt_core::ids::ProjectId;
use parking_lot::Mutex;
use tokio::sync::Mutex as AsyncMutex;

use crate::cache::GithubCacheRepo;
use crate::client::{GithubRemoteClient, RemoteFetchOutcome};
use crate::error::GithubResult;
use crate::events::{
    GithubIssuesChangedPayload, GithubLabelsSyncedPayload, GithubRateLimitPayload,
};
use crate::labels::{canonical_labels_catalog, find_missing_canonical_labels};
use crate::model::{
    GithubRepoIdentity, GithubTransportKind, IssueDetail, IssueFilter, IssueListPage,
    IssueStateFilter, LabelEnsureReport, RepoLabel,
};
use crate::ports::GithubEventSink;

/// Coordenador de sincronização GitHub com deduplicação single-flight por repositório.
pub struct GithubSyncCoordinator {
    cache: GithubCacheRepo,
    client: Arc<dyn GithubRemoteClient>,
    events: Arc<dyn GithubEventSink>,
    repo_locks: Mutex<HashMap<String, Arc<AsyncMutex<()>>>>,
}

impl GithubSyncCoordinator {
    /// Cria um novo `GithubSyncCoordinator`.
    #[must_use]
    pub fn new(
        cache: GithubCacheRepo,
        client: Arc<dyn GithubRemoteClient>,
        events: Arc<dyn GithubEventSink>,
    ) -> Self {
        Self {
            cache,
            client,
            events,
            repo_locks: Mutex::new(HashMap::new()),
        }
    }

    fn lock_for_repo(&self, repo_key: &str) -> Arc<AsyncMutex<()>> {
        let mut map = self.repo_locks.lock();
        map.entry(repo_key.to_string())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone()
    }

    /// Executa sincronização completa de issues e labels para o repositório (coalescida por `repo_key`).
    pub async fn refresh_repo(
        &self,
        project_id: ProjectId,
        repo: &GithubRepoIdentity,
        state_filter: IssueStateFilter,
        force_ignore_etag: bool,
    ) -> GithubResult<IssueListPage> {
        let repo_lock = self.lock_for_repo(&repo.repo_key);
        let _guard = repo_lock.lock().await;

        let now_ms = current_epoch_ms();
        let cache_key = format!("issues:{}:{:?}", repo.repo_key, state_filter);
        let cached_etag = if force_ignore_etag {
            None
        } else {
            self.cache.get_cached_etag(&cache_key).await?
        };

        match self
            .client
            .fetch_issues(repo, state_filter, 100, 3, cached_etag.as_deref())
            .await
        {
            Ok(RemoteFetchOutcome::Fresh {
                data,
                etag,
                transport,
                rate_limit,
            }) => {
                let open_count = self
                    .cache
                    .upsert_issues_batch(repo, &data, state_filter, now_ms)
                    .await?;
                self.cache
                    .save_http_cache(&cache_key, &repo.repo_key, etag.as_deref(), 200, now_ms)
                    .await?;
                self.cache
                    .update_sync_state(&repo.repo_key, transport, rate_limit.as_ref(), None, now_ms)
                    .await?;

                if let Some(ref rl) = rate_limit {
                    self.events.emit_rate_limit(GithubRateLimitPayload {
                        repo_key: Some(repo.repo_key.clone()),
                        rate_limit: rl.clone(),
                    });
                }

                self.events.emit_issues_changed(GithubIssuesChangedPayload {
                    project_id,
                    repo_key: repo.repo_key.clone(),
                    open_issues_count: open_count,
                    transport_used: transport,
                    synced_at_ms: now_ms,
                });

                self.cache
                    .list_issues(
                        &repo.repo_key,
                        &IssueFilter {
                            state: state_filter,
                            ..Default::default()
                        },
                        false,
                    )
                    .await
            }
            Ok(RemoteFetchOutcome::NotModified {
                transport,
                rate_limit,
            }) => {
                self.cache
                    .save_http_cache(
                        &cache_key,
                        &repo.repo_key,
                        cached_etag.as_deref(),
                        304,
                        now_ms,
                    )
                    .await?;
                self.cache
                    .update_sync_state(&repo.repo_key, transport, rate_limit.as_ref(), None, now_ms)
                    .await?;

                if let Some(ref rl) = rate_limit {
                    self.events.emit_rate_limit(GithubRateLimitPayload {
                        repo_key: Some(repo.repo_key.clone()),
                        rate_limit: rl.clone(),
                    });
                }

                self.cache
                    .list_issues(
                        &repo.repo_key,
                        &IssueFilter {
                            state: state_filter,
                            ..Default::default()
                        },
                        false,
                    )
                    .await
            }
            Err(err) => {
                let _ = self
                    .cache
                    .update_sync_state(
                        &repo.repo_key,
                        GithubTransportKind::Cache,
                        None,
                        Some((&format!("{:?}", err.kind), &err.message)),
                        now_ms,
                    )
                    .await;
                // Se já temos dados em cache, retornamos em modo degradado (`stale = true`)
                let cached_page = self
                    .cache
                    .list_issues(
                        &repo.repo_key,
                        &IssueFilter {
                            state: state_filter,
                            ..Default::default()
                        },
                        true,
                    )
                    .await?;
                if !cached_page.items.is_empty() || cached_page.last_success_at_ms.is_some() {
                    Ok(cached_page)
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Carrega o detalhe completo de uma issue + comentários, sincronizando com o GitHub quando possível.
    pub async fn sync_issue_detail(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
        force_remote: bool,
    ) -> GithubResult<IssueDetail> {
        if !force_remote {
            if let Some(cached) = self
                .cache
                .get_issue_detail(&repo.repo_key, issue_number, true)
                .await?
            {
                if cached.comments.len() as u32 >= cached.summary.comments_count {
                    return Ok(cached);
                }
            }
        }

        let now_ms = current_epoch_ms();
        match self.client.fetch_issue_detail(repo, issue_number).await {
            Ok(bundle) => {
                self.cache
                    .upsert_issue_detail(
                        repo,
                        &bundle.issue,
                        &bundle.comments,
                        bundle.comments_complete,
                        now_ms,
                    )
                    .await
            }
            Err(err) => {
                if let Some(cached) = self
                    .cache
                    .get_issue_detail(&repo.repo_key, issue_number, false)
                    .await?
                {
                    Ok(cached)
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Sincroniza a lista de labels do repositório com o cache SQLite.
    pub async fn sync_repo_labels(
        &self,
        project_id: ProjectId,
        repo: &GithubRepoIdentity,
        force_remote: bool,
    ) -> GithubResult<Vec<RepoLabel>> {
        let now_ms = current_epoch_ms();
        let cache_key = format!("labels:{}", repo.repo_key);
        let cached_etag = if force_remote {
            None
        } else {
            self.cache.get_cached_etag(&cache_key).await?
        };

        match self
            .client
            .fetch_repo_labels(repo, cached_etag.as_deref())
            .await
        {
            Ok(RemoteFetchOutcome::Fresh { data, etag, .. }) => {
                let missing = self.cache.upsert_repo_labels(repo, &data, now_ms).await?;
                self.cache
                    .save_http_cache(&cache_key, &repo.repo_key, etag.as_deref(), 200, now_ms)
                    .await?;
                self.events.emit_labels_synced(GithubLabelsSyncedPayload {
                    project_id,
                    repo_key: repo.repo_key.clone(),
                    missing_canonical_count: missing,
                    total_labels: u32::try_from(data.len()).unwrap_or(0),
                });
                Ok(data)
            }
            Ok(RemoteFetchOutcome::NotModified { .. }) => {
                self.cache.list_repo_labels(&repo.repo_key).await
            }
            Err(err) => {
                let cached = self.cache.list_repo_labels(&repo.repo_key).await?;
                if !cached.is_empty() {
                    Ok(cached)
                } else {
                    Err(err)
                }
            }
        }
    }

    /// Garante que as 24 labels canônicas do workflow MaxCode existam no repositório (`dry_run = true` apenas inspeciona).
    pub async fn ensure_canonical_labels(
        &self,
        project_id: ProjectId,
        repo: &GithubRepoIdentity,
        dry_run: bool,
    ) -> GithubResult<LabelEnsureReport> {
        let current = self.sync_repo_labels(project_id, repo, true).await?;
        let missing_specs = find_missing_canonical_labels(&current);

        let already_present: Vec<String> = canonical_labels_catalog()
            .iter()
            .filter(|spec| {
                !missing_specs
                    .iter()
                    .any(|m| m.name.eq_ignore_ascii_case(&spec.name))
            })
            .map(|spec| spec.name.clone())
            .collect();

        if dry_run || missing_specs.is_empty() {
            return Ok(LabelEnsureReport {
                repo_key: repo.repo_key.clone(),
                created: Vec::new(),
                already_present,
                missing: missing_specs.into_iter().map(|s| s.name).collect(),
                repo_labels: current,
            });
        }

        let mut created = Vec::new();
        let mut still_missing = Vec::new();
        let mut all_labels = current;

        for spec in missing_specs {
            match self
                .client
                .create_repo_label(repo, &spec.name, &spec.color_hex, &spec.description)
                .await
            {
                Ok(created_label) => {
                    created.push(spec.name.clone());
                    all_labels.push(created_label);
                }
                Err(_) => {
                    still_missing.push(spec.name);
                }
            }
        }

        let now_ms = current_epoch_ms();
        let missing_cnt = self
            .cache
            .upsert_repo_labels(repo, &all_labels, now_ms)
            .await?;

        self.events.emit_labels_synced(GithubLabelsSyncedPayload {
            project_id,
            repo_key: repo.repo_key.clone(),
            missing_canonical_count: missing_cnt,
            total_labels: u32::try_from(all_labels.len()).unwrap_or(0),
        });

        Ok(LabelEnsureReport {
            repo_key: repo.repo_key.clone(),
            created,
            already_present,
            missing: still_missing,
            repo_labels: all_labels,
        })
    }
}

fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}
