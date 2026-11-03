//! Fontes de busca global (`SearchSource`) e helper parametrizado para `search_metadata_fts`.

pub mod command_blocks;
pub mod groups;
pub mod issues;
pub mod projects;
pub mod scrollback;
pub mod sessions;
pub mod templates;

use rusqlite::{Connection, named_params};

use crate::sanitizer::SanitizedText;
use crate::search::error::SearchError;
use crate::search::model::{
    ExitFilter, SearchFilters, SearchHit, SearchSort, SearchSourceKind, SearchTarget,
};
use crate::search::query::ParsedQuery;
use crate::search::rank::{RankedHit, sort_ranked_hits};
use crate::search::snapshot::SnapshotCache;

/// Contexto imutável compartilhado com cada fonte durante uma coleta `SearchEngine::search`.
#[derive(Debug, Clone)]
pub struct SearchContext<'a> {
    /// Consulta FTS5 já parseada e validada.
    pub query: &'a ParsedQuery,
    /// Filtros estruturados da requisição.
    pub filters: &'a SearchFilters,
    /// Critério de ordenação solicitado.
    pub sort: SearchSort,
    /// Orçamento restante de hits no snapshot global.
    pub remaining_hits: usize,
    /// Orçamento restante de bytes no snapshot global.
    pub remaining_bytes: usize,
}

/// Seção coletada e ordenada por uma implementação de `SearchSource`.
#[derive(Debug, Clone)]
pub struct CollectedSection {
    /// Fonte desta seção.
    pub source: SearchSourceKind,
    /// Resultados já ordenados e limitados ao orçamento.
    pub items: Vec<SearchHit>,
    /// Indica se houve corte por limite de hits ou bytes.
    pub truncated: bool,
    /// Maior timestamp `occurred_at_ms` visto nesta fonte.
    pub max_occurred_at_ms: Option<i64>,
    /// Estimativa de bytes consumidos por `items`.
    pub approx_bytes: usize,
}

/// Trait implementada pelos coletores de cada fonte de busca global.
pub trait SearchSource: Send + Sync {
    /// Fontes atendidas por este coletor.
    fn kinds(&self) -> &[SearchSourceKind];

