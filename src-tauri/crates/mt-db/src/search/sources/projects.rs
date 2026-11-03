//! Fonte de busca `Projects` sobre `search_metadata_fts` (`source = 'projects'`).

use rusqlite::Connection;

use crate::search::error::SearchError;
use crate::search::model::SearchSourceKind;
use crate::search::sources::{
    CollectedSection, SearchContext, SearchSource, collect_from_metadata_table, should_query_source,
};

/// Coletor FTS5 para projetos (`SearchSourceKind::Projects`).
#[derive(Debug, Default)]
pub struct ProjectsSource;

impl SearchSource for ProjectsSource {
    fn kinds(&self) -> &[SearchSourceKind] {
        &[SearchSourceKind::Projects]
    }

    fn collect(
        &self,
        conn: &Connection,
        ctx: &SearchContext<'_>,
    ) -> Result<Vec<CollectedSection>, SearchError> {
        if !should_query_source(SearchSourceKind::Projects, ctx.filters) {
            return Ok(Vec::new());
        }
        Ok(vec![collect_from_metadata_table(
            conn,
            ctx,
            SearchSourceKind::Projects,
        )?])
    }
}
