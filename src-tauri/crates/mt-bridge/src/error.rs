//! Erros tipados e códigos de erro estáveis do subsistema `mt-bridge` (Fase 11).

use mt_core::error::{ErrorCode, MtError};
use serde::{Deserialize, Serialize};
use specta::Type;
use thiserror::Error;

/// Códigos de erro estáveis emitidos pela ponte bidirecional `MaxTerminal` ⇄ `MaxCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BridgeErrorCode {
    /// A ponte bidirecional está desabilitada nas configurações.
    BridgeDisabled,
    /// A extensão `MaxCode` não está conectada e o fallback não pôde entregar a mensagem.
    PeerDisconnected,
    /// O arquivo de descoberta do `MaxCode` não existe, pertence a outro UID ou seu PID morreu.
    DiscoveryNotFound,
    /// As permissões do diretório ou arquivo de descoberta são inseguras.
    DiscoveryInsecurePermissions,
    /// Falha de autenticação por token Bearer ou cabeçalho `Origin` inválido.
    Unauthorized,
    /// Cabeçalho `Host` ausente, duplicado ou fora de `127.0.0.1:<porta>` / `localhost:<porta>`.
    HostForbidden,
    /// O envelope JSON não respeita o esquema v1.
    InvalidEnvelope,
    /// O envelope ou frame excede o limite de 256 KiB.
    PayloadTooLarge,
    /// Versão de protocolo incompatível no handshake `hello`.
    UnsupportedProtocolVersion,
    /// A requisição excedeu o tempo limite de ACK (3 000 ms).
    AckTimeout,
    /// O caminho de projeto ou arquivo recebido não existe ou contém componentes inválidos.
    InvalidTargetPath,
    /// Alias de caminho inválido.
    InvalidPath,
    /// O caminho existe mas não corresponde a um projeto cadastrado quando o auto-cadastro está desativado.
    ProjectNotFound,
    /// O diretório de trabalho ou worktree informado está fora do projeto autorizado.
    CwdOutsideProject,
    /// O bloco de comando informado não existe.
    BlockNotFound,
    /// O bloco de comando informado não falhou (`exit == 0`).
    BlockDidNotFail,
    /// O bloco de comando informado não pode ser inspecionado.
    BlockNotInspectable,
    /// A URI `maxterminal://` ou `vscode://johnattas.maxcode/...` é inválida ou excede 8 192 bytes.
    InvalidUri,
    /// Falha ao invocar o lançador de URI do sistema operacional para acordar o `MaxCode`.
    UriLaunchFailed,
    /// Erro interno de I/O ou transporte.
    TransportIo,
}

impl BridgeErrorCode {
    /// Retorna a representação textual canônica do código de erro.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BridgeDisabled => "BRIDGE_DISABLED",
            Self::PeerDisconnected => "BRIDGE_PEER_DISCONNECTED",
            Self::DiscoveryNotFound => "BRIDGE_DISCOVERY_NOT_FOUND",
            Self::DiscoveryInsecurePermissions => "BRIDGE_DISCOVERY_INSECURE_PERMISSIONS",
            Self::Unauthorized => "BRIDGE_UNAUTHORIZED",
            Self::HostForbidden => "BRIDGE_HOST_FORBIDDEN",
            Self::InvalidEnvelope => "BRIDGE_INVALID_ENVELOPE",
            Self::PayloadTooLarge => "BRIDGE_PAYLOAD_TOO_LARGE",
            Self::UnsupportedProtocolVersion => "BRIDGE_UNSUPPORTED_PROTOCOL_VERSION",
            Self::AckTimeout => "BRIDGE_ACK_TIMEOUT",
            Self::InvalidTargetPath | Self::InvalidPath => "BRIDGE_INVALID_TARGET_PATH",
            Self::ProjectNotFound => "BRIDGE_PROJECT_NOT_FOUND",
            Self::CwdOutsideProject => "BRIDGE_CWD_OUTSIDE_PROJECT",
            Self::BlockNotFound => "BRIDGE_BLOCK_NOT_FOUND",
            Self::BlockDidNotFail | Self::BlockNotInspectable => "BRIDGE_BLOCK_NOT_INSPECTABLE",
            Self::InvalidUri => "BRIDGE_INVALID_URI",
            Self::UriLaunchFailed => "BRIDGE_URI_LAUNCH_FAILED",
            Self::TransportIo => "BRIDGE_TRANSPORT_IO",
        }
    }

    /// Retorna o código canônico pontuado (`bridge.<motivo>`) usado na conversão para `MtError`.
    #[must_use]
    pub const fn canonical_code(self) -> &'static str {
        match self {
            Self::BridgeDisabled => "bridge.disabled",
            Self::PeerDisconnected => "bridge.peer_disconnected",
            Self::DiscoveryNotFound => "bridge.discovery_not_found",
            Self::DiscoveryInsecurePermissions => "bridge.discovery_insecure_permissions",
            Self::Unauthorized => "bridge.unauthorized",
            Self::HostForbidden => "bridge.host_forbidden",
            Self::InvalidEnvelope => "bridge.invalid_envelope",
            Self::PayloadTooLarge => "bridge.payload_too_large",
            Self::UnsupportedProtocolVersion => "bridge.unsupported_protocol_version",
            Self::AckTimeout => "bridge.ack_timeout",
            Self::InvalidTargetPath | Self::InvalidPath => "bridge.invalid_path",
            Self::ProjectNotFound => "bridge.project_not_found",
            Self::CwdOutsideProject => "bridge.cwd_outside_project",
            Self::BlockNotFound => "bridge.block_not_found",
            Self::BlockDidNotFail | Self::BlockNotInspectable => "bridge.block_not_inspectable",
            Self::InvalidUri => "bridge.invalid_uri",
            Self::UriLaunchFailed => "bridge.uri_launch_failed",
            Self::TransportIo => "bridge.transport_io",
        }
    }
}

/// Erro de domínio do crate `mt-bridge`.
#[derive(Debug, Clone, Error, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[error("[{code:?}] {message}")]
pub struct BridgeError {
    /// Código estruturado do erro.
    pub code: BridgeErrorCode,
    /// Mensagem amigável em `pt-BR` (já sanitizada, sem segredos ou tokens).
    pub message: String,
}

impl BridgeError {
    /// Constrói um novo `BridgeError`.
    #[must_use]
    pub fn new(code: BridgeErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    /// Converte para o `MtError` canônico do IPC Tauri.
    #[must_use]
    pub fn into_mt_error(self) -> MtError {
        MtError::new(ErrorCode(self.code.canonical_code()), self.message)
    }
}

impl From<BridgeError> for MtError {
    fn from(value: BridgeError) -> Self {
        value.into_mt_error()
    }
}
