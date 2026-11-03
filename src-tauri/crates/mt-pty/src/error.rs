//! Catálogo de erros do domínio `mt-pty` e mapeamento para `mt_core::MtError`.

use mt_core::{ErrorCode, MtError, ids::PtyId};
use serde_json::json;

/// Erros de domínio do gerenciador de PTY e supervisão de processos.
#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    /// PTY não encontrado no registro.
    #[error("PTY não encontrado: {0}")]
    NotFound(PtyId),
    /// Limite de terminais simultâneos atingido.
    #[error("Limite de {0} terminais simultâneos atingido")]
    LimitReached(usize),
    /// Diretório de trabalho inicial inválido ou inacessível.
    #[error("Diretório inválido: {0}")]
    CwdInvalid(String),
    /// Executável de shell não encontrado ou não executável.
    #[error("Shell não encontrado: {0}")]
    ShellNotFound(String),
    /// Variável de ambiente inválida (chave ou valor).
    #[error("Variável de ambiente inválida: {0}")]
    EnvInvalid(String),
    /// Dimensões de redimensionamento fora dos limites permitidos.
    #[error("Dimensões inválidas: {rows}x{cols}")]
    ResizeInvalid {
        /// Quantidade de linhas solicitada.
        rows: u16,
        /// Quantidade de colunas solicitada.
        cols: u16,
    },
    /// Falha ao iniciar o processo de terminal.
    #[error("Falha ao iniciar o terminal: {0}")]
    SpawnFailed(String),
    /// Operação rejeitada porque o terminal já encerrou ou está encerrando.
    #[error("O processo do terminal já foi finalizado")]
    AlreadyExited,
    /// Fila de escrita cheia por falta de leitura do processo filho.
    #[error("Fila de escrita cheia; o processo não está lendo a entrada")]
    WriteQueueFull,
    /// Falha de escrita no descritor do terminal.
    #[error("Falha de escrita no terminal: {0}")]
    WriteFailed(String),
    /// Entrada inválida para escrita ou decodificação de frame.
    #[error("Entrada inválida para escrita no terminal: {0}")]
    InputInvalid(String),
    /// Processo não pertence ao cgroup deste terminal ou teve PID reutilizado.
    #[error("Processo {0} não pertence a este terminal")]
    PidNotOwned(u32),
    /// Informações de processo em `/proc` indisponíveis.
    #[error("Informações de processo indisponíveis: {0}")]
    ProcUnavailable(String),
    /// Falha ao sinalizar ou encerrar processos do terminal.
    #[error("Falha ao encerrar processos: {0}")]
    KillFailed(String),
    /// Subtree cgroup v2 delegado, `cgroup.kill` ou `pidfd` indisponível.
    #[error("Contenção de processos Linux (cgroup v2 / pidfd) indisponível: {0}")]
    ContainmentUnavailable(String),
    /// Falha operacional na contenção cgroup v2 ou helper launcher/watchdog.
    #[error("Falha na contenção de processos do terminal: {0}")]
    ContainmentFailed(String),
    /// Processos sobreviveram ao deadline de encerramento (`CleanupFailed`).
    #[error("Falha na limpeza completa dos processos do terminal: {0}")]
    CleanupFailed(String),
    /// Geração de conexão de leitura expirada ou substituída por novo `attach`.
    #[error("Conexão de leitura do terminal expirada ou substituída")]
    AttachmentStale,
    /// Confirmação de leitura (ACK) inválida.
    #[error("Confirmação de leitura (ACK) inválida: {0}")]
    AckInvalid(String),
    /// Cursor de leitura/replay inválido.
    #[error("Cursor de leitura inválido: {0}")]
    CursorInvalid(String),
    /// O gerenciador de terminais está em processo de desligamento.
    #[error("O gerenciador de terminais está em processo de encerramento")]
    ShuttingDown,
    /// Diretório de runtime inválido ou com permissões inseguras.
    #[error("Diretório de runtime inválido ou inseguro: {0}")]
    RuntimeDirInvalid(String),
    /// Erro de entrada/saída do sistema operacional.
    #[error("Erro de E/S: {0}")]
    Io(#[from] std::io::Error),
}

