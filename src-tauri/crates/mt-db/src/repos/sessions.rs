//! Repositório transacional da tabela `sessions` (Fase 05).
//!
//! Todos os campos textuais livres (`shell_executable`, `initial_cwd`, `last_cwd`) exigem
//! [`SanitizedText`] na assinatura pública, impedindo persistência de texto bruto não higienizado.

use std::str::FromStr;

use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_core::shell::{IntegrationMode, ShellKind};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::sanitizer::SanitizedText;

/// Dados de entrada para preparar uma nova sessão em `sessions` antes da partida do PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    /// Identificador único da sessão (UUID v7).
    pub id: SessionId,
    /// Aba proprietária opcional.
    pub tab_id: Option<TabId>,
    /// Painel proprietário opcional.
    pub pane_id: Option<PaneId>,
    /// Projeto associado opcional (Fase 06).
    pub project_id: Option<ProjectId>,
    /// Sessão de aba do workspace opcional (Fase 07).
    pub tab_session_id: Option<TabSessionId>,
    /// Identificador do PTY associado.
    pub pty_id: PtyId,
    /// Família de shell detectada.
    pub shell_kind: ShellKind,
    /// Caminho do executável do shell higienizado.
    pub shell_executable: SanitizedText,
    /// Diretório de trabalho inicial higienizado.
    pub initial_cwd: SanitizedText,
    /// Modo de integração inicial (`Pending` ou `Disabled`).
    pub integration_mode: IntegrationMode,
    /// Timestamp UNIX em milissegundos de início da sessão.
    pub started_at: i64,
}

/// Dados finais registrados ao encerrar uma sessão (`SessionsRepo::close`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionEnd {
    /// Timestamp UNIX em milissegundos de encerramento.
    pub ended_at: i64,
    /// Código de saída do processo líder do shell, quando aplicável.
    pub exit_code: Option<i32>,
    /// Nome do sinal POSIX que encerrou o shell, quando aplicável.
    pub exit_signal: Option<String>,
    /// Último `cwd` higienizado conhecido da sessão.
    pub last_cwd: Option<SanitizedText>,
    /// Modo de integração efetivo ao final da sessão.
    pub integration_mode: IntegrationMode,
    /// Total de comandos executados na sessão.
    pub command_count: i64,
    /// Total de comandos com falha na sessão.
    pub error_count: i64,
}

/// DTO de leitura de uma linha da tabela `sessions`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDto {
    /// Identificador único da sessão.
    pub id: SessionId,
    /// Identificador opcional da aba.
    pub tab_id: Option<TabId>,
    /// Identificador opcional do painel.
    pub pane_id: Option<PaneId>,
    /// Identificador opcional do projeto.
    pub project_id: Option<ProjectId>,
    /// Identificador opcional da sessão de aba.
    pub tab_session_id: Option<TabSessionId>,
    /// Identificador do PTY.
    pub pty_id: PtyId,
    /// Família do shell.
    pub shell_kind: ShellKind,
    /// Executável do shell sanitizado.
    pub shell_executable: String,
    /// Diretório inicial sanitizado.
    pub initial_cwd: String,
    /// Último diretório de trabalho sanitizado.
    pub last_cwd: Option<String>,
    /// Modo de integração efetivo.
    pub integration_mode: IntegrationMode,
    /// Timestamp UNIX em ms de início.
    pub started_at: i64,
    /// Timestamp UNIX em ms de término (`None` enquanto ativa).
    pub ended_at: Option<i64>,
    /// Código de saída do shell.
    pub exit_code: Option<i32>,
    /// Sinal de término do shell.
    pub exit_signal: Option<String>,
    /// Total de comandos executados.
    pub command_count: i64,
    /// Total de comandos com erro.
    pub error_count: i64,
}

/// Repositório tipado para operações sobre a tabela `sessions`.
#[derive(Clone, Debug)]
pub struct SessionsRepo {
    db: Db,
}

impl SessionsRepo {
    /// Cria uma nova instância de [`SessionsRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insere uma nova sessão pendente de forma assíncrona.
    pub async fn prepare(&self, session: NewSession) -> DbResult<()> {
        let approx = 256 + session.shell_executable.len() + session.initial_cwd.len();
        self.db
            .write_sized("sessions_prepare", approx, move |tx| {
                insert_session_tx(tx, &session)
            })
            .await
    }

    /// Insere uma nova sessão pendente de forma síncrona (usado em `SpawnLifecycle::prepare`).
    pub fn prepare_blocking(&self, session: NewSession) -> DbResult<()> {
        let approx = 256 + session.shell_executable.len() + session.initial_cwd.len();
        self.db
            .write_blocking("sessions_prepare", approx, move |tx| {
                insert_session_tx(tx, &session)
            })
    }

