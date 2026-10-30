# Implementação 01 — Fundamentos do Crate `mt-bridge` (Erros, Config, Descoberta, Autenticação e Protocolo) (`spec.md`)

> Escopo da Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§1–§3, §5).
> Diretrizes canônicas: [`../../../AGENTS.md`](../../../AGENTS.md) §6.

---

## 1. Contexto, Objetivo e Escopo Exato

Esta é a **primeira unidade de implementação da Fase 11 (Integração Bidirecional com MaxCode)**. Seu objetivo é estruturar o novo crate puro `mt-bridge` (sem dependência de `tauri`), implementando as fundações de segurança, configuração em TOML, arquivo de descoberta privado (`bridge.json` com permissão `0600` e validação de identidade de processo/instância no Linux via `/proc`), autenticação em tempo constante (`Authorization: Bearer`, `OriginPolicy` e `HostPolicy` contra CSRF/DNS-rebinding), o protocolo JSON versionado v1 (`Envelope` + `Message` + `Capability`), o motor de deduplicação idempotente (`dedup.rs`) e o contrato de inversão de dependência `BridgeHost` (`handler.rs`).

### Escopo Incluído nesta Implementação:
- Criação do crate `src-tauri/crates/mt-bridge/` e registro no workspace Cargo de `src-tauri/Cargo.toml`.
- Catálogo completo de erros `BridgeError` (`src/error.rs`) mapeado para `mt_core::MtError` com códigos `bridge.*` e mensagens em `pt-BR`.
- Persistência e validação de `BridgeConfig` (`$XDG_CONFIG_HOME/maxterminal/bridge.toml`, modo `0600`) em `src/config.rs`.
- Escrita atômica e leitura segura de `DiscoveryFile` (`$XDG_RUNTIME_DIR/maxterminal/bridge.json` ou fallback `$XDG_STATE_HOME/maxterminal/run/bridge.json`), validação de `UID`, `boot_id`, `process_start_ticks` e rejeição de symlinks em `src/discovery.rs`, além de `MaxCodeDiscovery`.
- Geração e verificação de `Token` (256 bits via `OsRng`, codificado em `base64url` sem padding, comparação `subtle::ConstantTimeEq`), `OriginPolicy` e `HostPolicy` em `src/auth.rs`.
- Definição de `Envelope`, `MessageType`, `Message`, `Capability` e todos os payloads do protocolo v1 em `src/protocol.rs`.
- Reserva atômica e cache de idempotência (`Deduplicator`) por chave `(sender_id, id)` em `src/dedup.rs`.
- Definição da trait `BridgeHost` e estruturas associadas (`ProjectRef`, `FocusRequest`, `SyncContext`, `OpenTabRequest`, `BridgeEvent`, `BridgeStatus`, `BridgeState`, `PeerInfo`) em `src/handler.rs` e reexports em `src/lib.rs`.
- Testes unitários em cada módulo e teste de integração `src-tauri/crates/mt-bridge/tests/discovery_it.rs`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Modificar | `src-tauri/Cargo.toml` | Adicionar `"crates/mt-bridge"` em `[workspace].members` e declarar dependências compartilhadas (`axum`, `tower-http`, `subtle`, `base64`, `url`, `percent-encoding`, `strip-ansi-escapes`, `toml`). |
| Criar | `src-tauri/crates/mt-bridge/Cargo.toml` | Manifesto do crate `mt-bridge` (zero dependência de `tauri` ou `mt-db`). |
| Criar | `src-tauri/crates/mt-bridge/src/lib.rs` | Exportação pública dos módulos e tipos centrais (`BridgeState`, `BridgeStatus`, `PeerInfo`). |
| Criar | `src-tauri/crates/mt-bridge/src/error.rs` | `BridgeError`, método `code(&self) -> &'static str` e conversão `From<BridgeError> for mt_core::MtError`. |
| Criar | `src-tauri/crates/mt-bridge/src/config.rs` | `BridgeConfig` com serialização TOML, validação estrita e escrita atômica `0600`. |
| Criar | `src-tauri/crates/mt-bridge/src/discovery.rs` | `DiscoveryFile`, `MaxCodeDiscovery`, escrita atômica `0600` em diretório `0700`, `foreign_live_instance` via `/proc` e `boot_id`. |
| Criar | `src-tauri/crates/mt-bridge/src/auth.rs` | `Token`, `OriginPolicy`, `HostPolicy` e `verify_bearer`. |
| Criar | `src-tauri/crates/mt-bridge/src/protocol.rs` | `PROTOCOL_VERSION`, `MAX_FRAME_BYTES`, `Envelope`, `MessageType`, `Message`, `Capability` e payloads tipados. |
| Criar | `src-tauri/crates/mt-bridge/src/dedup.rs` | `Deduplicator`, `DedupReservation`, `DedupOutcome`, `DedupEntry` com TTL de 5 min e limite de 1024 entradas. |
| Criar | `src-tauri/crates/mt-bridge/src/handler.rs` | Trait `BridgeHost`, `ProjectRef`, `FocusRequest`, `SyncContext`, `OpenTabRequest`, `BridgeEvent`. |
| Criar | `src-tauri/crates/mt-bridge/tests/discovery_it.rs` | Testes de integração de descoberta (permissões `0600`/`0700`, sobrescrita de PID morto, proteção contra remoção de outra instância e rejeição de symlinks). |

