//! Repositório transacional de agrupamentos de sessão por aba (`tab_sessions` e `tab_session_panes`)
//! e arquivamento da tabela base `sessions` (Fase 07).

use std::str::FromStr;

use mt_core::ids::{PaneId, SessionId, TabId, TabSessionId};
use mt_core::workspace::{
    CursorPage, InitialDispatchState, SessionDetail, SessionEndReason, SessionPaneSummary,
    SessionSummary,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::repos::sessions::{SessionEnd, close_session_tx};
use crate::repos::tabs::CONSTRAINT_TAB_NOT_FOUND;

/// Sentinela de constraint indicando sessão ou agrupamento não encontrado.
pub const CONSTRAINT_SESSION_NOT_FOUND: &str = "session.not_found";
/// Sentinela de constraint indicando consulta de sessão inválida.
pub const CONSTRAINT_SESSION_QUERY_INVALID: &str = "session.query_invalid";
/// Sentinela de constraint indicando que ainda há blocos/saídas pendentes antes do arquivamento.
pub const CONSTRAINT_PERSISTENCE_PENDING: &str = "workspace.persistence_pending";

/// Repositório tipado para operações sobre `tab_sessions`, `tab_session_panes` e arquivamento de `sessions`.
#[derive(Clone, Debug)]
pub struct TabSessionsRepo {
    db: Db,
}

impl TabSessionsRepo {
    /// Cria uma nova instância de [`TabSessionsRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Busca o resumo de um agrupamento `tab_sessions` pelo seu [`TabSessionId`].
    pub async fn get_summary(
        &self,
        tab_session_id: TabSessionId,
    ) -> DbResult<Option<SessionSummary>> {
        self.db
            .read(move |conn| get_tab_session_summary_conn(conn, tab_session_id))
            .await
    }

    /// Busca o agrupamento ativo (`archived_at IS NULL`) de uma aba.
    pub async fn get_active_for_tab(&self, tab_id: TabId) -> DbResult<Option<SessionSummary>> {
        self.db
            .read(move |conn| get_active_tab_session_conn(conn, tab_id))
            .await
    }

    /// Busca o detalhe completo de um agrupamento `tab_sessions` (resumo + painéis participantes).
    pub async fn get_detail(
        &self,
        tab_session_id: TabSessionId,
    ) -> DbResult<Option<SessionDetail>> {
        self.db
            .read(move |conn| get_tab_session_detail_conn(conn, tab_session_id))
            .await
    }

    /// Lista os agrupamentos `tab_sessions` de uma aba (aberta ou fechada) em ordem `seq DESC`.
    pub async fn list_by_tab(
        &self,
        tab_id: TabId,
        cursor: Option<String>,
        limit: u32,
    ) -> DbResult<CursorPage<SessionSummary>> {
        self.db
            .read(move |conn| list_tab_sessions_by_tab_conn(conn, tab_id, cursor.as_deref(), limit))
            .await
    }

    /// Reserva atomicamente o despacho do comando inicial (`pending/waiting -> claimed`) para `session_id`.
    ///
    /// Retorna `Ok(true)` somente se a transição ocorreu nesta chamada (garantindo no máximo uma tentativa).
    pub async fn claim_initial_dispatch(&self, session_id: SessionId) -> DbResult<bool> {
        self.db
            .write_sized("tab_sessions_claim_dispatch", 128, move |tx| {
                let changed = tx
                    .execute(
                        "UPDATE tab_session_panes
                         SET initial_dispatch = 'claimed'
                         WHERE session_id = ?1
                           AND closed_at IS NULL
                           AND initial_dispatch IN ('pending', 'waiting');",
                        [session_id.to_string()],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_claim_dispatch"))?;
                Ok(changed > 0)
            })
            .await
    }

    /// Marca o comando inicial como `waiting` quando o prompt confiável não chega dentro do timeout de 5 s.
    pub async fn mark_initial_dispatch_waiting(&self, session_id: SessionId) -> DbResult<bool> {
        self.db
            .write_sized("tab_sessions_waiting_dispatch", 128, move |tx| {
                let changed = tx
                    .execute(
                        "UPDATE tab_session_panes
                         SET initial_dispatch = 'waiting'
                         WHERE session_id = ?1
                           AND closed_at IS NULL
                           AND initial_dispatch = 'pending';",
                        [session_id.to_string()],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_waiting_dispatch"))?;
                Ok(changed > 0)
            })
            .await
    }

    /// Confirma que o comando inicial reservado (`claimed`) foi enviado ao PTY (`sent`).
    pub async fn mark_initial_dispatch_sent(&self, session_id: SessionId) -> DbResult<()> {
        self.db
            .write_sized("tab_sessions_sent_dispatch", 128, move |tx| {
                tx.execute(
                    "UPDATE tab_session_panes
                     SET initial_dispatch = 'sent'
                     WHERE session_id = ?1 AND initial_dispatch = 'claimed';",
                    [session_id.to_string()],
                )
                .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_sent_dispatch"))?;
                Ok(())
            })
            .await
    }

    /// Consulta o estado atual de `initial_dispatch` para um `session_id`.
    pub async fn get_initial_dispatch_state(
        &self,
        session_id: SessionId,
    ) -> DbResult<Option<InitialDispatchState>> {
        self.db
            .read(move |conn| {
                let raw: Option<String> = conn
                    .query_row(
                        "SELECT initial_dispatch FROM tab_session_panes WHERE session_id = ?1;",
                        [session_id.to_string()],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_get_dispatch"))?;
                Ok(raw.and_then(|s| InitialDispatchState::from_db_str(&s)))
            })
            .await
    }

    /// Recupera sessões órfãs de execução anterior no startup da aplicação (`crash_recovered`).
    pub async fn recover_crash_sessions(&self, now_ms: i64) -> DbResult<u32> {
        self.db
            .write_sized("tab_sessions_recover_crash", 2048, move |tx| {
                recover_crash_sessions_tx(tx, now_ms)
            })
            .await
    }

    /// Versão síncrona de [`Self::recover_crash_sessions`].
    pub fn recover_crash_sessions_blocking(&self, now_ms: i64) -> DbResult<u32> {
        self.db
            .write_blocking("tab_sessions_recover_crash", 2048, move |tx| {
                recover_crash_sessions_tx(tx, now_ms)
            })
    }

    /// Executa a limpeza de retenção por contagem (`keep_per_tab`) e idade (`max_age_days`) sobre
    /// agrupamentos arquivados e remove abas manuais fechadas que ficaram sem histórico.
    pub async fn enforce_retention(
        &self,
        keep_per_tab: u32,
        max_age_days: u32,
        now_ms: i64,
    ) -> DbResult<u64> {
        self.db
            .write_sized("tab_sessions_enforce_retention", 2048, move |tx| {
                enforce_tab_sessions_retention_tx(tx, keep_per_tab, max_age_days, now_ms)
            })
            .await
    }
}

/// Inicia um novo agrupamento `tab_sessions` ativo para `tab_id`, incrementando `tabs.last_session_seq`.
pub fn start_tab_session_tx(
    tx: &Transaction<'_>,
    tab_session_id: TabSessionId,
    tab_id: TabId,
    started_at: i64,
) -> DbResult<SessionSummary> {
    let tab_str = tab_id.to_string();
    let changed = tx
        .execute(
            "UPDATE tabs SET last_session_seq = last_session_seq + 1 WHERE id = ?1;",
            [&tab_str],
        )
        .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_bump_seq"))?;
    if changed == 0 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
        });
    }

    let seq: i64 = tx
        .query_row(
            "SELECT last_session_seq FROM tabs WHERE id = ?1;",
            [&tab_str],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_read_seq"))?;

    tx.execute(
        "INSERT INTO tab_sessions (id, tab_id, seq, started_at, archived_at, end_reason)
         VALUES (?1, ?2, ?3, ?4, NULL, NULL);",
        params![tab_session_id.to_string(), tab_str, seq, started_at],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_insert"))?;

    Ok(SessionSummary {
        id: tab_session_id,
        tab_id,
        seq,
        archived: false,
        started_at,
        ended_at: None,
        end_reason: None,
        command_count: 0,
        failed_command_count: 0,
        scrollback_bytes: 0,
        truncated: false,
    })
}

/// Vincula uma sessão base recém-preparada (`sessions`) ao agrupamento ativo `tab_sessions` e ao painel `panes`.
pub fn attach_pane_session_tx(
    tx: &Transaction<'_>,
    session_id: SessionId,
    tab_session_id: TabSessionId,
    pane_id: PaneId,
    generation: i64,
    joined_at: i64,
    initial_dispatch: InitialDispatchState,
) -> DbResult<()> {
    let ts_str = tab_session_id.to_string();
    let pane_str = pane_id.to_string();
    let sess_str = session_id.to_string();

    // Verifica que o agrupamento existe, está ativo e obtém (tab_id, project_id)
    let group_meta: Option<(String, String)> = tx
        .query_row(
            "SELECT ts.tab_id, t.project_id
             FROM tab_sessions ts
             JOIN tabs t ON t.id = ts.tab_id
             WHERE ts.id = ?1 AND ts.archived_at IS NULL;",
            [&ts_str],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "tab_session_panes_check_group"))?;
    let Some((group_tab_id, group_project_id)) = group_meta else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
        });
    };

    // Verifica que o painel pertence à mesma aba
    let pane_tab_id: Option<String> = tx
        .query_row(
            "SELECT tab_id FROM panes WHERE id = ?1;",
            [&pane_str],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "tab_session_panes_check_pane"))?;
    if pane_tab_id.as_deref() != Some(group_tab_id.as_str()) {
        return Err(DbError::ConstraintViolation {
            constraint: String::from("pane.not_found"),
        });
    }

    // Verifica que a sessão base existe e pertence aos mesmos tab_id, pane_id e project_id
    let sess_meta: Option<(Option<String>, Option<String>, Option<String>)> = tx
        .query_row(
            "SELECT tab_id, pane_id, project_id FROM sessions WHERE id = ?1;",
            [&sess_str],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "tab_session_panes_check_session"))?;
    let Some((s_tab, s_pane, s_proj)) = sess_meta else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
        });
    };
    if s_tab.as_deref() != Some(group_tab_id.as_str())
        || s_pane.as_deref() != Some(pane_str.as_str())
        || s_proj.as_deref() != Some(group_project_id.as_str())
    {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
        });
    }

    tx.execute(
        "UPDATE sessions
         SET tab_session_id = ?2,
             status = 'active'
         WHERE id = ?1;",
        params![sess_str, ts_str],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tab_session_panes_update_session"))?;

    tx.execute(
        "INSERT INTO tab_session_panes (
            session_id, tab_session_id, pane_id, generation, joined_at, closed_at, initial_dispatch
        ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6);",
        params![
            sess_str,
            ts_str,
            pane_str,
            generation,
            joined_at,
            initial_dispatch.as_db_str(),
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tab_session_panes_insert"))?;

    crate::search::projection::sync_session_if_ready(tx, session_id)?;
    Ok(())
}

/// Fecha a sessão base (`sessions.status = 'closed'`, `ended_at`) e marca `tab_session_panes.closed_at`.
pub fn close_pane_session_tx(
    tx: &Transaction<'_>,
    session_id: SessionId,
    end: &SessionEnd,
) -> DbResult<()> {
    let sess_str = session_id.to_string();
    let current_status: Option<String> = tx
        .query_row(
            "SELECT status FROM sessions WHERE id = ?1;",
            [&sess_str],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "close_pane_session_check"))?;

    if current_status.as_deref() == Some("archived") {
        // Nunca refechar sessão já arquivada
        tx.execute(
            "UPDATE tab_session_panes
             SET closed_at = COALESCE(closed_at, ?2)
             WHERE session_id = ?1;",
            params![sess_str, end.ended_at],
        )
        .map_err(|e| DbError::from_rusqlite(e, "close_pane_session_link"))?;
        return Ok(());
    }

    close_session_tx(tx, session_id, end)?;

    tx.execute(
        "UPDATE sessions
         SET status = 'closed'
         WHERE id = ?1 AND status <> 'archived';",
        [&sess_str],
    )
    .map_err(|e| DbError::from_rusqlite(e, "close_pane_session_status"))?;

    tx.execute(
        "UPDATE tab_session_panes
         SET closed_at = COALESCE(closed_at, (SELECT ended_at FROM sessions WHERE id = ?1), ?2)
         WHERE session_id = ?1;",
        params![sess_str, end.ended_at],
    )
    .map_err(|e| DbError::from_rusqlite(e, "close_pane_session_link"))?;

    Ok(())
}

