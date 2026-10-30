# Implementação 02 — Crate `mt-ports`: Escaneamento `/proc`, Poller Adaptativo, Killer e Opener

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `02 - Crate mt-ports Escaneamento Procfs Poller Adaptativo Killer e Opener`
> **Referências Canônicas:** [`../instructions.md`](../instructions.md) §2.1, §4.2, §5.1–§5.4; [`../spec.md`](../spec.md) §3.1–§3.7, §9, §10, §11.2.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói integralmente o crate independente `mt-ports` (`src-tauri/crates/mt-ports/`), responsável por detectar portas TCP em escuta (`LISTEN`) e sockets UDP vinculados pertencentes aos processos filhos supervisionados de cada aba/painel, gerenciar o polling adaptativo com leitura única de `/proc/net` por tick, construir URLs locais seguras para abertura no navegador e coordenar o encerramento seguro do processo proprietário com proteção anti-TOCTOU.

Assim como `mt-git`, o crate `mt-ports` **não depende de Tauri**, expondo as traits `ProcessTreeSource`, `ExactProcessSignaler`, `PortUrlOpener` e `PortEventSink`, o que permite testar deterministicamente o poller (com `tokio::time::pause()`), o parser de `/proc/net`, o mapeamento de descritores `/proc/<pid>/fd` e a máquina de estados de encerramento (`SIGTERM` → 3 s → `SIGKILL`).

### 1.1. Objetivos Específicos
1. **Parser de `/proc/net/{tcp,tcp6,udp,udp6}` (`procnet.rs`):** Analisar tabelas hexadecimais do kernel Linux em < 3 ms para milhares de linhas, filtrando TCP no estado `0A` (`LISTEN`) e UDP vinculado (`port > 0`, `inode > 0`, estados `07`/`01`), decodificando endereços IPv4 little-endian e IPv6 (4 palavras de 32 bits little-endian, incluindo IPv4-mapeado `::ffff:a.b.c.d`), filtrando por `uid == geteuid()` e registrando o `netns_inode` de `/proc/self/ns/net`.
2. **Mapeamento de Inodes para Árvore de Processos (`tree.rs`, `fdmap.rs`):** Definir a trait `ProcessTreeSource` (e a implementação `ProcfsTreeSource` para testes/fallback), verificar `starttime` (campo 22 de `/proc/<pid>/stat`) antes e depois da leitura de `/proc/<pid>/fd/*`, validar o mesmo *network namespace* (`/proc/<pid>/ns/net`) e mapear `inode → Vec<FdOwner>` preservando múltiplos donos em sockets herdados/compartilhados.
3. **Poller Único com Agendamento Adaptativo (`poller.rs`, `lib.rs`):** Executar um único loop assíncrono que lê `/proc/net/*` **uma única vez por tick** para todas as abas vencidas (`next_due <= now`), alternando automaticamente entre os modos `Active` (1 s), `Lingering` (5 s), `IdleProbe` (30 s) e `Off`, gerando tokens opacos `PortId` por `(dono, protocolo, porta)` e emitindo deltas versionados (`port://opened`, `port://updated`, `port://closed`, `port://snapshot`).
4. **Encerramento Seguro com Revalidação Anti-TOCTOU (`killer.rs`):** Definir a trait `ExactProcessSignaler` e o fluxo `kill_port_owner`: recusar o shell raiz da aba (`KillRootRefused`), adquirir handle `pidfd`, revalidar posse/UID/namespace/starttime/inode antes do `SIGTERM`, aguardar até 3 s pela saída do processo, revalidar novamente antes do `SIGKILL` (detectando `SocketReleased` caso o processo tenha liberado o socket sem morrer) e realizar nova varredura retornando `PortKillResult { outcome, remaining }`.
5. **Resolução Segura de URL Local para Navegador (`opener.rs`):** Definir a trait `PortUrlOpener` e `open_in_browser`, construindo a URL HTTP local exclusivamente no backend (`http://localhost:PORT` para dual-stack, `http://127.0.0.1:PORT` para IPv4 loopback/wildcard, `http://[::1]:PORT` para IPv6 loopback/wildcard) e recusando sockets UDP ou binds exclusivos em IP de LAN (`NotBrowserCompatible`).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Modificações no Workspace Cargo
- `src-tauri/Cargo.toml` — Adicionar `"crates/mt-ports"` em `[workspace].members` e declarar `mt-ports = { path = "crates/mt-ports" }` em `[workspace.dependencies]`.
- `src-tauri/crates/mt-core/src/error.rs` (se necessário) — Garantir suporte aos códigos de erro `ports.*` no `MtError`.

