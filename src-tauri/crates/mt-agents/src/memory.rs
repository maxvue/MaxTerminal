//! Memória contextual sanitizada e reprodutível de comandos recorrentes (`CommandMemory`).

use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use mt_core::{CommandBlockId, MtError, MtResult, ProjectId, TabId};
use mt_db::sanitizer::SanitizedText;
use rusqlite::{OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error;
use crate::ports::WorkspacePort;

/// Limite máximo de entradas de memória retidas por projeto.
pub const MAX_MEMORY_ENTRIES_PER_PROJECT: u32 = 2_000;
/// Limite máximo de comandos fixados (`pinned = true`) por projeto.
pub const MAX_PINNED_PER_PROJECT: u32 = 100;
/// Intervalo de inserções para acionar poda automática por projeto.
pub const PRUNE_INTERVAL_INSERTIONS: u32 = 200;

/// Entrada de comando recorrente armazenada na memória contextual do projeto.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    /// Projeto ao qual a entrada pertence.
    pub project_id: ProjectId,
    /// Comando exibido (sanitizado).
    pub command: String,
    /// Total de execuções contabilizadas.
    pub use_count: u32,
    /// Total de execuções concluídas com `exit_code == 0`.
    pub success_count: u32,
    /// Total de execuções concluídas com `exit_code != 0`.
    pub failure_count: u32,
    /// Timestamp UNIX em ms da última execução.
    pub last_used_at_ms: i64,
    /// Último diretório de trabalho (`cwd`) observado.
    pub last_cwd: Option<String>,
    /// Último código de saída observado.
    pub last_exit_code: Option<i32>,
    /// Indica se o comando está fixado no topo pelo usuário.
    pub pinned: bool,
    /// Indica se o comando foi ocultado das sugestões automáticas.
    pub hidden: bool,
    /// Pontuação calculada de *frecency* + afinidade de diretório.
    pub score: f32,
}

/// Parâmetros de consulta de sugestões da memória contextual (reutilizado pela Fase 15).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryQuery {
    /// Identificador do projeto.
    pub project_id: ProjectId,
    /// Prefixo literal opcional para filtragem.
    pub prefix: Option<String>,
    /// Limite máximo de resultados (truncado a `<= 50`).
    pub limit: u16,
    /// Se verdadeiro, inclui entradas ocultas (`hidden = true`).
    pub include_hidden: bool,
    /// Diretório de trabalho atual para bônus de afinidade (`+0.25`).
    pub cwd: Option<String>,
}

/// Representação de um bloco de comando concluído e persistido na tabela `command_blocks`.
#[derive(Debug, Clone)]
pub struct PersistedCommandBlock {
    /// Identificador único do bloco.
    pub block_id: CommandBlockId,
    /// Identificador do projeto proprietário.
    pub project_id: ProjectId,
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Texto do comando executado.
    pub command: String,
    /// Diretório de execução.
    pub cwd: Option<String>,
    /// Código de saída (`Some(0)` sucesso, `Some(n != 0)` falha, `None` indeterminado).
    pub exit_code: Option<i32>,
    /// Início da execução em ms.
    pub started_at_ms: i64,
    /// Fim da execução em ms.
    pub ended_at_ms: i64,
    /// Se o comando foi gerado internamente pelo launcher/relay de agentes.
    pub is_agent_launcher: bool,
}

/// Motor de memória contextual de comandos recorrentes por projeto.
pub struct CommandMemory {
    db: mt_db::Db,
    workspace: Arc<dyn WorkspacePort>,
}

impl CommandMemory {
    /// Cria uma nova instância de `CommandMemory`.
    #[must_use]
    pub fn new(db: mt_db::Db, workspace: Arc<dyn WorkspacePort>) -> Self {
        Self { db, workspace }
    }

