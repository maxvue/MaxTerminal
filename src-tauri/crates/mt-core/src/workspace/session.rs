//! Modelos de Histórico de Sessões de Aba (`TabSessionId`), Sessões de Painel (`SessionId`) e Scrollback (Fase 07).

use serde::{Deserialize, Serialize};

use crate::ids::{PaneId, SessionId, TabId, TabSessionId};

/// Motivo do encerramento e arquivamento de um agrupamento de sessão de aba (`tab_sessions.end_reason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SessionEndReason {
    /// Usuário acionou "Iniciar nova Sessão" na aba.
    NewSession,
    /// Aba foi fechada pelo usuário.
    TabClosed,
    /// Aplicativo encerrou graciosamente.
    AppExit,
    /// Sessão de execução anterior recuperada no boot após encerramento abrupto.
    CrashRecovered,
    /// Projeto proprietário foi removido.
    ProjectRemoved,
    /// Falha ao iniciar todos os painéis durante abertura/restauração.
    SpawnFailed,
}

impl SessionEndReason {
    /// Representação textual persistida em `tab_sessions.end_reason`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::NewSession => "new_session",
            Self::TabClosed => "tab_closed",
            Self::AppExit => "app_exit",
            Self::CrashRecovered => "crash_recovered",
            Self::ProjectRemoved => "project_removed",
            Self::SpawnFailed => "spawn_failed",
        }
    }

    /// Reconstrói [`SessionEndReason`] a partir do valor salvo no SQLite.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "new_session" => Some(Self::NewSession),
            "tab_closed" => Some(Self::TabClosed),
            "app_exit" => Some(Self::AppExit),
            "crash_recovered" => Some(Self::CrashRecovered),
            "project_removed" => Some(Self::ProjectRemoved),
            "spawn_failed" => Some(Self::SpawnFailed),
            _ => None,
        }
    }
}

/// Resumo de um agrupamento de sessão de uma aba (`tab_sessions`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    /// Identificador único do agrupamento de sessão da aba.
    pub id: TabSessionId,
    /// Aba proprietária.
    pub tab_id: TabId,
    /// Número sequencial monotônico da sessão dentro da aba (`1..=N`).
    pub seq: i64,
    /// Indica se a sessão já foi arquivada (`archived_at IS NOT NULL`).
    pub archived: bool,
    /// Timestamp UNIX em ms de início.
    pub started_at: i64,
    /// Timestamp UNIX em ms de término/arquivamento (`None` enquanto ativa).
    pub ended_at: Option<i64>,
    /// Motivo do arquivamento (`None` enquanto ativa).
    pub end_reason: Option<SessionEndReason>,
    /// Total agregado de comandos executados em todos os painéis desta sessão.
    pub command_count: i64,
    /// Total agregado de comandos que falharam em todos os painéis desta sessão.
    pub failed_command_count: i64,
    /// Total agregado de bytes de scrollback sanitizado gravados nesta sessão.
    pub scrollback_bytes: i64,
    /// Indica se o scrollback atingiu o limite de orçamento (8 MiB) ou linha longa truncada.
    pub truncated: bool,
}

/// Resumo de uma sessão base de painel (`sessions` + `tab_session_panes`) dentro de [`SessionDetail`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionPaneSummary {
    /// Identificador da sessão base do painel na tabela `sessions` (Fase 05).
    pub session_id: SessionId,
    /// Identificador do painel folha.
    pub pane_id: PaneId,
    /// Geração do painel dentro do agrupamento.
    pub generation: i64,
    /// Último diretório de trabalho sanitizado registrado na sessão do painel.
    pub cwd: String,
    /// Timestamp UNIX em ms de fechamento do vínculo do painel, se encerrado.
    pub closed_at: Option<i64>,
}

/// Detalhe de um agrupamento de sessão de aba retornado por `session_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    /// Metadados e contadores agregados do agrupamento.
    pub summary: SessionSummary,
    /// Sessões base por painel associadas a este agrupamento.
    pub panes: Vec<SessionPaneSummary>,
}

/// Natureza do chunk de scrollback persistido em `session_scrollback`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ScrollbackChunkKind {
    /// Texto normal de saída do terminal higienizado.
    #[default]
    Output,
    /// Marcador de omissão/truncamento (alt-screen, linha > 16 KiB, overflow ou teto de 8 MiB).
    Gap,
}

impl ScrollbackChunkKind {
    /// Representação textual persistida em `session_scrollback.kind`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Output => "output",
            Self::Gap => "gap",
        }
    }

    /// Reconstrói [`ScrollbackChunkKind`] a partir do banco.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Self {
        match raw {
            "gap" => Self::Gap,
            _ => Self::Output,
        }
    }
}

/// Chunk individual de scrollback sanitizado retornado por `session_scrollback_get`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ScrollbackChunk {
    /// Identificador da sessão base do painel.
    pub session_id: SessionId,
    /// Sequência monotônica do chunk dentro da sessão do painel (`>= 1`).
    pub seq: i64,
    /// Timestamp UNIX em ms da gravação do chunk.
    pub ts: i64,
    /// Conteúdo textual estritamente sanitizado (sem escapes ativos).
    pub text: String,
    /// Tipo do chunk (`output` ou `gap`).
    pub kind: ScrollbackChunkKind,
    /// Comprimento em bytes UTF-8 do texto sanitizado.
    pub byte_len: i64,
}

/// Página genérica paginada por cursor opaco.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[specta(inline)]
pub struct CursorPage<T> {
    /// Itens da página atual.
    pub items: Vec<T>,
    /// Cursor opaco para a próxima página (`None` quando não há mais registros).
    pub next_cursor: Option<String>,
}

/// Resultado de busca FTS5 no histórico de sessões de uma aba (`session_search`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionSearchHit {
    /// Agrupamento de sessão da aba onde ocorreu o match.
    pub tab_session_id: TabSessionId,
    /// Sessão base do painel onde ocorreu o match.
    pub session_id: SessionId,
    /// Painel associado.
    pub pane_id: PaneId,
    /// Sequência do comando ou chunk de scrollback.
    pub seq: i64,
    /// Snippet textual puro e sanitizado (nunca HTML).
    pub snippet: String,
}
