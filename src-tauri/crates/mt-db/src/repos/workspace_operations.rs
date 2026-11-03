//! Repositório transacional da tabela `workspace_operations` (ledger de idempotência e recuperação da Fase 07).

use std::str::FromStr;

use mt_core::ids::TabId;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};

/// Sentinela de constraint indicando conflito de argumentos para um mesmo `request_id`.
pub const CONSTRAINT_WORKSPACE_REQUEST_CONFLICT: &str = "workspace.request_conflict";
/// Sentinela de constraint indicando que a operação pertencia a um boot anterior e foi interrompida.
pub const CONSTRAINT_WORKSPACE_OPERATION_INTERRUPTED: &str = "workspace.operation_interrupted";

/// Estágio de uma operação mutável no ledger `workspace_operations`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperationStage {
    /// Intenção reservada antes do commit/spawn.
    Prepared,
    /// Encerrando PTYs existentes antes do arquivamento/substituição.
    Stopping,
    /// Transação SQLite de metadados comitada, aguardando spawn/finalização.
    Committed,
    /// Operação concluída com sucesso (`result_json` preenchido quando aplicável).
    Completed,
    /// Operação bloqueada por falha ao encerrar PTY/descendentes (`workspace.stop_failed`).
    Blocked,
    /// Operação de boot anterior interrompida por crash/encerramento abrupto.
    Interrupted,
}

impl OperationStage {
    /// Representação textual no SQLite (`CHECK (stage IN (...))`).
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::Stopping => "stopping",
            Self::Committed => "committed",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
            Self::Interrupted => "interrupted",
        }
    }

    /// Converte do texto persistido no SQLite.
    #[must_use]
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "prepared" => Some(Self::Prepared),
            "stopping" => Some(Self::Stopping),
            "committed" => Some(Self::Committed),
            "completed" => Some(Self::Completed),
            "blocked" => Some(Self::Blocked),
            "interrupted" => Some(Self::Interrupted),
            _ => None,
        }
    }
}

/// Linha persistida da tabela `workspace_operations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceOperationRow {
    /// Identificador de idempotência da requisição.
    pub request_id: String,
    /// Tipo da operação (`tab_open`, `tab_close`, `pane_split`, `pane_close`, `pane_restart`, `session_new`, `initial_command_run`).
    pub kind: String,
    /// Hash SHA-256 hexadecimal dos argumentos canônicos validados (nunca armazena comando/env bruto).
    pub args_hash: String,
    /// Identificador opcional da aba alvo/criada.
    pub tab_id: Option<TabId>,
    /// Identificador do boot em que a operação foi iniciada.
    pub boot_id: String,
    /// Estágio atual da operação.
    pub stage: OperationStage,
    /// Resultado JSON sanitizado (contendo apenas IDs/metadados públicos).
    pub result_json: Option<String>,
    /// Timestamp UNIX em ms de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms da última atualização.
    pub updated_at: i64,
}

/// Resultado da tentativa de reservar uma operação no ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareOperationOutcome {
    /// Novo registro criado no estágio `Prepared`.
    Fresh(WorkspaceOperationRow),
    /// Registro existente no mesmo boot e com os mesmos argumentos (`kind` e `args_hash`).
    ExistingSameArgs(WorkspaceOperationRow),
}

/// Repositório tipado para a tabela `workspace_operations`.
#[derive(Clone, Debug)]
pub struct WorkspaceOperationsRepo {
    db: Db,
}

impl WorkspaceOperationsRepo {
    /// Cria uma nova instância de [`WorkspaceOperationsRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Busca uma operação pelo seu `request_id`.
    pub async fn get(&self, request_id: String) -> DbResult<Option<WorkspaceOperationRow>> {
        self.db
            .read(move |conn| get_operation_conn(conn, &request_id))
            .await
    }

    /// Reserva ou reutiliza uma operação no ledger validando `kind`, `args_hash` e `boot_id`.
    pub async fn prepare(
        &self,
        request_id: String,
        kind: String,
        args_hash: String,
        tab_id: Option<TabId>,
        boot_id: String,
        now_ms: i64,
    ) -> DbResult<PrepareOperationOutcome> {
        self.db
            .write_sized("workspace_ops_prepare", 512, move |tx| {
                prepare_operation_tx(tx, &request_id, &kind, &args_hash, tab_id, &boot_id, now_ms)
            })
            .await
    }

    /// Atualiza o estágio, `tab_id` e opcionalmente `result_json` de uma operação existente.
    pub async fn update_stage(
        &self,
        request_id: String,
        stage: OperationStage,
        tab_id: Option<TabId>,
        result_json: Option<String>,
        now_ms: i64,
    ) -> DbResult<()> {
        let approx = 256 + result_json.as_ref().map_or(0, String::len);
        self.db
            .write_sized("workspace_ops_update_stage", approx, move |tx| {
                update_operation_stage_tx(
                    tx,
                    &request_id,
                    stage,
                    tab_id,
                    result_json.as_deref(),
                    now_ms,
                )
            })
            .await
    }

    /// Marca todas as operações não concluídas de boots anteriores como `interrupted`.
    pub async fn mark_previous_boots_interrupted(
        &self,
        current_boot_id: String,
        now_ms: i64,
    ) -> DbResult<u64> {
        self.db
            .write_sized("workspace_ops_mark_interrupted", 512, move |tx| {
                mark_previous_boots_interrupted_tx(tx, &current_boot_id, now_ms)
            })
            .await
    }