    /// Coleta as seções solicitadas dentro da transação de leitura ativa em `conn`.
    fn collect(
        &self,
        conn: &Connection,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<CollectedSection>, SearchError>;
}

/// Verifica se o usuário solicitou `kind` e se `kind` é compatível com os filtros atuais.
#[must_use]
pub fn should_query_source(kind: SearchSourceKind, filters: &SearchFilters) -> bool {
    if !filters.sources.is_empty() && !filters.sources.contains(&kind) {
        return false;
    }
    kind.is_compatible_with(filters)
}

/// Executa uma consulta parametrizada sobre `search_metadata_fts` JOIN `search_metadata` para `source_kind`.
#[allow(clippy::too_many_lines)]
pub fn collect_from_metadata_table(
    conn: &Connection,
    ctx: &SearchContext<'_>,
    source_kind: SearchSourceKind,
) -> Result<CollectedSection, SearchError> {
    if ctx.remaining_hits == 0 || ctx.remaining_bytes == 0 {
        return Ok(CollectedSection {
            source: source_kind,
            items: Vec::new(),
            truncated: true,
            max_occurred_at_ms: None,
            approx_bytes: 0,
        });
    }

    let exit_mode: i64 = match ctx.filters.exit {
        ExitFilter::Any => 0,
        ExitFilter::Zero => 1,
        ExitFilter::Nonzero => 2,
        ExitFilter::Unknown => 3,
    };

    let proj_filter = ctx.filters.project_id.map(|id| id.to_string());
    let tab_filter = ctx.filters.tab_id.map(|id| id.to_string());
    let sess_filter = ctx.filters.session_id.map(|id| id.to_string());
    let pane_filter = ctx.filters.pane_id.map(|id| id.to_string());
    let sql_limit = i64::try_from(ctx.remaining_hits.saturating_add(1)).unwrap_or(10_001);

    let mut stmt = conn
        .prepare(
            "SELECT
                sm.entity_key,
                sm.title,
                sm.body,
                sm.target_json,
                sm.occurred_at,
                sm.source_truncated,
                sm.fetched_at,
                bm25(search_metadata_fts, 5.0, 1.0) AS score,
                snippet(search_metadata_fts, 1, :open_s, :close_s, '…', 24) AS body_snip
             FROM search_metadata_fts
             JOIN search_metadata sm ON sm.rowid = search_metadata_fts.rowid
             WHERE search_metadata_fts MATCH :match_expr
               AND sm.source = :source
               AND (:project_id IS NULL OR sm.project_id = :project_id)
               AND (:tab_id IS NULL OR sm.tab_id = :tab_id)
               AND (:session_id IS NULL OR sm.session_id = :session_id)
               AND (:pane_id IS NULL OR sm.pane_id = :pane_id)
               AND (:from_ms IS NULL OR (sm.occurred_at IS NOT NULL AND sm.occurred_at >= :from_ms))
               AND (:until_ms IS NULL OR (sm.occurred_at IS NOT NULL AND sm.occurred_at < :until_ms))
               AND (
                   :exit_mode = 0
                   OR (:exit_mode = 1 AND sm.exit_code = 0)
                   OR (:exit_mode = 2 AND sm.exit_code IS NOT NULL AND sm.exit_code != 0)
                   OR (:exit_mode = 3 AND sm.exit_code IS NULL)
               )
             LIMIT :sql_limit;",
        )
        .map_err( map_sqlite_err )?;

    let rows = stmt
        .query_map(
            named_params! {
                ":open_s": ctx.query.open_sentinel,
                ":close_s": ctx.query.close_sentinel,
                ":match_expr": ctx.query.fts_match_text,
                ":source": source_kind.as_str(),
                ":project_id": proj_filter,
                ":tab_id": tab_filter,
                ":session_id": sess_filter,
                ":pane_id": pane_filter,
                ":from_ms": ctx.filters.from_ms,
                ":until_ms": ctx.filters.until_ms,
                ":exit_mode": exit_mode,
                ":sql_limit": sql_limit,
            },
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<i64>>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, Option<i64>>(6)?,
                    r.get::<_, f64>(7)?,
                    r.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .map_err(map_sqlite_err)?;

    let mut ranked = Vec::new();
    let mut max_occurred: Option<i64> = None;

    for row_res in rows {
        let (
            entity_key,
            raw_title,
            raw_body,
            target_json,
            occurred_at_ms,
            source_truncated,
            fetched_at_ms,
            bm25_score,
            body_snip,
        ) = row_res.map_err(map_sqlite_err)?;

        let Ok(target) = serde_json::from_str::<SearchTarget>(&target_json) else {
            continue;
        };

        let clean_title = SanitizedText::new(&raw_title);
        let clean_body = SanitizedText::new(&raw_body);
        let clean_snip = body_snip
            .as_deref()
            .map_or_else(|| clean_body.clone(), SanitizedText::new);

        if let Some(ts) = occurred_at_ms {
            max_occurred = Some(max_occurred.map_or(ts, |m| m.max(ts)));
        }

        let title_segments = ctx.query.highlight_title(clean_title.as_str());
        let excerpt_segments = ctx
            .query
            .parse_snippet_segments(clean_body.as_str(), clean_snip.as_str());

        ranked.push(RankedHit {
            bm25_score,
            hit: SearchHit {
                key: format!("{}:{entity_key}", source_kind.as_str()),
                source: source_kind,
                target,
                title: title_segments,
                excerpt: excerpt_segments,
                occurred_at_ms,
                source_truncated: source_truncated != 0,
                fetched_at_ms,
            },
        });
    }

    sort_ranked_hits(&mut ranked, ctx.sort);
    Ok(finalize_section(
        source_kind,
        ranked,
        ctx.remaining_hits,
        ctx.remaining_bytes,
        max_occurred,
    ))
}

/// Aplica os limites de `remaining_hits` e `remaining_bytes` a uma lista já ordenada de `RankedHit`.
#[must_use]
pub fn finalize_section(
    source: SearchSourceKind,
    ranked: Vec<RankedHit>,
    remaining_hits: usize,
    remaining_bytes: usize,
    max_occurred_at_ms: Option<i64>,
) -> CollectedSection {
    let mut items = Vec::with_capacity(ranked.len().min(remaining_hits));
    let mut approx_bytes = 0usize;
    let mut truncated = false;

    for candidate in ranked {
        let hit_bytes = SnapshotCache::estimate_hit_bytes(&candidate.hit);
        if items.len() >= remaining_hits
            || (approx_bytes.saturating_add(hit_bytes) > remaining_bytes && !items.is_empty())
        {
            truncated = true;
            break;
        }
        approx_bytes = approx_bytes.saturating_add(hit_bytes);
        items.push(candidate.hit);
    }

    CollectedSection {
        source,
        items,
        truncated,
        max_occurred_at_ms,
        approx_bytes,
    }
}

/// Mapeia erros do `rusqlite` preservando interrupções (`SearchError::Cancelled`).
#[must_use]
pub fn map_sqlite_err(err: rusqlite::Error) -> SearchError {
    if let rusqlite::Error::SqliteFailure(ffi, _) = &err {
        if ffi.code == rusqlite::ErrorCode::OperationInterrupted {
            return SearchError::Cancelled;
        }
        if matches!(
            ffi.code,
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
        ) {
            return SearchError::Busy;
        }
    }
    SearchError::Internal(err)
}
