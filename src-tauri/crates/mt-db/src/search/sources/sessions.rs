//! Fonte de busca `Sessions` sobre `search_metadata_fts` (`source = 'sessions'`).

use rusqlite::Connection;

use crate::search::error::SearchError;
use crate::search::model::SearchSourceKind;
use crate::search::sources::{
    CollectedSection, SearchContext, SearchSource, collect_from_metadata_table, should_query_source,
};

/// Coletor FTS5 para sessões (`SearchSourceKind::Sessions`).
#[derive(Debug, Default)]
pub struct SessionsSource;

impl SearchSource for SessionsSource {
    fn kinds(&self) -> &[SearchSourceKind] {
        &[SearchSourceKind::Sessions]
    }

    fn collect(
        &self,
        conn: &Connection,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<CollectedSection>, SearchError> {
        if !should_query_source(SearchSourceKind::Sessions, ctx.filters) {
            return Ok(Vec::new());
        }
        Ok(vec![collect_from_metadata_table(
            conn,
            ctx,
            SearchSourceKind::Sessions,
        )?])
    }
}