---

## 3. Assinaturas Exatas e Completas (Rust)

### 3.1. `src-tauri/crates/mt-bridge/Cargo.toml`

```toml
[package]
name = "mt-bridge"
version = "0.1.0"
edition.workspace = true   # D-09: 2024 herdado do [workspace.package]
publish = false

[dependencies]
mt-core = { path = "../mt-core" }
axum = { workspace = true, features = ["ws", "json", "tokio", "http1"] }
tokio = { workspace = true, features = ["rt-multi-thread", "net", "sync", "time", "process", "fs", "macros"] }
tower-http = { workspace = true, features = ["limit", "timeout", "trace"] }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
specta = { workspace = true }
tracing = { workspace = true }
uuid = { workspace = true, features = ["v7", "serde"] }
rand = "0.9"
base64 = "0.22"
subtle = "2"
url = "2"
percent-encoding = "2"
strip-ansi-escapes = "0.2"
toml = "0.8"
thiserror = { workspace = true }
futures-util = "0.3"
async-trait = "0.1"
reqwest = { version = "0.12", default-features = false, features = ["json"] }
time = { workspace = true, features = ["serde", "formatting"] }
sha2 = "0.10"

[dev-dependencies]
tokio-tungstenite = "0.26"
tempfile = "3"
```

### 3.2. `src-tauri/crates/mt-bridge/src/error.rs`

