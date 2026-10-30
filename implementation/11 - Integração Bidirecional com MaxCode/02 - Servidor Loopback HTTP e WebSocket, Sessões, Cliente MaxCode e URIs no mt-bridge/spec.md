# Implementação 02 — Servidor Loopback HTTP e WebSocket, Sessões, Cliente MaxCode e URIs no `mt-bridge` (`spec.md`)

> Escopo da Fase 11: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§4, §6, §8).
> Depende de: **Implementação 01**.

---

## 1. Contexto, Objetivo e Escopo Exato

Esta é a **segunda unidade de implementação da Fase 11**. Com os tipos de protocolo, autenticação, descoberta e deduplicação prontos da Implementação 01, esta etapa constrói o motor de rede e transporte do crate `mt-bridge`:
1. Parser de deep links de entrada `maxterminal://...` e construtor de URIs de wake-up `vscode://johnattas.maxcode/bridge/wake?requestId=<uuid>` em `src/uri.rs`.
2. Registro de sessões WebSocket (`PeerRegistry`, `PeerSession`), controle de taxa (50 msg/s por sessão, 200 req/s global), limite de 8 sessões simultâneas, timer de `hello` (5 s) e heartbeat monotônico (ping a cada 15 s, timeout de pong em 45 s) em `src/session.rs`.
3. Servidor HTTP + WebSocket em `src/server.rs` com bind **exclusivo** em `127.0.0.1` (`Ipv4Addr::LOCALHOST`), fallback sequencial de portas (`47615..=47624` e `:0`), rotas `GET /v1/health`, `POST /v1/messages` (com sessão efêmera HTTP `X-Bridge-Session`) e `GET /v1/ws`.
4. Cliente de saída `MaxCodeClient` (`src/client.rs`) com `MaxCodeAdapter`, `UriOpener`/`XdgOpener` (`tokio::process::Command` sem shell e timeout de 5 s), fila efêmera de wake-up (máx. 32 operações / 2 MiB, TTL de 30 s) e entrega autenticada com confirmação terminal (`DeliveryReport`).
5. Fachada `BridgeHandle` e `start_bridge()` em `src/lib.rs` (com métodos `status`, `client`, `restart`, `regenerate_token`, `shutdown`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Criar | `src-tauri/crates/mt-bridge/src/uri.rs` | `MaxTerminalUri::parse`, `MaxCodeUri::build` e constantes de limite (16 KiB, 4096 bytes, 2 KiB). |
| Criar | `src-tauri/crates/mt-bridge/src/session.rs` | `PeerSession`, `PeerRegistry`, `EphemeralHttpSessionStore`, rate limiter token-bucket e heartbeat. |
| Criar | `src-tauri/crates/mt-bridge/src/server.rs` | `BridgeServer`, `bind` estrito em `127.0.0.1` com fallback, router `axum`, handlers `/v1/health`, `/v1/messages`, `/v1/ws`. |
| Criar | `src-tauri/crates/mt-bridge/src/client.rs` | `DeliveryChannel`, `DeliveryReport`, `UriOpener`, `XdgOpener`, `MaxCodeAdapter`, `DefaultMaxCodeAdapter`, `MaxCodeClient` e fila de wake-up. |
| Modificar | `src-tauri/crates/mt-bridge/src/lib.rs` | Adicionar `BridgeHandle`, `start_bridge` e integração entre servidor, registro de peers, cliente e descoberta. |
| Criar | `src-tauri/crates/mt-bridge/tests/uri_it.rs` | Testes de integração e tabela de casos válidos/inválidos para `MaxTerminalUri` e `MaxCodeUri`. |
| Criar | `src-tauri/crates/mt-bridge/tests/server_it.rs` | Testes de integração de `start_bridge`, `/v1/health`, `/v1/messages`, `/v1/ws`, timeout de `hello` (`4001`), ping/pong (`45s`), rotação de token (`4003`) e fallback de porta. |

---

## 3. Assinaturas Exatas e Completas (Rust)

### 3.1. `src-tauri/crates/mt-bridge/src/uri.rs`

```rust
use crate::error::BridgeError;
use std::path::PathBuf;
use uuid::Uuid;

pub const MAX_INCOMING_URI_BYTES: usize = 16 * 1024;
pub const MAX_URI_PARAM_BYTES: usize = 4096;
pub const MAX_OUTGOING_URI_BYTES: usize = 2 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaxTerminalUri {
    ProjectOpen {
        path: PathBuf,
    },
    TabOpen {
        project: PathBuf,
        cwd: Option<PathBuf>,
        request_id: Uuid,
    },
}

impl MaxTerminalUri {
    /// Faz o parse estrito de `maxterminal://project/open?path=<abs>` ou
    /// `maxterminal://tab/open?project=<abs>&cwd=<abs|rel>&requestId=<uuid>`.
    /// Regras:
    /// - Rejeita URI > 16 KiB, caracteres de controle/NUL, userinfo, porta ou fragmento (`#`).
    /// - Aceita apenas scheme `maxterminal`, host `project` ou `tab`, e path `/open`.
    /// - Rejeita parâmetros repetidos, desconhecidos ou proibidos (`template`, `command`, `env`, `prompt`, `token`).
    /// - Valida percent-encoding bruto antes de decodificar exatamente uma vez em UTF-8;
    ///   cada valor decodificado deve ter <= 4096 bytes e não conter `\0`.
    /// - `path` e `project` devem ser caminhos absolutos (iniciando com `/`).
    pub fn parse(raw: &str) -> Result<Self, BridgeError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaxCodeUri {
    Wake { request_id: Uuid },
}

impl MaxCodeUri {
    pub const BASE: &'static str = "vscode://johnattas.maxcode";

    /// Constrói `vscode://johnattas.maxcode/bridge/wake?requestId=<uuid>` usando RFC 3986
    /// unreserved percent-encoding e garantindo comprimento <= 2 KiB.
    pub fn build(&self) -> Result<String, BridgeError>;
}
```

### 3.2. `src-tauri/crates/mt-bridge/src/session.rs`

```rust
use crate::error::BridgeError;
use crate::handler::PeerInfo;
use crate::protocol::{Capability, Envelope};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

pub const MAX_WS_SESSIONS: usize = 8;
pub const MAX_PENDING_ACKS_GLOBAL: usize = 128;
pub const SESSION_CHANNEL_CAPACITY: usize = 64;
pub const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
pub const PING_INTERVAL: Duration = Duration::from_secs(15);
pub const PONG_TIMEOUT: Duration = Duration::from_secs(45);

pub const WS_CLOSE_NORMAL_SHUTDOWN: u16 = 1001;
pub const WS_CLOSE_MESSAGE_TOO_BIG: u16 = 1009;
pub const WS_CLOSE_INTERNAL_ERROR: u16 = 1011;
pub const WS_CLOSE_HELLO_INVALID: u16 = 4001;
pub const WS_CLOSE_UNSUPPORTED_VERSION: u16 = 4002;
pub const WS_CLOSE_TOKEN_REVOKED: u16 = 4003;
pub const WS_CLOSE_RATE_LIMITED: u16 = 4008;

pub struct PeerSession {
    pub session_id: Uuid,
    pub sender_id: Uuid,
    pub info: PeerInfo,
    pub capabilities: Vec<Capability>,
    pub workspace_folders: Vec<PathBuf>,
    // Canais internos de envio de Envelope/CloseFrame e mapa de pending ACKs
}

impl PeerSession {
    pub fn has_capability(&self, cap: &Capability) -> bool;
    pub fn matches_workspace(&self, project_path: &Path) -> bool;
    pub fn record_pong(&self);
    pub fn is_pong_expired(&self, timeout: Duration) -> bool;
}

pub struct PeerRegistry {
    // RwLock<HashMap<Uuid, Arc<PeerSession>>> + contador atômico de pending ACKs + notifier de novos peers
}

impl PeerRegistry {
    pub fn new() -> Self;
    pub fn register(&self, session: Arc<PeerSession>) -> Result<(), BridgeError>;
    pub fn unregister(&self, session_id: Uuid) -> Option<Arc<PeerSession>>;
    pub fn list_info(&self) -> Vec<PeerInfo>;
    pub fn len(&self) -> usize;
    pub fn is_empty(&self) -> bool;

    /// Seleciona um peer conectado que possua todas as `required_caps`.
    /// Se `project_path` for `Some`, filtra pelos peers cujo `workspace_folders` contenha o projeto/worktree;
    /// se não houver match de workspace mas existir exatamente 1 peer conectado com as capabilities, usa-o;
    /// se houver múltiplos peers ambíguos sem match unívoco de workspace, retorna `None`.
    pub fn pick_for(
        &self,
        project_path: Option<&Path>,
        required_caps: &[Capability],
    ) -> Option<Arc<PeerSession>>;

    /// Envia `msg` ao `peer` e aguarda um `Envelope` de resposta (`ack` ou `error`) com `reply_to == msg.id`
    /// vindo do mesmo `peer.sender_id` dentro de `timeout`.
    pub async fn request(
        &self,
        peer: &Arc<PeerSession>,
        msg: Envelope,
        timeout: Duration,
    ) -> Result<Envelope, BridgeError>;

    /// Aguarda até que um peer compatível se conecte (usado pela fila de wake-up).
    pub async fn wait_for_peer(
        &self,
        project_path: Option<&Path>,
        required_caps: &[Capability],
        timeout: Duration,
    ) -> Result<Arc<PeerSession>, BridgeError>;

    /// Fecha todas as sessões abertas enviando CloseFrame com `code` (ex.: `4003` ou `1001`).
    pub fn close_all(&self, code: u16, reason: &'static str);
}
```

### 3.3. `src-tauri/crates/mt-bridge/src/server.rs`

```rust
use crate::auth::{HostPolicy, OriginPolicy, Token};
use crate::config::BridgeConfig;
use crate::dedup::Deduplicator;
use crate::error::BridgeError;
use crate::handler::BridgeHost;
use crate::session::PeerRegistry;
use axum::Router;
use std::sync::Arc;
use tokio::net::TcpListener;

/// Faz o bind estrito em `127.0.0.1` (`Ipv4Addr::LOCALHOST`) testando:
/// 1) `127.0.0.1:preferred_port`
/// 2) `preferred_port + 1 ..= preferred_port + fallback_range`
/// 3) `127.0.0.1:0` se `allow_ephemeral_fallback == true`
/// Nunca utiliza `0.0.0.0` ou `::`.
pub async fn bind(cfg: &BridgeConfig) -> Result<(TcpListener, u16), BridgeError>;

/// Constrói o `axum::Router` com as rotas:
/// - `GET /v1/health` (valida `HostPolicy` + `OriginPolicy`, retorna `200 {"ok":true,"protocol":1}`)
/// - `POST /v1/messages` (valida `HostPolicy` + `OriginPolicy` + `Bearer` + `Content-Type: application/json`,
///   limite de 256 KiB, negociação `X-Bridge-Session` para `hello` e despacho autenticado com `Deduplicator`)
/// - `GET /v1/ws` (valida `HostPolicy` + `OriginPolicy` + `Bearer`, limite de 8 conexões, handshake `hello` em 5 s,
///   rejeita frames binários e mensagens fragmentadas agregadas > 256 KiB)
/// - Qualquer outro método nas rotas conhecidas → `405 Method Not Allowed`; outras rotas → `404 Not Found` sem corpo.
/// Nunca emite cabeçalhos CORS (`Access-Control-Allow-*`).
pub fn build_router(state: Arc<ServerState>) -> Router;
```

### 3.4. `src-tauri/crates/mt-bridge/src/client.rs`

```rust
use crate::discovery::MaxCodeDiscovery;
use crate::error::BridgeError;
use crate::handler::{BridgeHost, SyncContext};
use crate::protocol::{Capability, ConversationMode, ConversationSource};
use crate::session::PeerRegistry;
use mt_core::paths::AppPaths;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use uuid::Uuid;

pub const WAKE_QUEUE_MAX_ITEMS: usize = 32;
pub const WAKE_QUEUE_MAX_BYTES: usize = 2 * 1024 * 1024;
pub const WAKE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryChannel {
    Websocket,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct DeliveryReport {
    pub channel: DeliveryChannel,
    pub message_id: String,
    pub truncated: bool,
    pub wake_used: bool,
    pub confirmed: bool,
    pub conversation_id: Option<String>,
    pub mode: Option<ConversationMode>,
    pub fallback_reason: Option<String>,
}

#[async_trait::async_trait]
pub trait UriOpener: Send + Sync + 'static {
    async fn open(&self, uri: &str) -> Result<(), BridgeError>;
}

/// Abre URIs no Linux via `tokio::process::Command::new("xdg-open").arg(uri)`
/// sem shell, `stdin`/`stdout`/`stderr` nulos, `kill_on_drop(true)` e timeout de 5 s.
pub struct XdgOpener;

#[async_trait::async_trait]
pub trait MaxCodeAdapter: Send + Sync + 'static {
    /// Retorna as capabilities locais suportadas e se o adaptador externo está habilitado para wake/HTTP.
    fn capabilities(&self) -> Vec<Capability>;
    /// Constrói a URI pública de wake-up (ex.: `vscode://johnattas.maxcode/bridge/wake?requestId=<uuid>`).
    /// Retorna `Err(BridgeError::UnsupportedCapability)` se o wake-up não estiver habilitado no adaptador.
    fn build_wake_uri(&self, request_id: Uuid) -> Result<String, BridgeError>;
    /// Descobre endpoint HTTP do MaxCode quando validado; retorna `Ok(None)` por padrão até validação real.
    async fn discover_http(&self, paths: &AppPaths) -> Result<Option<MaxCodeDiscovery>, BridgeError>;
}

pub struct DefaultMaxCodeAdapter {
    pub allow_wake_uri: bool,
    pub allow_http_discovery: bool,
}

pub struct MaxCodeClient {
    // instance_id, registry, opener, paths, adapter, host, ack_timeout, fila de wake-up limitada
}

impl MaxCodeClient {
    pub fn new(
        instance_id: Uuid,
        registry: Arc<PeerRegistry>,
        opener: Arc<dyn UriOpener>,
        paths: AppPaths,
        adapter: Arc<dyn MaxCodeAdapter>,
        host: Arc<dyn BridgeHost>,
        ack_timeout: Duration,
    ) -> Self;

    /// Abre uma conversa em Plan Mode no MaxCode (`maxcode.open_conversation`).
    /// Sanitiza `prompt` via `host.sanitize()`, valida limite de 64 KiB, exige capabilities
    /// `OpenConversation` e `PlanMode`, e aguarda ACK terminal com `handled == true`,
    /// `conversation_id.is_some()` e `mode == Some(ConversationMode::Plan)`.
    pub async fn open_plan(
        &self,
        prompt: &str,
        project_path: Option<&Path>,
        source: ConversationSource,
        pre_truncated: bool,
    ) -> Result<DeliveryReport, BridgeError>;

    /// Solicita ao MaxCode abrir um projeto (`maxcode.open_project`).
    pub async fn open_project(&self, project_path: &Path) -> Result<DeliveryReport, BridgeError>;

    /// Notifica troca de foco (`project.focus`) somente se houver peer conectado com `ProjectSync`
    /// (nunca dispara wake-up).
    pub async fn notify_focus(
        &self,
        project_path: &Path,
        context: SyncContext,
    ) -> Result<(), BridgeError>;

    /// Cancela todas as operações pendentes na fila de wake-up (chamado em disable/shutdown/rotação).
    pub fn cancel_pending(&self, reason: BridgeError);
}
```

### 3.5. `src-tauri/crates/mt-bridge/src/lib.rs`

```rust
pub mod auth;
pub mod client;
pub mod config;
pub mod dedup;
pub mod discovery;
pub mod error;
pub mod handler;
pub mod protocol;
pub mod server;
pub mod session;
pub mod uri;

use crate::client::{MaxCodeAdapter, MaxCodeClient, UriOpener};
use crate::config::BridgeConfig;
use crate::error::BridgeError;
use crate::handler::{BridgeHost, BridgeState, BridgeStatus, PeerInfo};
use mt_core::paths::AppPaths;
use std::sync::Arc;

#[derive(Clone)]
pub struct BridgeHandle {
    // Arc<BridgeInner>
}

pub async fn start_bridge(
    paths: AppPaths,
    config: BridgeConfig,
    host: Arc<dyn BridgeHost>,
) -> Result<BridgeHandle, BridgeError>;

pub async fn start_bridge_with(
    paths: AppPaths,
    config: BridgeConfig,
    host: Arc<dyn BridgeHost>,
    opener: Arc<dyn UriOpener>,
    adapter: Arc<dyn MaxCodeAdapter>,
) -> Result<BridgeHandle, BridgeError>;

impl BridgeHandle {
    pub fn status(&self) -> BridgeStatus;
    pub fn config(&self) -> BridgeConfig;
    pub fn client(&self) -> &MaxCodeClient;
    pub async fn restart(&self, config: BridgeConfig) -> Result<BridgeStatus, BridgeError>;
    pub async fn regenerate_token(&self) -> Result<BridgeStatus, BridgeError>;
    pub async fn shutdown(&self);
}
```

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **Bind Estrito em Loopback (`CA-06`, `CA-07`)**:
   - Todo `SocketAddr` utiliza `SocketAddr::from((Ipv4Addr::LOCALHOST, port))`.
   - Se `config.enabled == false`, `start_bridge` retorna um `BridgeHandle` em `BridgeState::Disabled`, sem abrir porta e removendo qualquer `bridge.json` residual desta instância.
   - Se o bind falhar em todas as portas tentadas, o erro é registrado em `last_error`, `state` passa a `BridgeState::Failed`, `on_event(BridgeEvent::StatusChanged(...))` é emitido e `start_bridge` ainda retorna o `BridgeHandle` funcional para comandos de configuração/restart.
2. **Segurança e Limites no HTTP e WebSocket (`CA-09`, `CA-10`, `CA-11`, `CA-13`, `CA-36`)**:
   - Rate limit global de 200 req/s e por sessão de 50 msg/s. Excesso HTTP retorna `429 Too Many Requests` com header `Retry-After: 1`; saturação de conexões (> 16 requests HTTP em voo ou > 8 sessões WS) retorna `503 Service Unavailable`.
   - No WebSocket, mensagens fragmentadas (frames `Text` seguidos de `Continuation` sem `fin`) são acumuladas em buffer limitado a `MAX_FRAME_BYTES` (256 KiB); se ultrapassar 256 KiB antes do `fin`, a conexão é encerrada imediatamente com código `1009`.
   - Se o cliente WS não enviar `hello` válido em até 5 s após o upgrade, fechar com código `4001`.
   - Se o cliente enviar 10 payloads inválidos consecutivos, fechar com código `4001`.
   - Heartbeat envia `ping` a cada 15 s e verifica a cada ciclo se `Instant::now() - last_pong > 45 s`; se expirado, fecha a sessão, remove do `PeerRegistry` e emite `BridgeEvent::PeerDisconnected` + `BridgeEvent::StatusChanged`.
3. **Semântica de Entrega e Wake-up (`CA-21`, `CA-22`, `CA-23`)**:
   - A URI `vscode://johnattas.maxcode/bridge/wake?requestId=<uuid>` contém **apenas** o `requestId` público. Nenhum prompt, comando, caminho ou token é incluído na URI.
   - Sem peer WS ativo, `MaxCodeClient` enfileira a operação em memória (TTL de 30 s, máx. 32 itens / 2 MiB), chama `opener.open(&wake_uri)` e aguarda `registry.wait_for_peer(...)`. Assim que o MaxCode lê `bridge.json`, conecta ao WS e conclui `hello` anunciando `OpenConversation` e `PlanMode`, a operação é transmitida pelo canal autenticado e só retorna `Ok(DeliveryReport { confirmed: true, wake_used: true, ... })` após o ACK terminal.
   - Se o timeout expirar **após** o frame já ter sido enviado no socket sem resposta (`AckTimeout`), `MaxCodeClient` só tenta reenvio se o mesmo peer continuar conectado (mesmo `sender_id`) e tiver anunciado `Capability::IdempotentRequests`; caso contrário, retorna `Err(BridgeError::DeliveryUnknown)` sem recriar a conversa.
