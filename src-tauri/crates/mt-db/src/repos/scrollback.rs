//! Repositório transacional e de escrita em lote (`BatchRecord`) para `session_scrollback`
//! e busca FTS5 em `session_scrollback_fts` (Fase 07).

use std::str::FromStr;

use mt_core::ids::{PaneId, SessionId, TabId, TabSessionId};
use mt_core::workspace::{CursorPage, ScrollbackChunk, ScrollbackChunkKind, SessionSearchHit};
use rusqlite::{Connection, OptionalExtension, Statement, Transaction, params};

use crate::Db;
use crate::batch::BatchRecord;
use crate::error::{DbError, DbResult};
use crate::fts::{FtsSpec, FtsTokenizer, build_match_query_checked, format_safe_snippet};
use crate::maintenance::RetentionRule;
use crate::repos::tab_sessions::{CONSTRAINT_SESSION_NOT_FOUND, CONSTRAINT_SESSION_QUERY_INVALID};
use crate::repos::tabs::CONSTRAINT_TAB_NOT_FOUND;
use crate::sanitizer::SanitizedText;

/// Limite máximo de bytes retornados em uma única página de `session_scrollback_get` (1 MiB).
pub const MAX_SCROLLBACK_PAGE_BYTES: usize = 1024 * 1024;

/// Especificação FTS5 canônica da tabela `session_scrollback_fts` registrada na manutenção periódica.
pub const SESSION_SCROLLBACK_FTS_SPEC: FtsSpec = FtsSpec {
    content_table: "session_scrollback",
    fts_table: "session_scrollback_fts",
    columns: &["text"],
    tokenizer: FtsTokenizer::Text,
    prefix: &[2, 3],
};

/// Regra de retenção por idade para sessões base arquivadas (`workspace.sessions.max_age_days`).
pub const SESSIONS_ARCHIVED_RETENTION_RULE: RetentionRule = RetentionRule {
    name: "sessions_archived_retention",
    table: "sessions",
    timestamp_column: "archived_at",
    setting_key: "workspace.sessions.max_age_days",
    extra_where: Some("archived_at IS NOT NULL AND status = 'archived'"),
    batch_size: 200,
    default_days: Some(90),
};

/// Registro sanitizado de um chunk de scrollback persistido via [` crate::BatchWriter`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrollbackRecord {
    /// Identificador da sessão base do painel.
    pub session_id: SessionId,
    /// Sequência monotônica (`>= 1`) dentro de `session_id`.
    pub seq: i64,
    /// Timestamp UNIX em ms da captura.
    pub ts: i64,
    /// Texto sanitizado do chunk (`<= 64 KiB`).
    pub text: SanitizedText,
    /// Tipo do chunk (`Output` ou `Gap`).
    pub kind: ScrollbackChunkKind,
    /// Tamanho em bytes UTF-8 do texto sanitizado (`0..=65536`).
    pub byte_len: i64,
}

impl ScrollbackRecord {
    /// Constrói um novo [`ScrollbackRecord`] calculando `byte_len` a partir do [`SanitizedText`].
    #[must_use]
    pub fn new(
        session_id: SessionId,
        seq: i64,
        ts: i64,
        text: SanitizedText,
        kind: ScrollbackChunkKind,
    ) -> Self {
        let byte_len = i64::try_from(text.len().min(65_536)).unwrap_or(0);
        Self {
            session_id,
            seq,
            ts,
            text,
            kind,
            byte_len,
        }
    }
}

impl BatchRecord for ScrollbackRecord {
    const WRITER_NAME: &'static str = "session_scrollback";
    const INSERT_SQL: &'static str = "
        INSERT INTO session_scrollback (session_id, seq, ts, text, kind, byte_len)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(session_id, seq) DO NOTHING;
    ";

    fn bind(&self, stmt: &mut Statement<'_>) -> rusqlite::Result<usize> {
        stmt.execute(params![
            self.session_id.to_string(),
            self.seq,
            self.ts,
            self.text.as_str(),
            self.kind.as_db_str(),
            self.byte_len,
        ])
    }

    fn approx_bytes(&self) -> usize {
        128 + self.text.len()
    }
}

/// Repositório tipado para leitura paginada e busca FTS5 em `session_scrollback`.
#[derive(Clone, Debug)]
pub struct ScrollbackRepo {
    db: Db,
}