/// Arquiva uma sessão base já fechada (`status = 'closed' -> 'archived'`), exigindo `ended_at IS NOT NULL`
/// e nenhum bloco em estado `running`. Idempotente se já estiver `archived`.
pub fn archive_closed_session_tx(
    tx: &Transaction<'_>,
    session_id: SessionId,
    archived_at: i64,
) -> DbResult<()> {
    let sess_str = session_id.to_string();
    let row: Option<(String, Option<i64>, i64)> = tx
        .query_row(
            "SELECT status, ended_at, started_at FROM sessions WHERE id = ?1;",
            [&sess_str],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "archive_closed_session_check"))?;

    let Some((status, ended_at, started_at)) = row else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
        });
    };

    if status == "archived" {
        tx.execute(
            "UPDATE tab_session_panes
             SET closed_at = COALESCE(closed_at, ?2)
             WHERE session_id = ?1;",
            params![sess_str, ended_at.unwrap_or(archived_at)],
        )
        .map_err(|e| DbError::from_rusqlite(e, "archive_closed_session_repair_link"))?;
        return Ok(());
    }

    let Some(ended_ms) = ended_at else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PERSISTENCE_PENDING),
        });
    };
    if status != "closed" {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PERSISTENCE_PENDING),
        });
    }

    let running_blocks: i64 = tx
        .query_row(
            "SELECT COUNT(1) FROM command_blocks WHERE session_id = ?1 AND status = 'running';",
            [&sess_str],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "archive_closed_session_running_blocks"))?;
    if running_blocks > 0 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PERSISTENCE_PENDING),
        });
    }

    let final_archived = archived_at.max(ended_ms).max(started_at);
    tx.execute(
        "UPDATE sessions
         SET status = 'archived',
             archived_at = COALESCE(archived_at, ?2)
         WHERE id = ?1;",
        params![sess_str, final_archived],
    )
    .map_err(|e| DbError::from_rusqlite(e, "archive_closed_session_update"))?;

    tx.execute(
        "UPDATE tab_session_panes
         SET closed_at = COALESCE(closed_at, ?2)
         WHERE session_id = ?1;",
        params![sess_str, ended_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "archive_closed_session_link"))?;

    Ok(())
}

