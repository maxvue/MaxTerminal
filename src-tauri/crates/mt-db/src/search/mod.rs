//! Motor de Busca Global FTS5 (`SearchEngine`), paginação por snapshot materializado,
//! cancelamento determinístico e reconstrução de índices.

pub mod cancel;
pub mod cursor;
pub mod error;
pub mod model;
pub mod projection;
pub mod query;
pub mod rank;
pub mod snapshot;
pub mod sources;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rusqlite::Connection;
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::Db;
use crate::time::now_unix_ms;

pub use cancel::{CancelRegistry, RegisteredSearchGuard, RequestKey, RequestState};
pub use cursor::{CursorSigner, SearchCursorPayload, compute_query_fingerprint};
pub use error::{SearchError, codes as search_error_codes};
pub use model::{
    CancelOutcome, ExitFilter, HighlightSegment, RebuildJobHandle, SearchFilters, SearchHit,
    SearchIndexProgressEvent, SearchIndexStat, SearchInvalidatedEvent, SearchRequest,
    SearchResponse, SearchSection, SearchSort, SearchSourceKind, SearchTarget,
};
pub use projection::{
    ProjectionSource, SEARCH_METADATA_FTS, SEARCH_METADATA_FTS_SPEC, SanitizedProjection,
    backfill_all_projections, delete_projection, rebuild_search_fts, sync_project_by_id,
    sync_session_by_id, sync_template_by_id, upsert_projection,
};
pub use query::{MAX_QUERY_BYTES, MAX_QUERY_CHARS, ParsedQuery, highlight_tokens_safe};
pub use rank::{RankedHit, sort_ranked_hits};
pub use snapshot::{MaterializedSection, MaterializedSnapshot, SearchOwner, SnapshotCache};
pub use sources::{
    CollectedSection, SearchContext, SearchSource, command_blocks::CommandBlocksSource,
    groups::GroupsSource, issues::IssuesSource, projects::ProjectsSource,
    scrollback::ScrollbackSource, sessions::SessionsSource, templates::TemplatesSource,
};

static NONCE_SEQ: AtomicU64 = AtomicU64::new(1);

/// Configuração operacional do `SearchEngine`.
#[derive(Debug, Clone)]
pub struct SearchConfig {
    /// Limite máximo de hits somados entre todas as seções em um snapshot (`10_000`).
    pub max_total_hits: usize,
    /// Orçamento máximo de memória por janela para snapshots (`16 MiB`).
    pub max_snapshot_bytes: usize,
    /// Tamanho padrão de página por seção (`25`).
    pub default_page_size: u16,
    /// Tamanho máximo permitido de página por seção (`100`).
    pub max_page_size: u16,
    /// Tempo de vida de cada snapshot em milissegundos (`120_000`).
    pub snapshot_ttl_ms: i64,
    /// Máximo de snapshots mantidos simultaneamente por `SearchOwner` (`2`).
    pub max_snapshots_per_owner: usize,
    /// Máximo de buscas novas concorrentes no semáforo (`4`).
    pub max_concurrent_searches: usize,
    /// Tempo limite de execução SQLite por busca (`1_500 ms`).
    pub query_deadline_ms: u64,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            max_total_hits: 10_000,
            max_snapshot_bytes: 16 * 1024 * 1024,
            default_page_size: 25,
            max_page_size: 100,
            snapshot_ttl_ms: 120_000,
            max_snapshots_per_owner: 2,
            max_concurrent_searches: 4,
            query_deadline_ms: 1_500,
        }
    }
}

/// Fachada thread-safe do motor de Busca Global FTS5.
#[derive(Clone)]
pub struct SearchEngine {
    db: Db,
    config: SearchConfig,
    sources: Arc<Vec<Box<dyn SearchSource>>>,
    cancel_registry: Arc<CancelRegistry>,
    snapshot_cache: Arc<SnapshotCache>,
    cursor_signer: Arc<CursorSigner>,
    semaphore: Arc<Semaphore>,
    active_rebuild_job: Arc<Mutex<Option<String>>>,
}