    /// Normaliza um comando para indexação na memória contextual (`spec.md` §7.6).
    ///
    /// Retorna `None` quando o comando não deve ser memorizado:
    /// - Inicia com espaço ou tabulação (`HISTCONTROL=ignorespace`);
    /// - Contém quebras de linha (`\n`, `\r`) ou caracteres de controle/ANSI;
    /// - É comando interno de launcher/relay (`MAXTERMINAL_AGENT_TOKEN` ou `mt-agent-relay`);
    /// - Após sanitização com `mt_db::sanitizer::sanitize`, contém `[REDACTED:`;
    /// - Fica vazio ou excede 1024 caracteres após `trim()` externo.
    #[must_use]
    pub fn normalize(raw_cmd: &str) -> Option<String> {
        if raw_cmd.starts_with(' ') || raw_cmd.starts_with('\t') {
            return None;
        }
        if raw_cmd.chars().any(char::is_control) {
            return None;
        }
        let trimmed = raw_cmd.trim();
        if trimmed.is_empty() || trimmed.len() > 1024 {
            return None;
        }
        if trimmed.contains("MAXTERMINAL_AGENT_TOKEN") || trimmed.contains("mt-agent-relay") {
            return None;
        }
        let sanitized = SanitizedText::new(trimmed);
        if sanitized.as_str().contains("[REDACTED:") {
            return None;
        }
        let final_str = sanitized.into_inner();
        if final_str.is_empty() || final_str.len() > 1024 {
            return None;
        }
        Some(final_str)
    }

    /// Calcula o hash SHA-256 hex de `command_norm` para indexação em `command_memory_preferences`.
    #[must_use]
    pub fn command_hash(command_norm: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(command_norm.as_bytes());
        let digest = hasher.finalize();
        let mut s = String::with_capacity(64);
        for b in digest {
            use std::fmt::Write;
            let _ = write!(&mut s, "{b:02x}");
        }
        s
    }

    /// Calcula o score de *frecency* (`spec.md` §7.6):
    /// `(pinned ? 1000.0 : 0.0) + ln(1.0 + use_count as f32) * exp(-age_days / 14.0) + (same_cwd ? 0.25 : 0.0)`
    #[must_use]
    pub fn score(entry: &MemoryEntry, query_cwd: Option<&str>, now_ms: i64) -> f32 {
        let pin_bonus = if entry.pinned { 1000.0_f32 } else { 0.0_f32 };
        #[allow(clippy::cast_precision_loss)]
        let freq = (1.0_f32 + entry.use_count as f32).ln();
        let age_ms = now_ms.saturating_sub(entry.last_used_at_ms).max(0);
        #[allow(clippy::cast_precision_loss)]
        let age_days = (age_ms as f32) / 86_400_000.0_f32;
        let recency = (-age_days / 14.0_f32).exp();
        let cwd_bonus = match (query_cwd, entry.last_cwd.as_deref()) {
            (Some(q), Some(c)) if !q.is_empty() && q == c => 0.25_f32,
            _ => 0.0_f32,
        };
        pin_bonus + freq * recency + cwd_bonus
    }

    /// Processa um bloco concluído de forma idempotente na mesma transação writer (`CA-31`, `CA-33`).
    pub async fn process_block(
        &self,
        block: &PersistedCommandBlock,
        now_ms: i64,
    ) -> MtResult<bool> {
        let blk = block.clone();
        self.db
            .write(move |tx| process_block_in_tx(tx, &blk, now_ms))
            .await
            .map_err(MtError::from)
    }

