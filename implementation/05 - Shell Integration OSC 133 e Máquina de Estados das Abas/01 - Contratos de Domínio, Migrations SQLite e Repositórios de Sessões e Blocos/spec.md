# Especificação Técnica — Implementação 01: Contratos de Domínio, Lifecycle de Spawn, Migrations SQLite e Repositórios de Sessões e Blocos

> **Fase:** 05 — Shell Integration OSC 133 e Máquina de Estados das Abas
> **Implementação:** `01 - Contratos de Domínio, Migrations SQLite e Repositórios de Sessões e Blocos`
> **Fontes da verdade:** [`../../AGENTS.md`](../../../AGENTS.md), [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) (§§1, 2, 3 e 7).

---

## 1. Contexto, Objetivo e Escopo

Esta primeira implementação estabelece toda a fundação de tipos compartilhados (`mt-core`), o contrato genérico de preparação/início de processo antes das threads leitoras no PTY (`mt-pty`), e a camada de persistência SQLite (`mt-db`) para sessões mínimas (`sessions`) e blocos de comando (`command_blocks` + índice `FTS5` `command_blocks_fts`).

### Escopo Incluído
1. **Tipos de domínio e erros em `mt-core`:**
   - Todos os DTOs públicos e enums de integração de shell, blocos de comando e estados de abas exportados para Rust e TypeScript (`specta::Type`), utilizando strings decimais (`U64String`) para todos os inteiros de 64 bits sem sinal.
   - Códigos de erro canônicos do domínio `shell.*` integrados ao `MtError`.
2. **Lifecycle genérico de Spawn em `mt-pty`:**
   - Adição da trait `SpawnLifecycle` e da struct `ResolvedSpawn` sem acoplamento ao domínio de shell.
   - Adição do método `PtyManager::spawn_with_lifecycle`, garantindo que a preparação de argumentos/ambiente (`prepare`) ocorra antes do spawn do processo filho e o registro do vínculo/tap (`started`) ocorra após conhecer `pid`/`start_time` e **antes** de iniciar as threads `reader`/`writer`/`waiter`.
3. **Migration SQLite e FTS5 em `mt-db`:**
   - Migration `0002_05_shell_sessions_command_blocks.sql` criando as tabelas `STRICT` `sessions` e `command_blocks` (com `rowid INTEGER PRIMARY KEY` estável) e a tabela virtual `command_blocks_fts` (`unicode61 remove_diacritics 2`, `prefix='2 3 4'`) com triggers de sincronização `AFTER INSERT`, `AFTER DELETE` e `AFTER UPDATE`.
   - Especificação `FtsSpec` registrada em `DbSetup.fts` para manutenção (`optimize` e `integrity-check`).
