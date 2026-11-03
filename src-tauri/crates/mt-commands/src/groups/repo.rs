//! Repositório SQLite transacional (`CommandGroupRepo` e `SqliteCommandGroupRepo`) para
//! Grupos de Comandos e Histórico de Execuções da Fase 15.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use mt_core::{
    CommandBlockId, CommandGroupId, MtError, MtResult, PaneId, ProjectId, PtyId, SessionId, TabId,
};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use crate::groups::aggregate::{can_transition_item, compute_counts, reduce_run_status};
use crate::groups::model::{
    CommandGroup, CommandGroupCatalog, CommandGroupInput, CommandGroupItem, DeleteGroupReceipt,
    ExecutionMode, GroupErrorCode, GroupItemId, GroupRequestId, GroupRun, GroupRunId, GroupRunItem,
    GroupRunItemId, GroupRunItemSnapshot, GroupRunListPage, GroupRunSnapshot, GroupRunSummary,
    GroupScope, ItemStatus, ReorderGroupEntry, RunStatus, SequentialSeparator,
};
use crate::groups::validate::validate_group_input;

/// Limite máximo em bytes da cauda de saída persistida por item (`256 KiB`).
pub const MAX_PERSISTED_OUTPUT_TAIL_BYTES: usize = 256 * 1024;

/// Parâmetros para criação atômica de um registro `command_group_runs` + `command_group_run_items`.
#[derive(Debug, Clone)]
pub struct CreateRunRecord {
    /// Identificador único da execução.
    pub run_id: GroupRunId,
    /// Chave de idempotência do cliente.
    pub request_id: GroupRequestId,
    /// Hash SHA-256 canônico do payload da requisição.
    pub request_hash: String,
    /// Grupo de comandos associado.
    pub group_id: CommandGroupId,
    /// Projeto alvo.
    pub project_id: ProjectId,
    /// Sessão de origem opcional.
    pub origin_session_id: Option<SessionId>,
    /// Snapshot imutável do grupo.
    pub snapshot: GroupRunSnapshot,
    /// Modo efetivo de execução.
    pub mode: ExecutionMode,
    /// Lista ordenada de `(GroupRunItemId, GroupRunItemSnapshot)`.
    pub items: Vec<(GroupRunItemId, GroupRunItemSnapshot)>,
    /// Timestamp de criação (epoch ms).
    pub created_at: i64,
}

/// Resultado da tentativa de aceitar/registrar uma execução por `request_id`.
#[derive(Debug, Clone)]
pub enum AcceptRunOutcome {
    /// Nova execução criada na transação com todos os itens em `pending`.
    Created(GroupRun),
    /// `request_id` já existia com o mesmo `request_hash`; retorna a execução existente.
    Existing(GroupRun),
}

/// Atualização atômica de um item de execução com recomputação automática do estado agregado e `version` do run.
#[derive(Debug, Clone, Default)]
pub struct RunItemPatch {
    /// Novo estado do item.
    pub status: Option<ItemStatus>,
    /// Atualização de `pty_id`.
    pub pty_id: Option<Option<PtyId>>,
    /// Atualização de `tab_id`.
    pub tab_id: Option<Option<TabId>>,
    /// Atualização de `pane_id`.
    pub pane_id: Option<Option<PaneId>>,
    /// Atualização de `session_id`.
    pub session_id: Option<Option<SessionId>>,
    /// Atualização de `command_block_id`.
    pub command_block_id: Option<Option<CommandBlockId>>,
    /// Atualização de `exit_code`.
    pub exit_code: Option<Option<i32>>,
    /// Atualização de `signal`.
    pub signal: Option<Option<i32>>,
    /// Atualização de `error_code`.
    pub error_code: Option<Option<String>>,
    /// Atualização de `error_message`.
    pub error_message: Option<Option<String>>,
    /// Atualização de `started_at`.
    pub started_at: Option<Option<i64>>,
    /// Atualização de `finished_at`.
    pub finished_at: Option<Option<i64>>,
    /// Cauda sanitizada e flag de truncamento `(cauda, truncada)`.
    pub output_tail: Option<(String, bool)>,
}

/// Repositório síncrono/transacional sobre `mt-db` para Grupos de Comandos e Histórico de Execuções.
pub trait CommandGroupRepo: Send + Sync {
    /// Lista os grupos locais do projeto (quando informado) e todos os grupos globais.
    fn list(&self, project_id: Option<&ProjectId>) -> MtResult<CommandGroupCatalog>;

    /// Obtém um grupo pelo ID.
    fn get(&self, group_id: &CommandGroupId) -> MtResult<CommandGroup>;

    /// Cria um novo grupo validando limites, unicidade e ausência de segredos.
    fn create(&self, input: &CommandGroupInput) -> MtResult<CommandGroup>;

    /// Atualiza um grupo verificando `expected_revision` e imutabilidade de `(scope, project_id)`.
    fn update(
        &self,
        group_id: &CommandGroupId,
        expected_revision: u64,
        input: &CommandGroupInput,
    ) -> MtResult<CommandGroup>;

    /// Exclui um grupo verificando `expected_revision`.
    fn delete(
        &self,
        group_id: &CommandGroupId,
        expected_revision: u64,
    ) -> MtResult<DeleteGroupReceipt>;

    /// Reordena atomicamente todos os grupos de um `(scope, project_id)`.
    fn reorder(
        &self,
        scope: GroupScope,
        project_id: Option<&ProjectId>,
        entries: &[ReorderGroupEntry],
    ) -> MtResult<CommandGroupCatalog>;

    /// Consulta rápida por `request_id` antes de validar revisão atual do grupo (§4.1 item 1).
    fn find_run_by_request_id(
        &self,
        request_id: &GroupRequestId,
    ) -> MtResult<Option<(String, GroupRun)>>;

    /// Insere o snapshot da execução e todos os seus itens `pending` em uma única transação.
    fn accept_run(&self, record: CreateRunRecord) -> MtResult<AcceptRunOutcome>;

    /// Obtém uma execução completa com todos os seus itens.
    fn get_run(&self, run_id: &GroupRunId) -> MtResult<GroupRun>;

    /// Lista execuções de um projeto com paginação keyset (`created_at DESC, id DESC`).
    fn list_runs(
        &self,
        project_id: &ProjectId,
        cursor: Option<&str>,
        limit: u32,
        active_only: bool,
    ) -> MtResult<GroupRunListPage>;

    /// Marca `cancel_requested_at`, cancela itens `pending` (ou apenas um item específico)
    /// e recalcula o estado agregado e `version` do run na mesma transação.
    fn mark_cancel_requested(
        &self,
        run_id: &GroupRunId,
        item_id: Option<&GroupRunItemId>,
        now_ms: i64,
    ) -> MtResult<(GroupRun, Vec<GroupRunItem>)>;

    /// Aplica transição em um item, atualiza `last_exit_code`, `started_at`, `finished_at`,
    /// incrementa `version` do item e do run e retorna `(GroupRun atualizado, GroupRunItem atualizado)`.
    fn update_run_item(
        &self,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
        patch: RunItemPatch,
        now_ms: i64,
    ) -> MtResult<(GroupRun, GroupRunItem)>;

    /// Marca todos os itens ainda `Pending` de uma execução como `Skipped` (política Sequencial `&&`).
    fn skip_remaining_pending_items(
        &self,
        run_id: &GroupRunId,
        now_ms: i64,
    ) -> MtResult<(GroupRun, Vec<GroupRunItem>)> {
        let _ = (run_id, now_ms);
        Err(GroupErrorCode::PersistenceFailed
            .into_error("Operação skip_remaining_pending_items não implementada."))
    }

    /// Obtém a cauda sanitizada (`output_tail` e `output_truncated`) de um item.
    fn get_run_item_output_tail(
        &self,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
    ) -> MtResult<(String, bool)>;