### 2.2. Novos Arquivos em `src-tauri/crates/mt-ports/`
- `src-tauri/crates/mt-ports/Cargo.toml`
- `src-tauri/crates/mt-ports/src/lib.rs`
- `src-tauri/crates/mt-ports/src/types.rs`
- `src-tauri/crates/mt-ports/src/error.rs`
- `src-tauri/crates/mt-ports/src/procnet.rs`
- `src-tauri/crates/mt-ports/src/tree.rs`
- `src-tauri/crates/mt-ports/src/fdmap.rs`
- `src-tauri/crates/mt-ports/src/poller.rs`
- `src-tauri/crates/mt-ports/src/killer.rs`
- `src-tauri/crates/mt-ports/src/opener.rs`

### 2.3. Testes e Fixtures em `src-tauri/crates/mt-ports/tests/`
- `src-tauri/crates/mt-ports/tests/fixtures/proc_net_tcp.txt`
- `src-tauri/crates/mt-ports/tests/fixtures/proc_net_tcp6.txt`
- `src-tauri/crates/mt-ports/tests/fixtures/proc_net_udp.txt`
- `src-tauri/crates/mt-ports/tests/fixtures/proc_net_udp6.txt`
- `src-tauri/crates/mt-ports/tests/procnet.rs`
- `src-tauri/crates/mt-ports/tests/poller.rs`
- `src-tauri/crates/mt-ports/tests/live_listen.rs`

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. Manifesto `src-tauri/crates/mt-ports/Cargo.toml`

```toml
[package]
name = "mt-ports"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
mt-core = { workspace = true }
parking_lot = { workspace = true }
rustix = { workspace = true, features = ["fs", "process"] }
serde = { workspace = true, features = ["derive"] }
specta = { workspace = true, features = ["derive"] }
thiserror = { workspace = true }
tokio = { workspace = true, features = ["macros", "rt", "sync", "time"] }
tracing = { workspace = true }
uuid = { workspace = true, features = ["v4", "v7", "serde"] }

[dev-dependencies]
tempfile = { workspace = true }
tokio = { workspace = true, features = ["full", "test-util"] }
```

### 3.2. Tipos Públicos (`src-tauri/crates/mt-ports/src/types.rs`)