4. **Repositórios tipados e sanitizados em `mt-db`:**
   - `SessionsRepo` (`prepare`, `close`, `abort`, `get`) e `CommandBlocksRepo` (`start`, `finish`, `get`, `list`, `search`, `close_dangling`), exigindo `Option<SanitizedText>` em todas as colunas textuais livres (`command`, `cwd`, `output_text`, `error_tail`).

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho do Arquivo | Responsabilidade |
|------|--------------------|------------------|
| Criar | `src-tauri/crates/mt-core/src/shell.rs` | DTOs compartilhados, enums de estado/observação e tipos de requisição/resposta da Fase 05 |
| Modificar | `src-tauri/crates/mt-core/src/error.rs` | Adição dos códigos de erro `shell.*` e construtores auxiliares de `MtError` |
| Modificar | `src-tauri/crates/mt-core/src/lib.rs` | Re-export do módulo `shell` e seus tipos públicos |
| Criar | `src-tauri/crates/mt-pty/src/spawn/lifecycle.rs` | Definição de `ResolvedSpawn`, `SpawnLifecycle` e `NoopSpawnLifecycle` |
| Modificar | `src-tauri/crates/mt-pty/src/spawn/mod.rs` | Integração dos callbacks `prepare`, `started` e `failed` no fluxo de spawn antes das threads de I/O |
| Modificar | `src-tauri/crates/mt-pty/src/manager.rs` | Adição de `PtyManager::spawn_with_lifecycle` (com `spawn` delegando sem hook) |
| Modificar | `src-tauri/crates/mt-pty/src/lib.rs` | Re-export de `SpawnLifecycle` e `ResolvedSpawn` |
| Criar | `src-tauri/crates/mt-db/migrations/0002_05_shell_sessions_command_blocks.sql` | DDL `STRICT` para `sessions`, `command_blocks`, índices e `command_blocks_fts` + triggers |
| Criar | `src-tauri/crates/mt-db/src/repos/mod.rs` | Export de `SessionsRepo`, `CommandBlocksRepo` e `shell_fts_spec()` |
| Criar | `src-tauri/crates/mt-db/src/repos/sessions.rs` | Repositório de sessões mínimas por painel (`prepare`, `close`, `abort`, `get`) |
| Criar | `src-tauri/crates/mt-db/src/repos/command_blocks.rs` | Repositório de blocos de comando e busca FTS5 (`start`, `finish`, `get`, `list`, `search`, `close_dangling`) |
| Modificar | `src-tauri/crates/mt-db/src/lib.rs` | Export do módulo `repos` |
| Criar | `src-tauri/crates/mt-db/tests/shell_repos_and_fts.rs` | Testes de integração de migrations, constraints `STRICT`, FTS5, cascata e sanitização |

---

## 3. Assinaturas Exatas e Estruturas de Dados (Rust)

### 3.1. Tipos Compartilhados em `src-tauri/crates/mt-core/src/shell.rs`

Todos os tipos possuem `#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]`. Structs utilizam `#[serde(rename_all = "camelCase")]` e enums sem payload utilizam `#[serde(rename_all = "snake_case")]`. Contadores `u64` utilizam o tipo `U64String` (serializado como string decimal no JSON/Specta, conversível de/para `u64` em Rust).