/// Arquiva um agrupamento `tab_sessions` após validar/arquivar todas as suas sessões base vinculadas.
pub fn archive_tab_session_tx(
    tx: &Transaction<'_>,
    tab_session_id: TabSessionId,
    end_reason: SessionEndReason,
    archived_at: i64,
) -> DbResult<SessionSummary> {
    let ts_str = tab_session_id.to_string();
    let existing = get_tab_session_summary_conn(tx, tab_session_id)?.ok_or_else(|| {
        DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
        }
    })?;
    if existing.archived {
        return Ok(existing);
    }

    let linked_sessions: Vec<SessionId> = {
        let mut stmt = tx
            .prepare_cached("SELECT session_id FROM tab_session_panes WHERE tab_session_id = ?1;")
            .map_err(|e| DbError::from_rusqlite(e, "archive_tab_session_list_panes"))?;
        let rows = stmt
            .query_map([&ts_str], |r| {
                let s: String = r.get(0)?;
                SessionId::from_str(&s).map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::from("SessionId inválido"),
                    )
                })
            })
            .map_err(|e| DbError::from_rusqlite(e, "archive_tab_session_list_panes"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "archive_tab_session_list_panes"))?
    };

    for sid in linked_sessions {
        archive_closed_session_tx(tx, sid, archived_at)?;
    }

    let max_ended: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(s.ended_at), 0)
             FROM tab_session_panes tsp
             JOIN sessions s ON s.id = tsp.session_id
             WHERE tsp.tab_session_id = ?1;",
            [&ts_str],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "archive_tab_session_max_ended"))?;

    let final_archived = archived_at.max(existing.started_at).max(max_ended);
    tx.execute(
        "UPDATE tab_sessions
         SET archived_at = ?2,
             end_reason = ?3
         WHERE id = ?1;",
        params![ts_str, final_archived, end_reason.as_db_str()],
    )
    .map_err(|e| DbError::from_rusqlite(e, "archive_tab_session_update"))?;

    get_tab_session_summary_conn(tx, tab_session_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
    })
}

