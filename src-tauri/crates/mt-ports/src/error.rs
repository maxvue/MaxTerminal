//! Erros tipados do detector de portas ativas (`mt-ports`) e conversão para [`MtError`].

use mt_core::error::{ErrorCode, MtError, codes};
use mt_core::ids::PaneId;
use serde_json::json;
use thiserror::Error;

/// Erros de domínio produzidos pelas operações do crate `mt-ports`.
#[derive(Debug, Error)]
pub enum PortsError {
    /// `/proc/net/tcp` inacessível no sistema.
    #[error("Não foi possível ler as tabelas de rede em /proc/net: {reason}")]
    ProcfsUnavailable {
        /// Motivo detalhado.
        reason: String,
    },

    /// Porta ativa não encontrada para o painel informado.
    #[error("A porta :{port} não está mais ativa neste painel.")]
    NotFound {
        /// Painel consultado.
        pane_id: PaneId,
        /// Porta solicitada.
        port: u16,
    },

    /// Identidade do processo `(pid, start_time_ticks)` mudou antes do envio do sinal.
    #[error(
        "A identidade do processo da porta :{port} mudou (PID {expected_pid}); operação cancelada por segurança."
    )]
    ProcessMismatch {
        /// Porta solicitada.
        port: u16,
        /// PID esperado.
        expected_pid: u32,
    },

    /// Permissão negada (`EPERM`) ao enviar sinal ao processo.
    #[error(
        "Permissão negada pelo sistema operacional ao encerrar o processo PID {pid} na porta :{port}."
    )]
    KillDenied {
        /// Porta solicitada.
        port: u16,
        /// PID alvo.
        pid: u32,
    },

    /// Processo não encerrou mesmo após escalonamento `SIGTERM -> SIGKILL`.
    #[error(
        "O processo PID {pid} na porta :{port} não respondeu ao encerramento forçado dentro do tempo limite."
    )]
    KillTimeout {
        /// Porta solicitada.
        port: u16,
        /// PID alvo.
        pid: u32,
    },

    /// Falha ao abrir URL `http://127.0.0.1:<port>/` no navegador padrão.
    #[error("Falha ao abrir a porta :{port} no navegador: {reason}")]
    OpenFailed {
        /// Porta solicitada.
        port: u16,
        /// Motivo da falha.
        reason: String,
    },
}

/// Alias padrão de resultado do crate `mt-ports`.
pub type PortsResult<T> = Result<T, PortsError>;

impl PortsError {
    /// Retorna o [`ErrorCode`] canônico correspondente.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::ProcfsUnavailable { .. } => codes::PORTS_PROCFS_UNAVAILABLE,
            Self::NotFound { .. } => codes::PORTS_NOT_FOUND,
            Self::ProcessMismatch { .. } => codes::PORTS_PROCESS_MISMATCH,
            Self::KillDenied { .. } => codes::PORTS_KILL_DENIED,
            Self::KillTimeout { .. } => codes::PORTS_KILL_TIMEOUT,
            Self::OpenFailed { .. } => codes::PORTS_OPEN_FAILED,
        }
    }
}

impl From<PortsError> for MtError {
    fn from(err: PortsError) -> Self {
        let code = err.code();
        let message = err.to_string();
        let details = match &err {
            PortsError::ProcfsUnavailable { reason } => Some(json!({ "reason": reason })),
            PortsError::NotFound { pane_id, port } => Some(json!({
                "paneId": pane_id,
                "port": port,
            })),
            PortsError::ProcessMismatch { port, expected_pid } => Some(json!({
                "port": port,
                "expectedPid": expected_pid,
            })),
            PortsError::KillDenied { port, pid } | PortsError::KillTimeout { port, pid } => {
                Some(json!({
                    "port": port,
                    "pid": pid,
                }))
            }
            PortsError::OpenFailed { port, reason } => Some(json!({
                "port": port,
                "reason": reason,
            })),
        };

        let mut mt_err = Self::new(code, message).with_source(err);
        if let Some(d) = details {
            mt_err = mt_err.with_details(d);
        }
        mt_err
    }
}