```rust
use serde::{Deserialize, Serialize};
use crate::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId};
use crate::num::U64String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ContinuousPolicy {
    Auto,
    On,
    Off,
}

impl Default for ContinuousPolicy {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IntegrationMode {
    Pending,
    Hooks,
    Proc,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ObservationSource {
    Hook,
    Proc,
    PtyLifecycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ObservationConfidence {
    Correlated,
    Inferred,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CommandTextQuality {
    Complete,
    Partial,
    Unavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PersistenceState {
    Pending,
    Committed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockStatus {
    Running,
    Succeeded,
    Failed,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockEndReason {
    ShellReported,
    ProcDetected,
    MissingEnd,
    ModeSwitched,
    PtyExited,
    CaptureOverflow,
    Shutdown,
    Recovery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    CombinedPty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TabStatus {
    Running,
    Continuous,
    Finished,
    Error,
    Idle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShellIntegrationRequest {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: Option<SessionId>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub continuous: ContinuousPolicy,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeBinding {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub pty_id: PtyId,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRef {
    pub pty_id: PtyId,
    pub block_id: CommandBlockId,
    pub generation: U64String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StreamPosition {
    pub frame_seq: U64String,
    pub byte_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CompletionSnapshot {
    pub execution: ExecutionRef,
    pub outcome: BlockStatus,
    pub exit_code: Option<i32>,
    pub reason: BlockEndReason,
    pub completed_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AttentionSnapshot {
    pub attention_id: String,
    pub execution: Option<ExecutionRef>,
    pub revision: U64String,
    pub seen: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PaneStateDto {
    pub binding: RuntimeBinding,
    pub alive: bool,
    pub mode: IntegrationMode,
    pub status: TabStatus,
    pub seen: bool,
    pub badge_bucket: TabStatus,
    pub execution: Option<ExecutionRef>,
    pub completion: Option<CompletionSnapshot>,
    pub seen_execution: Option<ExecutionRef>,
    pub attention: Option<AttentionSnapshot>,
    pub policy: ContinuousPolicy,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
    pub cwd: Option<String>,
    pub revision: U64String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabStateSnapshot {
    pub tab_id: TabId,
    pub revision: U64String,
    pub status: TabStatus,
    pub seen: bool,
    pub badge_bucket: TabStatus,
    pub representative_pane_id: PaneId,
    pub panes: Vec<PaneStateDto>,
    pub has_active_processes: bool,
    pub last_exit_code: Option<i32>,
    pub last_block_id: Option<CommandBlockId>,
    pub changed_at_ms: i64,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabStateListResponse {
    pub items: Vec<TabStateSnapshot>,
    pub watermark_revision: U64String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SeenRequest {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub execution: ExecutionRef,
    pub observed_revision: U64String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FocusSnapshot {
    pub tab_id: Option<TabId>,
    pub visible_pane_ids: Vec<PaneId>,
    pub window_focused: bool,
    pub document_visible: bool,
    pub focus_revision: U64String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandStarted {
    pub binding: RuntimeBinding,
    pub execution: ExecutionRef,
    pub seq: U64String,
    pub command: Option<String>,
    pub command_quality: CommandTextQuality,
    pub cwd: Option<String>,
    pub started_at_ms: i64,
    pub output_start: StreamPosition,
    pub continuous: bool,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
    pub persistence: PersistenceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandFinished {
    pub binding: RuntimeBinding,
    pub execution: ExecutionRef,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    pub ended_at_ms: i64,
    pub duration_ms: U64String,
    pub output_end: Option<StreamPosition>,
    pub output_bytes: U64String,
    pub output_truncated: bool,
    pub dropped_bytes: U64String,
    pub end_reason: BlockEndReason,
    pub persistence: PersistenceState,
    pub has_error_context: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandBlockDto {
    pub id: CommandBlockId,
    pub binding: RuntimeBinding,
    pub execution: ExecutionRef,
    pub seq: U64String,
    pub shell: String,
    pub mode: IntegrationMode,
    pub command: Option<String>,
    pub command_quality: CommandTextQuality,
    pub cwd: Option<String>,
    pub status: BlockStatus,
    pub exit_code: Option<i32>,
    pub end_reason: Option<BlockEndReason>,
    pub continuous: bool,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
    pub persistence: PersistenceState,
    pub started_at_ms: i64,
    pub ended_at_ms: Option<i64>,
    pub duration_ms: Option<U64String>,
    pub output_start: StreamPosition,
    pub output_end: Option<StreamPosition>,
    pub output_bytes: U64String,
    pub output_truncated: bool,
    pub dropped_bytes: U64String,
    pub has_error_context: bool,
    pub output_text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_before_seq: Option<U64String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandBlockHit {
    pub block: CommandBlockDto,
    pub snippet: String,
    pub rank: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ErrorContext {
    pub execution: ExecutionRef,
    pub session_id: SessionId,
    pub command: Option<String>,
    pub command_quality: CommandTextQuality,
    pub cwd: Option<String>,
    pub exit_code: i32,
    pub tail: String,
    pub lines: u32,
    pub truncated: bool,
    pub output_kind: OutputKind,
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
    pub persistence: PersistenceState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IntegrationInfo {
    pub pty_id: PtyId,
    pub shell: String,
    pub mode: IntegrationMode,
    pub reason: Option<String>,
    pub scripts_version: String,
    pub last_correlated_mark_at: Option<i64>,
    pub command_text_quality: CommandTextQuality,
    pub prompt_markers_available: bool,
    pub persistence: PersistenceState,
}
```

### 3.2. Códigos de Erro em `src-tauri/crates/mt-core/src/error.rs`