```rust
use std::net::IpAddr;
use mt_core::{MtError, PaneId, PtyId, TabId, TabStatus};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum IpFamily {
    V4,
    V6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum PortProtocol {
    Tcp,
    Udp,
}

/// Token opaco de uma observação; nunca reutilizado após fechamento, troca de socket ou troca de dono.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
pub struct PortId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortAddress {
    pub ip: String,
    pub family: IpFamily,
    pub is_loopback: bool,
    pub is_wildcard: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ListeningPort {
    pub id: PortId,
    pub protocol: PortProtocol,
    pub port: u16,
    /// Endereços vinculados pelo mesmo dono, PTY e protocolo nesta porta.
    pub addresses: Vec<PortAddress>,
    pub pid: i32,
    /// Ticks decimais (campo 22 de `/proc/<pid>/stat`) em String para evitar perda de precisão em JS.
    pub start_time: String,
    /// Nome curto do processo (`/proc/<pid>/comm`).
    pub process_name: String,
    pub pty_id: PtyId,
    pub pane_id: PaneId,
    pub tab_id: TabId,
    pub can_open_browser: bool,
    pub can_kill: bool,
    /// `true` quando há mais de um processo conhecido segurando o mesmo inode de socket.
    pub shared_socket: bool,
    /// Epoch ms em que o token foi detectado pela primeira vez.
    pub detected_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum KillOutcome {
    Terminated,
    Killed,
    AlreadyGone,
    /// O processo permaneceu vivo, mas fechou/liberou o socket durante a janela de espera.
    SocketReleased,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortKillResult {
    pub outcome: KillOutcome,
    /// Outros donos/portas ainda presentes na aba após nova varredura imediata.
    pub remaining: Vec<ListeningPort>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollMode {
    /// 1 s — aba `running`/`continuous` ou algum painel com processo foreground ativo.
    Active,
    /// 5 s — aba ociosa (`idle`/`finished`/`error`), mas com descendente vivo ou porta conhecida aberta.
    Lingering,
    /// 30 s — PTY vivo sem descendentes nem portas conhecidas (reconciliação leve).
    IdleProbe,
    /// Desligado — nenhum PTY vivo na aba.
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortSnapshot {
    pub epoch: String,
    pub revision: u32,
    pub tab_id: TabId,
    pub ports: Vec<ListeningPort>,
    pub stale: bool,
    pub error: Option<MtError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortOpened {
    pub epoch: String,
    pub revision: u32,
    pub tab_id: TabId,
    pub port: ListeningPort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortUpdated {
    pub epoch: String,
    pub revision: u32,
    pub tab_id: TabId,
    pub port: ListeningPort,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PortClosed {
    pub epoch: String,
    pub revision: u32,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub port_id: PortId,
}

/// Identidade interna imutável de um socket individual no kernel.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SocketIdentity {
    pub netns_inode: u64,
    pub protocol: PortProtocol,
    pub family: IpFamily,
    pub local_addr: IpAddr,
    pub port: u16,
    pub inode: u64,
}
```

### 3.3. Erros (`src-tauri/crates/mt-ports/src/error.rs`)

```rust
use mt_core::MtError;
use mt_core::error::ErrorCode;

#[derive(Debug, thiserror::Error)]
pub enum PortsError {
    #[error("/proc indisponível: {0}")]
    ProcUnavailable(std::io::Error),
    #[error("Porta não pertence a esta aba")]
    NotTracked,
    #[error("O processo dono da porta já não existe")]
    OwnerGone,
    #[error("Sem permissão para encerrar o processo")]
    PermissionDenied,
    #[error("Recusado: o processo é o shell da aba")]
    KillRootRefused,
    #[error("O processo não encerrou após SIGKILL")]
    KillTimeout,
    #[error("Falha ao abrir o navegador padrão: {0}")]
    OpenerFailed(String),
    #[error("Este socket não oferece endereço HTTP local")]
    NotBrowserCompatible,
    #[error("Sinalização segura indisponível neste sistema")]
    SafeSignalUnavailable,
    #[error("Monitoramento parcial; atualize antes de agir")]
    SnapshotStale,
}

impl PortsError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::ProcUnavailable(_) => "ports.proc_unavailable",
            Self::NotTracked => "ports.not_tracked",
            Self::OwnerGone => "ports.owner_gone",
            Self::PermissionDenied => "ports.permission_denied",
            Self::KillRootRefused => "ports.kill_root_refused",
            Self::KillTimeout => "ports.kill_timeout",
            Self::OpenerFailed(_) => "ports.opener_failed",
            Self::NotBrowserCompatible => "ports.not_browser_compatible",
            Self::SafeSignalUnavailable => "ports.safe_signal_unavailable",
            Self::SnapshotStale => "ports.snapshot_stale",
        }
    }
}

impl From<PortsError> for MtError {
    fn from(err: PortsError) -> Self {
        MtError::new(ErrorCode(err.code()), err.to_string())
    }
}
```

### 3.4. Parser `/proc/net/{tcp,tcp6,udp,udp6}` (`src-tauri/crates/mt-ports/src/procnet.rs`)