/// Busca o [`SessionSummary`] de um `tab_session_id` agregando métricas dos painéis vinculados.
pub fn get_tab_session_summary_conn(
    conn: &Connection,
    tab_session_id: TabSessionId,
) -> DbResult<Option<SessionSummary>> {
    let ts_str = tab_session_id.to_string();
    conn.query_row(
        "SELECT ts.id,
                ts.tab_id,
                ts.seq,
                ts.started_at,
                ts.archived_at,
                ts.end_reason,
                COALESCE(SUM(s.command_count), 0) AS total_commands,
                COALESCE(SUM(s.error_count), 0) AS total_errors,
                COALESCE(SUM(s.scrollback_bytes), 0) AS total_scrollback_bytes,
                COALESCE(MAX(s.scrollback_truncated), 0) AS any_truncated
         FROM tab_sessions ts
         LEFT JOIN tab_session_panes tsp ON tsp.tab_session_id = ts.id
         LEFT JOIN sessions s ON s.id = tsp.session_id
         WHERE ts.id = ?1
         GROUP BY ts.id;",
        [&ts_str],
        map_session_summary_row,
    )
    .optional()
    .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_get_summary"))
}

/// Busca o [`SessionSummary`] ativo (`archived_at IS NULL`) de uma aba.
pub fn get_active_tab_session_conn(
    conn: &Connection,
    tab_id: TabId,
) -> DbResult<Option<SessionSummary>> {
    let tab_str = tab_id.to_string();
    let active_id: Option<String> = conn
        .query_row(
            "SELECT id FROM tab_sessions WHERE tab_id = ?1 AND archived_at IS NULL;",
            [&tab_str],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_get_active"))?;
    let Some(raw_id) = active_id else {
        return Ok(None);
    };
    let ts_id = TabSessionId::from_str(&raw_id).map_err(|_| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_SESSION_NOT_FOUND),
    })?;
    get_tab_session_summary_conn(conn, ts_id)
}