Adicionar constantes e construtores de `MtError` com mensagens estritamente em `pt-BR` e sem vazar comandos crus, SQL ou nonces:
- `shell.tab_not_found` — `"Aba não encontrada no registro de estados."`
- `shell.pane_not_found` — `"Painel não encontrado no registro de estados."`
- `shell.pty_not_found` — `"Instância de terminal (PTY) não encontrada na integração de shell."`
- `shell.binding_conflict` — `"Conflito de vínculo: o painel já possui um terminal ativo registrado."`
- `shell.session_mismatch` — `"A sessão informada não pertence a esta aba/painel ou já está encerrada."`
- `shell.invalid_argument` — `"Parâmetro inválido para a operação de histórico de comandos."`
- `shell.block_not_found` — `"Bloco de comando não encontrado."`
- `shell.block_running` — `"O comando ainda está em execução; o contexto de erro só está disponível após a conclusão."`
- `shell.block_not_failed` — `"O bloco de comando não terminou com erro."`
- `shell.context_unavailable` — `"O contexto de saída deste comando não está mais disponível no histórico ou cache."`

### 3.3. Lifecycle Genérico de Spawn em `src-tauri/crates/mt-pty/src/spawn/lifecycle.rs`

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use mt_core::{PtyId};
use crate::error::PtyResult;
use crate::types::{PtyOwner, PtySize, PtySummary};

#[derive(Debug, Clone)]
pub struct ResolvedSpawn {
    pub pty_id: PtyId,
    pub owner: PtyOwner,
    pub program: PathBuf,
    pub args: Vec<String>,
    pub login: bool,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub size: PtySize,
}

pub trait SpawnLifecycle: Send + Sync {
    /// Invocado após a resolução inicial de shell/cwd/env pela Fase 02 e ANTES de criar o processo filho.
    /// Permite injetar flags de bootstrap (--rcfile, --init-command, ZDOTDIR) e variáveis de ambiente.
    fn prepare(&self, spawn: &mut ResolvedSpawn) -> PtyResult<()>;

    /// Invocado após o processo filho nascer e ter pid/start_time validados, mas ANTES de iniciar
    /// as threads reader/writer/waiter e antes de liberar o exec do launcher.
    fn started(&self, summary: &PtySummary) -> PtyResult<()>;

    /// Invocado se qualquer etapa após `prepare` falhar, garantindo limpeza de estado associado.
    fn failed(&self, pty_id: PtyId);
}

pub struct NoopSpawnLifecycle;
impl SpawnLifecycle for NoopSpawnLifecycle {
    fn prepare(&self, _spawn: &mut ResolvedSpawn) -> PtyResult<()> { Ok(()) }
    fn started(&self, _summary: &PtySummary) -> PtyResult<()> { Ok(()) }
    fn failed(&self, _pty_id: PtyId) {}
}
```

Em `PtyManager` (`src-tauri/crates/mt-pty/src/manager.rs`):
```rust
pub fn spawn(
    &self,
    spec: SpawnSpec,
    sink: Option<Arc<dyn OutputSink>>,
) -> PtyResult<PtySummary> {
    self.spawn_with_lifecycle(spec, sink, &NoopSpawnLifecycle)
}

