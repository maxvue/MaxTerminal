# Especificação Técnica — Implementação 01: Fundação do Crate `mt-pty`, Contratos, Spawn e Contenção Cgroup v2

## 1. Contexto, Objetivo e Escopo

Esta é a primeira unidade de implementação da **Fase 02 — Core PTY e Supervisão de Processos**. Seu objetivo é construir a fundação do crate `mt-pty` (`src-tauri/crates/mt-pty/`), independente do Tauri, estabelecendo:
1. Todos os tipos de domínio, DTOs serializáveis via `serde` + `specta`, configuração `PtyConfig`, catálogo completo de erros `PtyError` com conversão para `mt_core::MtError` e traits de extensão (`OutputSink`, `PtyEventListener`, `OutputTap`, `EnvPolicy`).
2. O subsistema de validação e resolução de spawn (`spawn/shell.rs`, `spawn/env.rs`, `spawn/mod.rs`), incluindo hierarquia de resolução do shell, validação estrita de `cwd`, política de ambiente herdado e injeção das variáveis obrigatórias do MaxTerminal.
3. O subsistema de contenção Linux (`containment/cgroup.rs`, `containment/launcher.rs`, `containment/watchdog.rs`, `containment/mod.rs`) e o binário auxiliar `src/bin/mt-pty-helper.rs` operando nos modos `launcher` (bloqueado em socket Unix privado antes do `execve` até confirmação de inscrição no cgroup v2 e no watchdog) e `watchdog` (processo externo independente que monitora EOF do canal privado do app e aciona `cgroup.kill` em todos os cgroups ativos).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Modificados
- `src-tauri/Cargo.toml` — inclusão de `crates/mt-pty` em `[workspace.members]` e declaração das dependências compartilhadas em `[workspace.dependencies]`.

### 2.2. Arquivos Criados
- `src-tauri/crates/mt-pty/Cargo.toml` — manifesto do crate `mt-pty` e do binário `mt-pty-helper`.
- `src-tauri/crates/mt-pty/src/lib.rs` — declarações de módulos e reexports públicos.
- `src-tauri/crates/mt-pty/src/config.rs` — `PtyConfig` e valores padrão canônicos.
- `src-tauri/crates/mt-pty/src/error.rs` — `PtyError`, códigos `pty.*` e `From<PtyError> for mt_core::MtError`.
- `src-tauri/crates/mt-pty/src/types.rs` — tipos de domínio, `U64Decimal`, `AttachmentId`, `PtySize`, `SpawnSpec`, `PtyStatus`, `ExitInfo`, `ProcessIdentity`, `ProcessInfo`, `PtyProcessReport`, `OutputCursor`, `AttachInfo`.
- `src-tauri/crates/mt-pty/src/sink.rs` — trait `OutputSink`, `SinkClosed`, `NullSink` e `VecSink`.
- `src-tauri/crates/mt-pty/src/events.rs` — `PtyEvent`, DTOs de eventos (`PtySpawnedEvent`, `PtyExitEvent`, `PtyForegroundChangedEvent`, `PtyCleanupFailedEvent`), trait `PtyEventListener` e fila de assinantes com fan-out limitado.
- `src-tauri/crates/mt-pty/src/tap.rs` — trait `OutputTap`, `TapId`, `TapRegistry` e `TapHandle` (RAII de desregistro seguro).
- `src-tauri/crates/mt-pty/src/spawn/mod.rs` — orquestração de preparação e validação do `PreparedSpawn`.
- `src-tauri/crates/mt-pty/src/spawn/shell.rs` — resolução de shell (`spec.shell` → `$SHELL` → `getpwuid` → `/bin/bash` → `/bin/sh`) e flags de login.
- `src-tauri/crates/mt-pty/src/spawn/env.rs` — trait `EnvPolicy`, `DefaultEnvPolicy`, validação de chaves/valores e injeção de variáveis do MaxTerminal.
- `src-tauri/crates/mt-pty/src/containment/mod.rs` — traits `ContainmentProvider`, `ContainmentHandle` e protocolo IPC local entre pai, launcher e watchdog.
- `src-tauri/crates/mt-pty/src/containment/cgroup.rs` — gerenciamento de subtree cgroup v2 delegado, criação de cgroup por PTY, inscrição em `cgroup.procs`, escrita em `cgroup.kill` e leitura de `cgroup.events` (`populated`).
- `src-tauri/crates/mt-pty/src/containment/launcher.rs` — coordenação do socket Unix privado (`0600` em diretório `0700`), validação de credenciais (`SO_PEERCRED`) + `(boot_id, pid, start_time)` e envio do payload de `exec`.
- `src-tauri/crates/mt-pty/src/containment/watchdog.rs` — cliente e servidor do processo `mt-pty-helper --mode watchdog` com canal `CLOEXEC` exclusivo.
- `src-tauri/crates/mt-pty/src/bin/mt-pty-helper.rs` — executável auxiliar nativo sem dependência do Tauri para os modos `launcher` e `watchdog`.

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. Manifesto `src-tauri/crates/mt-pty/Cargo.toml`

