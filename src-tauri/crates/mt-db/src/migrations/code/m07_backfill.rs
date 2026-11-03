//! Backfill transacional da Fase 07 (`m07_backfill`):
//! 1. Garante que todos os projetos existentes possuam exatamente um template padrão (`TemplatesRepo::ensure_default_tx`).
//! 2. Vincula de forma não destrutiva sessões históricas da Fase 05 cujo `tab_id` e `project_id`
//!    sejam comprováveis, criando registros fechados de aba/painel/agrupamento quando ausentes
//!    e conservando intactas as sessões sem vínculo comprovável.

use std::collections::BTreeMap;
use std::str::FromStr;

use mt_core::ids::{PaneId, ProjectId, SessionId, TabId, TabSessionId};
use mt_core::workspace::{EnvironmentKind, LaunchSnapshot, PaneLayoutNode, TemplateEnvironment};
use rusqlite::{OptionalExtension, Transaction, params};

use crate::error::{DbError, DbResult};
use crate::migrations::registry::CodeMigration;
use crate::repos::templates::ensure_all_projects_have_default_tx;
use crate::time::now_unix_ms;

/// Semente canônica para cálculo de checksum quando registrada como `CodeMigration`.
pub const M07_BACKFILL_SEED: &str = "m07_backfill_v1_templates_and_legacy_sessions";

/// Relatório produzido pela execução do backfill da Fase 07.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BackfillReport {
    /// Quantidade de projetos que receberam template padrão no backfill.
    pub default_templates_created: usize,
    /// Quantidade de sessões base vinculadas a agrupamentos `tab_sessions`.
    pub linked_sessions: usize,
    /// Quantidade de sessões base preservadas sem vínculo comprovável de aba/projeto.
    pub unlinked_sessions: usize,
}

/// Constrói uma [`CodeMigration`] associada a [`run_code_migration_tx`].
#[must_use]
pub fn code_migration(version: i64) -> CodeMigration {
    CodeMigration::new(
        version,
        7,
        "0004_07_m07_backfill",
        M07_BACKFILL_SEED,
        run_code_migration_tx,
    )
}

/// Wrapper compatível com [`crate::migrations::CodeMigrationFn`].
pub fn run_code_migration_tx(tx: &Transaction<'_>) -> DbResult<()> {
    let _ = run_backfill_tx(tx)?;
    Ok(())
}

#[derive(Debug)]
struct LegacySessionRow {
    id: SessionId,
    tab_id: Option<String>,
    pane_id: Option<String>,
    project_id: Option<String>,
    tab_session_id: Option<String>,
    initial_cwd: String,
    last_cwd: Option<String>,
    started_at: i64,
    ended_at: Option<i64>,
}