pub fn spawn_with_lifecycle(
    &self,
    spec: SpawnSpec,
    sink: Option<Arc<dyn OutputSink>>,
    lifecycle: &dyn SpawnLifecycle,
) -> PtyResult<PtySummary>;
```
Regras no fluxo de spawn da Fase 02:
1. Após montar `ResolvedSpawn`, chamar `lifecycle.prepare(&mut resolved)`. Em caso de `Err(e)`, chamar `lifecycle.failed(pty_id)` e abortar antes de criar cgroup/filho.
2. Revalidar as chaves/valores de `resolved.env` contra a regra da Fase 02 (`^[A-Za-z_][A-Za-z0-9_]*$`, sem `\0`, `<= 128 KiB`).
3. Após criar o filho bloqueado no launcher e obter `PtySummary` inicial (`pid`, `start_time`, `pgid`, `sid`), chamar `lifecycle.started(&summary)` **antes** de iniciar as threads `reader`, `writer` e `waiter`.
4. Se `lifecycle.started(&summary)` ou a liberação do launcher falhar, encerrar/recolher o filho e o cgroup, chamar `lifecycle.failed(pty_id)` e propagar o erro.

### 3.4. Migration SQLite (`src-tauri/crates/mt-db/migrations/0002_05_shell_sessions_command_blocks.sql`)

```sql
CREATE TABLE sessions (
    id TEXT NOT NULL PRIMARY KEY,
    tab_id TEXT NOT NULL,
    pane_id TEXT NOT NULL,
    project_id TEXT,
    pty_id TEXT,
    shell TEXT NOT NULL,
    cwd TEXT,
    status TEXT NOT NULL DEFAULT 'active',
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    exit_code INTEGER,
    exit_signal INTEGER,
    next_seq INTEGER NOT NULL DEFAULT 1 CHECK (next_seq > 0)
) STRICT;
CREATE INDEX idx_sessions_tab_started ON sessions(tab_id, started_at DESC);
CREATE INDEX idx_sessions_active ON sessions(status) WHERE status = 'active';

CREATE TABLE command_blocks (
    rowid INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    tab_id TEXT NOT NULL,
    pane_id TEXT NOT NULL,
    pty_id TEXT NOT NULL,
    generation INTEGER NOT NULL CHECK (generation > 0),
    seq INTEGER NOT NULL CHECK (seq > 0),
    command TEXT,
    command_quality TEXT NOT NULL CHECK (command_quality IN ('complete','partial','unavailable')),
    cwd TEXT,
    shell TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('hooks','proc')),
    source TEXT NOT NULL,
    confidence TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running','succeeded','failed','unknown')),
    exit_code INTEGER,
    end_reason TEXT,
    continuous INTEGER NOT NULL CHECK (continuous IN (0,1)),
    started_at INTEGER NOT NULL,
    ended_at INTEGER,
    duration_ms INTEGER CHECK (duration_ms IS NULL OR duration_ms >= 0),
    output_start_seq INTEGER NOT NULL,
    output_start_offset INTEGER NOT NULL CHECK (output_start_offset >= 0),
    output_end_seq INTEGER,
    output_end_offset INTEGER,
    output_bytes INTEGER NOT NULL DEFAULT 0 CHECK (output_bytes >= 0),
    output_truncated INTEGER NOT NULL DEFAULT 0 CHECK (output_truncated IN (0,1)),
    dropped_bytes INTEGER NOT NULL DEFAULT 0 CHECK (dropped_bytes >= 0),
    output_text TEXT,
    error_tail TEXT,
    UNIQUE(session_id, seq),
    UNIQUE(pty_id, generation),
    CHECK ((status = 'running' AND ended_at IS NULL AND exit_code IS NULL)
        OR (status = 'succeeded' AND ended_at IS NOT NULL AND exit_code IS NOT NULL AND exit_code = 0)
        OR (status = 'failed' AND ended_at IS NOT NULL AND exit_code IS NOT NULL AND exit_code != 0)
        OR (status = 'unknown' AND ended_at IS NOT NULL AND exit_code IS NULL))
) STRICT;
CREATE INDEX idx_blocks_tab_started ON command_blocks(tab_id, started_at DESC);
CREATE INDEX idx_blocks_failed ON command_blocks(session_id, started_at DESC) WHERE status = 'failed';
CREATE INDEX idx_blocks_running ON command_blocks(status) WHERE status = 'running';

CREATE VIRTUAL TABLE command_blocks_fts USING fts5(
    command, cwd, output_text,
    content='command_blocks', content_rowid='rowid',
    tokenize='unicode61 remove_diacritics 2', prefix='2 3 4'
);
CREATE TRIGGER command_blocks_fts_ai AFTER INSERT ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(rowid, command, cwd, output_text)
    VALUES (new.rowid, new.command, new.cwd, new.output_text);