```toml
[package]
name = "mt-pty"
version = "0.1.0"
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[lints]
workspace = true

[[bin]]
name = "mt-pty-helper"
path = "src/bin/mt-pty-helper.rs"

[dependencies]
mt-core = { path = "../mt-core" }
portable-pty = "0.9"
rustix = { version = "0.38", features = ["process", "fs", "termios", "event", "net", "time"] }
nix = { version = "0.29", features = ["user", "process", "signal", "term", "poll", "fs", "socket"] }
procfs = "0.17"
crossbeam-channel = "0.5"
parking_lot = "0.12"
arc-swap = "1.7"
bytes = "1.7"
thiserror = "2.0"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
specta = { version = "=2.0.0-rc.27", features = ["derive", "serde"] }
tracing = "0.1"
signal-hook = "0.3"
uuid = { version = "1.11", features = ["v7", "serde"] }

[dev-dependencies]
tempfile = "3.13"
criterion = "0.5"
proptest = "1.5"
assert_matches = "1.5"
sha2 = "0.10"
```

### 3.2. Configuração (`src/config.rs`)

```rust
use std::path::PathBuf;
use std::time::Duration;

/// Configuração operacional do gerenciador de PTYs e supervisão de processos.
#[derive(Debug, Clone)]
pub struct PtyConfig {
    pub max_ptys: usize,
    pub read_buffer_size: usize,
    pub coalesce_window: Duration,
    pub max_frame_bytes: usize,
    pub high_watermark: usize,
    pub low_watermark: usize,
    pub ack_timeout: Duration,
    pub replay_capacity: usize,
    pub write_queue_capacity: usize,
    pub write_queue_bytes: usize,
    pub max_write_bytes: usize,
    pub global_buffer_budget: usize,
    pub write_chunk_bytes: usize,
    pub hup_grace: Duration,
    pub term_grace: Duration,
    pub shutdown_hup_grace: Duration,
    pub shutdown_term_grace: Duration,
    pub supervisor_interval: Duration,
    pub tombstone_ttl: Duration,
    pub thread_stack_size: usize,
    pub registry_dir: PathBuf,
    pub cgroup_root_override: Option<PathBuf>,
    pub helper_bin_override: Option<PathBuf>,
    pub orphan_sweep_on_startup: bool,
}

impl Default for PtyConfig {
    fn default() -> Self {
        let runtime_base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/run/user/1000"));
        let instance_id = uuid::Uuid::now_v7().to_string();
        Self {
            max_ptys: 256,
            read_buffer_size: 64 * 1024,
            coalesce_window: Duration::from_millis(4),
            max_frame_bytes: 256 * 1024,
            high_watermark: 1024 * 1024,
            low_watermark: 256 * 1024,
            ack_timeout: Duration::from_secs(5),
            replay_capacity: 1024 * 1024,
            write_queue_capacity: 1024,
            write_queue_bytes: 4 * 1024 * 1024,
            max_write_bytes: 1024 * 1024,
            global_buffer_budget: 256 * 1024 * 1024,
            write_chunk_bytes: 4 * 1024,
            hup_grace: Duration::from_secs(1),
            term_grace: Duration::from_secs(2),
            shutdown_hup_grace: Duration::from_millis(300),
            shutdown_term_grace: Duration::from_millis(700),
            supervisor_interval: Duration::from_secs(1),
            tombstone_ttl: Duration::from_secs(60),
            thread_stack_size: 256 * 1024,
            registry_dir: runtime_base.join("maxterminal").join(instance_id),
            cgroup_root_override: None,
            helper_bin_override: None,
            orphan_sweep_on_startup: true,
        }
    }
}
```