    /// Consome um `CommandBlockId` recém-concluído (com retry curto caso o broadcast chegue antes do commit da Fase 05).
    pub async fn on_command_finished(
        &self,
        block_id: CommandBlockId,
        now_ms: i64,
    ) -> MtResult<bool> {
        let mut found: Option<PersistedCommandBlock> = None;
        for attempt in 0..3 {
            if let Some(b) = self.load_persisted_block(&block_id).await? {
                found = Some(b);
                break;
            }
            if attempt < 2 {
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
        let Some(block) = found else {
            return Ok(false);
        };
        self.process_block(&block, now_ms).await
    }

    /// Reconcilia no startup quaisquer blocos concluídos em `command_blocks` ainda não registrados em `command_memory_processed`.
    pub async fn reconcile_unprocessed_on_startup(&self, now_ms: i64) -> MtResult<u32> {
        let pending_ids: Vec<String> = self
            .db
            .read(|conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT cb.id
                         FROM command_blocks cb
                         LEFT JOIN command_memory_processed cmp ON cmp.block_id = cb.id
                         WHERE cb.status <> 'running'
                           AND cb.ended_at IS NOT NULL
                           AND cmp.block_id IS NULL
                         ORDER BY cb.ended_at ASC, cb.id ASC
                         LIMIT 1000;",
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_reconcile_query"))?;
                let iter = stmt
                    .query_map([], |r| r.get(0))
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_reconcile_query"))?;
                let mut out = Vec::new();
                for item in iter {
                    out.push(
                        item.map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_reconcile_row"))?,
                    );
                }
                Ok(out)
            })
            .await
            .map_err(MtError::from)?;

        let mut count = 0u32;
        for raw_id in pending_ids {
            if let Ok(bid) = CommandBlockId::from_str(&raw_id)
                && self.on_command_finished(bid, now_ms).await.unwrap_or(false)
            {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Consulta sugestões ordenadas por `score DESC, last_used_at DESC, command ASC` com escape literal de prefixo (`%`, `_`, `\`).
    pub async fn suggestions(
        &self,
        query: &MemoryQuery,
        now_ms: i64,
    ) -> MtResult<Vec<MemoryEntry>> {
        let limit = query.limit.clamp(1, 50) as usize;
        let pid_str = query.project_id.to_string();
        let include_hidden = query.include_hidden;
        let prefix_opt = query
            .prefix
            .as_deref()
            .map(str::trim_start)
            .filter(|s| !s.is_empty())
            .map(escape_sql_like_prefix);
        let query_cwd = query.cwd.clone();
        let project_id = query.project_id;

        let mut entries = self
            .db
            .read(move |conn| {
                let mut rows = Vec::new();
                if let Some(ref escaped_prefix) = prefix_opt {
                    let pattern = format!("{escaped_prefix}%");
                    let sql = if include_hidden {
                        "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                         FROM command_memory
                         WHERE project_id = ?1 AND command_norm LIKE ?2 ESCAPE '\\';"
                    } else {
                        "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                         FROM command_memory
                         WHERE project_id = ?1 AND hidden = 0 AND command_norm LIKE ?2 ESCAPE '\\';"
                    };
                    let mut stmt = conn
                        .prepare(sql)
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_suggestions"))?;
                    let iter = stmt
                        .query_map(params![pid_str, pattern], |r| {
                            row_to_entry(project_id, r)
                        })
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_suggestions"))?;
                    for item in iter {
                        rows.push(item.map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "memory_suggestions_row")
                        })?);
                    }
                } else {
                    let sql = if include_hidden {
                        "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                         FROM command_memory
                         WHERE project_id = ?1;"
                    } else {
                        "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                         FROM command_memory
                         WHERE project_id = ?1 AND hidden = 0;"
                    };
                    let mut stmt = conn
                        .prepare(sql)
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_suggestions"))?;
                    let iter = stmt
                        .query_map(params![pid_str], |r| row_to_entry(project_id, r))
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_suggestions"))?;
                    for item in iter {
                        rows.push(item.map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "memory_suggestions_row")
                        })?);
                    }
                }
                Ok(rows)
            })
            .await
            .map_err(MtError::from)?;

        for entry in &mut entries {
            entry.score = Self::score(entry, query_cwd.as_deref(), now_ms);
        }
        sort_memory_entries(&mut entries);
        entries.truncate(limit);
        Ok(entries)
    }

    /// Lista entradas da memória contextual de um projeto (até 500 itens).
    pub async fn list(
        &self,
        project_id: ProjectId,
        include_hidden: bool,
        limit: u16,
        now_ms: i64,
    ) -> MtResult<Vec<MemoryEntry>> {
        let cap = limit.clamp(1, 500) as usize;
        let pid_str = project_id.to_string();
        let mut entries = self
            .db
            .read(move |conn| {
                let sql = if include_hidden {
                    "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                     FROM command_memory
                     WHERE project_id = ?1;"
                } else {
                    "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                     FROM command_memory
                     WHERE project_id = ?1 AND hidden = 0;"
                };
                let mut stmt = conn
                    .prepare(sql)
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_list"))?;
                let iter = stmt
                    .query_map(params![pid_str], |r| row_to_entry(project_id, r))
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_list"))?;
                let mut out = Vec::new();
                for item in iter {
                    out.push(
                        item.map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_list_row"))?,
                    );
                }
                Ok(out)
            })
            .await
            .map_err(MtError::from)?;

        for entry in &mut entries {
            entry.score = Self::score(entry, None, now_ms);
        }
        sort_memory_entries(&mut entries);
        entries.truncate(cap);
        Ok(entries)
    }

    /// Fixa ou desafixa um comando na memória do projeto (respeitando o teto de 100 comandos fixados → `memory.pin_limit`).
    pub async fn set_pinned(
        &self,
        project_id: ProjectId,
        command: &str,
        pinned: bool,
        now_ms: i64,
    ) -> MtResult<MemoryEntry> {
        let cmd_norm = Self::normalize(command).ok_or_else(|| error::memory_not_found(command))?;
        let cmd_hash = Self::command_hash(&cmd_norm);
        let pid_str = project_id.to_string();
        let cmd_owned = command.to_string();

        let res: Result<MemoryEntry, &'static str> = self
            .db
            .write(move |tx| {
                if pinned {
                    let current_pinned: u32 = tx
                        .query_row(
                            "SELECT COUNT(*) FROM command_memory WHERE project_id = ?1 AND pinned = 1 AND command_norm <> ?2;",
                            params![&pid_str, &cmd_norm],
                            |r| r.get(0),
                        )
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_pin_count"))?;
                    if current_pinned >= MAX_PINNED_PER_PROJECT {
                        return Ok(Err("PIN_LIMIT"));
                    }
                }

                let changed = tx
                    .execute(
                        "UPDATE command_memory SET pinned = ?1 WHERE project_id = ?2 AND command_norm = ?3;",
                        params![i64::from(pinned), &pid_str, &cmd_norm],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_pin_update"))?;
                if changed == 0 {
                    return Ok(Err("NOT_FOUND"));
                }

                tx.execute(
                    "INSERT INTO command_memory_preferences (project_id, command_hash, pinned, hidden, forgotten, updated_at)
                     VALUES (?1, ?2, ?3, 0, 0, ?4)
                     ON CONFLICT(project_id, command_hash) DO UPDATE SET
                        pinned = excluded.pinned,
                        updated_at = excluded.updated_at;",
                    params![&pid_str, &cmd_hash, i64::from(pinned), now_ms],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_pin_pref"))?;

                let row = tx
                    .query_row(
                        "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                         FROM command_memory
                         WHERE project_id = ?1 AND command_norm = ?2;",
                        params![&pid_str, &cmd_norm],
                        |r| row_to_entry(project_id, r),
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_pin_fetch"))?;

                Ok(Ok(row))
            })
            .await
            .map_err(MtError::from)?;

        let mut entry = res.map_err(|code| match code {
            "PIN_LIMIT" => error::memory_pin_limit(MAX_PINNED_PER_PROJECT),
            _ => error::memory_not_found(&cmd_owned),
        })?;

        entry.score = Self::score(&entry, None, now_ms);
        Ok(entry)
    }

    /// Oculta ou reexibe um comando nas sugestões automáticas.
    pub async fn set_hidden(
        &self,
        project_id: ProjectId,
        command: &str,
        hidden: bool,
        now_ms: i64,
    ) -> MtResult<MemoryEntry> {
        let cmd_norm = Self::normalize(command).ok_or_else(|| error::memory_not_found(command))?;
        let cmd_hash = Self::command_hash(&cmd_norm);
        let pid_str = project_id.to_string();
        let cmd_owned = command.to_string();

        let res: Result<MemoryEntry, &'static str> = self
            .db
            .write(move |tx| {
                let changed = tx
                    .execute(
                        "UPDATE command_memory SET hidden = ?1 WHERE project_id = ?2 AND command_norm = ?3;",
                        params![i64::from(hidden), &pid_str, &cmd_norm],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_hide_update"))?;
                if changed == 0 {
                    return Ok(Err("NOT_FOUND"));
                }

                tx.execute(
                    "INSERT INTO command_memory_preferences (project_id, command_hash, pinned, hidden, forgotten, updated_at)
                     VALUES (?1, ?2, 0, ?3, 0, ?4)
                     ON CONFLICT(project_id, command_hash) DO UPDATE SET
                        hidden = excluded.hidden,
                        updated_at = excluded.updated_at;",
                    params![&pid_str, &cmd_hash, i64::from(hidden), now_ms],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_hide_pref"))?;

                let row = tx
                    .query_row(
                        "SELECT command, use_count, success_count, failure_count, last_used_at, last_cwd, last_exit_code, pinned, hidden
                         FROM command_memory
                         WHERE project_id = ?1 AND command_norm = ?2;",
                        params![&pid_str, &cmd_norm],
                        |r| row_to_entry(project_id, r),
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_hide_fetch"))?;

                Ok(Ok(row))
            })
            .await
            .map_err(MtError::from)?;

        let mut entry = res.map_err(|_| error::memory_not_found(&cmd_owned))?;

        entry.score = Self::score(&entry, None, now_ms);
        Ok(entry)
    }

    /// Remove um comando da memória contextual e grava tombstone (`forgotten = 1`) em `command_memory_preferences`
    /// para impedir que futuras execuções ou `rebuild` ressuscitem o comando (`CA-33`).
    pub async fn forget(
        &self,
        project_id: ProjectId,
        command: &str,
        now_ms: i64,
    ) -> MtResult<bool> {
        let Some(cmd_norm) = Self::normalize(command) else {
            return Ok(false);
        };
        let cmd_hash = Self::command_hash(&cmd_norm);
        let pid_str = project_id.to_string();

        self.db
            .write(move |tx| {
                let deleted = tx
                    .execute(
                        "DELETE FROM command_memory WHERE project_id = ?1 AND command_norm = ?2;",
                        params![&pid_str, &cmd_norm],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_forget_delete"))?;

                tx.execute(
                    "INSERT INTO command_memory_preferences (project_id, command_hash, pinned, hidden, forgotten, updated_at)
                     VALUES (?1, ?2, 0, 0, 1, ?3)
                     ON CONFLICT(project_id, command_hash) DO UPDATE SET
                        pinned = 0,
                        hidden = 0,
                        forgotten = 1,
                        updated_at = excluded.updated_at;",
                    params![&pid_str, &cmd_hash, now_ms],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_forget_pref"))?;

                Ok(deleted > 0)
            })
            .await
            .map_err(MtError::from)
    }

    /// Reconstrói transacionalmente a memória contextual a partir de `command_blocks`, produzindo exatamente o mesmo estado do fluxo incremental (`CA-32`).
    pub async fn rebuild(&self, project_id: Option<ProjectId>, now_ms: i64) -> MtResult<u32> {
        let pid_filter = project_id.map(|p| p.to_string());
        self.db
            .write(move |tx| {
                if let Some(ref pid) = pid_filter {
                    tx.execute(
                        "DELETE FROM command_memory WHERE project_id = ?1;",
                        params![pid],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_clear"))?;
                    tx.execute(
                        "DELETE FROM command_memory_processed WHERE project_id = ?1;",
                        params![pid],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_clear_proc"))?;
                } else {
                    tx.execute("DELETE FROM command_memory;", [])
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_clear"))?;
                    tx.execute("DELETE FROM command_memory_processed;", [])
                        .map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "memory_rebuild_clear_proc")
                        })?;
                }

                let blocks = query_retained_blocks_in_tx(tx, pid_filter.as_deref())?;
                for blk in &blocks {
                    let _ = process_block_in_tx(tx, blk, now_ms)?;
                }

                let total: u32 = if let Some(ref pid) = pid_filter {
                    tx.query_row(
                        "SELECT COUNT(*) FROM command_memory WHERE project_id = ?1;",
                        params![pid],
                        |r| r.get(0),
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_count"))?
                } else {
                    tx.query_row("SELECT COUNT(*) FROM command_memory;", [], |r| r.get(0))
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_count"))?
                };

                Ok(total)
            })
            .await
            .map_err(MtError::from)
    }

    async fn load_persisted_block(
        &self,
        block_id: &CommandBlockId,
    ) -> MtResult<Option<PersistedCommandBlock>> {
        let bid_str = block_id.to_string();
        let row_opt = self
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT cb.id,
                            COALESCE(s.project_id, t.project_id) AS project_id,
                            COALESCE(cb.tab_id, s.tab_id) AS tab_id,
                            cb.command_text,
                            cb.cwd,
                            cb.exit_code,
                            cb.started_at,
                            cb.ended_at
                     FROM command_blocks cb
                     INNER JOIN sessions s ON s.id = cb.session_id
                     LEFT JOIN tabs t ON t.id = COALESCE(cb.tab_id, s.tab_id)
                     WHERE cb.id = ?1 AND cb.status <> 'running' AND cb.ended_at IS NOT NULL;",
                    params![bid_str],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, Option<String>>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, Option<String>>(3)?,
                            r.get::<_, Option<String>>(4)?,
                            r.get::<_, Option<i32>>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, i64>(7)?,
                        ))
                    },
                )
                .optional()
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_load_block"))
            })
            .await
            .map_err(MtError::from)?;

        let Some((id_str, pid_opt, tid_opt, cmd_opt, cwd, exit_code, started_at, ended_at)) =
            row_opt
        else {
            return Ok(None);
        };

        let Some(command) = cmd_opt else {
            return Ok(None);
        };
        let tab_id = tid_opt
            .as_deref()
            .and_then(|s| TabId::from_str(s).ok())
            .unwrap_or_default();
        let project_id = pid_opt
            .as_deref()
            .and_then(|s| ProjectId::from_str(s).ok())
            .or_else(|| self.workspace.project_of_tab(tab_id));

        let Some(project_id) = project_id else {
            return Ok(None);
        };
        let Ok(block_id) = CommandBlockId::from_str(&id_str) else {
            return Ok(None);
        };

        Ok(Some(PersistedCommandBlock {
            block_id,
            project_id,
            tab_id,
            command,
            cwd,
            exit_code,
            started_at_ms: started_at,
            ended_at_ms: ended_at,
            is_agent_launcher: false,
        }))
    }
}

fn process_block_in_tx(
    tx: &Transaction<'_>,
    block: &PersistedCommandBlock,
    now_ms: i64,
) -> Result<bool, mt_db::DbError> {
    let pid_str = block.project_id.to_string();
    let bid_str = block.block_id.to_string();

    // 1. Garante idempotência at-least-once
    let inserted = tx
        .execute(
            "INSERT OR IGNORE INTO command_memory_processed (block_id, project_id, processed_at)
             VALUES (?1, ?2, ?3);",
            params![&bid_str, &pid_str, now_ms],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_insert_processed"))?;
    if inserted == 0 {
        return Ok(false);
    }

    if block.is_agent_launcher {
        return Ok(false);
    }

    let Some(cmd_norm) = CommandMemory::normalize(&block.command) else {
        return Ok(false);
    };

    let cmd_hash = CommandMemory::command_hash(&cmd_norm);

    // 2. Consulta preferências/tombstone (forgotten = 1 impede ressurreição)
    let pref: Option<(i64, i64, i64)> = tx
        .query_row(
            "SELECT pinned, hidden, forgotten
             FROM command_memory_preferences
             WHERE project_id = ?1 AND command_hash = ?2;",
            params![&pid_str, &cmd_hash],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_query_pref"))?;

    let (pref_pinned, pref_hidden, pref_forgotten) = pref.unwrap_or((0, 0, 0));
    if pref_forgotten != 0 {
        return Ok(false);
    }

    let succ_inc: i64 = i64::from(block.exit_code == Some(0));
    let fail_inc: i64 = i64::from(matches!(block.exit_code, Some(c) if c != 0));
    let sanitized_cmd = SanitizedText::new(&cmd_norm);
    let sanitized_cwd = block.cwd.as_deref().map(SanitizedText::new);

    // 3. Verifica se já existe entrada para decidir atualização de campos last_*
    let existing_last: Option<i64> = tx
        .query_row(
            "SELECT last_used_at FROM command_memory WHERE project_id = ?1 AND command_norm = ?2;",
            params![&pid_str, &cmd_norm],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_check_existing"))?;

    if let Some(prev_last_used) = existing_last {
        if block.ended_at_ms >= prev_last_used {
            tx.execute(
                "UPDATE command_memory
                 SET command = ?1,
                     use_count = use_count + 1,
                     success_count = success_count + ?2,
                     failure_count = failure_count + ?3,
                     first_used_at = MIN(first_used_at, ?4),
                     last_used_at = ?5,
                     last_cwd = ?6,
                     last_exit_code = ?7,
                     pinned = ?8,
                     hidden = ?9
                 WHERE project_id = ?10 AND command_norm = ?11;",
                params![
                    sanitized_cmd,
                    succ_inc,
                    fail_inc,
                    block.started_at_ms,
                    block.ended_at_ms,
                    sanitized_cwd,
                    block.exit_code,
                    pref_pinned,
                    pref_hidden,
                    &pid_str,
                    &cmd_norm,
                ],
            )
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_update_newer"))?;
        } else {
            // Evento atrasado fora de ordem: incrementa contadores mas preserva last_* mais recente
            tx.execute(
                "UPDATE command_memory
                 SET use_count = use_count + 1,
                     success_count = success_count + ?1,
                     failure_count = failure_count + ?2,
                     first_used_at = MIN(first_used_at, ?3)
                 WHERE project_id = ?4 AND command_norm = ?5;",
                params![succ_inc, fail_inc, block.started_at_ms, &pid_str, &cmd_norm,],
            )
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_update_older"))?;
        }
    } else {
        tx.execute(
            "INSERT INTO command_memory
                (project_id, command_norm, command, use_count, success_count, failure_count,
                 first_used_at, last_used_at, last_cwd, last_exit_code, pinned, hidden)
             VALUES (?1, ?2, ?3, 1, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11);",
            params![
                &pid_str,
                &cmd_norm,
                sanitized_cmd,
                succ_inc,
                fail_inc,
                block.started_at_ms,
                block.ended_at_ms,
                sanitized_cwd,
                block.exit_code,
                pref_pinned,
                pref_hidden,
            ],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_insert_new"))?;
    }

    // 4. Poda se ultrapassou MAX_MEMORY_ENTRIES_PER_PROJECT (nunca remove pinned = 1)
    let count: u32 = tx
        .query_row(
            "SELECT COUNT(*) FROM command_memory WHERE project_id = ?1;",
            params![&pid_str],
            |r| r.get(0),
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_count_project"))?;

    if count > MAX_MEMORY_ENTRIES_PER_PROJECT {
        let excess = count - MAX_MEMORY_ENTRIES_PER_PROJECT;
        tx.execute(
            "DELETE FROM command_memory
             WHERE project_id = ?1
               AND command_norm IN (
                   SELECT command_norm FROM command_memory
                   WHERE project_id = ?1 AND pinned = 0
                   ORDER BY use_count ASC, last_used_at ASC, command_norm DESC
                   LIMIT ?2
               );",
            params![&pid_str, excess],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_prune_excess"))?;
    }

    Ok(true)
}

fn query_retained_blocks_in_tx(
    tx: &Transaction<'_>,
    project_id_filter: Option<&str>,
) -> Result<Vec<PersistedCommandBlock>, mt_db::DbError> {
    let sql = if project_id_filter.is_some() {
        "SELECT cb.id,
                COALESCE(s.project_id, t.project_id) AS project_id,
                COALESCE(cb.tab_id, s.tab_id) AS tab_id,
                cb.command_text,
                cb.cwd,
                cb.exit_code,
                cb.started_at,
                cb.ended_at
         FROM command_blocks cb
         INNER JOIN sessions s ON s.id = cb.session_id
         LEFT JOIN tabs t ON t.id = COALESCE(cb.tab_id, s.tab_id)
         WHERE cb.status <> 'running'
           AND cb.ended_at IS NOT NULL
           AND cb.command_text IS NOT NULL
           AND COALESCE(s.project_id, t.project_id) = ?1
         ORDER BY cb.ended_at ASC, cb.id ASC;"
    } else {
        "SELECT cb.id,
                COALESCE(s.project_id, t.project_id) AS project_id,
                COALESCE(cb.tab_id, s.tab_id) AS tab_id,
                cb.command_text,
                cb.cwd,
                cb.exit_code,
                cb.started_at,
                cb.ended_at
         FROM command_blocks cb
         INNER JOIN sessions s ON s.id = cb.session_id
         LEFT JOIN tabs t ON t.id = COALESCE(cb.tab_id, s.tab_id)
         WHERE cb.status <> 'running'
           AND cb.ended_at IS NOT NULL
           AND cb.command_text IS NOT NULL
           AND COALESCE(s.project_id, t.project_id) IS NOT NULL
         ORDER BY cb.ended_at ASC, cb.id ASC;"
    };

    let mut stmt = tx
        .prepare(sql)
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_select"))?;

    let mapper = |r: &rusqlite::Row<'_>| -> rusqlite::Result<Option<PersistedCommandBlock>> {
        let id_str: String = r.get(0)?;
        let pid_str: String = r.get(1)?;
        let tid_opt: Option<String> = r.get(2)?;
        let command: String = r.get(3)?;
        let cwd: Option<String> = r.get(4)?;
        let exit_code: Option<i32> = r.get(5)?;
        let started_at_ms: i64 = r.get(6)?;
        let ended_at_ms: i64 = r.get(7)?;

        let Ok(block_id) = CommandBlockId::from_str(&id_str) else {
            return Ok(None);
        };
        let Ok(project_id) = ProjectId::from_str(&pid_str) else {
            return Ok(None);
        };
        let tab_id = tid_opt
            .as_deref()
            .and_then(|s| TabId::from_str(s).ok())
            .unwrap_or_default();

        Ok(Some(PersistedCommandBlock {
            block_id,
            project_id,
            tab_id,
            command,
            cwd,
            exit_code,
            started_at_ms,
            ended_at_ms,
            is_agent_launcher: false,
        }))
    };

    let mut out = Vec::new();
    if let Some(pid) = project_id_filter {
        let iter = stmt
            .query_map(params![pid], mapper)
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_select"))?;
        for item in iter {
            if let Some(b) =
                item.map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_row"))?
            {
                out.push(b);
            }
        }
    } else {
        let iter = stmt
            .query_map([], mapper)
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_select"))?;
        for item in iter {
            if let Some(b) =
                item.map_err(|e| mt_db::DbError::from_rusqlite(e, "memory_rebuild_row"))?
            {
                out.push(b);
            }
        }
    }
    Ok(out)
}

fn row_to_entry(project_id: ProjectId, r: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryEntry> {
    let pinned: i64 = r.get(7)?;
    let hidden: i64 = r.get(8)?;
    Ok(MemoryEntry {
        project_id,
        command: r.get(0)?,
        use_count: r.get(1)?,
        success_count: r.get(2)?,
        failure_count: r.get(3)?,
        last_used_at_ms: r.get(4)?,
        last_cwd: r.get(5)?,
        last_exit_code: r.get(6)?,
        pinned: pinned != 0,
        hidden: hidden != 0,
        score: 0.0,
    })
}

fn sort_memory_entries(entries: &mut [MemoryEntry]) {
    entries.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.last_used_at_ms.cmp(&a.last_used_at_ms))
            .then_with(|| a.command.cmp(&b.command))
    });
}

fn escape_sql_like_prefix(raw: &str) -> String {
    raw.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}