END;
CREATE TRIGGER command_blocks_fts_ad AFTER DELETE ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(command_blocks_fts, rowid, command, cwd, output_text)
    VALUES ('delete', old.rowid, old.command, old.cwd, old.output_text);
END;
CREATE TRIGGER command_blocks_fts_au AFTER UPDATE OF command, cwd, output_text ON command_blocks BEGIN
    INSERT INTO command_blocks_fts(command_blocks_fts, rowid, command, cwd, output_text)
    VALUES ('delete', old.rowid, old.command, old.cwd, old.output_text);
    INSERT INTO command_blocks_fts(rowid, command, cwd, output_text)
    VALUES (new.rowid, new.command, new.cwd, new.output_text);
END;
```

### 3.5. Repositórios em `src-tauri/crates/mt-db/src/repos/`

#### `src-tauri/crates/mt-db/src/repos/mod.rs`
```rust
pub mod command_blocks;
pub mod sessions;

pub use command_blocks::{
    CommandBlockFinishRecord, CommandBlockStartRecord, CommandBlocksRepo, RecoveryReport,
};
pub use sessions::{PreparedSession, SessionRecord, SessionStatus, SessionsRepo};

use crate::fts::{FtsSpec, FtsTokenizer};

pub const fn shell_fts_spec() -> FtsSpec {
    FtsSpec {
        content_table: "command_blocks",
        fts_table: "command_blocks_fts",
        columns: &["command", "cwd", "output_text"],
        tokenizer: FtsTokenizer::Text,
        prefix: &[2, 3, 4],
    }
}
```

#### `src-tauri/crates/mt-db/src/repos/sessions.rs`
```rust
use mt_core::{PaneId, ProjectId, PtyId, SessionId, TabId};
use crate::error::DbError;
use crate::sanitizer::SanitizedText;
use crate::Db;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    Active,
    Closed,
}

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: SessionId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub project_id: Option<ProjectId>,
    pub pty_id: Option<PtyId>,
    pub shell: String,
    pub cwd: Option<String>,
    pub status: SessionStatus,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub exit_signal: Option<i32>,
    pub next_seq: u64,
}

#[derive(Debug, Clone)]
pub struct PrepareSessionInput {
    pub session_id: Option<SessionId>,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub project_id: Option<ProjectId>,
    pub pty_id: PtyId,
    pub shell: String,
    pub cwd: Option<SanitizedText>,
    pub started_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct PreparedSession {
    pub session_id: SessionId,
    pub initial_next_seq: u64,
    pub reused: bool,
}

#[derive(Clone)]
pub struct SessionsRepo {
    db: Db,
}

impl SessionsRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Cria uma nova sessão ativa ou reutiliza uma sessão existente quando `input.session_id` é fornecido.
    /// Se `session_id` for fornecido, valida na mesma transação:
    /// 1. A sessão existe e está com `status = 'active'`.
    /// 2. Pertence exatamente ao mesmo `(tab_id, pane_id)`.
    /// 3. Atualiza `pty_id`, `shell` e `cwd` e retorna o `next_seq` atual (sem reiniciar para 1).
    pub async fn prepare(&self, input: PrepareSessionInput) -> Result<PreparedSession, DbError>;

    /// Fecha a sessão (`status = 'closed'`, `ended_at`, `exit_code`, `exit_signal`) se o `pty_id` coincidir.
    pub async fn close(
        &self,
        session_id: SessionId,
        pty_id: PtyId,
        ended_at_ms: i64,
        exit_code: Option<i32>,
        exit_signal: Option<i32>,
    ) -> Result<(), DbError>;

    /// Aborta uma sessão recém-preparada cujo spawn falhou antes de executar comandos:
    /// se `next_seq == 1` e não houver blocos, fecha imediatamente com `ended_at = started_at`.
    pub async fn abort(&self, session_id: SessionId, pty_id: PtyId, now_ms: i64) -> Result<(), DbError>;

