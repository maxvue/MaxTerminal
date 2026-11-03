//! Protocolo de mensagens JSON do `mt-agent-relay` via Unix domain socket.

use serde::{Deserialize, Serialize};

/// Versão atual do protocolo do relay (`1`).
pub const RELAY_PROTOCOL_VERSION: u8 = 1;
/// Limite máximo de bytes por mensagem recebida no relay (64 KiB).
pub const MAX_RELAY_MESSAGE_BYTES: usize = 64 * 1024;

/// Envelope autenticado enviado pelo binário sidecar `mt-agent-relay` ao `RelayServer`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayEnvelope {
    /// Versão do protocolo (`1`).
    pub v: u8,
    /// Token de autenticação opaco (256 bits em `base64url` sem padding).
    pub token: String,
    /// Mensagem específica do evento.
    #[serde(flatten)]
    pub msg: RelayMessage,
}

/// Variantes de mensagem encaminhadas pelo `mt-agent-relay`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RelayMessage {
    /// Atualização estruturada da `statusLine` do Claude Code.
    Statusline {
        /// Payload JSON bruto lido do `stdin`.
        payload: serde_json::Value,
    },
    /// Evento de hook observacional do Claude Code (`Notification`, `Stop`, `UserPromptSubmit`, `PreToolUse`).
    Hook {
        /// Nome do evento de hook.
        event: String,
        /// Payload JSON do hook.
        payload: serde_json::Value,
    },
    /// Notificação de fim de turno do Codex (`codex-notify`).
    CodexNotify {
        /// Payload JSON recebido como argumento pelo comando `notify`.
        payload: serde_json::Value,
    },
}

/// Resposta curta devolvida pelo `RelayServer` ao `mt-agent-relay`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayAck {
    /// Indica se o envelope foi aceito.
    pub ok: bool,
    /// Código de erro curto opcional (nunca vaza detalhes de token inválido).
    pub error: Option<String>,
}
