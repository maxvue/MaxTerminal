//! Fontes de busca `Issues` e `Comments` sobre `search_metadata_fts` (`source IN ('issues', 'comments')`).

use rusqlite::Connection;

use crate::search::error::SearchError;
use crate::search::model::SearchSourceKind;
use crate::search::sources::{
    CollectedSection, SearchContext, SearchSource, collect_from_metadata_table, should_query_source,
};

/// Coletor FTS5 para issues e comentários do GitHub (`Issues`, `Comments`).
#[derive(Debug, Default)]
pub struct IssuesSource;

impl SearchSource for IssuesSource {
    fn kinds(&self) -> &[SearchSourceKind] {
        &[SearchSourceKind::Issues, SearchSourceKind::Comments]
    }

    fn collect(
        &self,
        conn: &Connection,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<CollectedSection>, SearchError> {
        let mut sections = Vec::new();
        let mut sub_ctx = ctx.clone();

        for kind in [SearchSourceKind::Issues, SearchSourceKind::Comments] {
            if !should_query_source(kind, ctx.filters) {
                continue;
            }
            let sec = collect_from_metadata_table(conn, &sub_ctx, kind)?;
            sub_ctx.remaining_hits = sub_ctx.remaining_hits.saturating_sub(sec.items.len());
            sub_ctx.remaining_bytes = sub_ctx.remaining_bytes.saturating_sub(sec.approx_bytes);
            sections.push(sec);
        }

        Ok(sections)
    }
}