    /// No boot (após recuperação de processos da Fase 02), marca execuções e itens não terminais
    /// como `interrupted` e preenche `finished_at`.
    fn recover_interrupted_runs(&self, now_ms: i64) -> MtResult<Vec<GroupRun>>;

    /// Remove execuções terminais cujo `finished_at` seja anterior a `cutoff_ms` (retenção padrão 30 dias).
    fn prune_finished_runs_older_than(&self, cutoff_ms: i64) -> MtResult<u64>;
}

/// Trunca uma string UTF-8 nos bytes finais de até `max_bytes`, respeitando fronteiras de codepoint.
#[must_use]
pub fn truncate_utf8_tail(raw: &str, max_bytes: usize) -> (String, bool) {
    if raw.len() <= max_bytes {
        return (raw.to_string(), false);
    }
    let mut start = raw.len() - max_bytes;
    while start < raw.len() && !raw.is_char_boundary(start) {
        start += 1;
    }
    (raw[start..].to_string(), true)
}

/// Implementação concreta de `CommandGroupRepo` sobre `mt_db::Db`.
#[derive(Clone)]
pub struct SqliteCommandGroupRepo {
    db: mt_db::Db,
}

impl SqliteCommandGroupRepo {
    /// Cria uma nova instância conectada ao banco `mt_db::Db`.
    #[must_use]
    pub fn new(db: mt_db::Db) -> Self {
        Self { db }
    }

    fn run_write<T, F>(&self, op: &'static str, approx_bytes: usize, f: F) -> MtResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&rusqlite::Transaction<'_>) -> MtResult<T> + Send + 'static,
    {
        let err_slot: std::sync::Arc<Mutex<Option<MtError>>> =
            std::sync::Arc::new(Mutex::new(None));
        let slot_clone = std::sync::Arc::clone(&err_slot);
        let res = self
            .db
            .write_blocking(op, approx_bytes, move |tx| match f(tx) {
                Ok(val) => Ok(val),
                Err(mt_err) => {
                    *slot_clone.lock() = Some(mt_err);
                    Err(mt_db::DbError::ConstraintViolation {
                        constraint: "command_group_domain_rollback".to_string(),
                    })
                }
            });

        if let Some(domain_err) = err_slot.lock().take() {
            return Err(domain_err);
        }
        res.map_err(MtError::from)
    }

    fn run_read<T, F>(&self, f: F) -> MtResult<T>
    where
        F: FnOnce(&Connection) -> MtResult<T>,
    {
        let err_slot: Mutex<Option<MtError>> = Mutex::new(None);
        let res = self.db.read_blocking(|conn| match f(conn) {
            Ok(val) => Ok(val),
            Err(mt_err) => {
                *err_slot.lock() = Some(mt_err);
                Err(mt_db::DbError::ConstraintViolation {
                    constraint: "command_group_read_error".to_string(),
                })
            }
        });

        if let Some(domain_err) = err_slot.into_inner() {
            return Err(domain_err);
        }
        res.map_err(MtError::from)
    }
}

fn map_sql_err(err: rusqlite::Error) -> MtError {
    GroupErrorCode::PersistenceFailed.into_error(format!(
        "Erro de persistência SQLite em Grupos de Comandos: {err}"
    ))
}

fn parse_id<T: FromStr<Err = MtError>>(raw: &str) -> MtResult<T> {
    raw.parse::<T>().map_err(|_| {
        GroupErrorCode::PersistenceFailed.into_error(format!(
            "Identificador inválido persistido no banco: '{raw}'."
        ))
    })
}

fn parse_opt_id<T: FromStr<Err = MtError>>(raw: Option<String>) -> MtResult<Option<T>> {
    match raw {
        Some(s) => Ok(Some(parse_id(&s)?)),
        None => Ok(None),
    }
}

fn load_group_by_id(conn: &Connection, group_id: &CommandGroupId) -> MtResult<CommandGroup> {
    let gid_str = group_id.to_string();
    let row = conn
        .query_row(
            "SELECT id, scope, project_id, name, description, mode, separator, cwd, env_json,
                    sort_order, revision, created_at, updated_at
             FROM command_groups
             WHERE id = ?1",
            params![&gid_str],
            |r| {
                let id: String = r.get(0)?;
                let scope_raw: String = r.get(1)?;
                let project_id: Option<String> = r.get(2)?;
                let name: String = r.get(3)?;
                let description: String = r.get(4)?;
                let mode_raw: String = r.get(5)?;
                let sep_raw: String = r.get(6)?;
                let cwd: Option<String> = r.get(7)?;
                let env_json: String = r.get(8)?;
                let sort_order: u32 = r.get(9)?;
                let revision_i64: i64 = r.get(10)?;
                let revision = u64::try_from(revision_i64).unwrap_or(1);
                let created_at: i64 = r.get(11)?;
                let updated_at: i64 = r.get(12)?;
                Ok((
                    id,
                    scope_raw,
                    project_id,
                    name,
                    description,
                    mode_raw,
                    sep_raw,
                    cwd,
                    env_json,
                    sort_order,
                    revision,
                    created_at,
                    updated_at,
                ))
            },
        )
        .optional()
        .map_err(map_sql_err)?;

    let Some((
        id,
        scope_raw,
        project_id_raw,
        name,
        description,
        mode_raw,
        sep_raw,
        cwd,
        env_json,
        sort_order,
        revision,
        created_at,
        updated_at,
    )) = row
    else {
        return Err(GroupErrorCode::NotFound
            .into_error(format!("Grupo de comandos '{gid_str}' não encontrado.")));
    };

    let parsed_group_id: CommandGroupId = parse_id(&id)?;
    let parsed_project_id: Option<ProjectId> = parse_opt_id(project_id_raw)?;

    let scope = GroupScope::from_db_str(&scope_raw).ok_or_else(|| {
        GroupErrorCode::PersistenceFailed
            .into_error(format!("Escopo inválido no banco: {scope_raw}"))
    })?;
    let mode = ExecutionMode::from_db_str(&mode_raw).ok_or_else(|| {
        GroupErrorCode::PersistenceFailed.into_error(format!("Modo inválido no banco: {mode_raw}"))
    })?;
    let separator = SequentialSeparator::from_db_str(&sep_raw).ok_or_else(|| {
        GroupErrorCode::PersistenceFailed
            .into_error(format!("Separador inválido no banco: {sep_raw}"))
    })?;
    let env: BTreeMap<String, String> = serde_json::from_str(&env_json).unwrap_or_default();

    let mut label_stmt = conn
        .prepare(
            "SELECT label FROM command_group_labels
             WHERE group_id = ?1
             ORDER BY label_key ASC",
        )
        .map_err(map_sql_err)?;
    let labels = label_stmt
        .query_map(params![&id], |r| r.get::<_, String>(0))
        .map_err(map_sql_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_err)?;

    let mut item_stmt = conn
        .prepare(
            "SELECT id, position, title, command, cwd, env_json, continuous
             FROM command_group_items
             WHERE group_id = ?1
             ORDER BY position ASC",
        )
        .map_err(map_sql_err)?;
    let items = item_stmt
        .query_map(params![&id], |r| {
            let item_id: String = r.get(0)?;
            let position: u32 = r.get(1)?;
            let title: String = r.get(2)?;
            let command: String = r.get(3)?;
            let item_cwd: Option<String> = r.get(4)?;
            let item_env_json: String = r.get(5)?;
            let continuous_int: i64 = r.get(6)?;
            let item_env: BTreeMap<String, String> =
                serde_json::from_str(&item_env_json).unwrap_or_default();
            Ok(CommandGroupItem {
                id: GroupItemId(item_id),
                group_id: parsed_group_id,
                position,
                title,
                command,
                cwd: item_cwd,
                env: item_env,
                continuous: continuous_int != 0,
            })
        })
        .map_err(map_sql_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_err)?;

    Ok(CommandGroup {
        id: parsed_group_id,
        scope,
        project_id: parsed_project_id,
        name,
        description,
        labels,
        mode,
        separator,
        cwd,
        env,
        sort_order,
        revision,
        created_at,
        updated_at,
        items,
    })
}