impl ScrollbackRepo {
    /// Cria uma nova instância de [`ScrollbackRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insere um chunk diretamente em uma transação (útil em testes ou marcador final síncrono).
    pub async fn append_chunk(&self, record: ScrollbackRecord) -> DbResult<()> {
        let approx = record.approx_bytes();
        self.db
            .write_sized("scrollback_append_chunk", approx, move |tx| {
                append_scrollback_chunk_tx(tx, &record)
            })
            .await
    }

    /// Marca `sessions.scrollback_truncated = 1` para a sessão informada.
    pub async fn mark_session_truncated(&self, session_id: SessionId) -> DbResult<()> {
        self.db
            .write_sized("scrollback_mark_truncated", 128, move |tx| {
                mark_session_truncated_tx(tx, session_id)
            })
            .await
    }

    /// Lista chunks de scrollback paginados por `(session_id, seq ASC)`, validando vínculo com `tab_session_id`
    /// e respeitando os tetos de `limit` (`1..=200`) e `1 MiB` por resposta.
    pub async fn list_chunks(
        &self,
        tab_session_id: TabSessionId,
        session_id: SessionId,
        cursor: Option<String>,
        limit: u32,
    ) -> DbResult<CursorPage<ScrollbackChunk>> {
        self.db
            .read(move |conn| {
                list_scrollback_chunks_conn(
                    conn,
                    tab_session_id,
                    session_id,
                    cursor.as_deref(),
                    limit,
                )
            })
            .await
    }

    /// Executa busca textual FTS5 em `session_scrollback_fts` restrita às sessões de `tab_id`.
    pub async fn search_in_tab(
        &self,
        tab_id: TabId,
        query: String,
        limit: u32,
    ) -> DbResult<Vec<SessionSearchHit>> {
        self.db
            .read(move |conn| search_scrollback_in_tab_conn(conn, tab_id, &query, limit))
            .await
    }
}

/// Insere um [`ScrollbackRecord`] em uma transação existente.
pub fn append_scrollback_chunk_tx(tx: &Transaction<'_>, record: &ScrollbackRecord) -> DbResult<()> {
    let mut stmt = tx
        .prepare_cached(ScrollbackRecord::INSERT_SQL)
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_append_chunk"))?;
    record
        .bind(&mut stmt)
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_append_chunk"))?;
    Ok(())
}

/// Marca `scrollback_truncated = 1` para `session_id` dentro de uma transação.
pub fn mark_session_truncated_tx(tx: &Transaction<'_>, session_id: SessionId) -> DbResult<()> {
    tx.execute(
        "UPDATE sessions SET scrollback_truncated = 1 WHERE id = ?1;",
        [session_id.to_string()],
    )
    .map_err(|e| DbError::from_rusqlite(e, "scrollback_mark_truncated"))?;
    crate::search::projection::sync_session_if_ready(tx, session_id)?;
    Ok(())
}

fn list_scrollback_chunks_conn(
    conn: &Connection,
    tab_session_id: TabSessionId,
    session_id: SessionId,
    cursor: Option<&str>,
    limit: u32,
) -> DbResult<CursorPage<ScrollbackChunk>> {
    if !(1..=200).contains(&limit) {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        });
    }

    let ts_str = tab_session_id.to_string();
    let sess_str = session_id.to_string();

    // Verifica ownership (session_id deve pertencer a tab_session_id)
    let linked: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM tab_session_panes WHERE tab_session_id = ?1 AND session_id = ?2;",
            params![ts_str, sess_str],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_check_ownership"))?;
    if linked.is_none() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
        });
    }

    let after_seq = match cursor {
        Some(raw) if !raw.trim().is_empty() => {
            decode_scrollback_cursor(raw, tab_session_id, session_id)?
        }
        _ => 0,
    };

    let fetch_limit = i64::from(limit) + 1;
    let mut stmt = conn
        .prepare_cached(
            "SELECT seq, ts, text, kind, byte_len
             FROM session_scrollback
             WHERE session_id = ?1 AND seq > ?2
             ORDER BY seq ASC
             LIMIT ?3;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_list_chunks"))?;

    let rows = stmt
        .query_map(params![sess_str, after_seq, fetch_limit], |r| {
            let seq: i64 = r.get(0)?;
            let ts: i64 = r.get(1)?;
            let text: String = r.get(2)?;
            let kind_raw: String = r.get(3)?;
            let byte_len: i64 = r.get(4)?;
            Ok((
                ScrollbackChunk {
                    session_id,
                    seq,
                    ts,
                    text,
                    kind: ScrollbackChunkKind::from_db_str(&kind_raw),
                    byte_len,
                },
                usize::try_from(byte_len.max(0)).unwrap_or(0),
            ))
        })
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_list_chunks"))?;

    let limit_usize = usize::try_from(limit).unwrap_or(100);
    let mut items = Vec::with_capacity(limit_usize);
    let mut accumulated_bytes = 0usize;
    let mut stopped_early = false;

    for row_res in rows {
        let (chunk, chunk_bytes) =
            row_res.map_err(|e| DbError::from_rusqlite(e, "scrollback_list_chunks_row"))?;
        if items.len() >= limit_usize {
            stopped_early = true;
            break;
        }
        if !items.is_empty()
            && accumulated_bytes.saturating_add(chunk_bytes) > MAX_SCROLLBACK_PAGE_BYTES
        {
            stopped_early = true;
            break;
        }
        accumulated_bytes = accumulated_bytes.saturating_add(chunk_bytes.max(chunk.text.len()));
        items.push(chunk);
    }

    let next_cursor = if stopped_early && let Some(last) = items.last() {
        Some(encode_scrollback_cursor(
            tab_session_id,
            session_id,
            last.seq,
        ))
    } else {
        None
    };

    Ok(CursorPage { items, next_cursor })
}