impl PtyError {
    /// Retorna o código estável legível por máquina (`pty.<motivo>`).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "pty.not_found",
            Self::LimitReached(_) => "pty.limit_reached",
            Self::CwdInvalid(_) => "pty.cwd_invalid",
            Self::ShellNotFound(_) => "pty.shell_not_found",
            Self::EnvInvalid(_) => "pty.env_invalid",
            Self::ResizeInvalid { .. } => "pty.resize_invalid",
            Self::SpawnFailed(_) => "pty.spawn_failed",
            Self::AlreadyExited => "pty.already_exited",
            Self::WriteQueueFull => "pty.write_queue_full",
            Self::WriteFailed(_) => "pty.write_failed",
            Self::InputInvalid(_) => "pty.input_invalid",
            Self::PidNotOwned(_) => "pty.pid_not_owned",
            Self::ProcUnavailable(_) => "pty.proc_unavailable",
            Self::KillFailed(_) => "pty.kill_failed",
            Self::ContainmentUnavailable(_) => "pty.containment_unavailable",
            Self::ContainmentFailed(_) => "pty.containment_failed",
            Self::CleanupFailed(_) => "pty.cleanup_failed",
            Self::AttachmentStale => "pty.attachment_stale",
            Self::AckInvalid(_) => "pty.ack_invalid",
            Self::CursorInvalid(_) => "pty.cursor_invalid",
            Self::ShuttingDown => "pty.shutting_down",
            Self::RuntimeDirInvalid(_) => "pty.runtime_dir_invalid",
            Self::Io(_) => "pty.io",
        }
    }
}

impl From<PtyError> for MtError {
    fn from(err: PtyError) -> Self {
        let code = ErrorCode(err.code());
        let message = err.to_string();
        let details = match &err {
            PtyError::NotFound(id) => Some(json!({ "ptyId": id })),
            PtyError::LimitReached(limit) => Some(json!({ "limit": limit })),
            PtyError::ResizeInvalid { rows, cols } => Some(json!({ "rows": rows, "cols": cols })),
            PtyError::PidNotOwned(pid) => Some(json!({ "pid": pid })),
            _ => None,
        };
        let error = Self::new(code, message).with_source(err);
        match details {
            Some(d) => error.with_details(d),
            None => error,
        }
    }
}

/// Alias padrão de resultado para operações internas do crate `mt-pty`.
pub type PtyResult<T> = Result<T, PtyError>;

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_pty_error_codes_and_details_to_mt_error() {
        let id = PtyId::new();
        let not_found: MtError = PtyError::NotFound(id).into();
        assert_eq!(not_found.code.0, "pty.not_found");
        assert_eq!(not_found.details.unwrap()["ptyId"], id.to_string());

        let limit: MtError = PtyError::LimitReached(256).into();
        assert_eq!(limit.code.0, "pty.limit_reached");
        assert_eq!(limit.details.unwrap()["limit"], 256);

        let resize: MtError = PtyError::ResizeInvalid { rows: 0, cols: 80 }.into();
        assert_eq!(resize.code.0, "pty.resize_invalid");
        assert_eq!(resize.details.unwrap()["rows"], 0);

        let pid_err: MtError = PtyError::PidNotOwned(4242).into();
        assert_eq!(pid_err.code.0, "pty.pid_not_owned");
        assert_eq!(pid_err.details.unwrap()["pid"], 4242);

        let cases = [
            (PtyError::CwdInvalid("/bad".into()), "pty.cwd_invalid"),
            (
                PtyError::ShellNotFound("nope".into()),
                "pty.shell_not_found",
            ),
            (PtyError::EnvInvalid("A=B".into()), "pty.env_invalid"),
            (PtyError::SpawnFailed("boom".into()), "pty.spawn_failed"),
            (PtyError::AlreadyExited, "pty.already_exited"),
            (PtyError::WriteQueueFull, "pty.write_queue_full"),
            (PtyError::WriteFailed("eio".into()), "pty.write_failed"),
            (PtyError::InputInvalid("bad".into()), "pty.input_invalid"),
            (
                PtyError::ProcUnavailable("proc".into()),
                "pty.proc_unavailable",
            ),
            (PtyError::KillFailed("kill".into()), "pty.kill_failed"),
            (
                PtyError::ContainmentUnavailable("cg".into()),
                "pty.containment_unavailable",
            ),
            (
                PtyError::ContainmentFailed("cg".into()),
                "pty.containment_failed",
            ),
            (PtyError::CleanupFailed("surv".into()), "pty.cleanup_failed"),
            (PtyError::AttachmentStale, "pty.attachment_stale"),
            (PtyError::AckInvalid("ack".into()), "pty.ack_invalid"),
            (PtyError::CursorInvalid("cur".into()), "pty.cursor_invalid"),
            (PtyError::ShuttingDown, "pty.shutting_down"),
            (
                PtyError::RuntimeDirInvalid("dir".into()),
                "pty.runtime_dir_invalid",
            ),
            (PtyError::Io(std::io::Error::other("io")), "pty.io"),
        ];

        for (pty_err, expected_code) in cases {
            assert_eq!(pty_err.code(), expected_code);
            let mt: MtError = pty_err.into();
            assert_eq!(mt.code.0, expected_code);
            assert!(!mt.message.is_empty());
        }
    }
}