```rust
use std::net::IpAddr;
use crate::error::PortsError;
use crate::types::{IpFamily, PortProtocol};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenSocket {
    pub inode: u64,
    pub port: u16,
    pub addr: IpAddr,
    pub uid: u32,
    pub protocol: PortProtocol,
    pub family: IpFamily,
    pub netns_inode: u64,
}

#[derive(Debug, Clone)]
pub struct SocketRead {
    pub sockets: Vec<ListenSocket>,
    pub netns_inode: u64,
    /// `true` se alguma tabela essencial falhou por permissão/formato (mantém snapshot `stale`).
    pub partial_error: bool,
}

pub fn parse_proc_net(
    content: &str,
    family: IpFamily,
    protocol: PortProtocol,
    netns_inode: u64,
    expected_uid: u32,
) -> Vec<ListenSocket>;

pub fn read_listen_sockets() -> Result<SocketRead, PortsError>;

/// Lê o inode de `/proc/self/ns/net` ou `/proc/<pid>/ns/net`.
pub fn read_netns_inode_for_pid(pid: Option<i32>) -> Option<u64>;
```

#### Regras de Parsing de `procnet.rs`
1. **Filtro de Estado (`st`, coluna 3, 0-indexed):**
   - Para `PortProtocol::Tcp`: aceitar **apenas** `st == "0A"` (`TCP_LISTEN`).
   - Para `PortProtocol::Udp`: aceitar estados vinculados (`st == "07"` `TCP_CLOSE`/unconnected bound ou `st == "01"` `TCP_ESTABLISHED`), desde que `port > 0` e `inode > 0`.
2. **Filtro de UID e Inode:**
   - Coluna 7 é `uid` decimal; descartar se `uid != expected_uid` (`rustix::process::geteuid().as_raw()`).
   - Coluna 9 é `inode` decimal; descartar se `inode == 0`.
3. **Decodificação de Endereço Hexadecimal (`local_address = <HEX_IP>:<HEX_PORT>`):**
   - `port = u16::from_str_radix(hex_port, 16)`. Se `port == 0`, descartar.
   - `IpFamily::V4`: `hex_ip` deve ter exatamente 8 caracteres hexadecimais. Converter `u32::from_str_radix(hex_ip, 16)?.swap_bytes()` ⇒ `IpAddr::V4(Ipv4Addr::from(host_u32))`. Exemplo: `"0100007F"` → `0x0100007F_u32.swap_bytes()` = `0x7F000001` → `127.0.0.1`.
   - `IpFamily::V6`: `hex_ip` deve ter exatamente 32 caracteres hexadecimais (4 palavras de 8 chars hex / 32 bits cada). Para cada fatia `i` de 8 caracteres (`0..8`, `8..16`, `16..24`, `24..32`), converter `u32::from_str_radix(word_hex, 16)?.swap_bytes().to_be_bytes()` e copiar para um array `[u8; 16]` ⇒ `IpAddr::V6(Ipv6Addr::from(octets))`.
   - Nota: endereços IPv4-mapeados em IPv6 (`::ffff:127.0.0.1`) permanecem com `family: IpFamily::V6` e `IpAddr::V6` na identidade do socket; na exibição/endereço (`PortAddress`), `is_loopback` considera `ip.is_loopback()` ou `v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())`, e `is_wildcard` considera `ip.is_unspecified()` ou `v6.to_ipv4_mapped().is_some_and(|v4| v4.is_unspecified())`.
4. **Tolerância a IPv6 Desativado:**
   - Em `read_listen_sockets()`, `/proc/net/tcp` e `/proc/net/udp` são obrigatórios (se ambos falharem com `ENOENT`, retornar `Err(PortsError::ProcUnavailable)`). `/proc/net/tcp6` e `/proc/net/udp6` ausentes (`ErrorKind::NotFound`) são ignorados sem erro.

### 3.5. Árvore de Processos (`tree.rs`) e Mapa de FDs (`fdmap.rs`)