/// Busca o [`SessionDetail`] de um `tab_session_id`.
pub fn get_tab_session_detail_conn(
    conn: &Connection,
    tab_session_id: TabSessionId,
) -> DbResult<Option<SessionDetail>> {
    let Some(summary) = get_tab_session_summary_conn(conn, tab_session_id)? else {
        return Ok(None);
    };
    let ts_str = tab_session_id.to_string();
    let mut stmt = conn
        .prepare_cached(
            "SELECT tsp.session_id,
                    tsp.pane_id,
                    tsp.generation,
                    COALESCE(s.last_cwd, s.initial_cwd) AS cwd,
                    COALESCE(tsp.closed_at, s.ended_at) AS closed_at
             FROM tab_session_panes tsp
             JOIN sessions s ON s.id = tsp.session_id
             WHERE tsp.tab_session_id = ?1
             ORDER BY tsp.joined_at ASC, tsp.generation ASC, tsp.pane_id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_get_detail_panes"))?;

    let rows = stmt
        .query_map([&ts_str], |r| {
            let sid_raw: String = r.get(0)?;
            let pid_raw: String = r.get(1)?;
            let session_id = SessionId::from_str(&sid_raw).map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::from("SessionId inválido"),
                )
            })?;
            let pane_id = PaneId::from_str(&pid_raw).map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    1,
                    rusqlite::types::Type::Text,
                    Box::from("PaneId inválido"),
                )
            })?;
            Ok(SessionPaneSummary {
                session_id,
                pane_id,
                generation: r.get(2)?,
                cwd: r.get(3)?,
                closed_at: r.get(4)?,
            })
        })
        .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_get_detail_panes"))?;

    let panes = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_get_detail_panes"))?;

    Ok(Some(SessionDetail { summary, panes }))
}