fn list_catalog_in_conn(
    conn: &Connection,
    project_id: Option<&ProjectId>,
) -> MtResult<CommandGroupCatalog> {
    let local_groups = if let Some(pid) = project_id {
        let pid_str = pid.to_string();
        let mut stmt = conn
            .prepare(
                "SELECT id FROM command_groups
                 WHERE scope = 'local' AND project_id = ?1
                 ORDER BY sort_order ASC, id ASC",
            )
            .map_err(map_sql_err)?;
        let ids = stmt
            .query_map(params![pid_str], |r| r.get::<_, String>(0))
            .map_err(map_sql_err)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sql_err)?;
        let mut groups = Vec::with_capacity(ids.len());
        for id in ids {
            let gid: CommandGroupId = parse_id(&id)?;
            groups.push(load_group_by_id(conn, &gid)?);
        }
        groups
    } else {
        Vec::new()
    };

    let mut global_stmt = conn
        .prepare(
            "SELECT id FROM command_groups
             WHERE scope = 'global'
             ORDER BY sort_order ASC, id ASC",
        )
        .map_err(map_sql_err)?;
    let global_ids = global_stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(map_sql_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_err)?;
    let mut global_groups = Vec::with_capacity(global_ids.len());
    for id in global_ids {
        let gid: CommandGroupId = parse_id(&id)?;
        global_groups.push(load_group_by_id(conn, &gid)?);
    }

    Ok(CommandGroupCatalog {
        project_id: project_id.copied(),
        local_groups,
        global_groups,
    })
}

fn load_run_by_id_in_conn(conn: &Connection, run_id: &GroupRunId) -> MtResult<(String, GroupRun)> {
    let row = conn
        .query_row(
            "SELECT id, request_id, request_hash, group_id, project_id, origin_session_id,
                    snapshot_json, mode, status, version, last_exit_code, created_at,
                    started_at, cancel_requested_at, finished_at
             FROM command_group_runs
             WHERE id = ?1",
            params![run_id.0],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                    r.get::<_, Option<String>>(5)?,
                    r.get::<_, String>(6)?,
                    r.get::<_, String>(7)?,
                    r.get::<_, String>(8)?,
                    u64::try_from(r.get::<_, i64>(9)?).unwrap_or(1),
                    r.get::<_, Option<i32>>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, Option<i64>>(12)?,
                    r.get::<_, Option<i64>>(13)?,
                    r.get::<_, Option<i64>>(14)?,
                ))
            },
        )
        .optional()
        .map_err(map_sql_err)?;

    let Some((
        id,
        request_id,
        request_hash,
        group_id_raw,
        project_id_raw,
        origin_session_id_raw,
        snapshot_json,
        mode_raw,
        status_raw,
        version,
        last_exit_code,
        created_at,
        started_at,
        cancel_requested_at,
        finished_at,
    )) = row
    else {
        return Err(GroupErrorCode::NotFound
            .into_error(format!("Execução de grupo '{}' não encontrada.", run_id.0)));
    };

    let parsed_group_id: Option<CommandGroupId> = parse_opt_id(group_id_raw)?;
    let parsed_project_id: ProjectId = parse_id(&project_id_raw)?;
    let parsed_origin_session_id: Option<SessionId> = parse_opt_id(origin_session_id_raw)?;

    let snapshot: GroupRunSnapshot = serde_json::from_str(&snapshot_json).map_err(|e| {
        GroupErrorCode::PersistenceFailed
            .into_error(format!("Snapshot inválido na execução '{id}': {e}"))
    })?;
    let mode = ExecutionMode::from_db_str(&mode_raw).ok_or_else(|| {
        GroupErrorCode::PersistenceFailed.into_error(format!("Modo inválido no run: {mode_raw}"))
    })?;
    let status = RunStatus::from_db_str(&status_raw).ok_or_else(|| {
        GroupErrorCode::PersistenceFailed
            .into_error(format!("Status inválido no run: {status_raw}"))
    })?;

    let mut item_stmt = conn
        .prepare(
            "SELECT id, run_id, position, snapshot_json, status, version, pty_id, tab_id,
                    pane_id, session_id, command_block_id, exit_code, signal, error_code,
                    error_message, started_at, finished_at, output_truncated
             FROM command_group_run_items
             WHERE run_id = ?1
             ORDER BY position ASC",
        )
        .map_err(map_sql_err)?;

    let items = item_stmt
        .query_map(params![&id], |r| {
            let item_id: String = r.get(0)?;
            let r_id: String = r.get(1)?;
            let position: u32 = r.get(2)?;
            let item_snap_json: String = r.get(3)?;
            let item_status_raw: String = r.get(4)?;
            let item_version_i64: i64 = r.get(5)?;
            let item_version = u64::try_from(item_version_i64).unwrap_or(1);
            let pty_id: Option<String> = r.get(6)?;
            let tab_id: Option<String> = r.get(7)?;
            let pane_id: Option<String> = r.get(8)?;
            let session_id: Option<String> = r.get(9)?;
            let command_block_id: Option<String> = r.get(10)?;
            let exit_code: Option<i32> = r.get(11)?;
            let signal: Option<i32> = r.get(12)?;
            let error_code: Option<String> = r.get(13)?;
            let error_message: Option<String> = r.get(14)?;
            let item_started_at: Option<i64> = r.get(15)?;
            let item_finished_at: Option<i64> = r.get(16)?;
            let output_truncated_int: i64 = r.get(17)?;
            Ok((
                item_id,
                r_id,
                position,
                item_snap_json,
                item_status_raw,
                item_version,
                pty_id,
                tab_id,
                pane_id,
                session_id,
                command_block_id,
                exit_code,
                signal,
                error_code,
                error_message,
                item_started_at,
                item_finished_at,
                output_truncated_int != 0,
            ))
        })
        .map_err(map_sql_err)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sql_err)?;

    let mut run_items = Vec::with_capacity(items.len());
    let mut statuses = Vec::with_capacity(items.len());
    for (
        item_id,
        r_id,
        position,
        item_snap_json,
        item_status_raw,
        item_version,
        pty_id_raw,
        tab_id_raw,
        pane_id_raw,
        session_id_raw,
        command_block_id_raw,
        exit_code,
        signal,
        error_code,
        error_message,
        item_started_at,
        item_finished_at,
        output_truncated,
    ) in items
    {
        let item_snapshot: GroupRunItemSnapshot =
            serde_json::from_str(&item_snap_json).map_err(|e| {
                GroupErrorCode::PersistenceFailed
                    .into_error(format!("Snapshot inválido no item '{item_id}': {e}"))
            })?;
        let item_status = ItemStatus::from_db_str(&item_status_raw).ok_or_else(|| {
            GroupErrorCode::PersistenceFailed
                .into_error(format!("Status inválido no item: {item_status_raw}"))
        })?;
        statuses.push(item_status);
        run_items.push(GroupRunItem {
            id: GroupRunItemId(item_id),
            run_id: GroupRunId(r_id),
            position,
            snapshot: item_snapshot,
            status: item_status,
            version: item_version,
            pty_id: parse_opt_id(pty_id_raw)?,
            tab_id: parse_opt_id(tab_id_raw)?,
            pane_id: parse_opt_id(pane_id_raw)?,
            session_id: parse_opt_id(session_id_raw)?,
            command_block_id: parse_opt_id(command_block_id_raw)?,
            exit_code,
            signal,
            error_code,
            error_message,
            started_at: item_started_at,
            finished_at: item_finished_at,
            output_truncated,
        });
    }

    let counts = compute_counts(&statuses);

    Ok((
        request_hash,
        GroupRun {
            id: GroupRunId(id),
            request_id: GroupRequestId(request_id),
            group_id: parsed_group_id,
            project_id: parsed_project_id,
            origin_session_id: parsed_origin_session_id,
            snapshot,
            mode,
            status,
            counts,
            version,
            last_exit_code,
            created_at,
            started_at,
            cancel_requested_at,
            finished_at,
            items: run_items,
        },
    ))
}