```rust
use mt_core::{ErrorCode, MtError};
use serde_json::json;

/// Erros da ponte MaxCode. Convertidos para `MtError` com código `bridge.*` e mensagem em pt-BR.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("Não foi possível abrir a porta da ponte em 127.0.0.1")]
    BindFailed(#[source] std::io::Error),
    #[error("Token de autenticação ausente ou inválido")]
    Unauthorized,
    #[error("Origem da requisição não permitida")]
    OriginForbidden,
    #[error("Host da requisição não permitido")]
    HostForbidden,
    #[error("Versão de protocolo não suportada: {0}")]
    UnsupportedVersion(u16),
    #[error("Tipo de mensagem desconhecido: {0}")]
    UnknownType(String),
    #[error("Mensagem inválida: {0}")]
    InvalidMessage(String),
    #[error("URI inválida: {0}")]
    InvalidUri(String),
    #[error("Caminho inválido ou inexistente: {0}")]
    InvalidPath(String),
    #[error("Pasta fora do projeto")]
    CwdOutsideProject,
    #[error("Nenhum projeto corresponde ao caminho informado")]
    ProjectNotFound,
    #[error("MaxCode não confirmou o recebimento a tempo")]
    AckTimeout,
    #[error("Não foi possível abrir o MaxCode: {0}")]
    LaunchFailed(String),
    #[error("Ponte desabilitada nas configurações")]
    Disabled,
    #[error("A extensão não oferece este recurso compatível")]
    UnsupportedCapability,
    #[error("Não foi possível confirmar a entrega ao MaxCode")]
    DeliveryUnknown,
    #[error("O MaxCode não se conectou dentro do prazo")]
    WakeTimeout,
    #[error("Limite de solicitações da ponte atingido")]
    Busy,
    #[error("Identificador reutilizado para outra operação")]
    IdempotencyConflict,
    #[error("Falha de E/S na ponte: {0}")]
    Io(#[from] std::io::Error),
}

impl BridgeError {
    /// Retorna o código estável `bridge.*` correspondente ao erro.
    pub fn code(&self) -> &'static str {
        match self {
            Self::BindFailed(_) => "bridge.bind_failed",
            Self::Unauthorized => "bridge.unauthorized",
            Self::OriginForbidden => "bridge.origin_forbidden",
            Self::HostForbidden => "bridge.host_forbidden",
            Self::UnsupportedVersion(_) => "bridge.unsupported_version",
            Self::UnknownType(_) => "bridge.unknown_type",
            Self::InvalidMessage(_) => "bridge.invalid_message",
            Self::InvalidUri(_) => "bridge.invalid_uri",
            Self::InvalidPath(_) => "bridge.invalid_path",
            Self::CwdOutsideProject => "bridge.cwd_outside_project",
            Self::ProjectNotFound => "bridge.project_not_found",
            Self::AckTimeout => "bridge.ack_timeout",
            Self::LaunchFailed(_) => "bridge.launch_failed",
            Self::Disabled => "bridge.disabled",
            Self::UnsupportedCapability => "bridge.unsupported_capability",
            Self::DeliveryUnknown => "bridge.delivery_unknown",
            Self::WakeTimeout => "bridge.wake_timeout",
            Self::Busy => "bridge.busy",
            Self::IdempotencyConflict => "bridge.idempotency_conflict",
            Self::Io(_) => "bridge.io",
        }
    }
}

impl From<BridgeError> for MtError {
    fn from(err: BridgeError) -> Self {
        // D-07: `ErrorCode` é `ErrorCode(&'static str)`; código `dominio.motivo` minúsculo; mensagem pt-BR.
        let code = err.code();
        let message = err.to_string();
        let mut out = MtError::new(ErrorCode(code), message);
        // Detalhes estruturados apenas quando úteis e sem segredo (ex.: versão não suportada).
        if let BridgeError::UnsupportedVersion(v) = &err {
            out = out.with_details(json!({ "version": v }));
        }
        out
    }
}
```

### 3.3. `src-tauri/crates/mt-bridge/src/config.rs`

```rust
use crate::error::BridgeError;
use mt_core::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuração persistida em `$XDG_CONFIG_HOME/maxterminal/bridge.toml` (permissão 0600).
/// Também é o DTO Tauri: usa `camelCase` (T-11) tanto no IPC quanto nas chaves do TOML
/// (`preferredPort`, `syncActiveProject`, ...), evitando uma segunda representação.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct BridgeConfig {
    pub enabled: bool,
    pub preferred_port: u16,
    pub fallback_range: u8,
    pub allow_ephemeral_fallback: bool,
    pub sync_active_project: bool,
    pub extra_allowed_origins: Vec<String>,
    pub ack_timeout_ms: u32,
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            preferred_port: 47615,
            fallback_range: 9,
            allow_ephemeral_fallback: true,
            sync_active_project: true,
            extra_allowed_origins: Vec::new(),
            ack_timeout_ms: 3000,
        }
    }
}

impl BridgeConfig {
    /// Caminho do arquivo `bridge.toml` dentro de `AppPaths::config_dir`.
    pub fn config_path(paths: &AppPaths) -> PathBuf;

    /// Carrega a configuração do disco ou cria com padrões (modo 0600) caso não exista.
    pub fn load(paths: &AppPaths) -> Result<Self, BridgeError>;