### 3.3. Erros (`src/error.rs`)

```rust
use mt_core::{MtError, PtyId};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum PtyError {
    #[error("PTY não encontrado: {0}")]
    NotFound(PtyId),
    #[error("Limite de {0} terminais simultâneos atingido")]
    LimitReached(usize),
    #[error("Diretório inválido: {0}")]
    CwdInvalid(String),
    #[error("Shell não encontrado: {0}")]
    ShellNotFound(String),
    #[error("Variável de ambiente inválida: {0}")]
    EnvInvalid(String),
    #[error("Dimensões inválidas: {rows}x{cols}")]
    ResizeInvalid { rows: u16, cols: u16 },
    #[error("Falha ao iniciar o terminal: {0}")]
    SpawnFailed(String),
    #[error("O processo do terminal já foi finalizado")]
    AlreadyExited,
    #[error("Fila de escrita cheia; o processo não está lendo a entrada")]
    WriteQueueFull,
    #[error("Falha de escrita no terminal: {0}")]
    WriteFailed(String),
    #[error("Entrada inválida para escrita no terminal: {0}")]
    InputInvalid(String),
    #[error("Processo {0} não pertence a este terminal")]
    PidNotOwned(u32),
    #[error("Informações de processo indisponíveis: {0}")]
    ProcUnavailable(String),
    #[error("Falha ao encerrar processos: {0}")]
    KillFailed(String),
    #[error("Contenção de processos Linux (cgroup v2 / pidfd) indisponível: {0}")]
    ContainmentUnavailable(String),
    #[error("Falha na contenção de processos do terminal: {0}")]
    ContainmentFailed(String),
    #[error("Falha na limpeza completa dos processos do terminal: {0}")]
    CleanupFailed(String),
    #[error("Conexão de leitura do terminal expirada ou substituída")]
    AttachmentStale,
    #[error("Confirmação de leitura (ACK) inválida: {0}")]
    AckInvalid(String),
    #[error("Cursor de leitura inválido: {0}")]
    CursorInvalid(String),
    #[error("O gerenciador de terminais está em processo de encerramento")]
    ShuttingDown,
    #[error("Diretório de runtime inválido ou inseguro: {0}")]
    RuntimeDirInvalid(String),
    #[error("Erro de E/S: {0}")]
    Io(#[from] std::io::Error),
}

impl PtyError {
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
        let code = err.code();
        let message = err.to_string();
        let details = match &err {
            PtyError::NotFound(id) => Some(json!({ "ptyId": id })),
            PtyError::LimitReached(limit) => Some(json!({ "limit": limit })),
            PtyError::ResizeInvalid { rows, cols } => Some(json!({ "rows": rows, "cols": cols })),
            PtyError::PidNotOwned(pid) => Some(json!({ "pid": pid })),
            _ => None,
        };
        match details {
            Some(d) => MtError::with_details(code, message, d),
            None => MtError::new(code, message),
        }
    }
}

pub type PtyResult<T> = Result<T, PtyError>;
```