    /// Atualiza os metadados finais de encerramento de uma sessão de forma assíncrona.
    pub async fn close(&self, id: SessionId, end: SessionEnd) -> DbResult<()> {
        self.db
            .write_sized("sessions_close", 256, move |tx| {
                close_session_tx(tx, id, &end)
            })
            .await
    }

    /// Atualiza os metadados finais de encerramento de uma sessão de forma síncrona.
    pub fn close_blocking(&self, id: SessionId, end: SessionEnd) -> DbResult<()> {
        self.db.write_blocking("sessions_close", 256, move |tx| {
            close_session_tx(tx, id, &end)
        })
    }

    /// Remove uma sessão preparada cujo `pty_spawn` falhou antes da partida (`SpawnLifecycle::failed`).
    pub async fn abort(&self, id: SessionId) -> DbResult<()> {
        self.db
            .write_sized("sessions_abort", 128, move |tx| abort_session_tx(tx, id))
            .await
    }

    /// Versão síncrona de [`Self::abort`] para chamada direta em `SpawnLifecycle::failed`.
    pub fn abort_blocking(&self, id: SessionId) -> DbResult<()> {
        self.db
            .write_blocking("sessions_abort", 128, move |tx| abort_session_tx(tx, id))
    }

    /// Atualiza `last_cwd` e/ou `integration_mode` de uma sessão ativa.
    pub async fn update_runtime_state(
        &self,
        id: SessionId,
        last_cwd: Option<SanitizedText>,
        mode: IntegrationMode,
    ) -> DbResult<()> {
        self.db
            .write_sized("sessions_update_runtime", 256, move |tx| {
                let id_str = id.to_string();
                let mode_str = mode.as_db_str();
                tx.execute(
                    "UPDATE sessions
                     SET last_cwd = COALESCE(?2, last_cwd),
                         integration_mode = ?3
                     WHERE id = ?1;",
                    params![id_str, last_cwd, mode_str],
                )
                .map_err(|e| DbError::from_rusqlite(e, "sessions_update_runtime"))?;
                crate::search::projection::sync_session_if_ready(tx, id)?;
                Ok(())
            })
            .await
    }

    /// Busca uma sessão pelo identificador de forma assíncrona.
    pub async fn get(&self, id: SessionId) -> DbResult<Option<SessionDto>> {
        let id_str = id.to_string();
        self.db
            .read(move |conn| {
                conn.query_row(
                    "SELECT id, tab_id, pane_id, project_id, tab_session_id, pty_id,
                            shell_kind, shell_executable, initial_cwd, last_cwd,
                            integration_mode, started_at, ended_at, exit_code, exit_signal,
                            command_count, error_count
                     FROM sessions
                     WHERE id = ?1;",
                    [&id_str],
                    map_session_row,
                )
                .optional()
                .map_err(|e| DbError::from_rusqlite(e, "sessions_get"))
            })
            .await
    }

    /// Busca uma sessão pelo identificador de forma síncrona.
    pub fn get_blocking(&self, id: SessionId) -> DbResult<Option<SessionDto>> {
        let id_str = id.to_string();
        self.db.read_blocking(move |conn| {
            conn.query_row(
                "SELECT id, tab_id, pane_id, project_id, tab_session_id, pty_id,
                        shell_kind, shell_executable, initial_cwd, last_cwd,
                        integration_mode, started_at, ended_at, exit_code, exit_signal,
                        command_count, error_count
                 FROM sessions
                 WHERE id = ?1;",
                [&id_str],
                map_session_row,
            )
            .optional()
            .map_err(|e| DbError::from_rusqlite(e, "sessions_get"))
        })
    }
}