impl std::fmt::Debug for SearchEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchEngine")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl SearchEngine {
    /// Cria uma nova instância do `SearchEngine` sobre a fachada `Db`.
    #[must_use]
    pub fn new(db: Db, config: SearchConfig) -> Self {
        let sources: Vec<Box<dyn SearchSource>> = vec![
            Box::new(CommandBlocksSource),
            Box::new(ScrollbackSource),
            Box::new(SessionsSource),
            Box::new(ProjectsSource),
            Box::new(TemplatesSource),
            Box::new(GroupsSource),
            Box::new(IssuesSource),
        ];

        let snapshot_cache = Arc::new(SnapshotCache::new(
            config.max_snapshots_per_owner,
            config.max_snapshot_bytes,
            config.snapshot_ttl_ms,
        ));

        Self {
            db,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent_searches.max(1))),
            config,
            sources: Arc::new(sources),
            cancel_registry: CancelRegistry::new(),
            snapshot_cache,
            cursor_signer: Arc::new(CursorSigner::new_random()),
            active_rebuild_job: Arc::new(Mutex::new(None)),
        }
    }

    /// Retorna referência ao banco `Db` subjacente.
    #[must_use]
    pub const fn db(&self) -> &Db {
        &self.db
    }

    /// Retorna a configuração ativa.
    #[must_use]
    pub const fn config(&self) -> &SearchConfig {
        &self.config
    }

    /// Executa uma consulta global ou pagina uma seção existente via cursor autenticado.
    #[allow(clippy::too_many_lines)]
    pub async fn search(
        &self,
        owner: SearchOwner,
        request: SearchRequest,
    ) -> Result<SearchResponse, SearchError> {
        validate_filters(&request.filters)?;

        let effective_page_size = if request.page_size == 0 {
            self.config.default_page_size
        } else {
            request.page_size.min(self.config.max_page_size)
        } as usize;

        if let Some(ref cursor_token) = request.cursor {
            return self.paginate_from_cursor(&owner, &request, cursor_token, effective_page_size);
        }

        let guard = self
            .cancel_registry
            .register(&owner, &request.request_id, request.generation);
        if guard.is_cancelled() {
            return Err(SearchError::Cancelled);
        }

        let nonce = NONCE_SEQ.fetch_add(1, Ordering::Relaxed);
        let Some(parsed_query) = ParsedQuery::parse(&request.query, nonce)? else {
            let now_ms = now_unix_ms();
            let fp =
                compute_query_fingerprint(request.query.trim(), &request.filters, request.sort);
            let snap = self
                .snapshot_cache
                .insert(owner, fp, now_ms, None, Vec::new());
            return Ok(SearchResponse {
                request_id: request.request_id,
                generation: request.generation,
                snapshot_id: snap.snapshot_id,
                expires_at_ms: snap.expires_at_ms,
                sections: Vec::new(),
                indexed_through_ms: None,
            });
        };

        let query_fp =
            compute_query_fingerprint(&parsed_query.raw_trimmed, &request.filters, request.sort);

        let deadline_dur = Duration::from_millis(self.config.query_deadline_ms.max(50));
        let Ok(Ok(_permit)) = tokio::time::timeout(
            Duration::from_millis(250),
            self.semaphore.clone().acquire_owned(),
        )
        .await
        else {
            return Err(SearchError::Busy);
        };

        if guard.is_cancelled() {
            return Err(SearchError::Cancelled);
        }

        let db = self.db.clone();
        let sources = Arc::clone(&self.sources);
        let max_hits = self.config.max_total_hits;
        let max_bytes = self.config.max_snapshot_bytes;
        let filters = request.filters.clone();
        let sort = request.sort;

        let collect_outcome = tokio::task::spawn_blocking(move || {
            db.read_blocking(|conn| {
                let interrupt_handle = conn.get_interrupt_handle();
                if !guard.attach_interrupt_handle(interrupt_handle) {
                    return Ok(Err(SearchError::Cancelled));
                }

                let deadline_instant = Instant::now() + deadline_dur;
                let cancelled_flag = guard.cancelled_flag();

                let collect_res = execute_sources_in_snapshot_tx(
                    conn,
                    &sources,
                    &parsed_query,
                    &filters,
                    sort,
                    max_hits,
                    max_bytes,
                    deadline_instant,
                    &cancelled_flag,
                );

                // Desvincula InterruptHandle ANTES de devolver a conexão ao pool
                guard.detach_interrupt_handle();

                match collect_res {
                    Ok(pair) => {
                        if guard.is_cancelled() {
                            Ok(Err(SearchError::Cancelled))
                        } else if Instant::now() >= deadline_instant {
                            Ok(Err(SearchError::Timeout))
                        } else {
                            Ok(Ok(pair))
                        }
                    }
                    Err(other) => Ok(Err(other)),
                }
            })
        })
        .await
        .map_err(|_| SearchError::Busy)??;

        let (materialized_sections, indexed_through_ms) = collect_outcome?;

        let now_ms = now_unix_ms();
        let snapshot = self.snapshot_cache.insert(
            owner.clone(),
            query_fp.clone(),
            now_ms,
            indexed_through_ms,
            materialized_sections,
        );

        let mut response_sections = Vec::with_capacity(snapshot.sections.len());
        for mat_sec in &snapshot.sections {
            if mat_sec.items.is_empty() && !mat_sec.truncated {
                continue;
            }
            let end = effective_page_size.min(mat_sec.items.len());
            let page_items = mat_sec.items[..end].to_vec();
            let next_cursor = if end < mat_sec.items.len() {
                Some(self.cursor_signer.encode(&SearchCursorPayload {
                    v: 1,
                    owner: owner.window_label.clone(),
                    snapshot_id: snapshot.snapshot_id.clone(),
                    source: mat_sec.source,
                    next_ordinal: end,
                    query_fingerprint: query_fp.clone(),
                    expires_at_ms: snapshot.expires_at_ms,
                })?)
            } else {
                None
            };

            response_sections.push(SearchSection {
                source: mat_sec.source,
                items: page_items,
                next_cursor,
                truncated: mat_sec.truncated,
            });
        }

        Ok(SearchResponse {
            request_id: request.request_id,
            generation: request.generation,
            snapshot_id: snapshot.snapshot_id,
            expires_at_ms: snapshot.expires_at_ms,
            sections: response_sections,
            indexed_through_ms: snapshot.indexed_through_ms,
        })
    }

    fn paginate_from_cursor(
        &self,
        owner: &SearchOwner,
        request: &SearchRequest,
        cursor_token: &str,
        page_size: usize,
    ) -> Result<SearchResponse, SearchError> {
        let payload = self.cursor_signer.decode(cursor_token)?;
        if payload.owner != owner.window_label {
            return Err(SearchError::InvalidCursor);
        }

        let nonce = NONCE_SEQ.fetch_add(1, Ordering::Relaxed);
        let parsed = ParsedQuery::parse(&request.query, nonce)?;
        let raw_trimmed = parsed
            .as_ref()
            .map_or_else(|| request.query.trim(), |p| p.raw_trimmed.as_str());
        let expected_fp = compute_query_fingerprint(raw_trimmed, &request.filters, request.sort);

        if payload.query_fingerprint != expected_fp {
            return Err(SearchError::InvalidCursor);
        }

        let now_ms = now_unix_ms();
        let snapshot = self
            .snapshot_cache
            .get(owner, &payload.snapshot_id, now_ms)?;

        let mat_sec = snapshot
            .section(payload.source)
            .ok_or(SearchError::InvalidCursor)?;

        if payload.next_ordinal > mat_sec.items.len() {
            return Err(SearchError::InvalidCursor);
        }

        let start = payload.next_ordinal;
        let end = start.saturating_add(page_size).min(mat_sec.items.len());
        let page_items = mat_sec.items[start..end].to_vec();
        let sec_source = mat_sec.source;
        let sec_truncated = mat_sec.truncated;
        let next_cursor = if end < mat_sec.items.len() {
            Some(self.cursor_signer.encode(&SearchCursorPayload {
                v: 1,
                owner: owner.window_label.clone(),
                snapshot_id: snapshot.snapshot_id.clone(),
                source: sec_source,
                next_ordinal: end,
                query_fingerprint: expected_fp,
                expires_at_ms: snapshot.expires_at_ms,
            })?)
        } else {
            None
        };

        Ok(SearchResponse {
            request_id: request.request_id.clone(),
            generation: request.generation,
            snapshot_id: snapshot.snapshot_id,
            expires_at_ms: snapshot.expires_at_ms,
            sections: vec![SearchSection {
                source: sec_source,
                items: page_items,
                next_cursor,
                truncated: sec_truncated,
            }],
            indexed_through_ms: snapshot.indexed_through_ms,
        })
    }

    /// Cancela uma busca em voo para `(owner, request_id)`.
    #[must_use]
    pub fn cancel(&self, owner: &SearchOwner, request_id: &str) -> CancelOutcome {
        self.cancel_registry.cancel(owner, request_id)
    }

    /// Libera explicitamente um snapshot materializado de `owner`.
    pub fn release(&self, owner: &SearchOwner, snapshot_id: &str) {
        let _ = self.snapshot_cache.release(owner, snapshot_id);
    }

    /// Invalida todos os snapshots de todas as janelas e retorna pares `(window_label, snapshot_id)`.
    #[must_use]
    pub fn invalidate_all(&self, _reason: &'static str) -> Vec<(String, String)> {
        self.snapshot_cache.invalidate_all()
    }

    /// Retorna estatísticas de todas as 9 fontes de busca na ordem canônica.
    pub async fn index_stats(&self) -> Result<Vec<SearchIndexStat>, SearchError> {
        let active_job = self.active_rebuild_job.lock().clone();
        self.db
            .read(move |conn| {
                let mut stats = Vec::with_capacity(SearchSourceKind::ALL_ORDERED.len());
                for kind in SearchSourceKind::ALL_ORDERED {
                    let (ready, row_count, updated_at_ms) = query_source_stat(conn, kind)?;
                    stats.push(SearchIndexStat {
                        source: kind,
                        ready,
                        row_count,
                        updated_at_ms,
                        active_job_id: active_job.clone(),
                    });
                }
                Ok(stats)
            })
            .await
            .map_err(SearchError::from)
    }

    /// Reconstrói os índices FTS5 das fontes solicitadas (ou todas quando `sources` estiver vazio),
    /// emitindo eventos de progresso no canal `progress_tx`.
    pub async fn rebuild_indexes(
        &self,
        sources: Vec<SearchSourceKind>,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<SearchIndexProgressEvent>>,
    ) -> Result<RebuildJobHandle, SearchError> {
        let targets: Vec<SearchSourceKind> = if sources.is_empty() {
            SearchSourceKind::ALL_ORDERED.to_vec()
        } else {
            let mut list = Vec::new();
            for kind in SearchSourceKind::ALL_ORDERED {
                if sources.contains(&kind) {
                    list.push(kind);
                }
            }
            list
        };

        let job_id = format!("rebuild_{}", Uuid::new_v4().simple());
        *self.active_rebuild_job.lock() = Some(job_id.clone());

        let db = self.db.clone();
        let fts_reg = Arc::clone(self.db.fts_registry());
        let active_ref = Arc::clone(&self.active_rebuild_job);
        let cache_ref = Arc::clone(&self.snapshot_cache);
        let job_id_bg = job_id.clone();

        let rebuild_res = db
            .write_sized("search_index_rebuild", 4096, move |tx| {
                let total = u32::try_from(targets.len()).unwrap_or(1);
                let needs_metadata = targets
                    .iter()
                    .any(|k| !matches!(k, SearchSourceKind::Commands | SearchSourceKind::Output));
                if needs_metadata {
                    backfill_all_projections(tx)?;
                }

                let mut rebuilt_commands = false;
                let mut rebuilt_output = false;
                let mut rebuilt_metadata = false;

                for (idx, &kind) in targets.iter().enumerate() {
                    match kind {
                        SearchSourceKind::Commands if !rebuilt_commands => {
                            rebuild_search_fts(tx, &fts_reg, "command_blocks_fts")?;
                            rebuilt_commands = true;
                        }
                        SearchSourceKind::Output if !rebuilt_output => {
                            rebuild_search_fts(tx, &fts_reg, "session_scrollback_fts")?;
                            rebuilt_output = true;
                        }
                        _ if !rebuilt_metadata => {
                            rebuild_search_fts(tx, &fts_reg, "search_metadata_fts")?;
                            rebuilt_metadata = true;
                        }
                        _ => {}
                    }

                    let completed = u32::try_from(idx + 1).unwrap_or(total);
                    if let Some(ref tx_chan) = progress_tx {
                        let _ = tx_chan.send(SearchIndexProgressEvent {
                            job_id: job_id_bg.clone(),
                            source: kind,
                            completed_sources: completed,
                            total_sources: total,
                            done: completed == total,
                        });
                    }
                }
                Ok(())
            })
            .await;

        *active_ref.lock() = None;
        let _ = cache_ref.invalidate_all();
        rebuild_res.map_err(SearchError::from)?;

        Ok(RebuildJobHandle { job_id })
    }
}