### 3.4. Tipos de Domínio (`src/types.rs`)

Todos os contadores, cursores, timestamps e `start_time` de 64 bits expostos em JSON utilizam o newtype `U64Decimal` (ou atributo `serde` equivalente com `#[specta(type = String)]`), garantindo que o TypeScript receba `string` decimal e converta para `bigint` sem perda de precisão IEEE-754.

```rust
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use mt_core::{PaneId, PtyId, TabId};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Representação de inteiro de 64 bits sem sinal serializado como string decimal para IPC seguro com JS/TS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, specta::Type)]
#[specta(transparent)]
pub struct U64Decimal(#[specta(type = String)] pub u64);

impl From<u64> for U64Decimal {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<U64Decimal> for u64 {
    fn from(value: U64Decimal) -> Self {
        value.0
    }
}

impl fmt::Display for U64Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for U64Decimal {
    type Err = std::num::ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>().map(Self)
    }
}

impl Serialize for U64Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for U64Decimal {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<u64>().map(Self).map_err(serde::de::Error::custom)
    }
}

/// Identificador opaco de uma geração de conexão (attach) de output do PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(transparent)]
pub struct AttachmentId(#[specta(type = String)] pub uuid::Uuid);

impl AttachmentId {
    pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
    }
}

impl Default for AttachmentId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for AttachmentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySize {
    pub rows: u16,
    pub cols: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtySize {
    pub fn validate(&self) -> Result<(), crate::error::PtyError> {
        if self.rows == 0 || self.rows > 1000 || self.cols == 0 || self.cols > 1000 {
            return Err(crate::error::PtyError::ResizeInvalid {
                rows: self.rows,
                cols: self.cols,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyOwner {
    pub tab_id: Option<TabId>,
    pub pane_id: Option<PaneId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SpawnSpec {
    pub shell: Option<String>,
    pub args: Vec<String>,
    pub login: bool,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub env_remove: Vec<String>,
    pub size: PtySize,
    pub owner: PtyOwner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ExitReason {
    Exited,
    Signaled,
    KilledByUser,
    Shutdown,
    ReaderFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExitInfo {
    pub code: Option<i32>,
    pub signal: Option<i32>,
    pub signal_name: Option<String>,
    pub reason: ExitReason,
    pub at_ms: U64Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PtyStatus {
    Running,
    #[serde(rename_all = "camelCase")]
    Exiting { since_ms: U64Decimal },
    #[serde(rename_all = "camelCase")]
    CleanupFailed { error: String },
    #[serde(rename_all = "camelCase")]
    Exited { exit: ExitInfo },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum KillMode {
    #[default]
    Graceful,
    Force,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProcessIdentity {
    pub boot_id: String,
    pub pid: u32,
    pub start_time: U64Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyPids {
    pub shell_pid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub foreground_pgid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySummary {
    pub id: PtyId,
    pub owner: PtyOwner,
    pub shell: String,
    pub args: Vec<String>,
    pub cwd_initial: String,
    pub size: PtySize,
    pub status: PtyStatus,
    pub pids: PtyPids,
    pub attached: bool,
    pub created_at_ms: U64Decimal,
    pub bytes_out: U64Decimal,
    pub seq: U64Decimal,
    pub revision: U64Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ProcState {
    Running,
    Sleeping,
    DiskSleep,
    Stopped,
    TracingStop,
    Zombie,
    Dead,
    Idle,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProcessInfo {
    pub pid: u32,
    pub ppid: u32,
    pub pgid: u32,
    pub sid: u32,
    pub tpgid: i32,
    pub comm: String,
    pub cmdline: Vec<String>,
    pub exe: Option<String>,
    pub cwd: Option<String>,
    pub state: ProcState,
    pub start_time: U64Decimal,
    pub rss_bytes: Option<U64Decimal>,
    pub threads: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProcessNode {
    pub info: ProcessInfo,
    pub children: Vec<ProcessNode>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyProcessReport {
    pub pty_id: PtyId,
    pub pids: PtyPids,
    pub shell: Option<ProcessInfo>,
    pub foreground: Option<ProcessInfo>,
    pub tree: Option<ProcessNode>,
    pub owned_processes: Vec<ProcessInfo>,
    pub partial: bool,
    pub descendant_count: u32,
    pub sampled_at_ms: U64Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OutputCursor {
    pub next_seq: U64Decimal,
    pub byte_offset: U64Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttachInfo {
    pub attachment_id: AttachmentId,
    pub replay_from: OutputCursor,
    pub live_from: OutputCursor,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShutdownReport {
    pub completed: Vec<PtyId>,
    pub pending: Vec<PtyId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedProcess {
    pub pty_id: PtyId,
    pub identity: ProcessIdentity,
    pub pgid: u32,
    pub sid: u32,
}

/// Entrada tipada para escrita no PTY via IPC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PtyInput {
    #[serde(rename_all = "camelCase")]
    Text { text: String },
    #[serde(rename_all = "camelCase")]
    Bytes { bytes: Vec<u8> },
}
```