/// Executa o backfill idempotente de templates padrão e sessões históricas dentro da transação `tx`.
#[allow(clippy::too_many_lines)]
pub fn run_backfill_tx(tx: &Transaction<'_>) -> DbResult<BackfillReport> {
    let now_ms = now_unix_ms();
    let default_templates_created = ensure_all_projects_have_default_tx(tx, now_ms)?;

    let legacy_rows: Vec<LegacySessionRow> = {
        let mut stmt = tx
            .prepare(
                "SELECT s.id, s.tab_id, s.pane_id, s.project_id, s.tab_session_id,
                        s.initial_cwd, s.last_cwd, s.started_at, s.ended_at
                 FROM sessions s
                 LEFT JOIN tab_session_panes tsp ON tsp.session_id = s.id
                 WHERE tsp.session_id IS NULL
                 ORDER BY s.started_at ASC, s.id ASC;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_select_sessions"))?;

        let mapped = stmt
            .query_map([], |r| {
                let id_raw: String = r.get(0)?;
                let id = SessionId::from_str(&id_raw).map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::from("SessionId inválido"),
                    )
                })?;
                Ok(LegacySessionRow {
                    id,
                    tab_id: r.get(1)?,
                    pane_id: r.get(2)?,
                    project_id: r.get(3)?,
                    tab_session_id: r.get(4)?,
                    initial_cwd: r.get(5)?,
                    last_cwd: r.get(6)?,
                    started_at: r.get(7)?,
                    ended_at: r.get(8)?,
                })
            })
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_select_sessions"))?;

        mapped
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_select_sessions"))?
    };

    let mut linked_sessions = 0usize;
    let mut unlinked_sessions = 0usize;

    for row in legacy_rows {
        let Some(raw_tab_id) = row.tab_id.as_deref() else {
            unlinked_sessions += 1;
            continue;
        };
        let Ok(tab_id) = TabId::from_str(raw_tab_id) else {
            unlinked_sessions += 1;
            continue;
        };

        // Determina o project_id comprovável (da própria aba já existente ou da sessão quando o projeto existe)
        let existing_tab_proj: Option<String> = tx
            .query_row(
                "SELECT project_id FROM tabs WHERE id = ?1;",
                [raw_tab_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_find_tab"))?;

        let resolved_project_id: Option<ProjectId> = if let Some(p_raw) = existing_tab_proj {
            ProjectId::from_str(&p_raw).ok()
        } else if let Some(ref p_raw) = row.project_id
            && let Ok(pid) = ProjectId::from_str(p_raw)
        {
            let exists: i64 = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1);",
                    [p_raw],
                    |r| r.get(0),
                )
                .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_check_project"))?;
            if exists != 0 { Some(pid) } else { None }
        } else {
            None
        };

        let Some(project_id) = resolved_project_id else {
            unlinked_sessions += 1;
            continue;
        };

        let pane_id = row
            .pane_id
            .as_deref()
            .and_then(|s| PaneId::from_str(s).ok())
            .unwrap_or_else(PaneId::new);
        let pane_id_str = pane_id.to_string();
        let tab_id_str = tab_id.to_string();
        let project_id_str = project_id.to_string();
        let cwd = row
            .last_cwd
            .clone()
            .unwrap_or_else(|| row.initial_cwd.clone());
        let closed_ts = row.ended_at.unwrap_or(row.started_at).max(row.started_at);

        // Se a aba não existir em `tabs`, cria registro fechado preservando o histórico
        let tab_exists: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM tabs WHERE id = ?1);",
                [&tab_id_str],
                |r| r.get(0),
            )
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_tab_exists"))?;

        if tab_exists == 0 {
            let launch_snap = LaunchSnapshot {
                template_id: None,
                title: String::from("Terminal"),
                icon: String::from("mdi:console"),
                resolved_cwd: cwd.clone(),
                configured_cwd: None,
                initial_command: None,
                continuous: false,
                environment: TemplateEnvironment {
                    kind: EnvironmentKind::Shell,
                    agent_id: None,
                },
                env_vars: BTreeMap::new(),
                connection: mt_core::workspace::TemplateConnection::Local,
            };
            let launch_json =
                serde_json::to_string(&launch_snap).unwrap_or_else(|_| String::from("{}"));
            let layout_json = serde_json::to_string(&PaneLayoutNode::single(pane_id))
                .unwrap_or_else(|_| String::from("{}"));

            tx.execute(
                "INSERT INTO tabs (
                    id, project_id, template_id, origin, title, icon, position,
                    continuous, launch_json, layout_json, active_pane_id, primary_pane_id,
                    revision, layout_revision, last_session_seq, opened_at, closed_at, last_focused_at
                 ) VALUES (
                    ?1, ?2, NULL, 'manual', 'Terminal', 'mdi:console', 0,
                    0, ?3, ?4, ?5, ?5,
                    1, 1, 0, ?6, ?7, ?7
                 );",
                params![
                    tab_id_str,
                    project_id_str,
                    launch_json,
                    layout_json,
                    pane_id_str,
                    row.started_at,
                    closed_ts,
                ],
            )
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_insert_tab"))?;
        }

        // Garante que o painel exista em `panes`
        let pane_exists: i64 = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM panes WHERE id = ?1);",
                [&pane_id_str],
                |r| r.get(0),
            )
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_pane_exists"))?;

        if pane_exists == 0 {
            tx.execute(
                "INSERT INTO panes (id, tab_id, cwd, generation, created_at, closed_at)
                 VALUES (?1, ?2, ?3, 1, ?4, ?5);",
                params![pane_id_str, tab_id_str, cwd, row.started_at, closed_ts],
            )
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_insert_pane"))?;
        }

        // Sem evidência de agrupamento antigo, cada sessão base vira um grupo histórico separado
        let next_seq: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(seq), 0) + 1 FROM tab_sessions WHERE tab_id = ?1;",
                [&tab_id_str],
                |r| r.get(0),
            )
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_next_tab_session_seq"))?;

        let tab_session_id = row
            .tab_session_id
            .as_deref()
            .and_then(|s| TabSessionId::from_str(s).ok())
            .unwrap_or_else(TabSessionId::new);
        let ts_id_str = tab_session_id.to_string();

        tx.execute(
            "INSERT OR IGNORE INTO tab_sessions (id, tab_id, seq, started_at, archived_at, end_reason)
             VALUES (?1, ?2, ?3, ?4, ?5, 'crash_recovered');",
            params![ts_id_str, tab_id_str, next_seq, row.started_at, closed_ts],
        )
        .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_insert_tab_session"))?;

        tx.execute(
            "UPDATE tabs SET last_session_seq = MAX(last_session_seq, ?2) WHERE id = ?1;",
            params![tab_id_str, next_seq],
        )
        .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_update_tab_seq"))?;

        let next_gen: i64 = tx
            .query_row(
                "SELECT COALESCE(MAX(generation), 0) + 1
                 FROM tab_session_panes
                 WHERE tab_session_id = ?1 AND pane_id = ?2;",
                params![ts_id_str, pane_id_str],
                |r| r.get(0),
            )
            .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_next_gen"))?;

        let session_id_str = row.id.to_string();
        tx.execute(
            "INSERT INTO tab_session_panes (
                session_id, tab_session_id, pane_id, generation, joined_at, closed_at, initial_dispatch
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'none');",
            params![
                session_id_str,
                ts_id_str,
                pane_id_str,
                next_gen,
                row.started_at,
                closed_ts
            ],
        )
        .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_insert_tsp"))?;

        tx.execute(
            "UPDATE sessions
             SET tab_id = ?2,
                 pane_id = ?3,
                 project_id = ?4,
                 tab_session_id = ?5,
                 ended_at = COALESCE(ended_at, ?6),
                 archived_at = COALESCE(archived_at, ?6),
                 status = 'archived'
             WHERE id = ?1;",
            params![
                session_id_str,
                tab_id_str,
                pane_id_str,
                project_id_str,
                ts_id_str,
                closed_ts,
            ],
        )
        .map_err(|e| DbError::from_rusqlite(e, "m07_backfill_update_session"))?;

        linked_sessions += 1;
    }

    Ok(BackfillReport {
        default_templates_created,
        linked_sessions,
        unlinked_sessions,
    })
}