impl CommandGroupRepo for SqliteCommandGroupRepo {
    fn list(&self, project_id: Option<&ProjectId>) -> MtResult<CommandGroupCatalog> {
        let pid_owned = project_id.copied();
        self.run_read(|conn| list_catalog_in_conn(conn, pid_owned.as_ref()))
    }

    fn get(&self, group_id: &CommandGroupId) -> MtResult<CommandGroup> {
        let gid = *group_id;
        self.run_read(|conn| load_group_by_id(conn, &gid))
    }

    fn create(&self, input: &CommandGroupInput) -> MtResult<CommandGroup> {
        let validated = validate_group_input(input, None)?;
        self.run_write("command_group_create", 2048, move |tx| {
            let pid_str = validated.project_id.map(|p| p.to_string());
            if let Some(ref p_str) = pid_str {
                let project_exists: Option<i64> = tx
                    .query_row(
                        "SELECT 1 FROM projects WHERE id = ?1",
                        params![p_str],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_sql_err)?;
                if project_exists.is_none() {
                    return Err(GroupErrorCode::NotFound
                        .into_error(format!("O projeto '{p_str}' não foi encontrado.")));
                }
            }

            // Verifica unicidade de name_key no escopo/projeto
            let existing_conflict: Option<String> = match validated.scope {
                GroupScope::Local => tx
                    .query_row(
                        "SELECT id FROM command_groups
                         WHERE scope = 'local' AND project_id = ?1 AND name_key = ?2",
                        params![pid_str.as_deref(), &validated.name_key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_sql_err)?,
                GroupScope::Global => tx
                    .query_row(
                        "SELECT id FROM command_groups
                         WHERE scope = 'global' AND name_key = ?1",
                        params![&validated.name_key],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_sql_err)?,
            };

            if existing_conflict.is_some() {
                return Err(GroupErrorCode::NameConflict.into_error_with_fields(
                    format!(
                        "Já existe um grupo de comandos com o nome '{}' neste escopo.",
                        validated.name
                    ),
                    vec!["name".to_string()],
                ));
            }

            let next_sort_order: u32 = match validated.scope {
                GroupScope::Local => tx
                    .query_row(
                        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM command_groups
                         WHERE scope = 'local' AND project_id = ?1",
                        params![pid_str.as_deref()],
                        |r| r.get(0),
                    )
                    .map_err(map_sql_err)?,
                GroupScope::Global => tx
                    .query_row(
                        "SELECT COALESCE(MAX(sort_order) + 1, 0) FROM command_groups
                         WHERE scope = 'global'",
                        [],
                        |r| r.get(0),
                    )
                    .map_err(map_sql_err)?,
            };

            let group_id = CommandGroupId::new();
            let gid_str = group_id.to_string();
            let now_ms = mt_db::time::now_unix_ms();
            let env_json = serde_json::to_string(&validated.env).unwrap_or_else(|_| "{}".into());

            tx.execute(
                "INSERT INTO command_groups (
                    id, scope, project_id, name, name_key, description, mode, separator,
                    cwd, env_json, sort_order, revision, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 1, ?12, ?12)",
                params![
                    &gid_str,
                    validated.scope.as_str(),
                    pid_str.as_deref(),
                    validated.name,
                    validated.name_key,
                    validated.description,
                    validated.mode.as_str(),
                    validated.separator.as_str(),
                    validated.cwd,
                    env_json,
                    next_sort_order,
                    now_ms,
                ],
            )
            .map_err(map_sql_err)?;

            for (label_key, label) in &validated.labels {
                tx.execute(
                    "INSERT INTO command_group_labels (group_id, label_key, label)
                     VALUES (?1, ?2, ?3)",
                    params![&gid_str, label_key, label],
                )
                .map_err(map_sql_err)?;
            }

            for (pos, item) in validated.items.iter().enumerate() {
                let item_id = item.id.clone().unwrap_or_default();
                let existing_owner: Option<String> = tx
                    .query_row(
                        "SELECT group_id FROM command_group_items WHERE id = ?1",
                        params![item_id.0],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_sql_err)?;
                if existing_owner.is_some() {
                    return Err(GroupErrorCode::Validation.into_error_with_fields(
                        "O identificador do item já está em uso por outro grupo.",
                        vec![format!("items[{pos}].id")],
                    ));
                }

                let item_env_json =
                    serde_json::to_string(&item.env).unwrap_or_else(|_| "{}".into());
                let pos_u32 = u32::try_from(pos).unwrap_or(0);
                tx.execute(
                    "INSERT INTO command_group_items (
                        id, group_id, position, title, command, cwd, env_json, continuous
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        item_id.0,
                        &gid_str,
                        pos_u32,
                        item.title,
                        item.command,
                        item.cwd,
                        item_env_json,
                        i64::from(item.continuous),
                    ],
                )
                .map_err(map_sql_err)?;
            }

            load_group_by_id(tx, &group_id)
        })
    }

    #[allow(clippy::too_many_lines)]
    fn update(
        &self,
        group_id: &CommandGroupId,
        expected_revision: u64,
        input: &CommandGroupInput,
    ) -> MtResult<CommandGroup> {
        let validated = validate_group_input(input, None)?;
        let gid = *group_id;
        let gid_str = gid.to_string();

        self.run_write("command_group_update", 2048, move |tx| {
            let existing = tx
                .query_row(
                    "SELECT scope, project_id, revision FROM command_groups WHERE id = ?1",
                    params![&gid_str],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            u64::try_from(r.get::<_, i64>(2)?).unwrap_or(1),
                        ))
                    },
                )
                .optional()
                .map_err(map_sql_err)?;

            let Some((existing_scope_raw, existing_pid_raw, current_revision)) = existing else {
                return Err(GroupErrorCode::NotFound.into_error(format!(
                    "Grupo de comandos '{gid_str}' não encontrado."
                )));
            };

            let existing_scope = GroupScope::from_db_str(&existing_scope_raw).ok_or_else(|| {
                GroupErrorCode::PersistenceFailed.into_error("Escopo inválido no banco.")
            })?;
            let existing_pid: Option<ProjectId> = parse_opt_id(existing_pid_raw)?;

            if existing_scope != validated.scope || existing_pid != validated.project_id {
                return Err(GroupErrorCode::ScopeMismatch.into_error(
                    "Não é permitido alterar o escopo ou o projeto proprietário de um grupo existente.",
                ));
            }

            if current_revision != expected_revision {
                return Err(GroupErrorCode::RevisionConflict
                    .into_error(format!(
                        "O grupo foi modificado por outra operação (revisão esperada {expected_revision}, atual {current_revision})."
                    ))
                    .with_details(serde_json::json!({
                        "expectedRevision": expected_revision,
                        "currentRevision": current_revision,
                    })));
            }

            let pid_str = validated.project_id.map(|p| p.to_string());
            let conflict_id: Option<String> = match validated.scope {
                GroupScope::Local => tx
                    .query_row(
                        "SELECT id FROM command_groups
                         WHERE scope = 'local' AND project_id = ?1 AND name_key = ?2 AND id != ?3",
                        params![pid_str.as_deref(), &validated.name_key, &gid_str],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_sql_err)?,
                GroupScope::Global => tx
                    .query_row(
                        "SELECT id FROM command_groups
                         WHERE scope = 'global' AND name_key = ?1 AND id != ?2",
                        params![&validated.name_key, &gid_str],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(map_sql_err)?,
            };

            if conflict_id.is_some() {
                return Err(GroupErrorCode::NameConflict.into_error_with_fields(
                    format!(
                        "Já existe outro grupo de comandos com o nome '{}' neste escopo.",
                        validated.name
                    ),
                    vec!["name".to_string()],
                ));
            }

            // Verifica que todos os IDs explícitos de itens pertencem a este grupo
            for (idx, item) in validated.items.iter().enumerate() {
                if let Some(ref item_id) = item.id {
                    let owner: Option<String> = tx
                        .query_row(
                            "SELECT group_id FROM command_group_items WHERE id = ?1",
                            params![item_id.0],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(map_sql_err)?;
                    if owner.as_deref() != Some(gid_str.as_str()) {
                        return Err(GroupErrorCode::Validation.into_error_with_fields(
                            "O item informado não pertence ao grupo sendo atualizado.",
                            vec![format!("items[{idx}].id")],
                        ));
                    }
                }
            }

            let new_revision = current_revision + 1;
            let new_revision_i64 = i64::try_from(new_revision).unwrap_or(i64::MAX);
            let now_ms = mt_db::time::now_unix_ms();
            let env_json = serde_json::to_string(&validated.env).unwrap_or_else(|_| "{}".into());

            tx.execute(
                "UPDATE command_groups
                 SET name = ?1, name_key = ?2, description = ?3, mode = ?4, separator = ?5,
                     cwd = ?6, env_json = ?7, revision = ?8, updated_at = ?9
                 WHERE id = ?10",
                params![
                    validated.name,
                    validated.name_key,
                    validated.description,
                    validated.mode.as_str(),
                    validated.separator.as_str(),
                    validated.cwd,
                    env_json,
                    new_revision_i64,
                    now_ms,
                    &gid_str,
                ],
            )
            .map_err(map_sql_err)?;

            tx.execute(
                "DELETE FROM command_group_labels WHERE group_id = ?1",
                params![&gid_str],
            )
            .map_err(map_sql_err)?;
            for (label_key, label) in &validated.labels {
                tx.execute(
                    "INSERT INTO command_group_labels (group_id, label_key, label)
                     VALUES (?1, ?2, ?3)",
                    params![&gid_str, label_key, label],
                )
                .map_err(map_sql_err)?;
            }

            tx.execute(
                "DELETE FROM command_group_items WHERE group_id = ?1",
                params![&gid_str],
            )
            .map_err(map_sql_err)?;
            for (pos, item) in validated.items.iter().enumerate() {
                let item_id = item.id.clone().unwrap_or_default();
                let item_env_json =
                    serde_json::to_string(&item.env).unwrap_or_else(|_| "{}".into());
                let pos_u32 = u32::try_from(pos).unwrap_or(0);
                tx.execute(
                    "INSERT INTO command_group_items (
                        id, group_id, position, title, command, cwd, env_json, continuous
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        item_id.0,
                        &gid_str,
                        pos_u32,
                        item.title,
                        item.command,
                        item.cwd,
                        item_env_json,
                        i64::from(item.continuous),
                    ],
                )
                .map_err(map_sql_err)?;
            }

            load_group_by_id(tx, &gid)
        })
    }

    fn delete(
        &self,
        group_id: &CommandGroupId,
        expected_revision: u64,
    ) -> MtResult<DeleteGroupReceipt> {
        let gid = *group_id;
        let gid_str = gid.to_string();
        self.run_write("command_group_delete", 512, move |tx| {
            let current_rev: Option<u64> = tx
                .query_row(
                    "SELECT revision FROM command_groups WHERE id = ?1",
                    params![&gid_str],
                    |r| Ok(u64::try_from(r.get::<_, i64>(0)?).unwrap_or(1)),
                )
                .optional()
                .map_err(map_sql_err)?;

            let Some(current_revision) = current_rev else {
                return Err(GroupErrorCode::NotFound.into_error(format!(
                    "Grupo de comandos '{gid_str}' não encontrado."
                )));
            };

            if current_revision != expected_revision {
                return Err(GroupErrorCode::RevisionConflict
                    .into_error(format!(
                        "O grupo foi modificado por outra operação (revisão esperada {expected_revision}, atual {current_revision})."
                    ))
                    .with_details(serde_json::json!({
                        "expectedRevision": expected_revision,
                        "currentRevision": current_revision,
                    })));
            }

            tx.execute("DELETE FROM command_groups WHERE id = ?1", params![&gid_str])
                .map_err(map_sql_err)?;

            Ok(DeleteGroupReceipt {
                id: gid,
                revision: current_revision + 1,
            })
        })
    }

    fn reorder(
        &self,
        scope: GroupScope,
        project_id: Option<&ProjectId>,
        entries: &[ReorderGroupEntry],
    ) -> MtResult<CommandGroupCatalog> {
        match scope {
            GroupScope::Local => {
                if project_id.is_none() {
                    return Err(GroupErrorCode::ProjectRequired.into_error(
                        "A reordenação de grupos locais exige o identificador do projeto.",
                    ));
                }
            }
            GroupScope::Global => {
                if project_id.is_some() {
                    return Err(GroupErrorCode::ScopeMismatch.into_error(
                        "A reordenação de grupos globais não aceita identificador de projeto.",
                    ));
                }
            }
        }

        let pid_owned = project_id.copied();
        let pid_str = pid_owned.map(|p| p.to_string());
        let entries_owned = entries.to_vec();

        self.run_write("command_group_reorder", 1024, move |tx| {
            let existing_rows: Vec<(String, u64)> = match scope {
                GroupScope::Local => {
                    let mut stmt = tx
                        .prepare(
                            "SELECT id, revision FROM command_groups
                             WHERE scope = 'local' AND project_id = ?1",
                        )
                        .map_err(map_sql_err)?;
                    stmt.query_map(params![pid_str.as_deref()], |r| {
                        Ok((
                            r.get(0)?,
                            u64::try_from(r.get::<_, i64>(1)?).unwrap_or(1),
                        ))
                    })
                    .map_err(map_sql_err)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(map_sql_err)?
                }
                GroupScope::Global => {
                    let mut stmt = tx
                        .prepare(
                            "SELECT id, revision FROM command_groups
                             WHERE scope = 'global'",
                        )
                        .map_err(map_sql_err)?;
                    stmt.query_map([], |r| {
                        Ok((
                            r.get(0)?,
                            u64::try_from(r.get::<_, i64>(1)?).unwrap_or(1),
                        ))
                    })
                    .map_err(map_sql_err)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(map_sql_err)?
                }
            };

            let existing_map: BTreeMap<String, u64> = existing_rows.into_iter().collect();
            if entries_owned.len() != existing_map.len() {
                return Err(GroupErrorCode::ReorderMismatch.into_error(
                    "A lista de grupos para reordenação não corresponde exatamente aos grupos existentes no escopo.",
                ));
            }

            let mut seen = BTreeSet::new();
            for entry in &entries_owned {
                let entry_id_str = entry.id.to_string();
                if !seen.insert(entry_id_str.clone()) {
                    return Err(GroupErrorCode::ReorderMismatch.into_error(
                        "A lista de reordenação contém identificadores duplicados.",
                    ));
                }
                let Some(&current_rev) = existing_map.get(&entry_id_str) else {
                    return Err(GroupErrorCode::ReorderMismatch.into_error(format!(
                        "O grupo '{entry_id_str}' não pertence ao escopo informado."
                    )));
                };
                if current_rev != entry.revision {
                    return Err(GroupErrorCode::RevisionConflict
                        .into_error(format!(
                            "Conflito de revisão no grupo '{entry_id_str}' (esperada {}, atual {current_rev}).",
                            entry.revision
                        ))
                        .with_details(serde_json::json!({
                            "groupId": entry_id_str,
                            "expectedRevision": entry.revision,
                            "currentRevision": current_rev,
                        })));
                }
            }

            let now_ms = mt_db::time::now_unix_ms();
            for (idx, entry) in entries_owned.iter().enumerate() {
                let order_u32 = u32::try_from(idx).unwrap_or(0);
                let entry_id_str = entry.id.to_string();
                tx.execute(
                    "UPDATE command_groups
                     SET sort_order = ?1, revision = revision + 1, updated_at = ?2
                     WHERE id = ?3",
                    params![order_u32, now_ms, &entry_id_str],
                )
                .map_err(map_sql_err)?;
            }

            list_catalog_in_conn(tx, pid_owned.as_ref())
        })
    }

    fn find_run_by_request_id(
        &self,
        request_id: &GroupRequestId,
    ) -> MtResult<Option<(String, GroupRun)>> {
        let req_id = request_id.clone();
        self.run_read(|conn| {
            let run_id_opt: Option<String> = conn
                .query_row(
                    "SELECT id FROM command_group_runs WHERE request_id = ?1",
                    params![req_id.0],
                    |r| r.get(0),
                )
                .optional()
                .map_err(map_sql_err)?;

            let Some(run_id_str) = run_id_opt else {
                return Ok(None);
            };
            let loaded = load_run_by_id_in_conn(conn, &GroupRunId(run_id_str))?;
            Ok(Some(loaded))
        })
    }

    fn accept_run(&self, record: CreateRunRecord) -> MtResult<AcceptRunOutcome> {
        self.run_write("command_group_accept_run", 4096, move |tx| {
            let existing_run_id: Option<String> = tx
                .query_row(
                    "SELECT id FROM command_group_runs WHERE request_id = ?1",
                    params![record.request_id.0],
                    |r| r.get(0),
                )
                .optional()
                .map_err(map_sql_err)?;

            if let Some(existing_id) = existing_run_id {
                let (stored_hash, existing_run) =
                    load_run_by_id_in_conn(tx, &GroupRunId(existing_id))?;
                if stored_hash == record.request_hash {
                    return Ok(AcceptRunOutcome::Existing(existing_run));
                }
                return Err(GroupErrorCode::RequestConflict.into_error(
                    "O identificador request_id já foi utilizado com parâmetros divergentes.",
                ));
            }

            let snap_json = serde_json::to_string(&record.snapshot).map_err(|e| {
                GroupErrorCode::PersistenceFailed
                    .into_error(format!("Falha ao serializar snapshot do run: {e}"))
            })?;

            let gid_str = record.group_id.to_string();
            let pid_str = record.project_id.to_string();
            let origin_sess_str = record.origin_session_id.map(|s| s.to_string());

            tx.execute(
                "INSERT INTO command_group_runs (
                    id, request_id, request_hash, group_id, project_id, origin_session_id,
                    snapshot_json, mode, status, version, last_exit_code, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', 1, NULL, ?9)",
                params![
                    record.run_id.0,
                    record.request_id.0,
                    record.request_hash,
                    &gid_str,
                    &pid_str,
                    origin_sess_str.as_deref(),
                    snap_json,
                    record.mode.as_str(),
                    record.created_at,
                ],
            )
            .map_err(map_sql_err)?;

            for (pos, (item_id, item_snap)) in record.items.iter().enumerate() {
                let item_snap_json = serde_json::to_string(item_snap).map_err(|e| {
                    GroupErrorCode::PersistenceFailed
                        .into_error(format!("Falha ao serializar snapshot do item: {e}"))
                })?;
                let pos_u32 = u32::try_from(pos).unwrap_or(0);
                tx.execute(
                    "INSERT INTO command_group_run_items (
                        id, run_id, position, snapshot_json, status, version, output_tail, output_truncated
                     ) VALUES (?1, ?2, ?3, ?4, 'pending', 1, '', 0)",
                    params![item_id.0, record.run_id.0, pos_u32, item_snap_json],
                )
                .map_err(map_sql_err)?;
            }

            let (_, created_run) = load_run_by_id_in_conn(tx, &record.run_id)?;
            Ok(AcceptRunOutcome::Created(created_run))
        })
    }

    fn get_run(&self, run_id: &GroupRunId) -> MtResult<GroupRun> {
        let rid = run_id.clone();
        self.run_read(|conn| {
            let (_, run) = load_run_by_id_in_conn(conn, &rid)?;
            Ok(run)
        })
    }

    fn list_runs(
        &self,
        project_id: &ProjectId,
        cursor: Option<&str>,
        limit: u32,
        active_only: bool,
    ) -> MtResult<GroupRunListPage> {
        let pid_str = project_id.to_string();
        let cursor_owned = cursor.map(str::to_string);
        let clamped_limit = limit.clamp(1, 100);

        self.run_read(|conn| {
            let cursor_parts: Option<(i64, String)> = match cursor_owned {
                Some(ref raw) if !raw.trim().is_empty() => {
                    let Some((ts_str, id_part)) = raw.split_once(':') else {
                        return Err(GroupErrorCode::Validation
                            .into_error("Cursor de paginação de execuções inválido."));
                    };
                    let ts = ts_str.parse::<i64>().map_err(|_| {
                        GroupErrorCode::Validation
                            .into_error("Timestamp inválido no cursor de paginação.")
                    })?;
                    Some((ts, id_part.to_string()))
                }
                _ => None,
            };

            let fetch_limit = i64::from(clamped_limit) + 1;
            let run_ids: Vec<String> = match (active_only, cursor_parts) {
                (false, None) => {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id FROM command_group_runs
                             WHERE project_id = ?1
                             ORDER BY created_at DESC, id DESC
                             LIMIT ?2",
                        )
                        .map_err(map_sql_err)?;
                    stmt.query_map(params![&pid_str, fetch_limit], |r| r.get(0))
                        .map_err(map_sql_err)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(map_sql_err)?
                }
                (true, None) => {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id FROM command_group_runs
                             WHERE project_id = ?1 AND finished_at IS NULL
                             ORDER BY created_at DESC, id DESC
                             LIMIT ?2",
                        )
                        .map_err(map_sql_err)?;
                    stmt.query_map(params![&pid_str, fetch_limit], |r| r.get(0))
                        .map_err(map_sql_err)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(map_sql_err)?
                }
                (false, Some((c_ts, c_id))) => {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id FROM command_group_runs
                             WHERE project_id = ?1
                               AND (created_at < ?2 OR (created_at = ?2 AND id < ?3))
                             ORDER BY created_at DESC, id DESC
                             LIMIT ?4",
                        )
                        .map_err(map_sql_err)?;
                    stmt.query_map(params![&pid_str, c_ts, c_id, fetch_limit], |r| r.get(0))
                        .map_err(map_sql_err)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(map_sql_err)?
                }
                (true, Some((c_ts, c_id))) => {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id FROM command_group_runs
                             WHERE project_id = ?1 AND finished_at IS NULL
                               AND (created_at < ?2 OR (created_at = ?2 AND id < ?3))
                             ORDER BY created_at DESC, id DESC
                             LIMIT ?4",
                        )
                        .map_err(map_sql_err)?;
                    stmt.query_map(params![&pid_str, c_ts, c_id, fetch_limit], |r| r.get(0))
                        .map_err(map_sql_err)?
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(map_sql_err)?
                }
            };

            let has_more = run_ids.len() > clamped_limit as usize;
            let page_ids = if has_more {
                &run_ids[..clamped_limit as usize]
            } else {
                &run_ids[..]
            };

            let mut summaries = Vec::with_capacity(page_ids.len());
            for id_str in page_ids {
                let (_, full_run) = load_run_by_id_in_conn(conn, &GroupRunId(id_str.clone()))?;
                summaries.push(GroupRunSummary {
                    id: full_run.id,
                    request_id: full_run.request_id,
                    group_id: full_run.group_id,
                    project_id: full_run.project_id,
                    origin_session_id: full_run.origin_session_id,
                    snapshot: full_run.snapshot,
                    mode: full_run.mode,
                    status: full_run.status,
                    counts: full_run.counts,
                    version: full_run.version,
                    last_exit_code: full_run.last_exit_code,
                    created_at: full_run.created_at,
                    started_at: full_run.started_at,
                    cancel_requested_at: full_run.cancel_requested_at,
                    finished_at: full_run.finished_at,
                });
            }

            let next_cursor = if has_more {
                summaries
                    .last()
                    .map(|last| format!("{}:{}", last.created_at, last.id.0))
            } else {
                None
            };

            Ok(GroupRunListPage {
                items: summaries,
                next_cursor,
            })
        })
    }

    fn mark_cancel_requested(
        &self,
        run_id: &GroupRunId,
        item_id: Option<&GroupRunItemId>,
        now_ms: i64,
    ) -> MtResult<(GroupRun, Vec<GroupRunItem>)> {
        let rid = run_id.clone();
        let target_item = item_id.cloned();

        self.run_write("command_group_mark_cancel", 1024, move |tx| {
            let (_, current_run) = load_run_by_id_in_conn(tx, &rid)?;
            if current_run.status.is_terminal() {
                return Ok((current_run, Vec::new()));
            }

            if let Some(ref target_id) = target_item {
                if !current_run.items.iter().any(|i| &i.id == target_id) {
                    return Err(GroupErrorCode::NotFound.into_error(format!(
                        "Item de execução '{}' não encontrado no run '{}'.",
                        target_id.0, rid.0
                    )));
                }
            }

            let mut changed_item_ids = BTreeSet::new();
            for item in &current_run.items {
                if let Some(ref target_id) = target_item {
                    if &item.id != target_id {
                        continue;
                    }
                }
                match item.status {
                    ItemStatus::Pending => {
                        tx.execute(
                            "UPDATE command_group_run_items
                             SET status = 'cancelled', finished_at = ?1, version = version + 1
                             WHERE id = ?2",
                            params![now_ms, item.id.0],
                        )
                        .map_err(map_sql_err)?;
                        changed_item_ids.insert(item.id.0.clone());
                    }
                    ItemStatus::Starting | ItemStatus::Running => {
                        tx.execute(
                            "UPDATE command_group_run_items
                             SET status = 'cancelling', version = version + 1
                             WHERE id = ?1",
                            params![item.id.0],
                        )
                        .map_err(map_sql_err)?;
                        changed_item_ids.insert(item.id.0.clone());
                    }
                    _ => {}
                }
            }

            let cancel_req_at = if target_item.is_none() {
                Some(current_run.cancel_requested_at.unwrap_or(now_ms))
            } else {
                current_run.cancel_requested_at
            };

            let (_, reloaded) = load_run_by_id_in_conn(tx, &rid)?;
            let statuses: Vec<ItemStatus> = reloaded.items.iter().map(|i| i.status).collect();
            let new_status = reduce_run_status(
                &statuses,
                reloaded.started_at.is_some(),
                cancel_req_at.is_some(),
            );
            let finished_at = if new_status.is_terminal() {
                Some(reloaded.finished_at.unwrap_or(now_ms))
            } else {
                reloaded.finished_at
            };

            let should_bump = !changed_item_ids.is_empty()
                || cancel_req_at != current_run.cancel_requested_at
                || new_status != current_run.status;

            if should_bump {
                tx.execute(
                    "UPDATE command_group_runs
                     SET status = ?1, cancel_requested_at = ?2, finished_at = ?3, version = version + 1
                     WHERE id = ?4",
                    params![new_status.as_str(), cancel_req_at, finished_at, rid.0],
                )
                .map_err(map_sql_err)?;
            }

            let (_, updated_run) = load_run_by_id_in_conn(tx, &rid)?;
            let changed_items = updated_run
                .items
                .iter()
                .filter(|i| changed_item_ids.contains(&i.id.0))
                .cloned()
                .collect();

            Ok((updated_run, changed_items))
        })
    }

    #[allow(clippy::too_many_lines)]
    fn update_run_item(
        &self,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
        patch: RunItemPatch,
        now_ms: i64,
    ) -> MtResult<(GroupRun, GroupRunItem)> {
        let rid = run_id.clone();
        let iid = item_id.clone();

        self.run_write("command_group_update_run_item", 2048, move |tx| {
            let (_, current_run) = load_run_by_id_in_conn(tx, &rid)?;
            let Some(existing_item) = current_run.items.iter().find(|i| i.id == iid) else {
                return Err(GroupErrorCode::NotFound.into_error(format!(
                    "Item '{}' não encontrado na execução '{}'.",
                    iid.0, rid.0
                )));
            };

            let target_status = match patch.status {
                Some(next) if existing_item.status != next => {
                    if can_transition_item(existing_item.status, next) {
                        next
                    } else {
                        existing_item.status
                    }
                }
                _ => existing_item.status,
            };

            let pty_id_str = patch
                .pty_id
                .unwrap_or(existing_item.pty_id)
                .map(|v| v.to_string());
            let tab_id_str = patch
                .tab_id
                .unwrap_or(existing_item.tab_id)
                .map(|v| v.to_string());
            let pane_id_str = patch
                .pane_id
                .unwrap_or(existing_item.pane_id)
                .map(|v| v.to_string());
            let session_id_str = patch
                .session_id
                .unwrap_or(existing_item.session_id)
                .map(|v| v.to_string());
            let command_block_id_str = patch
                .command_block_id
                .unwrap_or(existing_item.command_block_id)
                .map(|v| v.to_string());
            let exit_code = patch.exit_code.unwrap_or(existing_item.exit_code);
            let signal = patch.signal.unwrap_or(existing_item.signal);
            let error_code = patch
                .error_code
                .unwrap_or_else(|| existing_item.error_code.clone());
            let error_message = patch
                .error_message
                .unwrap_or_else(|| existing_item.error_message.clone());

            let mut item_started_at = patch.started_at.unwrap_or(existing_item.started_at);
            if matches!(target_status, ItemStatus::Starting | ItemStatus::Running)
                && item_started_at.is_none()
            {
                item_started_at = Some(now_ms);
            }

            let mut item_finished_at = patch.finished_at.unwrap_or(existing_item.finished_at);
            if target_status.is_terminal() && item_finished_at.is_none() {
                item_finished_at = Some(now_ms);
            }

            if let Some((raw_tail, flag_truncated)) = patch.output_tail {
                let sanitized = mt_db::sanitizer::sanitize(&raw_tail).into_owned();
                let (tail_str, cut_truncated) =
                    truncate_utf8_tail(&sanitized, MAX_PERSISTED_OUTPUT_TAIL_BYTES);
                let is_truncated =
                    flag_truncated || cut_truncated || existing_item.output_truncated;
                tx.execute(
                    "UPDATE command_group_run_items
                     SET status = ?1, pty_id = ?2, tab_id = ?3, pane_id = ?4, session_id = ?5,
                         command_block_id = ?6, exit_code = ?7, signal = ?8, error_code = ?9,
                         error_message = ?10, started_at = ?11, finished_at = ?12,
                         output_tail = ?13, output_truncated = ?14, version = version + 1
                     WHERE id = ?15",
                    params![
                        target_status.as_str(),
                        pty_id_str.as_deref(),
                        tab_id_str.as_deref(),
                        pane_id_str.as_deref(),
                        session_id_str.as_deref(),
                        command_block_id_str.as_deref(),
                        exit_code,
                        signal,
                        error_code,
                        error_message,
                        item_started_at,
                        item_finished_at,
                        tail_str,
                        i64::from(is_truncated),
                        iid.0,
                    ],
                )
                .map_err(map_sql_err)?;
            } else {
                tx.execute(
                    "UPDATE command_group_run_items
                     SET status = ?1, pty_id = ?2, tab_id = ?3, pane_id = ?4, session_id = ?5,
                         command_block_id = ?6, exit_code = ?7, signal = ?8, error_code = ?9,
                         error_message = ?10, started_at = ?11, finished_at = ?12,
                         version = version + 1
                     WHERE id = ?13",
                    params![
                        target_status.as_str(),
                        pty_id_str.as_deref(),
                        tab_id_str.as_deref(),
                        pane_id_str.as_deref(),
                        session_id_str.as_deref(),
                        command_block_id_str.as_deref(),
                        exit_code,
                        signal,
                        error_code,
                        error_message,
                        item_started_at,
                        item_finished_at,
                        iid.0,
                    ],
                )
                .map_err(map_sql_err)?;
            }

            let (_, reloaded) = load_run_by_id_in_conn(tx, &rid)?;
            let statuses: Vec<ItemStatus> = reloaded.items.iter().map(|i| i.status).collect();
            let mut run_started_at = reloaded.started_at;
            if run_started_at.is_none()
                && reloaded
                    .items
                    .iter()
                    .any(|i| i.started_at.is_some() || i.status != ItemStatus::Pending)
            {
                run_started_at = Some(item_started_at.unwrap_or(now_ms));
            }

            let new_run_status = reduce_run_status(
                &statuses,
                run_started_at.is_some(),
                reloaded.cancel_requested_at.is_some(),
            );
            let run_finished_at = if new_run_status.is_terminal() {
                Some(reloaded.finished_at.unwrap_or(now_ms))
            } else {
                reloaded.finished_at
            };
            let last_exit_code = exit_code.or(reloaded.last_exit_code);

            tx.execute(
                "UPDATE command_group_runs
                 SET status = ?1, last_exit_code = ?2, started_at = ?3, finished_at = ?4,
                     version = version + 1
                 WHERE id = ?5",
                params![
                    new_run_status.as_str(),
                    last_exit_code,
                    run_started_at,
                    run_finished_at,
                    rid.0,
                ],
            )
            .map_err(map_sql_err)?;

            let (_, updated_run) = load_run_by_id_in_conn(tx, &rid)?;
            let updated_item = updated_run
                .items
                .iter()
                .find(|i| i.id == iid)
                .cloned()
                .ok_or_else(|| {
                    GroupErrorCode::NotFound.into_error("Item atualizado não encontrado.")
                })?;

            Ok((updated_run, updated_item))
        })
    }

    fn skip_remaining_pending_items(
        &self,
        run_id: &GroupRunId,
        now_ms: i64,
    ) -> MtResult<(GroupRun, Vec<GroupRunItem>)> {
        let rid = run_id.clone();
        self.run_write("command_group_skip_remaining", 1024, move |tx| {
            let (_, current_run) = load_run_by_id_in_conn(tx, &rid)?;
            let mut skipped_ids = BTreeSet::new();
            for item in &current_run.items {
                if item.status == ItemStatus::Pending {
                    tx.execute(
                        "UPDATE command_group_run_items
                         SET status = 'skipped', finished_at = ?1, version = version + 1
                         WHERE id = ?2",
                        params![now_ms, item.id.0],
                    )
                    .map_err(map_sql_err)?;
                    skipped_ids.insert(item.id.0.clone());
                }
            }

            if !skipped_ids.is_empty() {
                let (_, reloaded) = load_run_by_id_in_conn(tx, &rid)?;
                let statuses: Vec<ItemStatus> = reloaded.items.iter().map(|i| i.status).collect();
                let new_status = reduce_run_status(
                    &statuses,
                    reloaded.started_at.is_some(),
                    reloaded.cancel_requested_at.is_some(),
                );
                let finished_at = if new_status.is_terminal() {
                    Some(reloaded.finished_at.unwrap_or(now_ms))
                } else {
                    reloaded.finished_at
                };
                tx.execute(
                    "UPDATE command_group_runs
                     SET status = ?1, finished_at = ?2, version = version + 1
                     WHERE id = ?3",
                    params![new_status.as_str(), finished_at, rid.0],
                )
                .map_err(map_sql_err)?;
            }

            let (_, updated_run) = load_run_by_id_in_conn(tx, &rid)?;
            let skipped_items = updated_run
                .items
                .iter()
                .filter(|i| skipped_ids.contains(&i.id.0))
                .cloned()
                .collect();
            Ok((updated_run, skipped_items))
        })
    }

    fn get_run_item_output_tail(
        &self,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
    ) -> MtResult<(String, bool)> {
        let rid = run_id.clone();
        let iid = item_id.clone();
        self.run_read(|conn| {
            let row: Option<(String, i64)> = conn
                .query_row(
                    "SELECT output_tail, output_truncated
                     FROM command_group_run_items
                     WHERE id = ?1 AND run_id = ?2",
                    params![iid.0, rid.0],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(map_sql_err)?;

            let Some((tail, truncated_int)) = row else {
                return Err(GroupErrorCode::NotFound.into_error(format!(
                    "Item '{}' não encontrado na execução '{}'.",
                    iid.0, rid.0
                )));
            };
            Ok((tail, truncated_int != 0))
        })
    }

    fn recover_interrupted_runs(&self, now_ms: i64) -> MtResult<Vec<GroupRun>> {
        self.run_write("command_group_recover_interrupted", 2048, move |tx| {
            let active_run_ids: Vec<String> = {
                let mut stmt = tx
                    .prepare(
                        "SELECT id FROM command_group_runs
                         WHERE finished_at IS NULL
                            OR status IN ('queued', 'running', 'cancelling')
                         ORDER BY created_at ASC",
                    )
                    .map_err(map_sql_err)?;
                stmt.query_map([], |r| r.get(0))
                    .map_err(map_sql_err)?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(map_sql_err)?
            };

            let mut recovered = Vec::with_capacity(active_run_ids.len());
            for rid_str in active_run_ids {
                tx.execute(
                    "UPDATE command_group_run_items
                     SET status = 'interrupted',
                         finished_at = COALESCE(finished_at, ?1),
                         version = version + 1
                     WHERE run_id = ?2
                       AND status IN ('pending', 'starting', 'running', 'cancelling')",
                    params![now_ms, &rid_str],
                )
                .map_err(map_sql_err)?;

                let (_, reloaded) = load_run_by_id_in_conn(tx, &GroupRunId(rid_str.clone()))?;
                let statuses: Vec<ItemStatus> = reloaded.items.iter().map(|i| i.status).collect();
                let new_status = reduce_run_status(&statuses, true, false);
                tx.execute(
                    "UPDATE command_group_runs
                     SET status = ?1,
                         finished_at = COALESCE(finished_at, ?2),
                         version = version + 1
                     WHERE id = ?3",
                    params![new_status.as_str(), now_ms, &rid_str],
                )
                .map_err(map_sql_err)?;

                let (_, final_run) = load_run_by_id_in_conn(tx, &GroupRunId(rid_str))?;
                recovered.push(final_run);
            }
            Ok(recovered)
        })
    }

    fn prune_finished_runs_older_than(&self, cutoff_ms: i64) -> MtResult<u64> {
        self.run_write("command_group_prune_runs", 512, move |tx| {
            let deleted = tx
                .execute(
                    "DELETE FROM command_group_runs
                     WHERE finished_at IS NOT NULL AND finished_at < ?1",
                    params![cutoff_ms],
                )
                .map_err(map_sql_err)?;
            Ok(u64::try_from(deleted).unwrap_or(0))
        })
    }
}