fn encode_scrollback_cursor(
    tab_session_id: TabSessionId,
    session_id: SessionId,
    seq: i64,
) -> String {
    format!("sb1:{tab_session_id}:{session_id}:{seq}")
}

fn decode_scrollback_cursor(
    raw: &str,
    expected_tab_session_id: TabSessionId,
    expected_session_id: SessionId,
) -> DbResult<i64> {
    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() != 4 || parts[0] != "sb1" {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        });
    }
    let ts_id = TabSessionId::from_str(parts[1]).map_err(|_| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
    })?;
    let sess_id = SessionId::from_str(parts[2]).map_err(|_| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
    })?;
    if ts_id != expected_tab_session_id || sess_id != expected_session_id {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        });
    }
    let seq = parts[3]
        .parse::<i64>()
        .map_err(|_| DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        })?;
    if seq <= 0 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        });
    }
    Ok(seq)
}

fn search_scrollback_in_tab_conn(
    conn: &Connection,
    tab_id: TabId,
    query: &str,
    limit: u32,
) -> DbResult<Vec<SessionSearchHit>> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.len() > 256 || !(1..=50).contains(&limit) {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        });
    }

    let tab_str = tab_id.to_string();
    let tab_exists: Option<i64> = conn
        .query_row("SELECT 1 FROM tabs WHERE id = ?1;", [&tab_str], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_search_check_tab"))?;
    if tab_exists.is_none() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
        });
    }

    let fts_match =
        build_match_query_checked(trimmed).map_err(|_| DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        })?;

    let first_term = trimmed.split_whitespace().next().unwrap_or(trimmed);
    let mut stmt = conn
        .prepare_cached(
            "SELECT ts.id,
                    sb.session_id,
                    tsp.pane_id,
                    sb.seq,
                    sb.text
             FROM session_scrollback_fts
             JOIN session_scrollback sb ON sb.rowid = session_scrollback_fts.rowid
             JOIN tab_session_panes tsp ON tsp.session_id = sb.session_id
             JOIN tab_sessions ts ON ts.id = tsp.tab_session_id
             WHERE session_scrollback_fts MATCH ?1
               AND ts.tab_id = ?2
             ORDER BY bm25(session_scrollback_fts) ASC, ts.seq DESC, sb.seq ASC
             LIMIT ?3;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_search_prepare"))?;

    let rows = stmt
        .query_map(params![fts_match, tab_str, i64::from(limit)], |r| {
            let ts_raw: String = r.get(0)?;
            let sess_raw: String = r.get(1)?;
            let pane_raw: String = r.get(2)?;
            let seq: i64 = r.get(3)?;
            let text: String = r.get(4)?;

            let tab_session_id = TabSessionId::from_str(&ts_raw).map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::from("TabSessionId inválido"),
                )
            })?;
            let session_id = SessionId::from_str(&sess_raw).map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::from("SessionId inválido"),
                )
            })?;
            let pane_id = PaneId::from_str(&pane_raw).map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::from("PaneId inválido"),
                )
            })?;

            Ok(SessionSearchHit {
                tab_session_id,
                session_id,
                pane_id,
                seq,
                snippet: format_safe_snippet(&text, first_term, 180),
            })
        })
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_search_query"))?;

    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "scrollback_search_row"))
}
