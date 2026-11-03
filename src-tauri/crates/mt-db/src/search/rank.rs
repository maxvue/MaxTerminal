//! Ordenação determinística intra-seção (`Relevance` vs `Recent`) para resultados de busca global.
//!
//! Nunca compara scores `bm25` entre tabelas FTS distintas; cada fonte ordena seus próprios candidatos
//! de maneira estritamente reproduzível.

use std::cmp::Ordering;

use crate::search::model::{SearchHit, SearchSort};

/// Candidato bruto retornado por uma fonte FTS5 antes da paginação.
#[derive(Debug, Clone)]
pub struct RankedHit {
    /// Score `bm25()` retornado pelo SQLite FTS5 (quanto menor/mais negativo, mais relevante).
    pub bm25_score: f64,
    /// Resultado estruturado e sanitizado.
    pub hit: SearchHit,
}

/// Ordena `candidates` in-place de forma determinística segundo `sort`:
/// - `SearchSort::Relevance`: `bm25_score` ASC (NaN tratado como +infinito), depois `occurred_at_ms` DESC, depois `hit.key` ASC.
/// - `SearchSort::Recent`: `occurred_at_ms` DESC (`None` por último), depois `hit.key` ASC.
pub fn sort_ranked_hits(candidates: &mut [RankedHit], sort: SearchSort) {
    candidates.sort_by(|a, b| compare_ranked_hits(a, b, sort));
}

#[must_use]
fn compare_ranked_hits(a: &RankedHit, b: &RankedHit, sort: SearchSort) -> Ordering {
    match sort {
        SearchSort::Relevance => {
            let sa = normalize_bm25(a.bm25_score);
            let sb = normalize_bm25(b.bm25_score);
            sa.total_cmp(&sb)
                .then_with(|| compare_occurred_desc(a.hit.occurred_at_ms, b.hit.occurred_at_ms))
                .then_with(|| a.hit.key.cmp(&b.hit.key))
        }
        SearchSort::Recent => compare_occurred_desc(a.hit.occurred_at_ms, b.hit.occurred_at_ms)
            .then_with(|| a.hit.key.cmp(&b.hit.key)),
    }
}

#[inline]
fn normalize_bm25(score: f64) -> f64 {
    if score.is_finite() {
        // Arredonda na 6ª casa decimal para eliminar jitter de ponto flutuante em empates
        (score * 1_000_000.0).round() / 1_000_000.0
    } else {
        f64::INFINITY
    }
}

#[inline]
fn compare_occurred_desc(a: Option<i64>, b: Option<i64>) -> Ordering {
    match (a, b) {
        (Some(ta), Some(tb)) => tb.cmp(&ta),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}