fn validate_filters(filters: &SearchFilters) -> Result<(), SearchError> {
    if let (Some(from_ms), Some(until_ms)) = (filters.from_ms, filters.until_ms)
        && from_ms > until_ms
    {
        return Err(SearchError::InvalidFilters {
            reason: "o limite inicial (fromMs) não pode ser maior que o limite final (untilMs)",
        });
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn execute_sources_in_snapshot_tx(
    conn: &Connection,
    sources: &[Box<dyn SearchSource>],
    query: &ParsedQuery,
    filters: &SearchFilters,
    sort: SearchSort,
    max_total_hits: usize,
    max_snapshot_bytes: usize,
    deadline_instant: Instant,
    cancelled_flag: &AtomicBool,
) -> Result<(Vec<MaterializedSection>, Option<i64>), SearchError> {
    conn.execute_batch("BEGIN DEFERRED;")
        .map_err(sources::map_sqlite_err)?;

    let result = (|| -> Result<(Vec<MaterializedSection>, Option<i64>), SearchError> {
        let mut ctx = SearchContext {
            query,
            filters,
            sort,
            remaining_hits: max_total_hits,
            remaining_bytes: max_snapshot_bytes,
        };

        let mut sections = Vec::new();
        let mut indexed_through_ms: Option<i64> = None;

        for source in sources {
            if cancelled_flag.load(Ordering::Acquire) {
                return Err(SearchError::Cancelled);
            }
            if Instant::now() >= deadline_instant {
                return Err(SearchError::Timeout);
            }
            let collected = source.collect(conn, &ctx)?;
            for sec in collected {
                ctx.remaining_hits = ctx.remaining_hits.saturating_sub(sec.items.len());
                ctx.remaining_bytes = ctx.remaining_bytes.saturating_sub(sec.approx_bytes);
                if let Some(ts) = sec.max_occurred_at_ms {
                    indexed_through_ms = Some(indexed_through_ms.map_or(ts, |m| m.max(ts)));
                }
                sections.push(MaterializedSection {
                    source: sec.source,
                    items: sec.items,
                    truncated: sec.truncated,
                });
            }
        }

        // Garante ordem canônica estrita das seções
        sections.sort_by_key(|s| s.source);
        Ok((sections, indexed_through_ms))
    })();

    let _ = conn.execute_batch("COMMIT;");
    result
}

fn query_source_stat(
    conn: &Connection,
    kind: SearchSourceKind,
) -> crate::DbResult<(bool, u64, Option<i64>)> {
    let (sql, param): (&str, Option<&str>) = match kind {
        SearchSourceKind::Commands => (
            "SELECT COUNT(*), MAX(started_at) FROM command_blocks;",
            None,
        ),
        SearchSourceKind::Output => ("SELECT COUNT(*), MAX(ts) FROM session_scrollback;", None),
        other => (
            "SELECT COUNT(*), MAX(occurred_at) FROM search_metadata WHERE source = ?1;",
            Some(other.as_str()),
        ),
    };

    let (count_i64, max_ts): (i64, Option<i64>) = if let Some(p) = param {
        conn.query_row(sql, [p], |r| Ok((r.get(0)?, r.get(1)?)))
    } else {
        conn.query_row(sql, [], |r| Ok((r.get(0)?, r.get(1)?)))
    }
    .map_err(|e| crate::DbError::from_rusqlite(e, "query_source_stat"))?;

    let row_count = u64::try_from(count_i64.max(0)).unwrap_or(0);
    Ok((true, row_count, max_ts))
}