fn list_tab_sessions_by_tab_conn(
    conn: &Connection,
    tab_id: TabId,
    cursor: Option<&str>,
    limit: u32,
) -> DbResult<CursorPage<SessionSummary>> {
    if !(1..=100).contains(&limit) {
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
        .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_check_tab"))?;
    if tab_exists.is_none() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
        });
    }

    let before_seq = match cursor {
        Some(raw) if !raw.trim().is_empty() => Some(decode_tab_session_cursor(raw, tab_id)?),
        _ => None,
    };

    let fetch_limit = i64::from(limit) + 1;
    let mut items = Vec::with_capacity(usize::try_from(fetch_limit).unwrap_or(51));

    if let Some(seq_bound) = before_seq {
        let mut stmt = conn
            .prepare_cached(
                "SELECT ts.id,
                        ts.tab_id,
                        ts.seq,
                        ts.started_at,
                        ts.archived_at,
                        ts.end_reason,
                        COALESCE(SUM(s.command_count), 0),
                        COALESCE(SUM(s.error_count), 0),
                        COALESCE(SUM(s.scrollback_bytes), 0),
                        COALESCE(MAX(s.scrollback_truncated), 0)
                 FROM tab_sessions ts
                 LEFT JOIN tab_session_panes tsp ON tsp.tab_session_id = ts.id
                 LEFT JOIN sessions s ON s.id = tsp.session_id
                 WHERE ts.tab_id = ?1 AND ts.seq < ?2
                 GROUP BY ts.id
                 ORDER BY ts.seq DESC
                 LIMIT ?3;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_list_cursor"))?;
        let rows = stmt
            .query_map(
                params![tab_str, seq_bound, fetch_limit],
                map_session_summary_row,
            )
            .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_list_cursor"))?;
        for r in rows {
            items.push(r.map_err(|e| DbError::from_rusqlite(e, "tab_sessions_list_cursor"))?);
        }
    } else {
        let mut stmt = conn
            .prepare_cached(
                "SELECT ts.id,
                        ts.tab_id,
                        ts.seq,
                        ts.started_at,
                        ts.archived_at,
                        ts.end_reason,
                        COALESCE(SUM(s.command_count), 0),
                        COALESCE(SUM(s.error_count), 0),
                        COALESCE(SUM(s.scrollback_bytes), 0),
                        COALESCE(MAX(s.scrollback_truncated), 0)
                 FROM tab_sessions ts
                 LEFT JOIN tab_session_panes tsp ON tsp.tab_session_id = ts.id
                 LEFT JOIN sessions s ON s.id = tsp.session_id
                 WHERE ts.tab_id = ?1
                 GROUP BY ts.id
                 ORDER BY ts.seq DESC
                 LIMIT ?2;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_list"))?;
        let rows = stmt
            .query_map(params![tab_str, fetch_limit], map_session_summary_row)
            .map_err(|e| DbError::from_rusqlite(e, "tab_sessions_list"))?;
        for r in rows {
            items.push(r.map_err(|e| DbError::from_rusqlite(e, "tab_sessions_list"))?);
        }
    }

    let limit_usize = usize::try_from(limit).unwrap_or(50);
    let has_more = items.len() > limit_usize;
    if has_more {
        items.truncate(limit_usize);
    }

    let next_cursor = if has_more && let Some(last) = items.last() {
        Some(encode_tab_session_cursor(tab_id, last.seq))
    } else {
        None
    };

    Ok(CursorPage { items, next_cursor })
}

fn encode_tab_session_cursor(tab_id: TabId, seq: i64) -> String {
    format!("ts1:{tab_id}:{seq}")
}

fn decode_tab_session_cursor(raw: &str, expected_tab_id: TabId) -> DbResult<i64> {
    let parts: Vec<&str> = raw.split(':').collect();
    if parts.len() != 3 || parts[0] != "ts1" {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        });
    }
    let cursor_tab = TabId::from_str(parts[1]).map_err(|_| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
    })?;
    if cursor_tab != expected_tab_id {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_SESSION_QUERY_INVALID),
        });
    }
    let seq = parts[2]
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

