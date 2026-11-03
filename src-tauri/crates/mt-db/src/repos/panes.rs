//! Repositório transacional da tabela `panes` (Fase 07).

use std::str::FromStr;

use mt_core::ids::{PaneId, TabId};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::sanitizer::SanitizedText;

/// Sentinela de constraint indicando painel não encontrado.
pub const CONSTRAINT_PANE_NOT_FOUND: &str = "pane.not_found";

/// Linha persistida da tabela `panes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredPaneRow {
    /// Identificador único do painel (UUID v7).
    pub id: PaneId,
    /// Identificador da aba proprietária.
    pub tab_id: TabId,
    /// Diretório de trabalho atual sanitizado do painel.
    pub cwd: String,
    /// Geração monotônica de execução do painel (`>= 1` quando ativo).
    pub generation: i64,
    /// Timestamp UNIX em ms de criação do painel.
    pub created_at: i64,
    /// Timestamp UNIX em ms de fechamento do painel (`None` enquanto aberto).
    pub closed_at: Option<i64>,
}

/// Repositório tipado para operações sobre a tabela `panes`.
#[derive(Clone, Debug)]
pub struct PanesRepo {
    db: Db,
}

impl PanesRepo {
    /// Cria uma nova instância de [`PanesRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Busca um painel pelo seu [`PaneId`].
    pub async fn get(&self, pane_id: PaneId) -> DbResult<Option<StoredPaneRow>> {
        self.db.read(move |conn| get_pane_conn(conn, pane_id)).await
    }

    /// Lista todos os painéis abertos (`closed_at IS NULL`) de uma aba.
    pub async fn list_open_by_tab(&self, tab_id: TabId) -> DbResult<Vec<StoredPaneRow>> {
        self.db
            .read(move |conn| list_open_panes_by_tab_conn(conn, tab_id))
            .await
    }

    /// Atualiza o `cwd` persistido de um painel.
    pub async fn update_cwd(&self, pane_id: PaneId, cwd: SanitizedText) -> DbResult<()> {
        let approx = 128 + cwd.len();
        self.db
            .write_sized("panes_update_cwd", approx, move |tx| {
                update_pane_cwd_tx(tx, pane_id, &cwd)
            })
            .await
    }
}

/// Insere uma nova linha em `panes` dentro de uma transação existente.
pub fn insert_pane_tx(tx: &Transaction<'_>, row: &StoredPaneRow) -> DbResult<()> {
    let cwd_sanitized = SanitizedText::new(&row.cwd);
    tx.execute(
        "INSERT INTO panes (id, tab_id, cwd, generation, created_at, closed_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
        params![
            row.id.to_string(),
            row.tab_id.to_string(),
            cwd_sanitized.as_str(),
            row.generation,
            row.created_at,
            row.closed_at,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "panes_insert"))?;
    Ok(())
}

/// Busca um painel na conexão/transação informada.
pub fn get_pane_conn(conn: &Connection, pane_id: PaneId) -> DbResult<Option<StoredPaneRow>> {
    let id_str = pane_id.to_string();
    conn.query_row(
        "SELECT id, tab_id, cwd, generation, created_at, closed_at
         FROM panes
         WHERE id = ?1;",
        [&id_str],
        map_pane_row,
    )
    .optional()
    .map_err(|e| DbError::from_rusqlite(e, "panes_get"))
}

/// Lista os painéis abertos (`closed_at IS NULL`) de uma aba na conexão/transação informada.
pub fn list_open_panes_by_tab_conn(
    conn: &Connection,
    tab_id: TabId,
) -> DbResult<Vec<StoredPaneRow>> {
    let tab_str = tab_id.to_string();
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, tab_id, cwd, generation, created_at, closed_at
             FROM panes
             WHERE tab_id = ?1 AND closed_at IS NULL
             ORDER BY created_at ASC, id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "panes_list_open"))?;
    let rows = stmt
        .query_map([&tab_str], map_pane_row)
        .map_err(|e| DbError::from_rusqlite(e, "panes_list_open"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "panes_list_open"))
}

/// Lista todos os painéis (abertos ou fechados) de uma aba na conexão/transação informada.
pub fn list_all_panes_by_tab_conn(
    conn: &Connection,
    tab_id: TabId,
) -> DbResult<Vec<StoredPaneRow>> {
    let tab_str = tab_id.to_string();
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, tab_id, cwd, generation, created_at, closed_at
             FROM panes
             WHERE tab_id = ?1
             ORDER BY created_at ASC, id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "panes_list_all"))?;
    let rows = stmt
        .query_map([&tab_str], map_pane_row)
        .map_err(|e| DbError::from_rusqlite(e, "panes_list_all"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "panes_list_all"))
}

/// Atualiza o `cwd` de um painel dentro de uma transação.
pub fn update_pane_cwd_tx(
    tx: &Transaction<'_>,
    pane_id: PaneId,
    cwd: &SanitizedText,
) -> DbResult<()> {
    let id_str = pane_id.to_string();
    let changed = tx
        .execute(
            "UPDATE panes SET cwd = ?2 WHERE id = ?1;",
            params![id_str, cwd.as_str()],
        )
        .map_err(|e| DbError::from_rusqlite(e, "panes_update_cwd"))?;
    if changed == 0 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PANE_NOT_FOUND),
        });
    }
    Ok(())
}

/// Incrementa a geração (`generation + 1`) de um painel, opcionalmente atualizando seu `cwd` e limpando `closed_at`.
pub fn bump_pane_generation_tx(
    tx: &Transaction<'_>,
    pane_id: PaneId,
    new_cwd: Option<&SanitizedText>,
) -> DbResult<StoredPaneRow> {
    let id_str = pane_id.to_string();
    let changed = match new_cwd {
        Some(cwd) => tx
            .execute(
                "UPDATE panes
                 SET generation = generation + 1,
                     cwd = ?2,
                     closed_at = NULL
                 WHERE id = ?1;",
                params![id_str, cwd.as_str()],
            )
            .map_err(|e| DbError::from_rusqlite(e, "panes_bump_generation"))?,
        None => tx
            .execute(
                "UPDATE panes
                 SET generation = generation + 1,
                     closed_at = NULL
                 WHERE id = ?1;",
                [&id_str],
            )
            .map_err(|e| DbError::from_rusqlite(e, "panes_bump_generation"))?,
    };
    if changed == 0 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PANE_NOT_FOUND),
        });
    }
    get_pane_conn(tx, pane_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_PANE_NOT_FOUND),
    })
}

/// Marca um painel como fechado (`closed_at`).
pub fn close_pane_row_tx(tx: &Transaction<'_>, pane_id: PaneId, closed_at: i64) -> DbResult<()> {
    let id_str = pane_id.to_string();
    tx.execute(
        "UPDATE panes SET closed_at = COALESCE(closed_at, ?2) WHERE id = ?1;",
        params![id_str, closed_at],
    )
    .map_err(|e| DbError::from_rusqlite(e, "panes_close"))?;
    Ok(())
}

pub(crate) fn map_pane_row(row: &Row<'_>) -> rusqlite::Result<StoredPaneRow> {
    let id_raw: String = row.get(0)?;
    let tab_raw: String = row.get(1)?;
    let id = PaneId::from_str(&id_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::from("PaneId inválido"),
        )
    })?;
    let tab_id = TabId::from_str(&tab_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::from("TabId inválido"),
        )
    })?;
    Ok(StoredPaneRow {
        id,
        tab_id,
        cwd: row.get(2)?,
        generation: row.get(3)?,
        created_at: row.get(4)?,
        closed_at: row.get(5)?,
    })
}