### 3.5. Traits de Extensão (`src/sink.rs`, `src/events.rs`, `src/tap.rs`)

- **`src/sink.rs`**:
```rust
use bytes::Bytes;
use parking_lot::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

pub trait OutputSink: Send + Sync + 'static {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed>;
}

#[derive(Debug, Default)]
pub struct NullSink;

impl OutputSink for NullSink {
    fn send(&self, _frame: Bytes) -> Result<(), SinkClosed> {
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct VecSink {
    frames: Mutex<Vec<Bytes>>,
}

impl VecSink {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn take_frames(&self) -> Vec<Bytes> {
        std::mem::take(&mut *self.frames.lock())
    }
    pub fn frames_snapshot(&self) -> Vec<Bytes> {
        self.frames.lock().clone()
    }
}

impl OutputSink for VecSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        self.frames.lock().push(frame);
        Ok(())
    }
}
```

- **`src/events.rs`**:
```rust
use mt_core::PtyId;
use serde::{Deserialize, Serialize};
use crate::types::{ExitInfo, ProcessIdentity, ProcessInfo, PtyOwner, PtySummary, U64Decimal};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtySpawnedEvent {
    pub summary: PtySummary,
    pub revision: U64Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyForegroundChangedEvent {
    pub pty_id: PtyId,
    pub pgid: Option<u32>,
    pub process: Option<ProcessInfo>,
    pub revision: U64Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyExitEvent {
    pub pty_id: PtyId,
    pub owner: PtyOwner,
    pub exit: ExitInfo,
    pub final_seq: U64Decimal,
    pub revision: U64Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PtyCleanupFailedEvent {
    pub pty_id: PtyId,
    pub survivors: Vec<ProcessIdentity>,
    pub revision: U64Decimal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum PtyEvent {
    #[serde(rename_all = "camelCase")]
    Spawned { summary: PtySummary, revision: U64Decimal },
    #[serde(rename_all = "camelCase")]
    ForegroundChanged {
        pty_id: PtyId,
        pgid: Option<u32>,
        process: Option<ProcessInfo>,
        revision: U64Decimal,
    },
    #[serde(rename_all = "camelCase")]
    Exited {
        pty_id: PtyId,
        owner: PtyOwner,
        exit: ExitInfo,
        final_seq: U64Decimal,
        revision: U64Decimal,
    },
    #[serde(rename_all = "camelCase")]
    CleanupFailed {
        pty_id: PtyId,
        survivors: Vec<ProcessIdentity>,
        revision: U64Decimal,
    },
}

pub trait PtyEventListener: Send + Sync + 'static {
    fn on_event(&self, event: &PtyEvent);
}
```

