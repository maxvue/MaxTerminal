//! Fonte de busca `Commands` sobre `command_blocks_fts` + `command_blocks` + `sessions`.

use std::str::FromStr;

use mt_core::ids::{CommandBlockId, PaneId, ProjectId, SessionId, TabId};
use rusqlite::{Connection, named_params};

use crate::sanitizer::SanitizedText;
use crate::search::error::SearchError;
use crate::search::model::{
    ExitFilter, HighlightSegment, SearchHit, SearchSourceKind, SearchTarget,
};
use crate::search::rank::{RankedHit, sort_ranked_hits};
use crate::search::sources::{
    CollectedSection, SearchContext, SearchSource, finalize_section, map_sqlite_err,
    should_query_source,
};

/// Coletor FTS5 para blocos de comando (`SearchSourceKind::Commands`).
#[derive(Debug, Default)]
pub struct CommandBlocksSource;

impl SearchSource for CommandBlocksSource {
    fn kinds(&self) -> &[SearchSourceKind] {
        &[SearchSourceKind::Commands]
    }

    #[allow(clippy::too_many_lines)]
    fn collect(
        &self,
        conn: &Connection,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<CollectedSection>, SearchError> {
        if !should_query_source(SearchSourceKind::Commands, ctx.filters) {
            return Ok(Vec::new());
        }
        if ctx.remaining_hits == 0 || ctx.remaining_bytes == 0 {
            return Ok(vec![CollectedSection {
                source: SearchSourceKind::Commands,
                items: Vec::new(),
                truncated: true,
                max_occurred_at_ms: None,
                approx_bytes: 0,
            }]);
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
                    cb.id,
                    cb.session_id,
                    COALESCE(cb.tab_id, s.tab_id, ts.tab_id) AS resolved_tab_id,
                    COALESCE(cb.pane_id, s.pane_id, tsp.pane_id) AS resolved_pane_id,
                    COALESCE(s.project_id, t.project_id) AS resolved_project_id,
                    cb.command_text,
                    cb.command_text_quality,
                    cb.output_text,
                    cb.error_excerpt,
                    cb.cwd,
                    cb.output_kind,
                    cb.started_at,
                    bm25(command_blocks_fts, 8.0, 2.0, 3.0, 1.0) AS score,
                    snippet(command_blocks_fts, -1, :open_s, :close_s, '…', 24) AS snip
                 FROM command_blocks_fts
                 JOIN command_blocks cb ON cb.rowid = command_blocks_fts.rowid
                 JOIN sessions s ON s.id = cb.session_id
                 LEFT JOIN tab_session_panes tsp ON tsp.session_id = s.id
                 LEFT JOIN tab_sessions ts ON ts.id = COALESCE(s.tab_session_id, tsp.tab_session_id)
                 LEFT JOIN tabs t ON t.id = COALESCE(cb.tab_id, s.tab_id, ts.tab_id)
                 WHERE command_blocks_fts MATCH :match_expr
                   AND (:project_id IS NULL OR COALESCE(s.project_id, t.project_id) = :project_id)
                   AND (:tab_id IS NULL OR COALESCE(cb.tab_id, s.tab_id, ts.tab_id) = :tab_id)
                   AND (:session_id IS NULL OR cb.session_id = :session_id)
                   AND (:pane_id IS NULL OR COALESCE(cb.pane_id, s.pane_id, tsp.pane_id) = :pane_id)
                   AND (:from_ms IS NULL OR cb.started_at >= :from_ms)
                   AND (:until_ms IS NULL OR cb.started_at < :until_ms)
                   AND (
                       :exit_mode = 0
                       OR (:exit_mode = 1 AND cb.exit_code = 0)
                       OR (:exit_mode = 2 AND cb.exit_code IS NOT NULL AND cb.exit_code != 0)
                       OR (:exit_mode = 3 AND cb.exit_code IS NULL)
                   )
                 LIMIT :sql_limit;",
            )
            .map_err(map_sqlite_err)?;

        let rows = stmt
            .query_map(
                named_params! {
                    ":open_s": ctx.query.open_sentinel,
                    ":close_s": ctx.query.close_sentinel,
                    ":match_expr": ctx.query.fts_match_code,
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
                        r.get::<_, Option<String>>(2)?,
                        r.get::<_, Option<String>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                        r.get::<_, Option<String>>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, Option<String>>(7)?,
                        r.get::<_, Option<String>>(8)?,
                        r.get::<_, Option<String>>(9)?,
                        r.get::<_, String>(10)?,
                        r.get::<_, i64>(11)?,
                        r.get::<_, f64>(12)?,
                        r.get::<_, Option<String>>(13)?,
                    ))
                },
            )
            .map_err(map_sqlite_err)?;

        let mut ranked = Vec::new();
        let mut max_occurred: Option<i64> = None;

        for row_res in rows {
            let (
                block_id_raw,
                session_id_raw,
                tab_id_raw,
                pane_id_raw,
                project_id_raw,
                command_text,
                command_quality,
                output_text,
                error_excerpt,
                cwd,
                output_kind,
                started_at,
                bm25_score,
                snip,
            ) = row_res.map_err(map_sqlite_err)?;

            let Ok(block_id) = CommandBlockId::from_str(&block_id_raw) else {
                continue;
            };
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

            max_occurred = Some(max_occurred.map_or(started_at, |m| m.max(started_at)));

            let title_segments = match command_text.as_deref() {
                Some(cmd)
                    if !cmd.trim().is_empty()
                        && !matches!(command_quality.as_str(), "unavailable" | "partial") =>
                {
                    let clean_cmd = SanitizedText::new(cmd);
                    ctx.query.highlight_title(clean_cmd.as_str())
                }
                _ => vec![HighlightSegment {
                    text: String::from("Comando indisponível"),
                    matched: false,
                }],
            };

            let raw_excerpt_source = output_text
                .as_deref()
                .or(error_excerpt.as_deref())
                .or(cwd.as_deref())
                .or(command_text.as_deref())
                .unwrap_or("");
            let clean_source = SanitizedText::new(raw_excerpt_source);
            let clean_snip = snip
                .as_deref()
                .map_or_else(|| clean_source.clone(), SanitizedText::new);

            let excerpt_segments = ctx
                .query
                .parse_snippet_segments(clean_source.as_str(), clean_snip.as_str());

            let source_truncated =
                matches!(output_kind.as_str(), "head_tail_truncated" | "tail_only");

            ranked.push(RankedHit {
                bm25_score,
                hit: SearchHit {
                    key: format!("commands:{block_id_raw}"),
                    source: SearchSourceKind::Commands,
                    target: SearchTarget::Command {
                        project_id,
                        tab_id,
                        session_id,
                        pane_id,
                        block_id,
                    },
                    title: title_segments,
                    excerpt: excerpt_segments,
                    occurred_at_ms: Some(started_at),
                    source_truncated,
                    fetched_at_ms: None,
                },
            });
        }

        sort_ranked_hits(&mut ranked, ctx.sort);
        Ok(vec![finalize_section(
            SearchSourceKind::Commands,
            ranked,
            ctx.remaining_hits,
            ctx.remaining_bytes,
            max_occurred,
        )])
    }
}
