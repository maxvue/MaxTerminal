//! Fonte de busca `Output` sobre `session_scrollback_fts` + `session_scrollback` + `sessions`.

use std::str::FromStr;

use mt_core::ids::{PaneId, ProjectId, SessionId, TabId};
use rusqlite::{Connection, named_params};

use crate::sanitizer::SanitizedText;
use crate::search::error::SearchError;
use crate::search::model::{SearchHit, SearchSourceKind, SearchTarget};
use crate::search::rank::{RankedHit, sort_ranked_hits};
use crate::search::sources::{
    CollectedSection, SearchContext, SearchSource, finalize_section, map_sqlite_err,
    should_query_source,
};

/// Coletor FTS5 para chunks de scrollback (`SearchSourceKind::Output`).
#[derive(Debug, Default)]
pub struct ScrollbackSource;

impl SearchSource for ScrollbackSource {
    fn kinds(&self) -> &[SearchSourceKind] {
        &[SearchSourceKind::Output]
    }

    #[allow(clippy::too_many_lines)]
    fn collect(
        &self,
        conn: &Connection,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<CollectedSection>, SearchError> {
        if !should_query_source(SearchSourceKind::Output, ctx.filters) {
            return Ok(Vec::new());
        }
        if ctx.remaining_hits == 0 || ctx.remaining_bytes == 0 {
            return Ok(vec![CollectedSection {
                source: SearchSourceKind::Output,
                items: Vec::new(),
                truncated: true,
                max_occurred_at_ms: None,
                approx_bytes: 0,
            }]);
        }

        let proj_filter = ctx.filters.project_id.map(|id| id.to_string());
        let tab_filter = ctx.filters.tab_id.map(|id| id.to_string());
        let sess_filter = ctx.filters.session_id.map(|id| id.to_string());
        let pane_filter = ctx.filters.pane_id.map(|id| id.to_string());
        let sql_limit = i64::try_from(ctx.remaining_hits.saturating_add(1)).unwrap_or(10_001);

        let mut stmt = conn
            .prepare(
                "SELECT
                    sc.session_id,
                    sc.seq,
                    sc.ts,
                    sc.text,
                    sc.kind,
                    COALESCE(s.tab_id, ts.tab_id) AS resolved_tab_id,
                    COALESCE(s.pane_id, tsp.pane_id) AS resolved_pane_id,
                    COALESCE(s.project_id, t.project_id) AS resolved_project_id,
                    COALESCE(s.scrollback_truncated, 0) AS sess_truncated,
                    COALESCE(t.title, s.initial_cwd) AS label,
                    bm25(session_scrollback_fts) AS score,
                    snippet(session_scrollback_fts, 0, :open_s, :close_s, '…', 24) AS snip
                 FROM session_scrollback_fts
                 JOIN session_scrollback sc ON sc.rowid = session_scrollback_fts.rowid
                 JOIN sessions s ON s.id = sc.session_id
                 LEFT JOIN tab_session_panes tsp ON tsp.session_id = s.id
                 LEFT JOIN tab_sessions ts ON ts.id = COALESCE(s.tab_session_id, tsp.tab_session_id)
                 LEFT JOIN tabs t ON t.id = COALESCE(s.tab_id, ts.tab_id)
                 WHERE session_scrollback_fts MATCH :match_expr
                   AND (:project_id IS NULL OR COALESCE(s.project_id, t.project_id) = :project_id)
                   AND (:tab_id IS NULL OR COALESCE(s.tab_id, ts.tab_id) = :tab_id)
                   AND (:session_id IS NULL OR sc.session_id = :session_id)
                   AND (:pane_id IS NULL OR COALESCE(s.pane_id, tsp.pane_id) = :pane_id)
                   AND (:from_ms IS NULL OR sc.ts >= :from_ms)
                   AND (:until_ms IS NULL OR sc.ts < :until_ms)
                 LIMIT :sql_limit;",
            )
            .map_err(map_sqlite_err)?;

        let rows = stmt
            .query_map(
                named_params! {
                    ":open_s": ctx.query.open_sentinel,
                    ":close_s": ctx.query.close_sentinel,
                    ":match_expr": ctx.query.fts_match_text,
                    ":project_id": proj_filter,
                    ":tab_id": tab_filter,
                    ":session_id": sess_filter,
                    ":pane_id": pane_filter,
                    ":from_ms": ctx.filters.from_ms,
                    ":until_ms": ctx.filters.until_ms,
                    ":sql_limit": sql_limit,
                },
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, Option<String>>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, i64>(8)?,
                        r.get::<_, String>(9)?,
                        r.get::<_, f64>(10)?,
                        r.get::<_, Option<String>>(11)?,
                    ))
                },
            )
            .map_err(map_sqlite_err)?;

        let mut ranked = Vec::new();
        let mut max_occurred: Option<i64> = None;

        for row_res in rows {
            let (
                session_id_raw,
                seq,
                ts,
                raw_text,
                kind,
                tab_id_raw,
                pane_id_raw,
                project_id_raw,
                sess_truncated,
                label,
                bm25_score,
                snip,
            ) = row_res.map_err(map_sqlite_err)?;

            let Ok(session_id) = SessionId::from_str(&session_id_raw) else {
                continue;
            };
            let tab_id = tab_id_raw.as_deref().and_then(|s| TabId::from_str(s).ok());
            let pane_id = pane_id_raw
                .as_deref()
                .and_then(|s| PaneId::from_str(s).ok());
            let project_id = project_id_raw
                .as_deref()
                .and_then(|s| ProjectId::from_str(s).ok());

            max_occurred = Some(max_occurred.map_or(ts, |m| m.max(ts)));

            let clean_label = SanitizedText::new(&format!("{label} (#{seq})"));
            let clean_text = SanitizedText::new(&raw_text);
            let clean_snip = snip
                .as_deref()
                .map_or_else(|| clean_text.clone(), SanitizedText::new);

            let title_segments = ctx.query.highlight_title(clean_label.as_str());
            let excerpt_segments = ctx
                .query
                .parse_snippet_segments(clean_text.as_str(), clean_snip.as_str());

            ranked.push(RankedHit {
                bm25_score,
                hit: SearchHit {
                    key: format!("output:{session_id_raw}:{seq}"),
                    source: SearchSourceKind::Output,
                    target: SearchTarget::Output {
                        project_id,
                        tab_id,
                        session_id,
                        pane_id,
                        chunk_seq: seq,
                    },
                    title: title_segments,
                    excerpt: excerpt_segments,
                    occurred_at_ms: Some(ts),
                    source_truncated: sess_truncated != 0 || kind == "gap",
                    fetched_at_ms: None,
                },
            });
        }

        sort_ranked_hits(&mut ranked, ctx.sort);
        Ok(vec![finalize_section(
            SearchSourceKind::Output,
            ranked,
            ctx.remaining_hits,
            ctx.remaining_bytes,
            max_occurred,
        )])
    }
}