- **`src/tap.rs`**:
  - `OutputTap`: `fn on_output(&self, pty_id: PtyId, seq: u64, bytes: &[u8]);` e `fn on_exit(&self, _pty_id: PtyId, _exit: &ExitInfo) {}`.
  - `TapRegistry`: mantém lista de taps com flag `active: AtomicBool` e contador `in_flight: AtomicUsize` + `Condvar`.
  - Quando a thread de leitura invoca `dispatch_output`, ela clona o `Vec<Arc<TapEntry>>` fora do lock, incrementa `in_flight` em cada entry ativa, executa o callback e decrementa `in_flight` notificando o `Condvar`.
  - `TapHandle::drop` (ou método explícito) marca `active = false`, remove do vetor do `TapRegistry` e, se a thread atual **não** for a thread de leitura em pleno callback daquele tap (verificado via `thread_local!` guard), aguarda `in_flight == 0`. Isso garante o **CA-33**: remover o handle impede qualquer callback posterior à conclusão da remoção e evita deadlock se removido de dentro do próprio callback.

### 3.6. Resolução de Shell e Ambiente (`src/spawn/`)

- **`src/spawn/shell.rs`**:
  - `pub fn resolve_shell(requested: Option<&str>, login: bool, explicit_args: &[String]) -> PtyResult<ResolvedShell>`
  - Se `requested` for `Some(s)`:
    - Se vazio ou contiver `\0`: retorna `PtyError::ShellNotFound`.
    - Se contiver `/`: valida que o caminho existe, é arquivo regular (ou symlink para arquivo regular) e possui bit de execução (`mode & 0o111 != 0`); caso contrário, retorna `PtyError::ShellNotFound` imediatamente (**sem fallback**).
    - Se não contiver `/`: busca no `PATH` do processo; se não encontrar executável regular, retorna `PtyError::ShellNotFound` (**sem fallback**).
  - Se `requested` for `None`:
    - Tenta na ordem: `$SHELL` → `nix::unistd::User::from_uid(nix::unistd::getuid())` → `/bin/bash` → `/bin/sh`. O primeiro executável regular válido vence; se nenhum existir, retorna `PtyError::ShellNotFound`.
  - Argumentos: se `explicit_args` não for vazio, preserva-o intacto. Se `explicit_args` for vazio e `login == true`:
    - Se o basename do executável for `bash` ou `zsh`, adiciona `vec!["-l".to_string()]`.
    - Se o basename for `fish`, adiciona `vec!["--login".to_string()]`.
    - Para `/bin/sh` ou outros binários desconhecidos, mantém `vec![]` (não presume que `/bin/sh` aceite `-l`).

- **`src/spawn/env.rs`**:
  - `pub trait EnvPolicy: Send + Sync + 'static { fn apply(&self, inherited: &mut BTreeMap<OsString, OsString>); }`
  - `DefaultEnvPolicy`: remove `MAXTERMINAL_PTY_ID`, `VTE_VERSION`, `TERMINAL_EMULATOR` e quaisquer chaves iniciadas por `WEBKIT_` ou `GIO_LAUNCHED_DESKTOP_FILE`.
  - `validate_env_key(key: &str) -> PtyResult<()>`: valida regex `^[A-Za-z_][A-Za-z0-9_]*$` (sem regex pesada: primeiro char ASCII alfabético ou `_`, demais ASCII alfanuméricos ou `_`, não vazia, sem `\0`).
  - `validate_env_value(key: &str, value: &str) -> PtyResult<()>`: sem `\0` e `value.len() <= 128 * 1024`.
  - Ordem de construção do mapa final (`BTreeMap<String, String>`):
    1. Ambiente herdado (`std::env::vars_os()`) convertido.
    2. `env_policy.apply(&mut inherited)`.
    3. Remoção das chaves listadas em `spec.env_remove` (validando sintaxe das chaves).
    4. Aplicação de `spec.env` (validando chaves e valores).
    5. Se `LANG` não estiver presente, define `LANG=C.UTF-8`.
    6. Variáveis forçadas pelo MaxTerminal (sobrescrevem qualquer valor):
       - `TERM=xterm-256color`
       - `COLORTERM=truecolor`
       - `TERM_PROGRAM=MaxTerminal`
       - `TERM_PROGRAM_VERSION=env!("CARGO_PKG_VERSION")`
       - `MAXTERMINAL=1`
       - `MAXTERMINAL_PTY_ID=<pty_id.to_string()>`

