//! Erros tipados do subsistema Git e Worktrees (`mt-git`) e conversão para [`MtError`].

use std::path::PathBuf;

use mt_core::error::{ErrorCode, MtError, codes};
use serde_json::json;
use thiserror::Error;

use crate::types::DegradedReason;

/// Erros de domínio produzidos pelas operações do crate `mt-git`.
#[derive(Debug, Error)]
pub enum GitError {
    /// Caminho nulo, inexistente ou inválido.
    #[error("Caminho inválido ou inacessível para inspeção Git: {path}")]
    InvalidPath {
        /// Caminho informado.
        path: PathBuf,
        /// Descrição interna do motivo.
        reason: String,
    },

    /// Binário `git` não encontrado no `$PATH`.
    #[error("O executável 'git' não foi encontrado no sistema.")]
    BinaryMissing,

    /// Subprocesso `git` excedeu o tempo limite configurado.
    #[error("O comando Git excedeu o tempo limite de {timeout_ms} ms.")]
    CommandTimeout {
        /// Subcomando executado.
        subcommand: String,
        /// Tempo limite em milissegundos.
        timeout_ms: u64,
    },

    /// Subprocesso `git` encerrou com código de saída diferente de zero.
    #[error("Falha ao executar comando Git ({subcommand}): {stderr}")]
    CommandFailed {
        /// Subcomando executado.
        subcommand: String,
        /// Código de saída opcional.
        exit_code: Option<i32>,
        /// Saída de erro sanitizada.
        stderr: String,
    },

    /// Caminho não pertence a um repositório Git válido.
    #[error("O diretório informado não pertence a um repositório Git: {path}")]
    NotRepository {
        /// Caminho consultado.
        path: PathBuf,
    },

    /// Nome de worktree ou branch inválido.
    #[error("Nome inválido para worktree ou branch ('{name}'): {reason}")]
    WorktreeNameInvalid {
        /// Nome rejeitado.
        name: String,
        /// Explicação em `pt-BR`.
        reason: String,
    },

    /// Conflito ao criar worktree (diretório ou branch já existe).
    #[error("Já existe uma worktree ou branch conflitante: {reason}")]
    WorktreeConflict {
        /// Caminho ou referência em conflito.
        target: String,
        /// Explicação em `pt-BR`.
        reason: String,
    },

    /// Tentativa de remover worktree principal ou fora das pastas gerenciadas.
    #[error(
        "A worktree em '{path}' não é gerenciada pelo MaxTerminal/MaxCode ou é a worktree principal."
    )]
    WorktreeNotManaged {
        /// Caminho da worktree.
        path: PathBuf,
    },

    /// Worktree possui alterações locais não commitadas e `force == false`.
    #[error(
        "A worktree em '{path}' possui alterações locais não salvas. Confirme a remoção forçada."
    )]
    WorktreeDirty {
        /// Caminho da worktree.
        path: PathBuf,
    },

    /// Worktree está aberta em ao menos uma aba/painel ativo.
    #[error("A worktree em '{path}' está em uso por abas abertas e não pode ser removida.")]
    WorktreeInUse {
        /// Caminho da worktree.
        path: PathBuf,
    },

    /// Falha ao configurar watcher de filesystem sobre `.git`.
    #[error("Falha ao iniciar monitoramento de arquivos Git: {message}")]
    WatcherFailed {
        /// Mensagem descritiva.
        message: String,
    },
}

/// Alias padrão de resultado do crate `mt-git`.
pub type GitResult<T> = Result<T, GitError>;

impl GitError {
    /// Retorna o [`ErrorCode`] canônico correspondente.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidPath { .. } => codes::GIT_INVALID_PATH,
            Self::BinaryMissing => codes::GIT_BINARY_MISSING,
            Self::CommandTimeout { .. } => codes::GIT_COMMAND_TIMEOUT,
            Self::CommandFailed { .. } => codes::GIT_COMMAND_FAILED,
            Self::NotRepository { .. } => codes::GIT_NOT_REPOSITORY,
            Self::WorktreeNameInvalid { .. } => codes::GIT_WORKTREE_NAME_INVALID,
            Self::WorktreeConflict { .. } => codes::GIT_WORKTREE_CONFLICT,
            Self::WorktreeNotManaged { .. } => codes::GIT_WORKTREE_NOT_MANAGED,
            Self::WorktreeDirty { .. } => codes::GIT_WORKTREE_DIRTY,
            Self::WorktreeInUse { .. } => codes::GIT_WORKTREE_IN_USE,
            Self::WatcherFailed { .. } => codes::GIT_WATCHER_FAILED,
        }
    }

    /// Converte o erro para um [`DegradedReason`] quando aplicável à inspeção de status.
    #[must_use]
    pub const fn to_degraded_reason(&self) -> DegradedReason {
        match self {
            Self::BinaryMissing => DegradedReason::GitBinaryMissing,
            Self::CommandTimeout { .. } => DegradedReason::CommandTimeout,
            Self::NotRepository { .. } | Self::InvalidPath { .. } => {
                DegradedReason::InvalidRepository
            }
            Self::CommandFailed { .. }
            | Self::WorktreeNameInvalid { .. }
            | Self::WorktreeConflict { .. }
            | Self::WorktreeNotManaged { .. }
            | Self::WorktreeDirty { .. }
            | Self::WorktreeInUse { .. }
            | Self::WatcherFailed { .. } => DegradedReason::CommandFailed,
        }
    }
}

impl From<GitError> for MtError {
    fn from(err: GitError) -> Self {
        let code = err.code();
        let message = err.to_string();
        let details = match &err {
            GitError::InvalidPath { path, reason } => Some(json!({
                "path": path,
                "reason": reason,
            })),
            GitError::CommandTimeout {
                subcommand,
                timeout_ms,
            } => Some(json!({
                "subcommand": subcommand,
                "timeoutMs": timeout_ms,
            })),
            GitError::CommandFailed {
                subcommand,
                exit_code,
                ..
            } => Some(json!({
                "subcommand": subcommand,
                "exitCode": exit_code,
            })),
            GitError::NotRepository { path }
            | GitError::WorktreeNotManaged { path }
            | GitError::WorktreeDirty { path }
            | GitError::WorktreeInUse { path } => Some(json!({
                "path": path,
            })),
            GitError::WorktreeNameInvalid { name, .. } => Some(json!({
                "name": name,
            })),
            GitError::WorktreeConflict { target, .. } => Some(json!({
                "target": target,
            })),
            GitError::BinaryMissing | GitError::WatcherFailed { .. } => None,
        };

        let mut mt_err = Self::new(code, message).with_source(err);
        if let Some(d) = details {
            mt_err = mt_err.with_details(d);
        }
        mt_err
    }
}
