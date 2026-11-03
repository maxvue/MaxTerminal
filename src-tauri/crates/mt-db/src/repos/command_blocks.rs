//! Repositório transacional da tabela `command_blocks` e índice FTS5 `command_blocks_fts` (Fase 05).
//!
//! Todos os campos de texto livre (`command_text`, `cwd`, `output_text`, `error_excerpt`)
//! aceitam exclusivamente `Option<SanitizedText>` nas estruturas de escrita, garantindo
//! que nenhum dado bruto contorne o sanitizador de segredos da Fase 03.

use std::fmt::Write as _;
use std::str::FromStr;

use mt_core::ids::{CommandBlockId, PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandBlockDto, CommandBlockHit, CommandBlockListQuery,
    CommandBlockSearchQuery, CommandTextQuality, ObservationConfidence, ObservationSource,
    OutputKind, Page, PersistenceState,
};
use rusqlite::{OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::fts::{
    KeysetCursor, SearchSnapshotCache, build_match_query_checked, format_safe_snippet,
};
use crate::maintenance::RetentionRule;
use crate::sanitizer::SanitizedText;

/// Regra de retenção padrão para `command_blocks` (90 dias por default).
pub const COMMAND_BLOCKS_RETENTION_RULE: RetentionRule = RetentionRule {
    name: "command_blocks_retention",
    table: "command_blocks",
    timestamp_column: "started_at",
    setting_key: "db.retention.command_blocks_days",
    extra_where: Some("status <> 'running'"),
    batch_size: 500,
    default_days: Some(90),
};

/// Dados de entrada para persistir o início de um bloco de comando (`status = 'running'`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewCommandBlock {
    /// Identificador único do bloco (UUID v7).
    pub id: CommandBlockId,
    /// Sessão proprietária.
    pub session_id: SessionId,
    /// Aba opcional.
    pub tab_id: Option<TabId>,
    /// Painel opcional.
    pub pane_id: Option<PaneId>,
    /// PTY onde o comando está rodando.
    pub pty_id: PtyId,
    /// Sequência monotônica do comando dentro da sessão (`>= 1`).
    pub command_seq: u32,
    /// Texto do comando higienizado, se conhecido.
    pub command_text: Option<SanitizedText>,
    /// Qualidade da captura do texto do comando.
    pub command_text_quality: CommandTextQuality,
    /// Diretório de trabalho higienizado.
    pub cwd: Option<SanitizedText>,
    /// Timestamp UNIX em ms de início.
    pub started_at: i64,
    /// Indica se já nasceu classificado como execução contínua (`Always` ou match imediato).
    pub is_continuous: bool,
    /// Fonte de observação.
    pub source: ObservationSource,
    /// Confiança na observação.
    pub confidence: ObservationConfidence,
    /// Sequência de frame do PTY no início do bloco.
    pub start_pty_seq: u64,
    /// Offset de bytes no PTY no início do bloco.
    pub start_byte_offset: u64,
    /// Sequência de frame do PTY no início da saída.
    pub output_start_pty_seq: Option<u64>,
    /// Offset de bytes no PTY no início da saída.
    pub output_start_byte_offset: Option<u64>,
}