```rust
// src-tauri/crates/mt-ports/src/tree.rs
use mt_core::PtyId;
use crate::error::PortsError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcId {
    pub pid: i32,
    /// Campo 22 (`starttime`) de `/proc/<pid>/stat`.
    pub starttime: u64,
}

pub trait ProcessTreeSource: Send + Sync + 'static {
    /// PID raiz (shell) do PTY, se vivo e validado contra o owner.
    fn root(&self, pty: PtyId) -> Result<Option<ProcId>, PortsError>;
    /// Raiz + todos os descendentes/membros vivos supervisionados do PTY.
    fn descendants(&self, pty: PtyId, root: ProcId) -> Result<Vec<ProcId>, PortsError>;
    /// Reconsulta supervisor e `/proc`; retorna `true` somente se o processo ainda pertencer ao PTY.
    fn owns(&self, pty: PtyId, process: ProcId) -> Result<bool, PortsError>;
    /// Informa se algum painel do PTY possui processo de foreground ativo além do shell ocioso.
    fn has_active_foreground(&self, pty: PtyId) -> bool { false }
}

/// Implementação de referência lendo `/proc/<pid>/task/*/children`; usada em testes de integração.
pub struct ProcfsTreeSource {
    roots: parking_lot::RwLock< std::collections::HashMap<PtyId, ProcId> >,
}
```

```rust
// src-tauri/crates/mt-ports/src/fdmap.rs
use std::collections::{HashMap, HashSet};
use crate::error::PortsError;
use crate::tree::ProcId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FdOwner {
    pub proc: ProcId,
    pub comm: String,
    pub uid: u32,
    pub netns_inode: u64,
}

#[derive(Debug, Default)]
pub struct FdScanReport {
    pub owners_by_inode: HashMap<u64, Vec<FdOwner>>,
    /// Conjunto de PIDs cuja leitura de `fd/` sofreu `EACCES` (desabilita ações destrutivas/marca parcial).
    pub partial_permission_error: bool,
}

pub fn map_inodes(
    pids: &[ProcId],
    wanted: &HashSet<u64>,
    expected_netns: u64,
) -> Result<FdScanReport, PortsError>;

/// Lê `/proc/<pid>/stat` com segurança (buscando o último `')'` para isolar `comm` com espaços/parênteses)
/// e retorna `(comm, state_char, starttime_field_22)`.
pub fn read_proc_stat(pid: i32) -> Option<(String, char, u64)>;

pub fn read_starttime(pid: i32) -> Option<u64>;
```

#### Regras de `fdmap.rs`
1. Se `wanted.is_empty()`, `map_inodes` retorna imediatamente `Ok(FdScanReport::default())` sem abrir `/proc/<pid>/fd`.
2. Para cada `proc_id` em `pids`:
   - Ler `read_proc_stat(proc_id.pid)` **antes** de varrer `fd/`. Se `starttime != proc_id.starttime` ou o processo não existir (`None`), descartar silenciosamente (`ENOENT` é corrida normal).
   - Verificar `read_netns_inode_for_pid(Some(proc_id.pid))`. Se divergir de `expected_netns`, ignorar o processo (não associar inode entre *network namespaces* distintos).
   - Ler entradas de `/proc/<pid>/fd`: para cada descritor, chamar `std::fs::read_link`. Extrair inode se o link tiver o formato `"socket:[<inode>]"`. Se `<inode>` estiver em `wanted`, registrar em uma lista temporária do PID.
   - Ler `read_proc_stat(proc_id.pid)` **depois** de varrer `fd/`. Se `starttime != proc_id.starttime`, descartar todas as amostras daquele PID!
   - Se a abertura de `/proc/<pid>/fd` falhar com `ErrorKind::PermissionDenied` (`EACCES`), marcar `partial_permission_error = true`.

### 3.6. Poller Adaptativo e `PortsService` (`src-tauri/crates/mt-ports/src/poller.rs`, `src/lib.rs`)

```rust
use std::sync::Arc;
use mt_core::{PaneId, PtyId, TabId, TabStatus};
use crate::error::PortsError;
use crate::killer::{ExactProcessSignaler, kill_port_owner};
use crate::opener::{PortUrlOpener, open_in_browser};
use crate::procnet::SocketRead;
use crate::tree::ProcessTreeSource;
use crate::types::{
    PortClosed, PortId, PortKillResult, PortOpened, PortSnapshot, PortUpdated,
};

pub trait PortEventSink: Send + Sync + 'static {
    fn opened(&self, event: PortOpened);
    fn updated(&self, event: PortUpdated);
    fn closed(&self, event: PortClosed);
    fn snapshot(&self, snapshot: PortSnapshot);
}

/// Abstração interna de leitura de sockets para permitir injeção determinística nos testes do poller.
pub trait SocketTableSource: Send + Sync + 'static {
    fn read_sockets(&self) -> Result<SocketRead, PortsError>;
    fn scan_fds(
        &self,
        pids: &[crate::tree::ProcId],
        wanted: &std::collections::HashSet<u64>,
        expected_netns: u64,
    ) -> Result<crate::fdmap::FdScanReport, PortsError>;
}

pub struct PortsService {
    // ... campos internos conforme spec.md §3.4
}

impl PortsService {
    pub fn spawn(
        tree: Arc<dyn ProcessTreeSource>,
        signaler: Arc<dyn ExactProcessSignaler>,
        opener: Arc<dyn PortUrlOpener>,
        emitter: Arc<dyn PortEventSink>,
    ) -> Arc<Self>;

    pub fn spawn_with_socket_source(
        tree: Arc<dyn ProcessTreeSource>,
        socket_source: Arc<dyn SocketTableSource>,
        signaler: Arc<dyn ExactProcessSignaler>,
        opener: Arc<dyn PortUrlOpener>,
        emitter: Arc<dyn PortEventSink>,
         can_safe_signal: bool,
    ) -> Arc<Self>;

    pub fn track_pane(&self, tab: TabId, pane: PaneId, pty: PtyId) -> Result<PortSnapshot, PortsError>;
    pub fn untrack_pane(&self, tab: TabId, pane: PaneId);
    pub fn set_tab_status(&self, tab: TabId, status: TabStatus);
    pub fn notify_command_finished(&self, tab: TabId);
    pub fn ports_for_tab(&self, tab: TabId) -> PortSnapshot;
    pub async fn scan_now(&self, tab: TabId) -> Result<PortSnapshot, PortsError>;
    pub async fn kill(&self, tab: TabId, id: PortId) -> Result<PortKillResult, PortsError>;
    pub async fn open_browser(&self, tab: TabId, id: PortId) -> Result<(), PortsError>;
    pub fn shutdown(&self);
}
```

#### Regras de Negócio do Poller (`poller.rs`)
1. **Agrupamento e Identidade de `PortId`:**
   - Para uma aba, cada socket detectado pertence a um dono `(pty_id, pane_id, pane_generation, pid, starttime)`.
   - Agrupar sockets da aba pela chave `(pty_id, pane_id, pane_generation, pid, starttime, protocol, port)`.
   - Conjunto de `SocketIdentity` do grupo é ordenado de forma canônica.
   - Se já existia um `TrackedPort` na aba para a mesma chave `(dono, protocol, port)` **e** o conjunto de `SocketIdentity` é idêntico:
     - O `PortId` é **preservado**.
     - Se `process_name`, `can_open_browser`, `can_kill` ou `shared_socket` mudaram ⇒ incrementar `revision` da aba e emitir `PortUpdated`.
   - Se o conjunto de `SocketIdentity` mudou (ex.: novo inode ou bind adicional/removido):
     - Invalidar o `PortId` antigo (incrementar `revision` e emitir `PortClosed`) e gerar um **novo `PortId` UUID v7** (incrementar `revision` e emitir `PortOpened`). Isso garante que qualquer confirmação pendente na UI seja cancelada se o socket subjacente mudou!
2. **Flags `can_open_browser`, `can_kill` e `shared_socket`:**
   - `can_open_browser`: `true` somente quando `protocol == PortProtocol::Tcp` e pelo menos um `PortAddress` do grupo tiver `is_loopback == true || is_wildcard == true`.
   - `can_kill`: `true` somente quando `proc != root_proc` (nunca matar o shell raiz do painel!), a coleta não está `stale`/`partial` e o sistema suporta sinalização segura (`pidfd`).
   - `shared_socket`: `true` quando qualquer `inode` do grupo também aparece associado a outro `ProcId` na árvore ou quando há múltiplos donos para o mesmo inode.
3. **Cadência Adaptativa (`PollMode`):**
   - `Active` (1 s): `tab.status` é `Running` ou `Continuous`, ou `tree.has_active_foreground(pty)` é `true` em qualquer painel vivo da aba.
   - `Lingering` (5 s): aba não está `Active`, tem pelo menos um PTY vivo e (`descendants.len() > 1` [há filhos além do shell raiz] **ou** `!tab.known.is_empty()`).
   - `IdleProbe` (30 s): aba tem pelo menos um PTY vivo, sem descendentes além do shell raiz e sem portas conhecidas.
   - `Off`: aba não possui nenhum painel com PTY vivo (`panes.is_empty()` ou todas as raízes saíram).
4. **Ordenação Determinística da Lista de Portas:**
   - Ordenar `ports` no `PortSnapshot` por `(port, protocol, pane_id, pid)`.

### 3.7. Killer Anti-TOCTOU (`src-tauri/crates/mt-ports/src/killer.rs`)

```rust
use mt_core::PtyId;
use crate::error::PortsError;
use crate::tree::ProcId;
use crate::types::{PortId, PortKillResult};
use crate::PortsService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalKind {
    Term,
    Kill,
}

/// Representa um handle de processo aberto (no adaptador Linux real, contém `OwnedFd` do `pidfd`).
pub struct ProcessHandle {
    pub pty_id: PtyId,
    pub root: ProcId,
    pub owner: ProcId,
    pub uid: u32,
    pub netns_inode: u64,
    inner: Box<dyn std::any::Any + Send + Sync>,
}

impl ProcessHandle {
    pub fn new<T: Send + Sync + 'static>(
        pty_id: PtyId,
        root: ProcId,
        owner: ProcId,
        uid: u32,
        netns_inode: u64,
        inner: T,
    ) -> Self {
        Self { pty_id, root, owner, uid, netns_inode, inner: Box::new(inner) }
    }

    pub fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.inner.downcast_ref::<T>()
    }
}

pub trait ExactProcessSignaler: Send + Sync + 'static {
    fn is_supported(&self) -> bool { true }
    /// Abre `pidfd`, revalida `starttime`, UID, `netns_inode`, posse no PTY e presença de pelo menos um `inode` esperado em `/proc/<pid>/fd`.
    fn acquire(
        &self,
        pty: PtyId,
        root: ProcId,
        owner: ProcId,
        expected_uid: u32,
        expected_netns: u64,
        expected_inodes: &[u64],
    ) -> Result<ProcessHandle, PortsError>;

    /// Revalida que o processo ainda pertence ao PTY e ainda segura pelo menos um dos `expected_inodes` antes de disparar `pidfd_send_signal`.
    /// Retorna `Ok(false)` se o processo continua vivo mas já liberou todos os `expected_inodes` (`SocketReleased`).
    fn revalidate_and_signal(
        &self,
        handle: &ProcessHandle,
        expected_inodes: &[u64],
        signal: SignalKind,
    ) -> Result<bool, PortsError>;

    /// Verifica se o processo referenciado pelo `pidfd` já encerrou (ou entrou em estado `Zombie`/`Dead`).
    fn has_exited(&self, handle: &ProcessHandle) -> Result<bool, PortsError>;
}

pub async fn kill_port_owner(
    svc: &PortsService,
    tab: mt_core::TabId,
    id: PortId,
) -> Result<PortKillResult, PortsError>;
```

#### Fluxo Exato de `kill_port_owner`
1. Localizar `TrackedPort` na aba `tab` pelo `PortId`. Se a aba estiver com snapshot `stale` ⇒ `Err(PortsError::SnapshotStale)`. Se `PortId` não existir ⇒ `Err(PortsError::NotTracked)`.
2. Se `tracked.owner.proc == tracked.root` ou `tracked.owner.proc.pid <= 1` ⇒ `Err(PortsError::KillRootRefused)`.
3. Verificar `tree.owns(tracked.pty_id, tracked.owner.proc)`: se `false` ⇒ `Err(PortsError::NotTracked)`.
4. Chamar `signaler.acquire(...)`. Se o processo morreu antes ou durante `acquire` ⇒ após `scan_now`, retornar `Ok(PortKillResult { outcome: KillOutcome::AlreadyGone, remaining })` (ou `Err(PortsError::OwnerGone)` se `starttime` no PID divergiu para outro processo!).
5. Enviar `SignalKind::Term` via `signaler.revalidate_and_signal(&handle, &inodes, SignalKind::Term)`:
   - Se retornar `Ok(false)` (socket já liberado) ⇒ rodar `scan_now` e retornar `KillOutcome::SocketReleased`.
   - Se retornar `Err(PortsError::OwnerGone)` (morreu exatamente antes do sinal) ⇒ rodar `scan_now` e retornar `KillOutcome::AlreadyGone`.
6. Fazer poll a cada 50 ms por até 3 s (`Duration::from_secs(3)`) verificando `signaler.has_exited(&handle)`:
   - Se `has_exited` retornar `true` ⇒ rodar `scan_now` e retornar `KillOutcome::Terminated`.
7. Após 3 s, se ainda vivo: chamar `signaler.revalidate_and_signal(&handle, &inodes, SignalKind::Kill)`:
   - Se retornar `Ok(false)` (o processo fechou o socket durante os 3 s de graceful shutdown mas continuou vivo) ⇒ **não** enviar `SIGKILL`, rodar `scan_now` e retornar `KillOutcome::SocketReleased`.
8. Se `SIGKILL` foi enviado, aguardar até 1 s (poll a cada 50 ms) por `signaler.has_exited(&handle)`:
   - Se saiu ⇒ rodar `scan_now` e retornar `KillOutcome::Killed`.
   - Se não confirmou saída após 1 s ⇒ retornar `Err(PortsError::KillTimeout)`.

### 3.8. Opener de URL Local (`src-tauri/crates/mt-ports/src/opener.rs`)

```rust
use mt_core::TabId;
use crate::error::PortsError;
use crate::types::{IpFamily, PortAddress, PortId, PortProtocol};
use crate::PortsService;

pub trait PortUrlOpener: Send + Sync + 'static {
    fn open(&self, url: &str) -> Result<(), PortsError>;
}

/// Seleciona a URL HTTP local compatível com os endereços de bind da porta.
pub fn resolve_browser_url(
    protocol: PortProtocol,
    port: u16,
    addresses: &[PortAddress],
) -> Result<String, PortsError>;

pub async fn open_in_browser(
    svc: &PortsService,
    tab: TabId,
    id: PortId,
) -> Result<(), PortsError>;
```

#### Regras de `resolve_browser_url`
1. Se `protocol != PortProtocol::Tcp` ⇒ `Err(PortsError::NotBrowserCompatible)`.
2. Verificar os endereços em `addresses`:
   - Tem loopback/wildcard IPv4 (`127.0.0.1` ou `0.0.0.0`) **e** loopback/wildcard IPv6 (`::1` ou `::`) ⇒ retornar `format!("http://localhost:{port}")`.
   - Tem `127.0.0.1` ou `0.0.0.0` (ou `::ffff:127.0.0.1` / `::ffff:0.0.0.0`) ⇒ retornar `format!("http://127.0.0.1:{port}")`.
   - Tem outro loopback IPv4 (ex.: `127.0.0.2`) ⇒ retornar `format!("http://{ip}:{port}")`.
   - Tem `::1` ou `::` ⇒ retornar `format!("http://[::1]:{port}")`.
   - Caso contrário (apenas IP de LAN como `192.168.1.10`) ⇒ `Err(PortsError::NotBrowserCompatible)` (jamais abrir `localhost` quando o processo só escuta na interface de LAN!).
3. Em `open_in_browser(svc, tab, id)`:
   - Verificar que o snapshot da aba não está `stale` (`PortsError::SnapshotStale`), que `id` pertence à aba (`PortsError::NotTracked`) e que `tree.owns(pty_id, owner)` e `read_starttime(pid) == Some(starttime)` continuam válidos (`PortsError::OwnerGone`).
   - Construir a URL via `resolve_browser_url` e chamar `opener.open(&url)`.