    /// Salva atomicamente (arquivo temporário 0600 + fsync + rename) após validar.
    pub fn save(&self, paths: &AppPaths) -> Result<(), BridgeError>;

    /// Valida intervalos de porta (1024..=65535), fallback_range (0..=9 sem overflow de u16),
    /// ack_timeout_ms (500..=10000) e extra_allowed_origins (máx. 8 itens, <= 256 bytes,
    /// sem '*', sem "null" e sem schemes http:// ou https://).
    pub fn validate(&self) -> Result<(), BridgeError>;
}
```

### 3.4. `src-tauri/crates/mt-bridge/src/discovery.rs`

```rust
use crate::error::BridgeError;
use mt_core::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const DISCOVERY_VERSION: u16 = 1;
pub const MAX_DISCOVERY_BYTES: usize = 8 * 1024;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveryFile {
    pub version: u16,
    pub app: String,
    pub host: String,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub protocol: u16,
    pub started_at: String,
    pub app_version: String,
    pub instance_id: Uuid,
    pub process_start_ticks: u64,
    pub boot_id: String,
}

impl fmt::Debug for DiscoveryFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DiscoveryFile")
            .field("version", &self.version)
            .field("app", &self.app)
            .field("host", &self.host)
            .field("port", &self.port)
            .field("token", &"***")
            .field("pid", &self.pid)
            .field("protocol", &self.protocol)
            .field("started_at", &self.started_at)
            .field("app_version", &self.app_version)
            .field("instance_id", &self.instance_id)
            .field("process_start_ticks", &self.process_start_ticks)
            .field("boot_id", &self.boot_id)
            .finish()
    }
}

#[derive(Clone, Deserialize, PartialEq, Eq)]
pub struct MaxCodeDiscovery {
    pub version: u16,
    pub port: u16,
    pub token: String,
    pub pid: u32,
    pub protocol: u16,
    #[serde(default)]
    pub workspace_folders: Vec<PathBuf>,
}

impl fmt::Debug for MaxCodeDiscovery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaxCodeDiscovery")
            .field("version", &self.version)
            .field("port", &self.port)
            .field("token", &"***")
            .field("pid", &self.pid)
            .field("protocol", &self.protocol)
            .field("workspace_folders", &self.workspace_folders)
            .finish()
    }
}

/// Retorna `$XDG_RUNTIME_DIR/maxterminal/bridge.json` ou fallback `$XDG_STATE_HOME/maxterminal/run/bridge.json`.
pub fn discovery_path(paths: &AppPaths) -> PathBuf;

/// Lê `/proc/sys/kernel/random/boot_id` e o campo 22 (`starttime` em clock ticks) de `/proc/<pid>/stat`.
pub fn read_process_identity(pid: u32) -> Result<(String, u64), BridgeError>;

/// Cria `DiscoveryFile` preenchido com a identidade do processo atual (`std::process::id()`).
pub fn build_current_discovery(
    port: u16,
    token: &str,
    protocol: u16,
    app_version: &str,
    instance_id: Uuid,
) -> Result<DiscoveryFile, BridgeError>;

/// Garante diretório pai com modo `0700` pertencente ao UID atual (sem seguir symlinks),
/// grava temporário exclusivo aleatório com `O_CREAT | O_EXCL | O_NOFOLLOW` e modo `0600`,
/// executa `sync_all()`, `rename()` atômico para `path` e `sync_all()` no diretório pai.
pub fn write_discovery(path: &Path, file: &DiscoveryFile) -> Result<(), BridgeError>;

/// Remove `path` somente se `instance_id`, `pid`, `boot_id` e `process_start_ticks` corresponderem
/// à instância fornecida.
pub fn remove_discovery(path: &Path, expected_instance_id: Uuid) -> Result<(), BridgeError>;

/// Verifica se existe outra instância viva dona de `path` (comparando UID, `boot_id` e `process_start_ticks`
/// do PID contido no arquivo contra `/proc/<pid>/stat` e `instance_id` diferente do atual).
pub fn foreign_live_instance(path: &Path, current_instance_id: Uuid) -> Option<u32>;