/// Dados completos para finalizar (ou inserir diretamente já finalizado via UPSERT) um bloco de comando.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishCommandBlock {
    /// Identificador único do bloco (UUID v7).
    pub id: CommandBlockId,
    /// Sessão proprietária.
    pub session_id: SessionId,
    /// Aba opcional.
    pub tab_id: Option<TabId>,
    /// Painel opcional.
    pub pane_id: Option<PaneId>,
    /// PTY onde o comando executou.
    pub pty_id: PtyId,
    /// Sequência monotônica do comando na sessão (`>= 1`).
    pub command_seq: u32,
    /// Texto sanitizado do comando.
    pub command_text: Option<SanitizedText>,
    /// Qualidade do texto do comando.
    pub command_text_quality: CommandTextQuality,
    /// Diretório de trabalho sanitizado.
    pub cwd: Option<SanitizedText>,
    /// Timestamp UNIX em ms do início.
    pub started_at: i64,
    /// Timestamp UNIX em ms do término.
    pub ended_at: i64,
    /// Duração total em ms (`>= 0`).
    pub duration_ms: i64,
    /// Código de saída numérico.
    pub exit_code: Option<i32>,
    /// Sinal POSIX textual (`ex.: "SIGINT"`).
    pub signal: Option<String>,
    /// Status terminal do bloco (nunca `Running`).
    pub status: BlockStatus,
    /// Razão detalhada do encerramento.
    pub end_reason: BlockEndReason,
    /// Indica se atingiu execução contínua.
    pub is_continuous: bool,
    /// Fonte de observação.
    pub source: ObservationSource,
    /// Confiança na observação.
    pub confidence: ObservationConfidence,
    /// Texto sanitizado e truncado da saída capturada (`<= 64 KiB`).
    pub output_text: Option<SanitizedText>,
    /// Tipo de armazenamento da saída.
    pub output_kind: OutputKind,
    /// Total de bytes de saída produzidos pelo comando.
    pub output_bytes: i64,
    /// Total de linhas de saída produzidas pelo comando.
    pub output_lines: i64,
    /// Excerto sanitizado do erro (`<= 16 KiB`).
    pub error_excerpt: Option<SanitizedText>,
    /// Indica se o bloco é inspecionável no `MaxCode`.
    pub inspectable: bool,
    /// Sequência inicial do PTY.
    pub start_pty_seq: u64,
    /// Offset inicial de bytes do PTY.
    pub start_byte_offset: u64,
    /// Sequência inicial da saída do PTY.
    pub output_start_pty_seq: Option<u64>,
    /// Offset inicial da saída do PTY.
    pub output_start_byte_offset: Option<u64>,
    /// Sequência final do PTY.
    pub end_pty_seq: u64,
    /// Offset final de bytes do PTY.
    pub end_byte_offset: u64,
}

impl FinishCommandBlock {
    /// Estimativa de bytes ocupados em memória pelo registro para controle de `ByteBudget`.
    #[must_use]
    pub fn approx_bytes(&self) -> usize {
        256 + self.command_text.as_ref().map_or(0, SanitizedText::len)
            + self.cwd.as_ref().map_or(0, SanitizedText::len)
            + self.output_text.as_ref().map_or(0, SanitizedText::len)
            + self.error_excerpt.as_ref().map_or(0, SanitizedText::len)
    }
}

/// Repositório tipado para operações sobre `command_blocks` e `command_blocks_fts`.
#[derive(Clone, Debug)]
pub struct CommandBlocksRepo {
    db: Db,
    search_cache: std::sync::Arc<SearchSnapshotCache<CommandBlockHit>>,
}