/// Insere uma nova sessão na transação informada.
pub fn insert_session_tx(tx: &Transaction<'_>, session: &NewSession) -> DbResult<()> {
    let id_str = session.id.to_string();
    let tab_str = session.tab_id.map(|v| v.to_string());
    let pane_str = session.pane_id.map(|v| v.to_string());
    let proj_str = session.project_id.map(|v| v.to_string());
    let ts_str = session.tab_session_id.map(|v| v.to_string());
    let pty_str = session.pty_id.to_string();
    let kind_str = session.shell_kind.as_db_str();
    let mode_str = session.integration_mode.as_db_str();

    tx.execute(
        "INSERT INTO sessions (
            id, tab_id, pane_id, project_id, tab_session_id, pty_id,
            shell_kind, shell_executable, initial_cwd, last_cwd,
            integration_mode, started_at, command_count, error_count
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11, 0, 0)
        ON CONFLICT(id) DO UPDATE SET
            tab_id = excluded.tab_id,
            pane_id = excluded.pane_id,
            project_id = excluded.project_id,
            tab_session_id = excluded.tab_session_id,
            pty_id = excluded.pty_id,
            shell_kind = excluded.shell_kind,
            shell_executable = excluded.shell_executable,
            initial_cwd = excluded.initial_cwd,
            last_cwd = COALESCE(sessions.last_cwd, excluded.last_cwd),
            integration_mode = excluded.integration_mode;",
        params![
            id_str,
            tab_str,
            pane_str,
            proj_str,
            ts_str,
            pty_str,
            kind_str,
            session.shell_executable,
            session.initial_cwd,
            mode_str,
            session.started_at,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "sessions_prepare"))?;
    crate::search::projection::sync_session_if_ready(tx, session.id)?;
    Ok(())
}

/// Atualiza os metadados finais de encerramento de uma sessão na transação informada.
pub fn close_session_tx(tx: &Transaction<'_>, id: SessionId, end: &SessionEnd) -> DbResult<()> {
    let id_str = id.to_string();
    let mode_str = end.integration_mode.as_db_str();
    let exit_sig = end
        .exit_signal
        .as_deref()
        .map(SanitizedText::new)
        .map(SanitizedText::into_inner);

    tx.execute(
        "UPDATE sessions
         SET ended_at = MAX(started_at, ?2),
             exit_code = ?3,
             exit_signal = ?4,
             last_cwd = COALESCE(?5, last_cwd),
             integration_mode = ?6,
             command_count = MAX(command_count, ?7),
             error_count = MAX(error_count, ?8)
         WHERE id = ?1;",
        params![
            id_str,
            end.ended_at,
            end.exit_code,
            exit_sig,
            end.last_cwd,
            mode_str,
            end.command_count.max(0),
            end.error_count.max(0),
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "sessions_close"))?;
    crate::search::projection::sync_session_if_ready(tx, id)?;
    Ok(())
}

fn abort_session_tx(tx: &Transaction<'_>, id: SessionId) -> DbResult<()> {
    let id_str = id.to_string();
    tx.execute("DELETE FROM sessions WHERE id = ?1;", [&id_str])
        .map_err(|e| DbError::from_rusqlite(e, "sessions_abort"))?;
    crate::search::projection::delete_projection_if_ready(
        tx,
        crate::search::projection::ProjectionSource::Sessions,
        &id_str,
    )?;
    Ok(())
}

fn parse_opt_id<T: FromStr>(raw: Option<String>, col_idx: usize) -> rusqlite::Result<Option<T>> {
    match raw {
        None => Ok(None),
        Some(s) => s.parse::<T>().map(Some).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                col_idx,
                rusqlite::types::Type::Text,
                Box::from("UUID v7 inválido no banco"),
            )
        }),
    }
}

fn map_session_row(row: &Row<'_>) -> rusqlite::Result<SessionDto> {
    let id_raw: String = row.get(0)?;
    let id = SessionId::from_str(&id_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::from("SessionId inválido"),
        )
    })?;

    let tab_id = parse_opt_id::<TabId>(row.get(1)?, 1)?;
    let pane_id = parse_opt_id::<PaneId>(row.get(2)?, 2)?;
    let project_id = parse_opt_id::<ProjectId>(row.get(3)?, 3)?;
    let tab_session_id = parse_opt_id::<TabSessionId>(row.get(4)?, 4)?;

    let pty_raw: String = row.get(5)?;
    let pty_id = PtyId::from_str(&pty_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            5,
            rusqlite::types::Type::Text,
            Box::from("PtyId inválido"),
        )
    })?;

    let kind_raw: String = row.get(6)?;
    let shell_kind = ShellKind::from_db_str(&kind_raw);
    let shell_executable: String = row.get(7)?;
    let initial_cwd: String = row.get(8)?;
    let last_cwd: Option<String> = row.get(9)?;
    let mode_raw: String = row.get(10)?;
    let integration_mode =
        IntegrationMode::from_db_str(&mode_raw).unwrap_or(IntegrationMode::Pending);

    Ok(SessionDto {
        id,
        tab_id,
        pane_id,
        project_id,
        tab_session_id,
        pty_id,
        shell_kind,
        shell_executable,
        initial_cwd,
        last_cwd,
        integration_mode,
        started_at: row.get(11)?,
        ended_at: row.get(12)?,
        exit_code: row.get(13)?,
        exit_signal: row.get(14)?,
        command_count: row.get(15)?,
        error_count: row.get(16)?,
    })
}