/// Lê `$XDG_RUNTIME_DIR/maxcode/bridge.json` (ou fallback em state_dir) validando ausência de symlink
/// (`O_NOFOLLOW`), tamanho <= 8 KiB, UID igual ao processo atual, modo `0600` (`mode & 0o777 == 0o600`)
/// e processo `/proc/<pid>` vivo com o mesmo UID.
pub fn read_maxcode_discovery(paths: &AppPaths) -> Option<MaxCodeDiscovery>;
```

### 3.5. `src-tauri/crates/mt-bridge/src/auth.rs`

```rust
use crate::error::BridgeError;
use axum::http::{HeaderMap, HeaderValue, header};
use std::fmt;
use std::sync::Arc;

#[derive(Clone)]
pub struct Token(Arc<str>);

impl Token {
    /// Gera 32 bytes aleatórios criptograficamente seguros (`rand::rngs::OsRng`)
    /// e codifica em `base64::engine::general_purpose::URL_SAFE_NO_PAD` (43 caracteres).
    pub fn generate() -> Self;

    /// Constrói um `Token` a partir de um valor existente (para testes determinísticos).
    pub fn from_raw(raw: impl Into<Arc<str>>) -> Self;

    /// Compara `presented` com o token interno em tempo constante usando `subtle::ConstantTimeEq`.
    pub fn verify(&self, presented: &str) -> bool;

    /// Uso exclusivo para escrita em `DiscoveryFile` e cabeçalhos de saída autenticados.
    pub fn expose(&self) -> &str;
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(***)")
    }
}

#[derive(Debug, Clone)]
pub struct OriginPolicy {
    allowed: Vec<String>,
}

impl OriginPolicy {
    /// Inicializa a política com `vscode-file://vscode-app` mais as origens extras validadas.
    pub fn new(extra_allowed_origins: &[String]) -> Result<Self, BridgeError>;

    /// Regra:
    /// - Header `Origin` ausente: permitido (clientes Node/WS nativos).
    /// - Mais de um header `Origin` no `HeaderMap` ou valor `"null"`: `Err(BridgeError::OriginForbidden)`.
    /// - Qualquer valor iniciando com `http://` ou `https://`: `Err(BridgeError::OriginForbidden)`.
    /// - Valor idêntico a `vscode-file://vscode-app` ou presente em `allowed`: `Ok(())`.
    /// - Caso contrário: `Err(BridgeError::OriginForbidden)`.
    pub fn check(&self, headers: &HeaderMap) -> Result<(), BridgeError>;
}

#[derive(Debug, Clone, Copy)]
pub struct HostPolicy {
    port: u16,
}

impl HostPolicy {
    pub fn new(port: u16) -> Self;

    /// Aceita exclusivamente `127.0.0.1:<port>` ou `localhost:<port>` (case-insensitive para `localhost`).
    /// Rejeita múltiplos headers `Host` ou ausência de `Host`.
    pub fn check(&self, headers: &HeaderMap) -> Result<(), BridgeError>;
}