impl CommandBlocksRepo {
    /// Cria uma nova instância de [`CommandBlocksRepo`].
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self {
            db,
            search_cache: std::sync::Arc::new(SearchSnapshotCache::default()),
        }
    }

    /// Insere um bloco em estado `running` no SQLite.
    pub async fn start(&self, block: NewCommandBlock) -> DbResult<()> {
        let approx = 256
            + block.command_text.as_ref().map_or(0, SanitizedText::len)
            + block.cwd.as_ref().map_or(0, SanitizedText::len);
        self.db
            .write_sized("command_blocks_start", approx, move |tx| {
                start_block_tx(tx, &block)
            })
            .await
    }

    /// Versão síncrona de [`Self::start`].
    pub fn start_blocking(&self, block: NewCommandBlock) -> DbResult<()> {
        let approx = 256
            + block.command_text.as_ref().map_or(0, SanitizedText::len)
            + block.cwd.as_ref().map_or(0, SanitizedText::len);
        self.db
            .write_blocking("command_blocks_start", approx, move |tx| {
                start_block_tx(tx, &block)
            })
    }

    /// Finaliza um bloco de comando (ou insere já finalizado caso o `start` tenha sido coalescido)
    /// e incrementa os contadores da sessão.
    pub async fn finish(&self, end: FinishCommandBlock) -> DbResult<()> {
        let approx = end.approx_bytes();
        let cache = std::sync::Arc::clone(&self.search_cache);
        self.db
            .write_sized("command_blocks_finish", approx, move |tx| {
                finish_block_tx(tx, &end)
            })
            .await?;
        cache.invalidate_all();
        Ok(())
    }

    /// Versão síncrona de [`Self::finish`].
    pub fn finish_blocking(&self, end: FinishCommandBlock) -> DbResult<()> {
        let approx = end.approx_bytes();
        self.db
            .write_blocking("command_blocks_finish", approx, move |tx| {
                finish_block_tx(tx, &end)
            })?;
        self.search_cache.invalidate_all();
        Ok(())
    }

    /// Executa um lote de inícios e términos de blocos em uma única transação `BEGIN IMMEDIATE`.
    pub async fn apply_batch(
        &self,
        starts: Vec<NewCommandBlock>,
        finishes: Vec<FinishCommandBlock>,
    ) -> DbResult<()> {
        if starts.is_empty() && finishes.is_empty() {
            return Ok(());
        }
        let approx: usize = starts
            .iter()
            .map(|s| 256 + s.command_text.as_ref().map_or(0, SanitizedText::len))
            .sum::<usize>()
            + finishes
                .iter()
                .map(FinishCommandBlock::approx_bytes)
                .sum::<usize>();
        let cache = std::sync::Arc::clone(&self.search_cache);
        self.db
            .write_sized("command_blocks_batch", approx.min(512 * 1024), move |tx| {
                for s in &starts {
                    start_block_tx(tx, s)?;
                }
                for f in &finishes {
                    finish_block_tx(tx, f)?;
                }
                Ok(())
            })
            .await?;
        cache.invalidate_all();
        Ok(())
    }

    /// Fecha blocos órfãos (`status = 'running'`) deixados por encerramento abrupto anterior (`close_dangling`).
    pub async fn close_dangling(&self, now_ms: i64) -> DbResult<u64> {
        let cache = std::sync::Arc::clone(&self.search_cache);
        let updated = self
            .db
            .write_sized("command_blocks_close_dangling", 256, move |tx| {
                close_dangling_tx(tx, now_ms)
            })
            .await?;
        if updated > 0 {
            cache.invalidate_all();
        }
        Ok(updated)
    }

    /// Versão síncrona de [`Self::close_dangling`] para execução no startup da aplicação.
    pub fn close_dangling_blocking(&self, now_ms: i64) -> DbResult<u64> {
        let updated = self
            .db
            .write_blocking("command_blocks_close_dangling", 256, move |tx| {
                close_dangling_tx(tx, now_ms)
            })?;
        if updated > 0 {
            self.search_cache.invalidate_all();
        }
        Ok(updated)
    }

    /// Obtém um bloco de comando pelo seu `CommandBlockId` de forma assíncrona.
    pub async fn get(&self, id: CommandBlockId) -> DbResult<Option<CommandBlockDto>> {
        let id_str = id.to_string();
        self.db
            .read(move |conn| get_block_conn(conn, &id_str))
            .await
    }

    /// Obtém um bloco de comando pelo seu `CommandBlockId` de forma síncrona.
    pub fn get_blocking(&self, id: CommandBlockId) -> DbResult<Option<CommandBlockDto>> {
        let id_str = id.to_string();
        self.db
            .read_blocking(move |conn| get_block_conn(conn, &id_str))
    }

    /// Lista blocos de comando com filtros e paginação keyset (`started_at DESC, rowid DESC`).
    pub async fn list(&self, query: CommandBlockListQuery) -> DbResult<Page<CommandBlockDto>> {
        self.db
            .read(move |conn| list_blocks_conn(conn, &query))
            .await
    }

    /// Versão síncrona de [`Self::list`].
    pub fn list_blocking(&self, query: CommandBlockListQuery) -> DbResult<Page<CommandBlockDto>> {
        self.db
            .read_blocking(move |conn| list_blocks_conn(conn, &query))
    }

    /// Executa busca textual FTS5 em `command_blocks_fts` com ranking BM25, snippets seguros e paginação.
    pub async fn search(&self, query: CommandBlockSearchQuery) -> DbResult<Page<CommandBlockHit>> {
        let cache = std::sync::Arc::clone(&self.search_cache);
        self.db
            .read(move |conn| search_blocks_conn(conn, &query, &cache))
            .await
    }

    /// Versão síncrona de [`Self::search`].
    pub fn search_blocking(
        &self,
        query: CommandBlockSearchQuery,
    ) -> DbResult<Page<CommandBlockHit>> {
        let cache = std::sync::Arc::clone(&self.search_cache);
        self.db
            .read_blocking(move |conn| search_blocks_conn(conn, &query, &cache))
    }
}