/// Recupera sessões e agrupamentos pendentes de um boot anterior encerrado abruptamente.
pub fn recover_crash_sessions_tx(tx: &Transaction<'_>, now_ms: i64) -> DbResult<u32> {
    // 1. Fecha blocos running órfãos sem inventar exit_code 0
    tx.execute(
        "UPDATE command_blocks
         SET status = 'interrupted_by_restart',
             end_reason = 'recovered_from_crash',
             ended_at = MAX(started_at, ?1),
             duration_ms = MAX(0, ?1 - started_at)
         WHERE status = 'running';",
        [now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "recover_crash_command_blocks"))?;

    // 2. Fecha e arquiva todas as sessões base que não estavam arquivadas
    tx.execute(
        "UPDATE sessions
         SET ended_at = COALESCE(
                 ended_at,
                 MAX(
                     started_at,
                     COALESCE((SELECT MAX(cb.ended_at) FROM command_blocks cb WHERE cb.session_id = sessions.id), 0),
                     COALESCE((SELECT MAX(sb.ts) FROM session_scrollback sb WHERE sb.session_id = sessions.id), 0),
                     ?1
                 )
             ),
             status = 'archived',
             archived_at = COALESCE(
                 archived_at,
                 MAX(
                     started_at,
                     COALESCE(ended_at, 0),
                     ?1
                 )
             )
         WHERE status <> 'archived' OR archived_at IS NULL;",
        [now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "recover_crash_sessions"))?;

    // 3. Fecha vínculos tab_session_panes abertos e cancela pending/waiting de boots anteriores
    tx.execute(
        "UPDATE tab_session_panes
         SET closed_at = COALESCE(
                 closed_at,
                 (SELECT s.ended_at FROM sessions s WHERE s.id = tab_session_panes.session_id),
                 ?1
             ),
             initial_dispatch = CASE
                 WHEN initial_dispatch IN ('pending', 'waiting') THEN 'none'
                 ELSE initial_dispatch
             END
         WHERE closed_at IS NULL;",
        [now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "recover_crash_tab_session_panes"))?;

    // 4. Arquiva todos os agrupamentos tab_sessions ativos com end_reason = 'crash_recovered'
    let recovered_groups = tx
        .execute(
            "UPDATE tab_sessions
             SET archived_at = MAX(
                     started_at,
                     COALESCE((
                         SELECT MAX(s.ended_at)
                         FROM tab_session_panes tsp
                         JOIN sessions s ON s.id = tsp.session_id
                         WHERE tsp.tab_session_id = tab_sessions.id
                     ), 0),
                     ?1
                 ),
                 end_reason = 'crash_recovered'
             WHERE archived_at IS NULL;",
            [now_ms],
        )
        .map_err(|e| DbError::from_rusqlite(e, "recover_crash_tab_sessions"))?;

    // 5. Marca painéis e abas de execuções anteriores como fechados antes do restore()
    tx.execute(
        "UPDATE panes SET closed_at = ?1 WHERE closed_at IS NULL;",
        [now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "recover_crash_panes"))?;

    tx.execute(
        "UPDATE tabs
         SET closed_at = ?1,
             revision = revision + 1
         WHERE closed_at IS NULL;",
        [now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "recover_crash_tabs"))?;

    Ok(u32::try_from(recovered_groups).unwrap_or(0))
}

