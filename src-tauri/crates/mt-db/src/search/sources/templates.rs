//! Fonte de busca `Templates` sobre `search_metadata_fts` (`source = 'templates'`).

use rusqlite::Connection;

use crate::search::error::SearchError;
use crate::search::model::SearchSourceKind;
use crate::search::sources::{
    CollectedSection, SearchContext, SearchSource, collect_from_metadata_table, should_query_source,
};

/// Coletor FTS5 para templates de aba (`SearchSourceKind::Templates`).
#[derive(Debug, Default)]
pub struct TemplatesSource;

impl SearchSource for TemplatesSource {
    fn kinds(&self) -> &[SearchSourceKind] {
        &[SearchSourceKind::Templates]
    }

    fn collect(
        &self,
        conn: &Connection,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<CollectedSection>, SearchError> {
        if !should_query_source(SearchSourceKind::Templates, ctx.filters) {
            return Ok(Vec::new());
        }
        Ok(vec![collect_from_metadata_table(
            conn,
            ctx,
            SearchSourceKind::Templates,
        )?])
    }
}