#[allow(clippy::cast_possible_wrap)]
pub(crate) fn start_block_tx(tx: &Transaction<'_>, block: &NewCommandBlock) -> DbResult<()> {
    let id_str = block.id.to_string();
    let session_str = block.session_id.to_string();
    let tab_str = block.tab_id.map(|v| v.to_string());
    let pane_str = block.pane_id.map(|v| v.to_string());
    let pty_str = block.pty_id.to_string();
    let quality_str = block.command_text_quality.as_db_str();
    let source_str = block.source.as_db_str();
    let confidence_str = block.confidence.as_db_str();
    let is_cont = i64::from(block.is_continuous);

    tx.execute(
        "INSERT INTO command_blocks (
            id, session_id, tab_id, pane_id, pty_id, command_seq,
            command_text, command_text_quality, cwd, started_at,
            status, is_continuous, source, confidence,
            output_kind, output_bytes, output_lines, inspectable,
            start_pty_seq, start_byte_offset, output_start_pty_seq, output_start_byte_offset
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            'running', ?11, ?12, ?13,
            'none', 0, 0, 0,
            ?14, ?15, ?16, ?17
        )
        ON CONFLICT(id) DO NOTHING;",
        params![
            id_str,
            session_str,
            tab_str,
            pane_str,
            pty_str,
            i64::from(block.command_seq),
            block.command_text,
            quality_str,
            block.cwd,
            block.started_at,
            is_cont,
            source_str,
            confidence_str,
            block.start_pty_seq as i64,
            block.start_byte_offset as i64,
            block.output_start_pty_seq.map(|v| v as i64),
            block.output_start_byte_offset.map(|v| v as i64),
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "command_blocks_start"))?;

    Ok(())
}