    /// Obtém os metadados da sessão por ID.
    pub async fn get(&self, session_id: SessionId) -> Result<Option<SessionRecord>, DbError>;
}
```

#### `src-tauri/crates/mt-db/src/repos/command_blocks.rs`
```rust
use mt_core::{CommandBlockId, PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandBlockDto, CommandBlockHit, CommandTextQuality,
    IntegrationMode, ObservationConfidence, ObservationSource, Page, StreamPosition,
};
use crate::error::DbError;
use crate::sanitizer::SanitizedText;
use crate::Db;

#[derive(Debug, Clone)]
pub struct CommandBlockStartRecord {
    pub id: CommandBlockId,
    pub session_id: SessionId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub pty_id: PtyId,
    pub generation: u64,
    pub seq: u64,
    pub command: Option<SanitizedText>,
    pub command_quality: CommandTextQuality,
    pub cwd: Option<SanitizedText>,
    pub shell: String,
    pub mode: IntegrationMode, // somente Hooks ou Proc
    pub source: ObservationSource,
    pub confidence: ObservationConfidence,
    pub continuous: bool,
    pub started_at_ms: i64,
    pub output_start: StreamPosition,
}

#[derive(Debug, Clone)]
pub struct CommandBlockFinishRecord {
    pub id: CommandBlockId,
    pub pty_id: PtyId,
    pub generation: u64,
    pub status: BlockStatus, // Succeeded, Failed ou Unknown
    pub exit_code: Option<i32>,
    pub end_reason: BlockEndReason,
    pub ended_at_ms: i64,
    pub duration_ms: u64,
    pub output_end: Option<StreamPosition>,
    pub output_bytes: u64,
    pub output_truncated: bool,
    pub dropped_bytes: u64,
    pub output_text: Option<SanitizedText>,
    pub error_tail: Option<SanitizedText>,
    /// Opcionalmente atualiza command/cwd se chegaram junto do fechamento ou reconciliação.
    pub late_command: Option<(SanitizedText, CommandTextQuality)>,
    pub late_cwd: Option<SanitizedText>,
}

#[derive(Debug, Clone, Default)]
pub struct RecoveryReport {
    pub closed_blocks: u64,
    pub closed_sessions: u64,
}

#[derive(Clone)]
pub struct CommandBlocksRepo {
    db: Db,
}

impl CommandBlocksRepo {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Insere um bloco em estado `running` e avança `sessions.next_seq = MAX(sessions.next_seq, seq + 1)`
    /// na mesma transação IMMEDIATE.
    /// Idempotente em retry: se já existir linha com mesmo `id`, `(session_id, seq)` e `(pty_id, generation)`,
    /// retorna `Ok(())` sem incrementar `next_seq` novamente; se conflitar com outro ID, retorna `DbError::Conflict`.
    pub async fn start(&self, record: CommandBlockStartRecord) -> Result<(), DbError>;

    /// Finaliza um bloco existente. Usa guarda `WHERE id = ?1 AND pty_id = ?2 AND generation = ?3`.
    /// Se a linha já estiver finalizada com o mesmo `(status, exit_code, end_reason)`, é idempotente (`Ok(true)`);
    /// se estiver `running`, aplica o UPDATE e retorna `Ok(true)`; se não existir, retorna `Ok(false)`.
    pub async fn finish(&self, record: CommandBlockFinishRecord) -> Result<bool, DbError>;

    /// Busca um bloco por ID. `output_text` só é carregado quando `include_output == true`.
    pub async fn get(
        &self,
        block_id: CommandBlockId,
        include_output: bool,
    ) -> Result<Option<CommandBlockDto>, DbError>;

    /// Retorna a cauda de erro (`error_tail`) e o bloco correspondente.
    pub async fn get_error_tail(
        &self,
        block_id: CommandBlockId,
    ) -> Result<Option<(CommandBlockDto, Option<String>)>, DbError>;