- **`src/spawn/mod.rs`**:
  - `resolve_cwd(requested: Option<&str>) -> PtyResult<PathBuf>`:
    - Se `None`, usa `$HOME` (ou `User::from_uid(getuid()).dir`).
    - Se iniciar com `~` (`~` ou `~/...`), expande para o diretório home do usuário.
    - Executa `std::fs::canonicalize(&path)`, verifica `metadata.is_dir()` e permissão de leitura/travessia (`rustix::fs::access(&canon, Access::READ_OK | Access::EXEC_OK)`). Em falha, retorna `PtyError::CwdInvalid`.

### 3.7. Contenção Cgroup v2, Launcher e Watchdog (`src/containment/` e `src/bin/mt-pty-helper.rs`)

- **Descoberta e Validação do Subtree Cgroup v2 (`src/containment/cgroup.rs`)**:
  - Localiza o ponto de montagem cgroup2 (`/sys/fs/cgroup`) e lê `/proc/self/cgroup` (`0::/<caminho>`).
  - Para não manter o processo principal no mesmo leaf cgroup das cargas (regra de *no internal processes* do cgroup v2), provisiona na inicialização:
    - Move o próprio processo app e o watchdog para um subgrupo `<subtree>/app` (ou usa `cgroup_root_override` em testes) e habilita controladores necessários na raiz delegada, criando os cgroups de PTY sob `<subtree>/ptys/pty-<id>`.
  - Verifica se a criação de subdiretório sob `<subtree>/ptys/`, a abertura de `cgroup.procs`, `cgroup.kill` e `cgroup.events`, bem como `rustix::process::pidfd_open(getpid(), ...)` funcionam. Se qualquer um falhar, retorna `PtyError::ContainmentUnavailable` **antes** de executar qualquer comando do usuário (CA-32).
  - `CgroupHandle` mantém `dir_fd: OwnedFd`, `procs_fd: OwnedFd`, `kill_fd: OwnedFd`, `events_fd: OwnedFd`, `cgroup_path: PathBuf` e `cgroup_id: u64` (inode do diretório cgroup via `fstat`). Isso impede ataques ou corridas de resolução de caminho no momento do kill.