#[allow(clippy::cast_possible_wrap, clippy::too_many_lines)]
pub(crate) fn finish_block_tx(tx: &Transaction<'_>, end: &FinishCommandBlock) -> DbResult<()> {
    let status = if end.status == BlockStatus::Running {
        BlockStatus::PtyExited
    } else {
        end.status
    };

    let ended_at = end.ended_at.max(end.started_at);
    let duration_ms = end
        .duration_ms
        .max(ended_at.saturating_sub(end.started_at))
        .max(0);

    let exit_code = match status {
        BlockStatus::Succeeded => Some(0),
        BlockStatus::Failed => match end.exit_code {
            Some(0) => Some(1),
            other => other,
        },
        _ => end.exit_code,
    };

    let id_str = end.id.to_string();
    let session_str = end.session_id.to_string();
    let tab_str = end.tab_id.map(|v| v.to_string());
    let pane_str = end.pane_id.map(|v| v.to_string());
    let pty_str = end.pty_id.to_string();
    let quality_str = end.command_text_quality.as_db_str();
    let status_str = status.as_db_str();
    let reason_str = end.end_reason.as_db_str();
    let source_str = end.source.as_db_str();
    let confidence_str = end.confidence.as_db_str();
    let out_kind_str = end.output_kind.as_db_str();
    let is_cont = i64::from(end.is_continuous);
    let is_insp = i64::from(end.inspectable);
    let signal_sanitized = end
        .signal
        .as_deref()
        .map(SanitizedText::new)
        .map(SanitizedText::into_inner);

    // Verifica se o bloco já estava finalizado para não incrementar `command_count` em duplicidade
    let existing_status: Option<String> = tx
        .query_row(
            "SELECT status FROM command_blocks WHERE id = ?1;",
            [&id_str],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_finish_check"))?;

    let already_finished = existing_status.as_deref().is_some_and(|st| st != "running");

    tx.execute(
        "INSERT INTO command_blocks (
            id, session_id, tab_id, pane_id, pty_id, command_seq,
            command_text, command_text_quality, cwd, started_at,
            ended_at, duration_ms, exit_code, signal, status, end_reason,
            is_continuous, source, confidence,
            output_text, output_kind, output_bytes, output_lines, error_excerpt, inspectable,
            start_pty_seq, start_byte_offset, output_start_pty_seq, output_start_byte_offset,
            end_pty_seq, end_byte_offset
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, ?14, ?15, ?16,
            ?17, ?18, ?19,
            ?20, ?21, ?22, ?23, ?24, ?25,
            ?26, ?27, ?28, ?29,
            ?30, ?31
        )
        ON CONFLICT(id) DO UPDATE SET
            command_text = COALESCE(excluded.command_text, command_blocks.command_text),
            command_text_quality = excluded.command_text_quality,
            cwd = COALESCE(excluded.cwd, command_blocks.cwd),
            ended_at = excluded.ended_at,
            duration_ms = excluded.duration_ms,
            exit_code = excluded.exit_code,
            signal = excluded.signal,
            status = excluded.status,
            end_reason = excluded.end_reason,
            is_continuous = excluded.is_continuous,
            source = excluded.source,
            confidence = excluded.confidence,
            output_text = excluded.output_text,
            output_kind = excluded.output_kind,
            output_bytes = excluded.output_bytes,
            output_lines = excluded.output_lines,
            error_excerpt = excluded.error_excerpt,
            inspectable = excluded.inspectable,
            output_start_pty_seq = COALESCE(excluded.output_start_pty_seq, command_blocks.output_start_pty_seq),
            output_start_byte_offset = COALESCE(excluded.output_start_byte_offset, command_blocks.output_start_byte_offset),
            end_pty_seq = excluded.end_pty_seq,
            end_byte_offset = excluded.end_byte_offset;",
        params![
            id_str,
            session_str,
            tab_str,
            pane_str,
            pty_str,
            i64::from(end.command_seq),
            end.command_text,
            quality_str,
            end.cwd,
            end.started_at,
            ended_at,
            duration_ms,
            exit_code,
            signal_sanitized,
            status_str,
            reason_str,
            is_cont,
            source_str,
            confidence_str,
            end.output_text,
            out_kind_str,
            end.output_bytes.max(0),
            end.output_lines.max(0),
            end.error_excerpt,
            is_insp,
            end.start_pty_seq as i64,
            end.start_byte_offset as i64,
            end.output_start_pty_seq.map(|v| v as i64),
            end.output_start_byte_offset.map(|v| v as i64),
            end.end_pty_seq as i64,
            end.end_byte_offset as i64,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "command_blocks_finish_upsert"))?;

    if !already_finished {
        let is_error = i64::from(matches!(
            status,
            BlockStatus::Failed | BlockStatus::TerminatedBySignal | BlockStatus::PtyExited
        ));
        tx.execute(
            "UPDATE sessions
             SET command_count = command_count + 1,
                 error_count = error_count + ?2,
                 last_cwd = COALESCE(?3, last_cwd)
             WHERE id = ?1;",
            params![session_str, is_error, end.cwd],
        )
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_finish_session_counts"))?;
    }

    Ok(())
}

fn close_dangling_tx(tx: &Transaction<'_>, now_ms: i64) -> DbResult<u64> {
    let changed = tx
        .execute(
            "UPDATE command_blocks
             SET status = 'interrupted_by_restart',
                 end_reason = 'recovered_from_crash',
                 ended_at = MAX(started_at, ?1),
                 duration_ms = MAX(0, ?1 - started_at)
             WHERE status = 'running';",
            [now_ms],
        )
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_close_dangling"))?;

    tx.execute(
        "UPDATE sessions
         SET ended_at = MAX(started_at, ?1)
         WHERE ended_at IS NULL;",
        [now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "sessions_close_dangling"))?;

    Ok(u64::try_from(changed).unwrap_or(0))
}

const SELECT_BLOCK_COLUMNS: &str = "
    cb.rowid,
    cb.id,
    cb.session_id,
    cb.tab_id,
    cb.pane_id,
    cb.pty_id,
    cb.command_seq,
    cb.command_text,
    cb.command_text_quality,
    cb.cwd,
    cb.started_at,
    cb.ended_at,
    cb.duration_ms,
    cb.exit_code,
    cb.signal,
    cb.status,
    cb.end_reason,
    cb.is_continuous,
    cb.source,
    cb.confidence,
    cb.output_text,
    cb.output_kind,
    cb.output_bytes,
    cb.output_lines,
    cb.error_excerpt,
    cb.inspectable,
    cb.start_pty_seq,
    cb.start_byte_offset,
    cb.output_start_pty_seq,
    cb.output_start_byte_offset,
    cb.end_pty_seq,
    cb.end_byte_offset
";