    /// Lista blocos de uma sessão em ordem decrescente de `seq` (`seq < before_seq`), limitado a `1..=500`.
    /// Nunca carrega `output_text` na listagem.
    pub async fn list(
        &self,
        session_id: SessionId,
        before_seq: Option<u64>,
        limit: u32,
    ) -> Result<Page<CommandBlockDto>, DbError>;

    /// Busca textual via `command_blocks_fts` usando `build_match_query` da Fase 03.
    /// Query vazia/pontuação retorna `Ok(vec![])`; query > 4096 bytes retorna `DbError::InvalidQuery`.
    /// Limit validado em `1..=100`. Usa `snippet(command_blocks_fts, -1, '«', '»', '…', 24)` e `bm25(command_blocks_fts)`.
    pub async fn search(
        &self,
        query: &str,
        session_id: Option<SessionId>,
        limit: u32,
    ) -> Result<Vec<CommandBlockHit>, DbError>;

    /// Executado uma única vez no boot (antes de aceitar novos spawns):
    /// fecha todos os blocos com `status = 'running'` como `status = 'unknown'`, `end_reason = 'recovery'`,
    /// `ended_at = now_ms`, `exit_code = NULL`, sem fabricar código 0.
    pub async fn close_dangling(&self, now_ms: i64) -> Result<RecoveryReport, DbError>;
}
```

---

## 4. Regras de Negócio, Sanitização e Casos de Borda

1. **Barreira de Sanitização Tipada (`SanitizedText`):**
   - `SessionsRepo` e `CommandBlocksRepo` jamais recebem `String` ou `&str` para `command`, `cwd`, `output_text` ou `error_tail`. Os structs de escrita exigem `Option<SanitizedText>`.
2. **Invariantes de `status`, `ended_at` e `exit_code`:**
   - A constraint `CHECK` da tabela `command_blocks` garante no nível do SQLite que:
     - `running` ⇔ `ended_at IS NULL AND exit_code IS NULL`
     - `succeeded` ⇔ `ended_at IS NOT NULL AND exit_code = 0`
     - `failed` ⇔ `ended_at IS NOT NULL AND exit_code != 0`
     - `unknown` ⇔ `ended_at IS NOT NULL AND exit_code IS NULL`
   - Antes de executar o SQL no `CommandBlocksRepo::finish`, validar a coerência em Rust (ex.: se `status == Succeeded` e `exit_code != Some(0)`, retornar `DbError::Constraint("command_blocks.status_exit_code_mismatch".into())`).
3. **Limites de Inteiros (`u64` → `i64` SQLite):**
   - Converter `generation`, `seq`, `duration_ms`, `output_start_seq`, `output_end_seq`, `output_bytes`, `dropped_bytes` usando `i64::try_from(val)` antes do bind. Em caso de overflow (`> i64::MAX`), retornar `DbError::Constraint("integer_overflow".into())`, nunca fazer cast `as i64` com wrap negativo.
4. **Estabilidade do `rowid` no FTS5:**
   - A coluna `rowid INTEGER PRIMARY KEY` em `command_blocks` garante que `VACUUM` ou `incremental_vacuum` nunca reescrevam os rowids referenciados por `command_blocks_fts`. O trigger `command_blocks_fts_au` dispara `AFTER UPDATE OF command, cwd, output_text`, portanto atualizações apenas de `status`/`exit_code` (quando `output_text` continua `NULL`) são rápidas, e quando `output_text` é preenchido no `finish`, o índice FTS5 é atualizado atomicamente na mesma transação.
5. **Proibição de `INSERT OR REPLACE`:**
   - Conforme a Fase 03 §4.5, nunca usar `INSERT OR REPLACE` em `sessions` ou `command_blocks`, pois isso causaria `DELETE` + `INSERT`, acionando `ON DELETE CASCADE` ou corrompendo triggers FTS5. Usar `INSERT` com checagem prévia de idempotência ou `UPDATE` parametrizado.