/// Extrai `Authorization: Bearer <token>` do `HeaderMap` (rejeitando múltiplos headers `Authorization`)
/// e valida com `token.verify()`.
pub fn verify_bearer(headers: &HeaderMap, token: &Token) -> Result<(), BridgeError>;
```

### 3.6. `src-tauri/crates/mt-bridge/src/protocol.rs`

Esta unidade entrega o protocolo base abaixo. **11/04** acrescenta a extensão proposta de arquivo (`editor_open_v1`/`maxcode.open_file`) e **11/06** acrescenta `issues_protocol_v1`, `bugs_cycle_v1`, `issue.command` e `bugs.cycle.{preview,start,status,progress}`, metadados de handshake e ACKs tipados. Manter `v=1` por adição e gates desabilitados até validação real; nenhuma fase posterior à 11 precisa editar este crate. DTOs Tauri de estado/entrega/eventos usam camelCase, enquanto o protocolo base conserva seus nomes explícitos.

```rust
use crate::error::BridgeError;
use mt_core::TabId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_FRAME_BYTES: usize = 256 * 1024;
pub const MAX_JSON_DEPTH: usize = 32;
pub const MAX_COLLECTION_ITEMS: usize = 32;
pub const MAX_PATH_BYTES: usize = 4096;
pub const MAX_PROMPT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PeerApp {
    Maxterminal,
    Maxcode,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub enum MessageType {
    #[serde(rename = "hello")]
    Hello,
    #[serde(rename = "hello.ack")]
    HelloAck,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "pong")]
    Pong,
    #[serde(rename = "ack")]
    Ack,
    #[serde(rename = "error")]
    Error,
    #[serde(rename = "bridge.capabilities")]
    Capabilities,
    #[serde(rename = "project.focus")]
    ProjectFocus,
    #[serde(rename = "project.opened")]
    ProjectOpened,
    #[serde(rename = "project.closed")]
    ProjectClosed,
    #[serde(rename = "terminal.open_tab")]
    TerminalOpenTab,
    #[serde(rename = "maxcode.open_conversation")]
    MaxcodeOpenConversation,
    #[serde(rename = "maxcode.open_project")]
    MaxcodeOpenProject,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct Envelope {
    pub v: u16,
    pub id: Uuid,
    #[serde(rename = "type")]
    pub kind: MessageType,
    pub ts: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to: Option<Uuid>,
    #[serde(rename = "correlationId")]
    pub correlation_id: Uuid,
    #[serde(rename = "senderId")]
    pub sender_id: Uuid,
    pub origin: PeerApp,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    ProjectSync,
    OpenTab,
    OpenConversation,
    OpenProject,
    IdempotentRequests,
    PlanMode,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationMode {
    Plan,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationSource {
    Inspect,
    Generic,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct HelloPayload {
    pub app: PeerApp,
    #[serde(rename = "senderId")]
    pub sender_id: Uuid,
    pub app_version: String,
    pub protocol: Vec<u16>,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub workspace_folders: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct HelloAckPayload {
    pub session_id: Uuid,
    pub app_version: String,
    pub protocol: u16,
    pub capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct AckPayload {
    pub handled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<TabId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ConversationMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct ErrorPayload {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct CapabilitiesPayload {
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub workspace_folders: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct ProjectPathPayload {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct OpenTabPayload {
    pub project_path: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct OpenConversationPayload {
    pub mode: ConversationMode,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_path: Option<PathBuf>,
    pub source: ConversationSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Hello(HelloPayload),
    HelloAck(HelloAckPayload),
    Ping,
    Pong,
    Ack(AckPayload),
    Error(ErrorPayload),
    Capabilities(CapabilitiesPayload),
    ProjectFocus(ProjectPathPayload),
    ProjectOpened(ProjectPathPayload),
    ProjectClosed(ProjectPathPayload),
    TerminalOpenTab(OpenTabPayload),
    MaxcodeOpenConversation(OpenConversationPayload),
    MaxcodeOpenProject(ProjectPathPayload),
}

impl Message {
    /// Valida `env.v == 1` (`UnsupportedVersion` se diferente), rejeita `MessageType::Unknown`
    /// (`UnknownType`), valida profundidade JSON <= 32, tamanhos de coleções <= 32,
    /// caminhos absolutos <= 4096 bytes e `prompt` <= 64 KiB.
    pub fn decode(env: &Envelope) -> Result<Self, BridgeError>;

    /// Constrói um `Envelope` v1 com `Uuid::now_v7()` e timestamp RFC 3339 UTC atual.
    pub fn encode(
        self,
        sender_id: Uuid,
        origin: PeerApp,
        reply_to: Option<Uuid>,
        correlation_id: Uuid,
    ) -> Envelope;
}
```

### 3.7. `src-tauri/crates/mt-bridge/src/dedup.rs`

```rust
use crate::error::BridgeError;
use crate::protocol::{AckPayload, ErrorPayload, MessageType};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::sync::broadcast;
use uuid::Uuid;

pub const DEDUP_CAPACITY: usize = 1024;
pub const DEDUP_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OperationKey {
    pub sender_id: Uuid,
    pub message_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CachedResult {
    Completed(AckPayload),
    Rejected(ErrorPayload),
}

pub enum ReservationStatus {
    /// Operação nova reservada; o chamador deve executá-la e chamar `complete` ou `reject`.
    Acquired,
    /// Operação idêntica já concluída dentro do TTL; reapresentar resultado sem reexecutar efeito.
    AlreadyResolved(CachedResult),
    /// Operação idêntica está em voo em outra task/transporte; aguardar no receiver.
    InFlight(broadcast::Receiver<CachedResult>),
}

pub struct Deduplicator {
    inner: Mutex<HashMap<OperationKey, Entry>>,
    ttl: Duration,
    capacity: usize,
}

impl Deduplicator {
    pub fn new(capacity: usize, ttl: Duration) -> Self;

    /// Calcula o fingerprint SHA-256 de `(kind, canonical_payload_bytes)` (sem armazenar o prompt em memória),
    /// expurga entradas finalizadas expiradas (`now - created_at > ttl`), nunca expulsa entradas `InFlight`,
    /// retorna `Err(BridgeError::IdempotencyConflict)` se a chave existir com fingerprint diferente,
    /// ou `Err(BridgeError::Busy)` se o cache atingir `capacity`.
    pub fn reserve(
        &self,
        key: OperationKey,
        kind: &MessageType,
        payload: &serde_json::Value,
    ) -> Result<ReservationStatus, BridgeError>;

    /// Finaliza uma operação em voo com sucesso (`AckPayload`) e notifica observadores concorrentes.
    pub fn complete(&self, key: OperationKey, ack: AckPayload);

    /// Finaliza uma operação em voo com erro (`ErrorPayload`) e notifica observadores concorrentes.
    pub fn reject(&self, key: OperationKey, error: ErrorPayload);

    /// Limpa todas as entradas (usado em restart/shutdown).
    pub fn clear(&self);
}
```

### 3.8. `src-tauri/crates/mt-bridge/src/handler.rs`

```rust
use crate::error::BridgeError;
use crate::protocol::PeerApp;
use mt_core::{MtError, ProjectId, TabId};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// T-11: todo DTO que cruza o IPC/eventos Tauri usa `camelCase`. Payloads de fio (WS/HTTP, `protocol.rs`)
// seguem o contrato externo snake_case, exceto `senderId`/`correlationId`.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRef {
    pub id: ProjectId,
    pub path: PathBuf,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SyncContext {
    pub origin: PeerApp,
    pub correlation_id: Uuid,
}

#[derive(Debug, Clone)]
pub struct FocusRequest {
    pub project: ProjectRef,
    pub worktree_path: Option<PathBuf>,
    pub context: SyncContext,
}

#[derive(Debug, Clone)]
pub struct OpenTabRequest {
    pub project: ProjectRef,
    pub template: Option<String>,
    pub cwd: Option<PathBuf>,
    pub context: SyncContext,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BridgeState {
    Disabled,
    Starting,
    Listening,
    Connected,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeerInfo {
    pub id: String,
    pub app: String,
    pub app_version: String,
    pub workspace_folders: Vec<String>,
    pub capabilities: Vec<String>,
    pub connected_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFocusRequested {
    pub project_id: ProjectId,
    pub worktree_path: Option<String>,
    pub context: SyncContext,
    pub revision: u64,
    pub instance_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    pub instance_id: String,
    pub revision: u64,
    pub selection: Option<ProjectFocusRequested>,
    pub state: BridgeState,
    pub port: Option<u16>,
    pub discovery_path: Option<String>,
    pub peers: Vec<PeerInfo>,
    pub last_error: Option<MtError>,
    pub protocol: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PeerDisconnectedEvent {
    pub peer_id: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TabOpenedEvent {
    pub tab_id: TabId,
    pub project_id: ProjectId,
    pub context: SyncContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryFailedEvent {
    pub message_id: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub enum BridgeEvent {
    StatusChanged(BridgeStatus),
    PeerConnected(PeerInfo),
    PeerDisconnected(PeerDisconnectedEvent),
    ProjectFocusRequested(ProjectFocusRequested),
    TabOpened(TabOpenedEvent),
    DeliveryFailed(DeliveryFailedEvent),
}

#[async_trait::async_trait]
pub trait BridgeHost: Send + Sync + 'static {
    /// Lista projetos cadastrados para match de caminho/worktree (Fase 06).
    async fn list_projects(&self) -> Result<Vec<ProjectRef>, BridgeError>;
    /// Resolve caminho canônico direto → projeto conhecido (Fase 06).
    async fn find_project_by_path(&self, canonical: &Path) -> Option<ProjectRef>;
    /// Seleciona/foca o projeto na UI e aguarda confirmação do consumidor (ou timeout de 3 s).
    async fn focus_project(&self, req: FocusRequest) -> Result<(), BridgeError>;
    /// Abre aba (Fase 07). Quando `template` é `None` (ex.: URI `maxterminal://tab/open`),
    /// abre obrigatoriamente template neutro `Shell` sem comando inicial.
    async fn open_tab(&self, req: OpenTabRequest) -> Result<TabId, BridgeError>;
    /// Mostra e foca a janela principal (Fase 09).
    async fn show_main_window(&self);
    /// Sanitiza segredos do texto usando o sanitizador da Fase 03.
    fn sanitize(&self, text: &str) -> String;
    /// Emite evento `bridge://*` para o frontend.
    fn on_event(&self, event: BridgeEvent);
}
```

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **Isolamento do Crate (`CA-04`)**:
   - `mt-bridge` não pode ter `tauri` nem `mt-db` na árvore de dependências normais.
   - O sanitizador da Fase 03 é acessado exclusivamente via `BridgeHost::sanitize`.
2. **Segurança de Descoberta (`CA-08`, `CA-14`, `CA-36`)**:
   - Diretório de descoberta criado com modo `0700` e validado via `std::fs::symlink_metadata` (rejeitar se for symlink ou se `uid != geteuid()`).
   - Arquivo `bridge.json` escrito via temporário `.bridge.json.tmp.<random>` aberto com `OpenOptionsExt::mode(0o600)` e `custom_flags(libc::O_NOFOLLOW)`, seguido de `sync_all()`, `std::fs::rename` e `sync_all()` no diretório.
   - Identidade do processo usa a tripla `(pid, boot_id, process_start_ticks)` extraída de `/proc/sys/kernel/random/boot_id` e do campo 22 (após o último `)` do `comm`) em `/proc/<pid>/stat`, evitando falsos positivos em reciclagem de PID.
3. **Autenticação e Anti-Rebinding (`CA-10`)**:
   - `Token::verify` decodifica ou compara os bytes usando `subtle::ConstantTimeEq::ct_eq`. Se os comprimentos divergirem, retorna `false` sem curto-circuito dependente de conteúdo secreto.
   - `OriginPolicy` rejeita qualquer `Origin` com scheme `http://` ou `https://` (mesmo `http://127.0.0.1` ou `http://localhost`), rejeita `"null"` e aceita apenas ausência de `Origin`, `vscode-file://vscode-app` ou entradas exatas de `extra_allowed_origins`.
   - `HostPolicy` aceita exclusivamente `127.0.0.1:<port>` ou `localhost:<port>`.
4. **Deduplicação e Idempotência (`CA-22`, `CA-25`, `CA-36`)**:
   - Apenas mensagens com efeito (`ProjectOpened`, `ProjectFocus`, `TerminalOpenTab`, `MaxcodeOpenConversation`, `MaxcodeOpenProject`) passam por `Deduplicator::reserve`.
   - O fingerprint armazena um hash SHA-256 de 32 bytes do tipo da mensagem + JSON canônico do payload — nunca o prompt em claro no cache de dedup.
   - Se a mesma chave `(sender_id, id)` chegar com outro tipo ou outro payload, `reserve` retorna imediatamente `Err(BridgeError::IdempotencyConflict)`.