fn get_block_conn(conn: &rusqlite::Connection, id_str: &str) -> DbResult<Option<CommandBlockDto>> {
    let sql = format!("SELECT {SELECT_BLOCK_COLUMNS} FROM command_blocks cb WHERE cb.id = ?1;");
    conn.query_row(&sql, [id_str], |r| map_block_row(r).map(|(_, dto)| dto))
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_get"))
}

fn list_blocks_conn(
    conn: &rusqlite::Connection,
    query: &CommandBlockListQuery,
) -> DbResult<Page<CommandBlockDto>> {
    let limit = usize::try_from(query.limit.unwrap_or(50).clamp(1, 200)).unwrap_or(50);
    let filter_hash = build_list_filter_hash(query);

    let decoded_cursor = match query.cursor.as_deref() {
        Some(raw) if !raw.is_empty() => Some(KeysetCursor::decode(
            raw,
            "command_blocks_list",
            &filter_hash,
        )?),
        _ => None,
    };

    let mut sql = format!("SELECT {SELECT_BLOCK_COLUMNS} FROM command_blocks cb WHERE 1=1");
    let mut param_values: Vec<rusqlite::types::Value> = Vec::new();

    if let Some(session_id) = query.session_id {
        param_values.push(rusqlite::types::Value::Text(session_id.to_string()));
        let _ = write!(&mut sql, " AND cb.session_id = ?{}", param_values.len());
    }
    if let Some(tab_id) = query.tab_id {
        param_values.push(rusqlite::types::Value::Text(tab_id.to_string()));
        let _ = write!(&mut sql, " AND cb.tab_id = ?{}", param_values.len());
    }
    if let Some(pty_id) = query.pty_id {
        param_values.push(rusqlite::types::Value::Text(pty_id.to_string()));
        let _ = write!(&mut sql, " AND cb.pty_id = ?{}", param_values.len());
    }
    if let Some(statuses) = &query.statuses
        && !statuses.is_empty()
    {
        sql.push_str(" AND cb.status IN (");
        for (idx, st) in statuses.iter().enumerate() {
            if idx > 0 {
                sql.push_str(", ");
            }
            param_values.push(rusqlite::types::Value::Text(st.as_db_str().to_string()));
            let _ = write!(&mut sql, "?{}", param_values.len());
        }
        sql.push(')');
    }

    if let Some(ref cur) = decoded_cursor {
        param_values.push(rusqlite::types::Value::Integer(cur.watermark));
        let wm_idx = param_values.len();
        param_values.push(rusqlite::types::Value::Integer(cur.sort_key));
        let sk_idx = param_values.len();
        param_values.push(rusqlite::types::Value::Integer(cur.tie_breaker));
        let tb_idx = param_values.len();
        let _ = write!(
            &mut sql,
            " AND cb.started_at <= ?{wm_idx} AND (cb.started_at < ?{sk_idx} OR (cb.started_at = ?{sk_idx} AND cb.rowid < ?{tb_idx}))"
        );
    }

    let fetch_limit = i64::try_from(limit + 1).unwrap_or(51);
    param_values.push(rusqlite::types::Value::Integer(fetch_limit));
    let _ = write!(
        &mut sql,
        " ORDER BY cb.started_at DESC, cb.rowid DESC LIMIT ?{};",
        param_values.len()
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_list_prepare"))?;
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_values), map_block_row)
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_list_query"))?;

    let mut collected: Vec<(i64, CommandBlockDto)> = Vec::with_capacity(limit + 1);
    for row in rows {
        collected.push(row.map_err(|e| DbError::from_rusqlite(e, "command_blocks_list_row"))?);
    }

    let has_more = collected.len() > limit;
    if has_more {
        collected.truncate(limit);
    }

    let watermark = decoded_cursor
        .as_ref()
        .map(|c| c.watermark)
        .or_else(|| collected.first().map(|(_, dto)| dto.started_at))
        .unwrap_or(0);

    let next_cursor = if has_more && let Some((last_rowid, last_dto)) = collected.last() {
        Some(
            KeysetCursor::new(
                "command_blocks_list",
                filter_hash,
                watermark,
                last_dto.started_at,
                *last_rowid,
            )
            .encode(),
        )
    } else {
        None
    };

    let items = collected.into_iter().map(|(_, dto)| dto).collect();
    Ok(Page { items, next_cursor })
}