- **Protocolo do Helper (`src/bin/mt-pty-helper.rs`)**:
  - Binário compilado no mesmo workspace, resolvido por caminho adjacente ao executável atual (`current_exe().parent().join("mt-pty-helper")`) ou `CARGO_BIN_EXE_mt-pty-helper` em testes, ou `config.helper_bin_override`.
  - **Modo 1: `mt-pty-helper --mode launcher --socket <path>`**:
    1. Iniciado como filho do `slave.spawn_command` do `portable-pty` (portanto já possui a sessão TTY controladora `pid == pgid == sid` e FDs 0, 1, 2 ligados ao slave PTY).
    2. Conecta ao socket Unix stream em `<path>` (com `SOCK_CLOEXEC`).
    3. Aguarda mensagem binária/JSON enquadrada (`u32 LE len + payload`) `LauncherReleaseMsg`:
       ```rust
       pub struct LauncherReleaseMsg {
           pub executable: String,
           pub argv: Vec<String>,
           pub cwd: String,
           pub env: Vec<(String, String)>,
       }
       ```
       com timeout máximo de 5 segundos. Se receber EOF, erro ou timeout antes da mensagem de autorização, sai imediatamente com código `126` sem fazer fork nem executar o shell (CA-32).
    4. Ao receber `LauncherReleaseMsg`, altera `cwd` (`std::env::set_current_dir`), limpa o ambiente e aplica `env`, fecha o socket (ou confia em `CLOEXEC` após fechar explicitamente) e invoca `nix::unistd::execve` substituindo a própria imagem (mesmo PID!) pelo shell solicitado. Se `execve` falhar, envia o erro pelo socket (ou escreve código e sai com `127`).
  - **Modo 2: `mt-pty-helper --mode watchdog --runtime-dir <dir>`**:
    1. Comunica-se com o processo pai via `UnixStream` par (`socketpair` criado com `SOCK_CLOEXEC` e mantido aberto apenas no pai e no watchdog; nunca herdado por filhos pois todos os FDs do pai têm `CLOEXEC`).
    2. Também abre um pipe/eventfd de emergência (`emergency_fd`) cujo lado de escrita fica no pai para o panic hook e signal handler escreverem 1 byte sem precisarem de locks.
    3. Processa comandos do pai:
       - `RegisterCgroup { pty_id, cgroup_path, cgroup_id }` → abre e retém o diretório do cgroup.
       - `UnregisterCgroup { pty_id }` → remove da lista ativa após cleanup confirmado.
       - `Ping` → responde `Pong`.
       - `Shutdown` → encerra o watchdog graciosamente quando não há cgroups ativos.
    4. Se detectar EOF no socket de controle ou byte no `emergency_fd` (indicando `panic!`, `abort` ou `SIGKILL` do app principal), percorre todos os cgroups registrados, valida `cgroup_id` (inode), escreve `"1"` em `cgroup.kill` de cada um (e subcgroups eventuais), aguarda `populated 0` em `cgroup.events`, remove os diretórios de cgroup vazios e atualiza/remove o arquivo `pty-registry.json` antes de encerrar.

---

## 4. Regras de Negócio, Segurança e Casos de Borda

1. **`unsafe_code = forbid`**: Todas as operações de baixo nível (`pidfd_open`, `SO_PEERCRED`, `stat`, `execve`, `chmod 0700/0600`, `socketpair`) devem usar exclusivamente APIs seguras de `rustix` e `nix`.
2. **Prevenção de vazamento de FDs**: Todos os sockets e arquivos do `CgroupHandle`, `WatchdogClient` e `LauncherSession` são abertos com `O_CLOEXEC` / `SOCK_CLOEXEC`. O helper launcher não herda nenhum FD do watchdog nem de outros PTYs.
3. **Segurança de logs e erros**: `SpawnSpec.env` e `LauncherReleaseMsg` nunca são formatados em `tracing::info!/debug!/error!` nem incluídos em `MtError.details`.
4. **Proibição de Fallback Silencioso**: Se `spec.shell` for informado e não existir, retorna `pty.shell_not_found`. Se o ambiente não tiver cgroup v2 gravável com `cgroup.kill`, retorna `pty.containment_unavailable`.

---

## 5. Critérios de Aceite Mapeados

- **CA-01 / CA-02 (parcial)**: `mt-pty` compila com `cargo check` e `cargo clippy -- -D warnings` sem avisos.
- **CA-05**: `cwd` inexistente → `pty.cwd_invalid`; shell inexistente → `pty.shell_not_found`; chave de env inválida (`"A=B"`, vazia ou com `\0`) → `pty.env_invalid`.
- **CA-06**: Variáveis `TERM=xterm-256color`, `COLORTERM=truecolor`, `TERM_PROGRAM=MaxTerminal`, `MAXTERMINAL_PTY_ID=<id>` e `cwd` preparadas e entregues ao processo no helper launcher.
- **CA-32**: Sem delegação de cgroup v2 ou pidfd, o preparo falha com `pty.containment_unavailable` antes de executar código do usuário; launcher cancelado ou com timeout no handshake não executa o shell.
- **CA-33 (infra)**: `TapRegistry` e `TapHandle` garantem registro e remoção thread-safe sem callbacks pós-remoção.