    /// Versão síncrona de [`Self::mark_previous_boots_interrupted`].
    pub fn mark_previous_boots_interrupted_blocking(
        &self,
        current_boot_id: &str,
        now_ms: i64,
    ) -> DbResult<u64> {
        let boot_owned = current_boot_id.to_string();
        self.db
            .write_blocking("workspace_ops_mark_interrupted", 512, move |tx| {
                mark_previous_boots_interrupted_tx(tx, &boot_owned, now_ms)
            })
    }
}

/// Busca uma operação na conexão/transação informada.
pub fn get_operation_conn(
    conn: &Connection,
    request_id: &str,
) -> DbResult<Option<WorkspaceOperationRow>> {
    conn.query_row(
        "SELECT request_id, kind, args_hash, tab_id, boot_id, stage, result_json, created_at, updated_at
         FROM workspace_operations
         WHERE request_id = ?1;",
        [request_id],
        map_operation_row,
    )
    .optional()
    .map_err(|e| DbError::from_rusqlite(e, "workspace_ops_get"))
}

/// Reserva ou valida uma operação dentro de uma transação `BEGIN IMMEDIATE`.
pub fn prepare_operation_tx(
    tx: &Transaction<'_>,
    request_id: &str,
    kind: &str,
    args_hash: &str,
    tab_id: Option<TabId>,
    boot_id: &str,
    now_ms: i64,
) -> DbResult<PrepareOperationOutcome> {
    if let Some(existing) = get_operation_conn(tx, request_id)? {
        if existing.kind != kind || existing.args_hash != args_hash {
            return Err(DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_WORKSPACE_REQUEST_CONFLICT),
            });
        }
        if existing.stage == OperationStage::Interrupted
            || (existing.boot_id != boot_id && existing.stage != OperationStage::Completed)
        {
            return Err(DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_WORKSPACE_OPERATION_INTERRUPTED),
            });
        }
        if existing.stage == OperationStage::Blocked {
            tx.execute(
                "UPDATE workspace_operations SET updated_at = ?2 WHERE request_id = ?1;",
                params![request_id, now_ms],
            )
            .map_err(|e| DbError::from_rusqlite(e, "workspace_ops_touch_blocked"))?;
        }
        return Ok(PrepareOperationOutcome::ExistingSameArgs(existing));
    }

    let row = WorkspaceOperationRow {
        request_id: request_id.to_string(),
        kind: kind.to_string(),
        args_hash: args_hash.to_string(),
        tab_id,
        boot_id: boot_id.to_string(),
        stage: OperationStage::Prepared,
        result_json: None,
        created_at: now_ms,
        updated_at: now_ms,
    };

    tx.execute(
        "INSERT INTO workspace_operations (
            request_id, kind, args_hash, tab_id, boot_id, stage, result_json, created_at, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?7);",
        params![
            row.request_id,
            row.kind,
            row.args_hash,
            row.tab_id.map(|t| t.to_string()),
            row.boot_id,
            row.stage.as_db_str(),
            now_ms,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "workspace_ops_insert"))?;

    Ok(PrepareOperationOutcome::Fresh(row))
}

/// Atualiza o estágio de uma operação dentro de uma transação existente.
pub fn update_operation_stage_tx(
    tx: &Transaction<'_>,
    request_id: &str,
    stage: OperationStage,
    tab_id: Option<TabId>,
    result_json: Option<&str>,
    now_ms: i64,
) -> DbResult<()> {
    tx.execute(
        "UPDATE workspace_operations
         SET stage = ?2,
             tab_id = COALESCE(?3, tab_id),
             result_json = COALESCE(?4, result_json),
             updated_at = ?5
         WHERE request_id = ?1;",
        params![
            request_id,
            stage.as_db_str(),
            tab_id.map(|t| t.to_string()),
            result_json,
            now_ms,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "workspace_ops_update_stage"))?;
    Ok(())
}

/// Marca operações incompletas de boots anteriores como `interrupted`.
pub fn mark_previous_boots_interrupted_tx(
    tx: &Transaction<'_>,
    current_boot_id: &str,
    now_ms: i64,
) -> DbResult<u64> {
    let changed = tx
        .execute(
            "UPDATE workspace_operations
             SET stage = 'interrupted',
                 updated_at = ?2
             WHERE boot_id <> ?1
               AND stage IN ('prepared', 'stopping', 'committed', 'blocked');",
            params![current_boot_id, now_ms],
        )
        .map_err(|e| DbError::from_rusqlite(e, "workspace_ops_mark_interrupted"))?;
    Ok(u64::try_from(changed).unwrap_or(0))
}

fn map_operation_row(row: &Row<'_>) -> rusqlite::Result<WorkspaceOperationRow> {
    let request_id: String = row.get(0)?;
    let kind: String = row.get(1)?;
    let args_hash: String = row.get(2)?;
    let tab_raw: Option<String> = row.get(3)?;
    let boot_id: String = row.get(4)?;
    let stage_raw: String = row.get(5)?;
    let result_json: Option<String> = row.get(6)?;
    let created_at: i64 = row.get(7)?;
    let updated_at: i64 = row.get(8)?;

    let tab_id = match tab_raw {
        Some(s) => Some(TabId::from_str(&s).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                3,
                rusqlite::types::Type::Text,
                Box::from("TabId inválido"),
            )
        })?),
        None => None,
    };
    let stage = OperationStage::from_db_str(&stage_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::from("OperationStage inválido"),
        )
    })?;

    Ok(WorkspaceOperationRow {
        request_id,
        kind,
        args_hash,
        tab_id,
        boot_id,
        stage,
        result_json,
        created_at,
        updated_at,
    })
}