fn search_blocks_conn(
    conn: &rusqlite::Connection,
    query: &CommandBlockSearchQuery,
    cache: &SearchSnapshotCache<CommandBlockHit>,
) -> DbResult<Page<CommandBlockHit>> {
    let fts_match = build_match_query_checked(&query.query)?;
    let limit = usize::try_from(query.limit.unwrap_or(25).clamp(1, 100)).unwrap_or(25);
    let query_hash = format!(
        "fts={fts_match}|s={:?}|t={:?}|err={:?}",
        query.session_id,
        query.tab_id,
        query.only_errors.unwrap_or(false)
    );

    if let Some(ref cur) = query.cursor
        && !cur.is_empty()
    {
        let page = cache.get_page(cur, &query_hash, limit)?;
        return Ok(Page {
            items: page.items,
            next_cursor: page.next_cursor,
        });
    }

    let mut sql = format!(
        "SELECT {SELECT_BLOCK_COLUMNS}, bm25(command_blocks_fts, 5.0, 1.0, 3.0, 0.5) AS rank
         FROM command_blocks_fts
         JOIN command_blocks cb ON cb.rowid = command_blocks_fts.rowid
         WHERE command_blocks_fts MATCH ?1"
    );
    let mut param_values: Vec<rusqlite::types::Value> =
        vec![rusqlite::types::Value::Text(fts_match)];

    if let Some(session_id) = query.session_id {
        param_values.push(rusqlite::types::Value::Text(session_id.to_string()));
        let _ = write!(&mut sql, " AND cb.session_id = ?{}", param_values.len());
    }
    if let Some(tab_id) = query.tab_id {
        param_values.push(rusqlite::types::Value::Text(tab_id.to_string()));
        let _ = write!(&mut sql, " AND cb.tab_id = ?{}", param_values.len());
    }
    if query.only_errors.unwrap_or(false) {
        sql.push_str(" AND cb.status IN ('failed', 'terminated_by_signal', 'pty_exited')");
    }

    sql.push_str(" ORDER BY rank ASC, cb.started_at DESC, cb.rowid DESC LIMIT 200;");

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_search_prepare"))?;
    let first_term = query
        .query
        .split_whitespace()
        .next()
        .unwrap_or(query.query.as_str());

    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_values), |r| {
            let (_, dto) = map_block_row(r)?;
            let rank: f64 = r.get(32)?;
            Ok((dto, rank))
        })
        .map_err(|e| DbError::from_rusqlite(e, "command_blocks_search_query"))?;

    let mut hits = Vec::new();
    let mut approx_bytes = 0usize;
    for row in rows {
        let (dto, rank) =
            row.map_err(|e| DbError::from_rusqlite(e, "command_blocks_search_row"))?;
        let command_snippet = dto
            .command_text
            .as_deref()
            .map(|txt| format_safe_snippet(txt, first_term, 160));
        let output_source = dto.error_excerpt.as_deref().or(dto.output_text.as_deref());
        let output_snippet = output_source.map(|txt| format_safe_snippet(txt, first_term, 200));

        approx_bytes += 256
            + command_snippet.as_ref().map_or(0, String::len)
            + output_snippet.as_ref().map_or(0, String::len);
        hits.push(CommandBlockHit {
            block: dto,
            command_snippet,
            output_snippet,
            rank,
        });
    }

    let cached = cache.insert_and_paginate(&query_hash, hits, approx_bytes, limit);
    Ok(Page {
        items: cached.items,
        next_cursor: cached.next_cursor,
    })
}

fn build_list_filter_hash(query: &CommandBlockListQuery) -> String {
    let statuses = query
        .statuses
        .as_ref()
        .map(|list| {
            let mut v: Vec<&str> = list.iter().map(|s| s.as_db_str()).collect();
            v.sort_unstable();
            v.join(",")
        })
        .unwrap_or_default();
    format!(
        "s={:?}|t={:?}|p={:?}|st={statuses}",
        query.session_id, query.tab_id, query.pty_id
    )
}

fn parse_opt_id<T: FromStr>(raw: Option<String>, col_idx: usize) -> rusqlite::Result<Option<T>> {
    match raw {
        None => Ok(None),
        Some(s) => s.parse::<T>().map(Some).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                col_idx,
                rusqlite::types::Type::Text,
                Box::from("UUID v7 inválido"),
            )
        }),
    }
}