/// Aplica a política de retenção de agrupamentos arquivados (`keep_per_tab` e `max_age_days`)
/// e limpa abas manuais fechadas que ficaram sem histórico.
pub fn enforce_tab_sessions_retention_tx(
    tx: &Transaction<'_>,
    keep_per_tab: u32,
    max_age_days: u32,
    now_ms: i64,
) -> DbResult<u64> {
    let keep = i64::from(keep_per_tab.clamp(1, 1000));
    let days = i64::from(max_age_days.clamp(1, 3650));
    let cutoff_ms = now_ms.saturating_sub(days.saturating_mul(86_400_000));

    // Coleta IDs de tab_sessions arquivados elegíveis para poda (nunca toca archived_at IS NULL)
    let to_prune: Vec<String> = {
        let mut stmt = tx
            .prepare_cached(
                "WITH ranked AS (
                     SELECT id,
                            tab_id,
                            archived_at,
                            ROW_NUMBER() OVER (PARTITION BY tab_id ORDER BY seq DESC) AS rn
                     FROM tab_sessions
                     WHERE archived_at IS NOT NULL
                 )
                 SELECT id
                 FROM ranked
                 WHERE rn > ?1 OR archived_at < ?2
                 LIMIT 500;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "retention_select_tab_sessions"))?;
        let rows = stmt
            .query_map(params![keep, cutoff_ms], |r| r.get::<_, String>(0))
            .map_err(|e| DbError::from_rusqlite(e, "retention_select_tab_sessions"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "retention_select_tab_sessions"))?
    };

    let mut deleted_groups = 0u64;
    for ts_id in &to_prune {
        // Coleta sessões base vinculadas ao agrupamento arquivado
        let base_ids: Vec<String> = {
            let mut stmt = tx
                .prepare_cached(
                    "SELECT session_id FROM tab_session_panes WHERE tab_session_id = ?1;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "retention_select_base_sessions"))?;
            let rows = stmt
                .query_map([ts_id], |r| r.get::<_, String>(0))
                .map_err(|e| DbError::from_rusqlite(e, "retention_select_base_sessions"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| DbError::from_rusqlite(e, "retention_select_base_sessions"))?
        };

        tx.execute(
            "DELETE FROM tab_session_panes WHERE tab_session_id = ?1;",
            [ts_id],
        )
        .map_err(|e| DbError::from_rusqlite(e, "retention_delete_tab_session_panes"))?;

        for sid in &base_ids {
            tx.execute(
                "DELETE FROM sessions WHERE id = ?1 AND status = 'archived';",
                [sid],
            )
            .map_err(|e| DbError::from_rusqlite(e, "retention_delete_base_session"))?;
        }

        let changed = tx
            .execute(
                "DELETE FROM tab_sessions WHERE id = ?1 AND archived_at IS NOT NULL;",
                [ts_id],
            )
            .map_err(|e| DbError::from_rusqlite(e, "retention_delete_tab_session"))?;
        deleted_groups += u64::try_from(changed).unwrap_or(0);
    }

    // Limpa painéis e abas manuais fechadas que não possuem mais nenhum histórico vinculado
    tx.execute(
        "DELETE FROM panes
         WHERE tab_id IN (
             SELECT t.id
             FROM tabs t
             WHERE t.origin = 'manual'
               AND t.closed_at IS NOT NULL
               AND NOT EXISTS (SELECT 1 FROM tab_sessions ts WHERE ts.tab_id = t.id)
               AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.tab_id = t.id)
         )
         AND NOT EXISTS (SELECT 1 FROM tab_session_panes tsp WHERE tsp.pane_id = panes.id);",
        [],
    )
    .map_err(|e| DbError::from_rusqlite(e, "retention_cleanup_closed_manual_panes"))?;

    tx.execute(
        "DELETE FROM tabs
         WHERE origin = 'manual'
           AND closed_at IS NOT NULL
           AND NOT EXISTS (SELECT 1 FROM tab_sessions ts WHERE ts.tab_id = tabs.id)
           AND NOT EXISTS (SELECT 1 FROM sessions s WHERE s.tab_id = tabs.id)
           AND NOT EXISTS (SELECT 1 FROM panes p WHERE p.tab_id = tabs.id);",
        [],
    )
    .map_err(|e| DbError::from_rusqlite(e, "retention_cleanup_closed_manual_tabs"))?;

    Ok(deleted_groups)
}

fn map_session_summary_row(row: &Row<'_>) -> rusqlite::Result<SessionSummary> {
    let ts_raw: String = row.get(0)?;
    let tab_raw: String = row.get(1)?;
    let seq: i64 = row.get(2)?;
    let started_at: i64 = row.get(3)?;
    let archived_at: Option<i64> = row.get(4)?;
    let end_reason_raw: Option<String> = row.get(5)?;
    let command_count: i64 = row.get(6)?;
    let error_count: i64 = row.get(7)?;
    let scrollback_bytes: i64 = row.get(8)?;
    let truncated_int: i64 = row.get(9)?;

    let tab_session_id = TabSessionId::from_str(&ts_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::from("TabSessionId inválido"),
        )
    })?;
    let tab_id = TabId::from_str(&tab_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::from("TabId inválido"),
        )
    })?;
    let end_reason = match end_reason_raw {
        Some(ref s) => Some(SessionEndReason::from_db_str(s).ok_or_else(|| {
            rusqlite::Error::FromSqlConversionFailure(
                5,
                rusqlite::types::Type::Text,
                Box::from("SessionEndReason inválido"),
            )
        })?),
        None => None,
    };

    Ok(SessionSummary {
        id: tab_session_id,
        tab_id,
        seq,
        archived: archived_at.is_some(),
        started_at,
        ended_at: archived_at,
        end_reason,
        command_count,
        failed_command_count: error_count,
        scrollback_bytes,
        truncated: truncated_int != 0,
    })
}