#[allow(clippy::cast_sign_loss)]
fn map_block_row(row: &Row<'_>) -> rusqlite::Result<(i64, CommandBlockDto)> {
    let rowid: i64 = row.get(0)?;
    let id_raw: String = row.get(1)?;
    let id = CommandBlockId::from_str(&id_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::from("CommandBlockId inválido"),
        )
    })?;

    let session_raw: String = row.get(2)?;
    let session_id = SessionId::from_str(&session_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            2,
            rusqlite::types::Type::Text,
            Box::from("SessionId inválido"),
        )
    })?;

    let tab_id = parse_opt_id::<TabId>(row.get(3)?, 3)?;
    let pane_id = parse_opt_id::<PaneId>(row.get(4)?, 4)?;

    let pty_raw: String = row.get(5)?;
    let pty_id = PtyId::from_str(&pty_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::from("PtyId inválido"),
        )
    })?;

    let command_seq_i64: i64 = row.get(6)?;
    let command_seq = u32::try_from(command_seq_i64).unwrap_or(0);
    let command_text: Option<String> = row.get(7)?;
    let quality_raw: String = row.get(8)?;
    let command_text_quality =
        CommandTextQuality::from_db_str(&quality_raw).unwrap_or(CommandTextQuality::Unavailable);
    let cwd: Option<String> = row.get(9)?;
    let started_at: i64 = row.get(10)?;
    let ended_at: Option<i64> = row.get(11)?;
    let duration_ms: Option<i64> = row.get(12)?;
    let exit_code: Option<i32> = row.get(13)?;
    let signal: Option<String> = row.get(14)?;
    let status_raw: String = row.get(15)?;
    let status = BlockStatus::from_db_str(&status_raw).unwrap_or(BlockStatus::Failed);
    let reason_raw: Option<String> = row.get(16)?;
    let end_reason = reason_raw.as_deref().and_then(BlockEndReason::from_db_str);
    let is_cont_i64: i64 = row.get(17)?;
    let source_raw: String = row.get(18)?;
    let source = ObservationSource::from_db_str(&source_raw).unwrap_or(ObservationSource::Osc133);
    let conf_raw: String = row.get(19)?;
    let confidence =
        ObservationConfidence::from_db_str(&conf_raw).unwrap_or(ObservationConfidence::Degraded);
    let output_text: Option<String> = row.get(20)?;
    let out_kind_raw: String = row.get(21)?;
    let output_kind = OutputKind::from_db_str(&out_kind_raw).unwrap_or(OutputKind::None);
    let output_bytes: i64 = row.get(22)?;
    let output_lines: i64 = row.get(23)?;
    let error_excerpt: Option<String> = row.get(24)?;
    let inspectable_i64: i64 = row.get(25)?;
    let start_pty_seq_i64: i64 = row.get(26)?;
    let start_byte_offset_i64: i64 = row.get(27)?;
    let out_start_seq_i64: Option<i64> = row.get(28)?;
    let out_start_off_i64: Option<i64> = row.get(29)?;
    let end_seq_i64: Option<i64> = row.get(30)?;
    let end_off_i64: Option<i64> = row.get(31)?;

    Ok((
        rowid,
        CommandBlockDto {
            id,
            session_id,
            tab_id,
            pane_id,
            pty_id,
            command_seq,
            command_text,
            command_text_quality,
            cwd,
            started_at,
            ended_at,
            duration_ms,
            exit_code,
            signal,
            status,
            end_reason,
            is_continuous: is_cont_i64 != 0,
            source,
            confidence,
            output_text,
            output_kind,
            output_bytes,
            output_lines,
            error_excerpt,
            inspectable: inspectable_i64 != 0,
            start_pty_seq: start_pty_seq_i64.max(0) as u64,
            start_byte_offset: start_byte_offset_i64.max(0) as u64,
            output_start_pty_seq: out_start_seq_i64.map(|v| v.max(0) as u64),
            output_start_byte_offset: out_start_off_i64.map(|v| v.max(0) as u64),
            end_pty_seq: end_seq_i64.map(|v| v.max(0) as u64),
            end_byte_offset: end_off_i64.map(|v| v.max(0) as u64),
            persistence: PersistenceState::Persisted,
        },
    ))
}
