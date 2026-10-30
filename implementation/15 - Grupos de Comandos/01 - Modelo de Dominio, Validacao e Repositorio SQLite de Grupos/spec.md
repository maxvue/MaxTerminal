# Implementação 01 — Modelo de Domínio, Validação e Repositório SQLite de Grupos (`spec.md`)

## 1. Contexto, Objetivo e Escopo

Esta implementação estabelece a base de domínio, validação determinística, resolução segura de diretórios/ambiente, redutor de estados de execução e persistência transacional no SQLite (em modo WAL) para a **Fase 15: Grupos de Comandos (`>>`)** dentro do crate `mt-commands` e das migrations do `mt-db`.

### 1.1. Escopo Incluído
- **Itens da Fase 15 cobertos:** `I-01` (Modelo de domínio e DTOs), `I-02` (Migration SQLite `NNNN_15_command_groups.sql`), `I-03` (`CommandGroupRepo` e `SqliteCommandGroupRepo`), `I-04` (Validação de rascunho, normalização Unicode NFC, env POSIX, `cwd` confinado e detecção bloqueante de segredos via `mt_db::sanitizer`) e `I-08` (Persistência do histórico de execuções `command_group_runs` e `command_group_run_items` e redutor determinístico de estados).
- **Critérios de Aceite cobertos:** `CA-01`, `CA-02`, `CA-03`, `CA-04`, `CA-22`, `CA-23`, `CA-24`, `CA-25`, `CA-26`.

### 1.2. Escopo Excluído (Fases/Implementações Posteriores)
- Motores de execução (`SequentialComposer`, `SequentialRunner`, `ParallelRunner`, `MultiTabPlanner`, `MultiTabRunner`) — cobertos na **Implementação 02**.
- Orquestrador `CommandGroupService`, `GroupSuggester` (IA Fase 12 / Memória Fase 13), adaptadores Tauri e comandos IPC — cobertos na **Implementação 03**.
- Store Pinia, composables e gatilho `>>` — cobertos na **Implementação 04**.
- Componentes Vue (Modal, Painel de Execução, Visualizador de Saída) — cobertos na **Implementação 05**.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Absoluto / Relativo ao Workspace | Descrição |
|------|------------------------------------------|-----------|
| Criar | `src-tauri/crates/mt-db/migrations/0015_15_command_groups.sql` | Migration SQLite com `command_groups`, `command_group_labels`, `command_group_items`, `command_group_runs`, `command_group_run_items` e índices (ajustar prefixo numérico `NNNN` caso o último arquivo em `migrations/` seja diferente). |
| Modificar | `src-tauri/crates/mt-commands/Cargo.toml` | Garantir dependências `unicode-normalization`, `sha2`, `uuid`, `serde`, `serde_json`, `specta`, `rusqlite` e `mt-core` / `mt-db`. |
| Modificar | `src-tauri/crates/mt-commands/src/lib.rs` | Exportar o módulo público `pub mod groups;` além do módulo `artisan` da Fase 14. |
| Criar | `src-tauri/crates/mt-commands/src/groups/mod.rs` | Reexportações públicas do submódulo `groups`. |
| Criar | `src-tauri/crates/mt-commands/src/groups/model.rs` | Identificadores newtype UUID v7, enums, structs de domínio, snapshots históricos, entradas de CRUD, filtros e códigos de erro `GroupErrorCode`. |
| Criar | `src-tauri/crates/mt-commands/src/groups/validate.rs` | Validador de `CommandGroupInput`, normalização Unicode NFC (`name_key`, `label_key`), validação POSIX de variáveis de ambiente e detecção bloqueante de segredos via `mt_db::sanitizer`. |
| Criar | `src-tauri/crates/mt-commands/src/groups/resolve.rs` | `CwdResolver` (canonicalização, verificação de existência e confinamento estrito na raiz do projeto/worktree sem escape por `..` ou symlink), mesclagem de `env` e cálculo de `request_hash` SHA-256 canônico. |
| Criar | `src-tauri/crates/mt-commands/src/groups/aggregate.rs` | Máquina de estados de itens (`ItemStatus`), redutor determinístico do estado agregado (`RunStatus`) e cálculo de contadores (`GroupRunCounts`). |
| Criar | `src-tauri/crates/mt-commands/src/groups/repo.rs` | Trait `CommandGroupRepo` e implementação `SqliteCommandGroupRepo` com CRUD transacional, controle de concorrência otimista (`revision` e `version`), reordenação atômica e retenção de histórico. |
| Criar | `src-tauri/crates/mt-commands/tests/groups_validate.rs` | Testes unitários e de integração de validação, `CwdResolver`, normalização Unicode e bloqueio de segredos (`CA-04`). |
| Criar | `src-tauri/crates/mt-commands/tests/groups_repo.rs` | Testes de integração SQLite cobrindo `CHECK` de escopo, unicidade case-insensitive Unicode, cascata de FK, substituição atômica de itens, `reorder`, `request_id` idempotente e retenção (`CA-02`, `CA-03`, `CA-23`, `CA-24`). |

---

## 3. Especificação da Migration SQLite (`0015_15_command_groups.sql`)

```sql
-- Fase 15: Grupos de Comandos (Local e Global), Itens, Labels e Histórico de Execuções

CREATE TABLE command_groups (
    id TEXT PRIMARY KEY,
    scope TEXT NOT NULL CHECK(scope IN ('local','global')),
    project_id TEXT REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL CHECK(length(name) BETWEEN 1 AND 80),
    name_key TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    mode TEXT NOT NULL CHECK(mode IN ('sequential','parallel','multi_tabs')),
    separator TEXT NOT NULL CHECK(separator IN ('and','semicolon')),
    cwd TEXT,
    env_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(env_json)),
    sort_order INTEGER NOT NULL CHECK(sort_order >= 0),
    revision INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    CHECK((scope = 'local' AND project_id IS NOT NULL) OR (scope = 'global' AND project_id IS NULL))
);

CREATE UNIQUE INDEX ux_groups_local_name ON command_groups(project_id, name_key) WHERE scope = 'local';
CREATE UNIQUE INDEX ux_groups_global_name ON command_groups(name_key) WHERE scope = 'global';
CREATE INDEX ix_groups_order ON command_groups(scope, project_id, sort_order, id);

CREATE TABLE command_group_labels (
    group_id TEXT NOT NULL REFERENCES command_groups(id) ON DELETE CASCADE,
    label_key TEXT NOT NULL,
    label TEXT NOT NULL,
    PRIMARY KEY(group_id, label_key)
);

CREATE INDEX ix_group_labels_lookup ON command_group_labels(label_key, group_id);

CREATE TABLE command_group_items (
    id TEXT PRIMARY KEY,
    group_id TEXT NOT NULL REFERENCES command_groups(id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK(position >= 0),
    title TEXT NOT NULL,
    command TEXT NOT NULL CHECK(length(command) > 0),
    cwd TEXT,
    env_json TEXT NOT NULL DEFAULT '{}' CHECK(json_valid(env_json)),
    continuous INTEGER NOT NULL DEFAULT 0 CHECK(continuous IN (0, 1)),
    UNIQUE(group_id, position)
);

CREATE TABLE command_group_runs (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    request_hash TEXT NOT NULL,
    group_id TEXT REFERENCES command_groups(id) ON DELETE SET NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    origin_session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
    mode TEXT NOT NULL CHECK(mode IN ('sequential','parallel','multi_tabs')),
    status TEXT NOT NULL CHECK(status IN ('queued','running','cancelling','succeeded','failed','partial','cancelled','interrupted')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    last_exit_code INTEGER,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    cancel_requested_at INTEGER,
    finished_at INTEGER
);

CREATE INDEX ix_group_runs_project_time ON command_group_runs(project_id, created_at DESC, id);
CREATE INDEX ix_group_runs_group_time ON command_group_runs(group_id, created_at DESC, id);
CREATE INDEX ix_group_runs_origin ON command_group_runs(origin_session_id);
CREATE INDEX ix_group_runs_active ON command_group_runs(status) WHERE finished_at IS NULL;

CREATE TABLE command_group_run_items (
    id TEXT PRIMARY KEY,
    run_id TEXT NOT NULL REFERENCES command_group_runs(id) ON DELETE CASCADE,
    position INTEGER NOT NULL,
    snapshot_json TEXT NOT NULL CHECK(json_valid(snapshot_json)),
    status TEXT NOT NULL CHECK(status IN ('pending','starting','running','cancelling','succeeded','failed','skipped','cancelled','interrupted')),
    version INTEGER NOT NULL DEFAULT 1 CHECK(version > 0),
    pty_id TEXT,
    tab_id TEXT REFERENCES tabs(id) ON DELETE SET NULL,
    pane_id TEXT REFERENCES panes(id) ON DELETE SET NULL,
    session_id TEXT REFERENCES sessions(id) ON DELETE SET NULL,
    command_block_id TEXT REFERENCES command_blocks(id) ON DELETE SET NULL,
    exit_code INTEGER,
    signal INTEGER,
    error_code TEXT,
    error_message TEXT,
    started_at INTEGER,
    finished_at INTEGER,
    output_tail TEXT NOT NULL DEFAULT '',
    output_truncated INTEGER NOT NULL DEFAULT 0 CHECK(output_truncated IN (0, 1)),
    UNIQUE(run_id, position)
);

CREATE INDEX ix_group_run_items_pty ON command_group_run_items(pty_id);
CREATE INDEX ix_group_run_items_session ON command_group_run_items(session_id);
CREATE INDEX ix_group_run_items_tab ON command_group_run_items(tab_id);
CREATE INDEX ix_group_run_items_pane ON command_group_run_items(pane_id);
CREATE INDEX ix_group_run_items_block ON command_group_run_items(command_block_id);
```

---

## 4. Assinaturas Exatas de Tipos e Módulos Rust

### 4.1. `src-tauri/crates/mt-commands/src/groups/model.rs`

```rust
use std::collections::BTreeMap;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;
use mt_core::{
    CommandBlockId, CommandGroupId, MtError, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId
};

/// Identificador único de um item da definição de um grupo de comandos (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupItemId(pub String);

impl GroupItemId {
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identificador único de uma execução de grupo de comandos (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupRunId(pub String);

impl GroupRunId {
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identificador único de um item instanciado em uma execução de grupo (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupRunItemId(pub String);

impl GroupRunItemId {
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identificador de idempotência enviado pelo cliente ao solicitar o lançamento de um grupo (UUID v7).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct GroupRequestId(pub String);

impl GroupRequestId {
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identificador de assinatura de stream de saída (`Channel<GroupOutputFrame>`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct OutputSubscriptionId(pub String);

impl OutputSubscriptionId {
    pub fn new() -> Self {
        Self(Uuid::now_v7().to_string())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Escopo de visibilidade do grupo de comandos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GroupScope {
    Local,
    Global,
}

impl GroupScope {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Global => "global",
        }
    }
}

/// Modo de execução do grupo de comandos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Sequential,
    Parallel,
    MultiTabs,
}

impl ExecutionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Sequential => "sequential",
            Self::Parallel => "parallel",
            Self::MultiTabs => "multi_tabs",
        }
    }
}

/// Política de continuação entre itens no modo Sequencial na mesma aba.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum SequentialSeparator {
    And,
    Semicolon,
}

impl SequentialSeparator {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Semicolon => "semicolon",
        }
    }
}

/// Dialeto real do shell onde o item será executado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ShellDialect {
    Bash,
    Zsh,
    Fish,
}

/// Estado individual de um item dentro de uma execução (`GroupRunItem`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Pending,
    Starting,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
    Interrupted,
}

impl ItemStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed
                | Self::Skipped
                | Self::Cancelled
                | Self::Interrupted
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

/// Estado agregado determinístico de uma execução de grupo (`GroupRun`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Cancelling,
    Succeeded,
    Failed,
    Partial,
    Cancelled,
    Interrupted,
}

impl RunStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Self::Succeeded
                | Self::Failed
                | Self::Partial
                | Self::Cancelled
                | Self::Interrupted
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Cancelling => "cancelling",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Partial => "partial",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

/// Entrada de criação/atualização para um item de grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandGroupItemInput {
    pub id: Option<GroupItemId>,
    pub title: String,
    pub command: String,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub continuous: bool,
}

/// Entrada de criação/atualização para um grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CommandGroupInput {
    pub scope: GroupScope,
    pub project_id: Option<ProjectId>,
    pub name: String,
    pub description: String,
    pub labels: Vec<String>,
    pub mode: ExecutionMode,
    pub separator: SequentialSeparator,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub items: Vec<CommandGroupItemInput>,
}

/// Item persistido de uma definição de grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroupItem {
    pub id: GroupItemId,
    pub group_id: CommandGroupId,
    pub position: u32,
    pub title: String,
    pub command: String,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub continuous: bool,
}

/// Definição completa persistida de um grupo de comandos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroup {
    pub id: CommandGroupId,
    pub scope: GroupScope,
    pub project_id: Option<ProjectId>,
    pub name: String,
    pub description: String,
    pub labels: Vec<String>,
    pub mode: ExecutionMode,
    pub separator: SequentialSeparator,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub sort_order: u32,
    pub revision: u64,
    pub created_at: i64,
    pub updated_at: i64,
    pub items: Vec<CommandGroupItem>,
}

/// Catálogo retornado por `command_group_list` e `command_group_reorder`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandGroupCatalog {
    pub project_id: Option<ProjectId>,
    pub local_groups: Vec<CommandGroup>,
    pub global_groups: Vec<CommandGroup>,
}

/// Entrada de reordenação com verificação otimista de revisão por grupo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReorderGroupEntry {
    pub id: CommandGroupId,
    pub revision: u64,
}

/// Recibo de exclusão de grupo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeleteGroupReceipt {
    pub id: CommandGroupId,
    pub revision: u64,
}

/// Requisição IPC para iniciar a execução de um grupo salvo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupRunRequest {
    pub request_id: GroupRequestId,
    pub group_id: CommandGroupId,
    pub expected_revision: u64,
    pub project_id: ProjectId,
    pub origin_pane_id: Option<PaneId>,
    pub mode_override: Option<ExecutionMode>,
    pub template_by_item: BTreeMap<GroupItemId, TemplateId>,
}

/// Contexto resolvido para o lançamento de uma execução.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunContext {
    pub project_id: ProjectId,
    pub root: PathBuf,
    pub origin: Option<(TabId, PaneId, PtyId, SessionId)>,
    pub shell: PathBuf,
    pub dialect: ShellDialect,
    pub generation: u64,
}

/// Snapshot imutável do grupo persistido em `command_group_runs.snapshot_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunSnapshot {
    pub group_id: CommandGroupId,
    pub group_revision: u64,
    pub scope: GroupScope,
    pub name: String,
    pub description: String,
    pub labels: Vec<String>,
    pub defined_mode: ExecutionMode,
    pub effective_mode: ExecutionMode,
    pub separator: SequentialSeparator,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub project_root: String,
    pub shell_path: String,
    pub shell_dialect: ShellDialect,
    pub origin_tab_id: Option<TabId>,
    pub origin_pane_id: Option<PaneId>,
    pub origin_pty_id: Option<PtyId>,
    pub origin_session_id: Option<SessionId>,
    pub origin_generation: Option<u64>,
}

/// Snapshot imutável de um item persistido em `command_group_run_items.snapshot_json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunItemSnapshot {
    pub source_item_id: GroupItemId,
    pub position: u32,
    pub title: String,
    pub command: String,
    pub defined_cwd: Option<String>,
    pub resolved_cwd: String,
    pub env: BTreeMap<String, String>,
    pub continuous: bool,
    pub template_id: Option<TemplateId>,
    pub shell_path: String,
    pub shell_dialect: ShellDialect,
}

/// Contadores derivados dos estados dos itens de uma execução.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunCounts {
    pub total: u32,
    pub pending: u32,
    pub starting: u32,
    pub running: u32,
    pub cancelling: u32,
    pub succeeded: u32,
    pub failed: u32,
    pub skipped: u32,
    pub cancelled: u32,
    pub interrupted: u32,
}

/// Estado detalhado de um item dentro de uma execução (`GroupRunItem`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunItem {
    pub id: GroupRunItemId,
    pub run_id: GroupRunId,
    pub position: u32,
    pub snapshot: GroupRunItemSnapshot,
    pub status: ItemStatus,
    pub version: u64,
    pub pty_id: Option<PtyId>,
    pub tab_id: Option<TabId>,
    pub pane_id: Option<PaneId>,
    pub session_id: Option<SessionId>,
    pub command_block_id: Option<CommandBlockId>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub output_truncated: bool,
}

/// Resumo de uma execução de grupo para listagem paginada (sem `output_tail` e sem lista de itens).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunSummary {
    pub id: GroupRunId,
    pub request_id: GroupRequestId,
    pub group_id: Option<CommandGroupId>,
    pub project_id: ProjectId,
    pub origin_session_id: Option<SessionId>,
    pub snapshot: GroupRunSnapshot,
    pub mode: ExecutionMode,
    pub status: RunStatus,
    pub counts: GroupRunCounts,
    pub version: u64,
    pub last_exit_code: Option<i32>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub cancel_requested_at: Option<i64>,
    pub finished_at: Option<i64>,
}

/// Detalhe completo de uma execução de grupo (`GroupRun`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRun {
    pub id: GroupRunId,
    pub request_id: GroupRequestId,
    pub group_id: Option<CommandGroupId>,
    pub project_id: ProjectId,
    pub origin_session_id: Option<SessionId>,
    pub snapshot: GroupRunSnapshot,
    pub mode: ExecutionMode,
    pub status: RunStatus,
    pub counts: GroupRunCounts,
    pub version: u64,
    pub last_exit_code: Option<i32>,
    pub created_at: i64,
    pub started_at: Option<i64>,
    pub cancel_requested_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub items: Vec<GroupRunItem>,
}

/// Página retornada por `command_group_run_list`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunListPage {
    pub items: Vec<GroupRunSummary>,
    pub next_cursor: Option<String>,
}

/// Códigos de erro canônicos da Fase 15 (`commands.group.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupErrorCode {
    Validation,
    NotFound,
    NameConflict,
    RevisionConflict,
    ReorderMismatch,
    ProjectRequired,
    ScopeMismatch,
    CwdOutsideProject,
    CwdNotFound,
    SecretDetected,
    UnsupportedShell,
    EnvCwdUnavailable,
    PaneBusy,
    TrackingUnavailable,
    TemplateIncompatible,
    RequestConflict,
    SpawnFailed,
    TabOpenFailed,
    PersistenceFailed,
}

impl GroupErrorCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Validation => "commands.group.validation",
            Self::NotFound => "commands.group.not_found",
            Self::NameConflict => "commands.group.name_conflict",
            Self::RevisionConflict => "commands.group.revision_conflict",
            Self::ReorderMismatch => "commands.group.reorder_mismatch",
            Self::ProjectRequired => "commands.group.project_required",
            Self::ScopeMismatch => "commands.group.scope_mismatch",
            Self::CwdOutsideProject => "commands.group.cwd_outside_project",
            Self::CwdNotFound => "commands.group.cwd_not_found",
            Self::SecretDetected => "commands.group.secret_detected",
            Self::UnsupportedShell => "commands.group.unsupported_shell",
            Self::EnvCwdUnavailable => "commands.group.env_cwd_unavailable",
            Self::PaneBusy => "commands.group.pane_busy",
            Self::TrackingUnavailable => "commands.group.tracking_unavailable",
            Self::TemplateIncompatible => "commands.group.template_incompatible",
            Self::RequestConflict => "commands.group.request_conflict",
            Self::SpawnFailed => "commands.group.spawn_failed",
            Self::TabOpenFailed => "commands.group.tab_open_failed",
            Self::PersistenceFailed => "commands.group.persistence_failed",
        }
    }

    pub fn into_error(self, message: impl Into<String>) -> MtError {
        MtError::new(self.as_str(), message.into())
    }

    pub fn into_error_with_fields(self, message: impl Into<String>, fields: Vec<String>) -> MtError {
        MtError::new(self.as_str(), message.into())
            .with_details(serde_json::json!({ "fields": fields }))
    }
}
```

### 4.2. `src-tauri/crates/mt-commands/src/groups/validate.rs`

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use unicode_normalization::UnicodeNormalization;
use mt_core::MtResult;
use mt_db::sanitizer::sanitize;
use crate::groups::model::{
    CommandGroupInput, CommandGroupItemInput, GroupErrorCode, GroupScope
};

pub const MAX_GROUP_NAME_CHARS: usize = 80;
pub const MAX_GROUP_DESC_CHARS: usize = 2048;
pub const MAX_GROUP_ITEMS: usize = 50;
pub const MAX_ITEM_TITLE_CHARS: usize = 80;
pub const MAX_COMMAND_BYTES: usize = 8192;
pub const MAX_LABELS: usize = 20;
pub const MAX_LABEL_CHARS: usize = 32;
pub const MAX_ENV_ENTRIES_PER_LEVEL: usize = 64;
pub const MAX_MERGED_ENV_BYTES: usize = 65_536;

/// Resultado validado e normalizado pronto para persistência no repositório.
#[derive(Debug, Clone)]
pub struct ValidatedGroupInput {
    pub scope: GroupScope,
    pub project_id: Option<mt_core::ProjectId>,
    pub name: String,
    pub name_key: String,
    pub description: String,
    pub labels: Vec<(String, String)>, // (label_key, label_sanitized)
    pub mode: crate::groups::model::ExecutionMode,
    pub separator: crate::groups::model::SequentialSeparator,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub items: Vec<ValidatedGroupItemInput>,
}

#[derive(Debug, Clone)]
pub struct ValidatedGroupItemInput {
    pub id: Option<crate::groups::model::GroupItemId>,
    pub title: String,
    pub command: String,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub continuous: bool,
}

/// Normaliza uma chave textual aplicando trim, Unicode NFC e lowercase determinístico.
pub fn normalize_key(raw: &str) -> String {
    raw.trim().nfc().collect::<String>().to_lowercase()
}

/// Verifica se uma chave de variável de ambiente obedece ao padrão POSIX `[A-Za-z_][A-Za-z0-9_]*`
/// e não utiliza o prefixo reservado `MAXTERMINAL_`.
pub fn validate_env_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("A chave da variável de ambiente não pode ser vazia.");
    }
    if key.starts_with("MAXTERMINAL_") {
        return Err("O prefixo MAXTERMINAL_ é reservado pelo sistema.");
    }
    let mut chars = key.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err("A chave da variável de ambiente deve iniciar com letra ASCII ou sublinhado (_).");
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err("A chave da variável de ambiente deve conter apenas letras ASCII, dígitos ou sublinhado (_).");
    }
    Ok(())
}

/// Valida sintaticamente um caminho relativo `cwd` na etapa de edição/persistência
/// (rejeita NUL, caminhos absolutos e componentes `..` que escapem da raiz do projeto).
pub fn validate_relative_cwd_syntax(raw_cwd: &str, field_path: &str) -> MtResult<String> {
    let trimmed = raw_cwd.trim();
    if trimmed.is_empty() {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            "O diretório de trabalho relativo não pode conter apenas espaços.",
            vec![field_path.to_string()],
        ));
    }
    if trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err(GroupErrorCode::Validation.into_error_with_fields(
            "O diretório de trabalho contém caracteres de controle inválidos.",
            vec![field_path.to_string()],
        ));
    }
    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(GroupErrorCode::CwdOutsideProject.into_error_with_fields(
            "O diretório de trabalho deve ser relativo à pasta raiz do projeto.",
            vec![field_path.to_string()],
        ));
    }
    let mut depth: i32 = 0;
    for component in path.components() {
        match component {
            std::path::Component::Normal(_) => depth += 1,
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                depth -= 1;
                if depth < 0 {
                    return Err(GroupErrorCode::CwdOutsideProject.into_error_with_fields(
                        "O diretório de trabalho não pode escapar da pasta raiz do projeto.",
                        vec![field_path.to_string()],
                    ));
                }
            }
            _ => {
                return Err(GroupErrorCode::CwdOutsideProject.into_error_with_fields(
                    "Caminho relativo inválido para o diretório de trabalho.",
                    vec![field_path.to_string()],
                ));
            }
        }
    }
    Ok(trimmed.to_string())
}

/// Valida integralmente um `CommandGroupInput`.
/// Caso `mt_db::sanitizer::sanitize()` altere qualquer comando ou valor de `env`, rejeita a operação
/// com `commands.group.secret_detected` listando todos os caminhos dos campos afetados em `details.fields`
/// sem jamais ecoar os valores secretos na mensagem ou nos detalhes.
pub fn validate_group_input(
    input: &CommandGroupInput,
    project_root: Option<&Path>,
) -> MtResult<ValidatedGroupInput>;
```

### 4.3. `src-tauri/crates/mt-commands/src/groups/resolve.rs`

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use mt_core::MtResult;
use crate::groups::model::{
    CommandGroup, CommandGroupItem, GroupErrorCode, GroupRunRequest, ShellDialect
};

/// Resolvedor seguro de diretório de trabalho e variáveis de ambiente para execução.
pub struct CwdResolver;

impl CwdResolver {
    /// Resolve o diretório efetivo (`item.cwd` > `group.cwd` > `project_root`), canonicaliza a raiz
    /// e o alvo e verifica se o diretório existe e permanece estritamente dentro da raiz canonicalizada
    /// (impedindo escape via symlinks que apontem para fora da raiz do projeto/worktree).
    pub fn resolve_effective_cwd(
        project_root: &Path,
        group_cwd: Option<&str>,
        item_cwd: Option<&str>,
        field_path: &str,
    ) -> MtResult<PathBuf>;

    /// Mescla variáveis de ambiente respeitando a precedência:
    /// `base_env` (herdado/template) → `group.env` → `item.env`, validando o limite de 64 KiB.
    pub fn merge_env(
        base_env: &BTreeMap<String, String>,
        group_env: &BTreeMap<String, String>,
        item_env: &BTreeMap<String, String>,
    ) -> MtResult<BTreeMap<String, String>>;
}

/// Detecta o `ShellDialect` a partir do nome do executável do shell (`bash`, `zsh`, `fish`).
/// Retorna `Err(GroupErrorCode::UnsupportedShell)` para qualquer outro executável.
pub fn detect_shell_dialect(shell_path: &Path) -> MtResult<ShellDialect>;

/// Calcula o hash SHA-256 hexadecimal minúsculo (64 caracteres) canônico de um `GroupRunRequest`
/// (incluindo `group_id`, `expected_revision`, `project_id`, `origin_pane_id`, `mode_override` e `template_by_item`)
/// para verificação de idempotência por `request_id`.
pub fn compute_request_hash(request: &GroupRunRequest) -> String;
```

### 4.4. `src-tauri/crates/mt-commands/src/groups/aggregate.rs`

```rust
use crate::groups::model::{GroupRunCounts, ItemStatus, RunStatus};

/// Calcula os contadores de estados a partir da lista ordenada de estados dos itens.
pub fn compute_counts(statuses: &[ItemStatus]) -> GroupRunCounts;

/// Calcula o `RunStatus` agregado determinístico de acordo com a tabela da spec §4.4:
/// 1. Se `cancel_requested` e houver recurso não terminal (`pending | starting | running | cancelling`) -> `Cancelling`
/// 2. Se houver algum item `starting | running | cancelling` ou (`has_started` e algum `pending`) -> `Running`
/// 3. Se todos estiverem `pending` e `!has_started` -> `Queued`
/// 4. Se todos forem terminais:
///    - Algum `Interrupted` -> `Interrupted`
///    - Algum `Cancelled` -> `Cancelled`
///    - Houver `Succeeded` (> 0) e `Failed` (> 0) -> `Partial`
///    - Houver `Failed` (> 0) e nenhum `Succeeded` -> `Failed`
///    - Todos `Succeeded` -> `Succeeded`
pub fn reduce_run_status(
    statuses: &[ItemStatus],
    has_started: bool,
    cancel_requested: bool,
) -> RunStatus;

/// Verifica se a transição de estado de um item é válida, garantindo que estados terminais
/// (`Succeeded`, `Failed`, `Skipped`, `Cancelled`, `Interrupted`) sejam imutáveis.
pub fn can_transition_item(from: ItemStatus, to: ItemStatus) -> bool;
```

### 4.5. `src-tauri/crates/mt-commands/src/groups/repo.rs`

```rust
use mt_core::{
    CommandBlockId, CommandGroupId, MtResult, PaneId, ProjectId, PtyId, SessionId, TabId
};
use crate::groups::model::{
    CommandGroup, CommandGroupCatalog, CommandGroupInput, DeleteGroupReceipt, ExecutionMode,
    GroupItemId, GroupRequestId, GroupRun, GroupRunId, GroupRunItem, GroupRunItemId,
    GroupRunItemSnapshot, GroupRunListPage, GroupRunSnapshot, GroupScope, ItemStatus,
    ReorderGroupEntry
};

/// Parâmetros para criação atômica de um registro `command_group_runs` + `command_group_run_items`.
#[derive(Debug, Clone)]
pub struct CreateRunRecord {
    pub run_id: GroupRunId,
    pub request_id: GroupRequestId,
    pub request_hash: String,
    pub group_id: CommandGroupId,
    pub project_id: ProjectId,
    pub origin_session_id: Option<SessionId>,
    pub snapshot: GroupRunSnapshot,
    pub mode: ExecutionMode,
    pub items: Vec<(GroupRunItemId, GroupRunItemSnapshot)>,
    pub created_at: i64,
}

/// Resultado da tentativa de aceitar/registrar uma execução por `request_id`.
#[derive(Debug, Clone)]
pub enum AcceptRunOutcome {
    /// Nova execução criada na transação com todos os itens em `pending`.
    Created(GroupRun),
    /// `request_id` já existia com o mesmo `request_hash`; retorna a execução existente.
    Existing(GroupRun),
}

/// Atualização atômica de um item de execução com recomputação automática do estado agregado e `version` do run.
#[derive(Debug, Clone, Default)]
pub struct RunItemPatch {
    pub status: Option<ItemStatus>,
    pub pty_id: Option<Option<PtyId>>,
    pub tab_id: Option<Option<TabId>>,
    pub pane_id: Option<Option<PaneId>>,
    pub session_id: Option<Option<SessionId>>,
    pub command_block_id: Option<Option<CommandBlockId>>,
    pub exit_code: Option<Option<i32>>,
    pub signal: Option<Option<i32>>,
    pub error_code: Option<Option<String>>,
    pub error_message: Option<Option<String>>,
    pub started_at: Option<Option<i64>>,
    pub finished_at: Option<Option<i64>>,
    pub output_tail: Option<(String, bool)>, // (cauda sanitizada <= 256 KiB, truncada)
}

/// Repositório síncrono/transacional sobre `mt-db` para Grupos de Comandos e Histórico de Execuções.
pub trait CommandGroupRepo: Send + Sync {
    fn list(&self, project_id: Option<&ProjectId>) -> MtResult<CommandGroupCatalog>;
    fn get(&self, group_id: &CommandGroupId) -> MtResult<CommandGroup>;
    fn create(&self, input: &CommandGroupInput) -> MtResult<CommandGroup>;
    fn update(
        &self,
        group_id: &CommandGroupId,
        expected_revision: u64,
        input: &CommandGroupInput,
    ) -> MtResult<CommandGroup>;
    fn delete(
        &self,
        group_id: &CommandGroupId,
        expected_revision: u64,
    ) -> MtResult<DeleteGroupReceipt>;
    fn reorder(
        &self,
        scope: GroupScope,
        project_id: Option<&ProjectId>,
        entries: &[ReorderGroupEntry],
    ) -> MtResult<CommandGroupCatalog>;

    /// Consulta rápida por `request_id` antes de validar revisão atual do grupo (§4.1 item 1).
    fn find_run_by_request_id(
        &self,
        request_id: &GroupRequestId,
    ) -> MtResult<Option<(String, GroupRun)>>;

    /// Insere o snapshot da execução e todos os seus itens `pending` em uma única transação.
    fn accept_run(&self, record: CreateRunRecord) -> MtResult<AcceptRunOutcome>;

    fn get_run(&self, run_id: &GroupRunId) -> MtResult<GroupRun>;
    fn list_runs(
        &self,
        project_id: &ProjectId,
        cursor: Option<&str>,
        limit: u32,
        active_only: bool,
    ) -> MtResult<GroupRunListPage>;

    /// Marca `cancel_requested_at`, cancela itens `pending` (ou apenas um item específico)
    /// e recalcula o estado agregado e `version` do run na mesma transação.
    fn mark_cancel_requested(
        &self,
        run_id: &GroupRunId,
        item_id: Option<&GroupRunItemId>,
        now_ms: i64,
    ) -> MtResult<(GroupRun, Vec<GroupRunItem>)>;

    /// Aplica transição em um item, atualiza `last_exit_code`, `started_at`, `finished_at`,
    /// incrementa `version` do item e do run e retorna `(GroupRun atualizado, GroupRunItem atualizado)`.
    fn update_run_item(
        &self,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
        patch: RunItemPatch,
        now_ms: i64,
    ) -> MtResult<(GroupRun, GroupRunItem)>;

    /// Persiste a cauda sanitizada (`output_tail` e `output_truncated`) de um item.
    fn get_run_item_output_tail(
        &self,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
    ) -> MtResult<(String, bool)>;

    /// No boot (após recuperação de processos da Fase 02), marca execuções e itens não terminais
    /// como `interrupted` e preenche `finished_at`.
    fn recover_interrupted_runs(&self, now_ms: i64) -> MtResult<Vec<GroupRun>>;

    /// Remove execuções terminais cujo `finished_at` seja anterior a `cutoff_ms` (retenção padrão 30 dias).
    fn prune_finished_runs_older_than(&self, cutoff_ms: i64) -> MtResult<u64>;
}

pub struct SqliteCommandGroupRepo {
    db: mt_db::DatabasePool,
}

impl SqliteCommandGroupRepo {
    pub fn new(db: mt_db::DatabasePool) -> Self {
        Self { db }
    }
}
```

---

## 5. Regras de Negócio e Casos de Borda Desta Implementação

1. **Normalização Unicode NFC (`name_key` e `label_key`):**
   - O SQLite `COLLATE NOCASE` lida apenas com ASCII. Portanto, `normalize_key` aplica `.trim().nfc().collect::<String>().to_lowercase()` em Rust antes de gravar `name_key` e `label_key`.
   - Nomes como `"Produção"` (composto) e `"Produc\u{0327}a\u{0303}o"` (decomposto) ou `"DEV UP"` e `"dev up"` produzem exatamente o mesmo `name_key` e disparam `commands.group.name_conflict` dentro do mesmo escopo/projeto, mas são permitidos em projetos distintos ou entre um projeto e o escopo `Global`.
2. **Imutabilidade de Escopo e Projeto no `update`:**
   - Se `input.scope != existing.scope` ou `input.project_id != existing.project_id`, `update` retorna imediatamente `commands.group.scope_mismatch` sem alterar o banco. Mudança de escopo só ocorre por cópia explícita ("Copiar para…") criando um novo grupo.
3. **Substituição Atômica de Itens e Labels:**
   - Para evitar violação transitória de `UNIQUE(group_id, position)` ao reordenar itens dentro de `update`, a transação remove os itens removidos, atualiza ou reinsere os itens dentro do mesmo `Transaction` do `rusqlite` (DEFERRED/IMMEDIATE), garantindo que qualquer erro faça rollback integral do grupo, das labels e dos itens (`CA-03`).
   - Se um `CommandGroupItemInput` trouxer `id: Some(item_id)` que não pertence ao `group_id` sendo atualizado, a transação falha com `commands.group.validation`.
4. **Reordenação de Grupos (`reorder`):**
   - Exige que `entries` contenha **exatamente** o conjunto completo de grupos daquele `(scope, project_id)`, sem IDs duplicados, sem IDs faltantes e sem IDs de outro escopo/projeto (caso contrário retorna `commands.group.reorder_mismatch`).
   - Verifica `revision` de cada grupo; se qualquer revisão divergir, desfaz a transação e retorna `commands.group.revision_conflict`. Em caso de sucesso, atualiza `sort_order = index` e incrementa `revision = revision + 1` de cada grupo.
5. **Detecção Bloqueante de Segredos (`commands.group.secret_detected`):**
   - Ao validar `CommandGroupInput`, cada `item.command`, cada valor de `input.env` e cada valor de `item.env` é passado para `mt_db::sanitizer::sanitize()`.
   - Se o texto retornado diferir do original (ex.: continha `Bearer eyJ...`, `APP_KEY=base64:...`, `ghp_...`, senhas em URLs), o validador **bloqueia** a gravação e retorna `commands.group.secret_detected` com `details: { "fields": ["items[0].command", "env.API_TOKEN"] }`.
   - Referências como `$TOKEN` ou `${API_KEY}` não são alteradas pelo sanitizer e são aceitas normalmente.
   - Campos descritivos (`name`, `description`, `labels`, `item.title`) também passam pelo sanitizer.
6. **Confinamento de `cwd` e Symlinks (`CwdResolver`):**
   - Se `cwd` for informado, `validate_relative_cwd_syntax` garante na gravação que é um caminho relativo que não sobe acima da raiz (`../..`).
   - Além disso, quando `project_root` estiver disponível (e obrigatoriamente em cada lançamento via `CwdResolver::resolve_effective_cwd`), `project_root.canonicalize()` e `joined.canonicalize()` são comparados com `canonical_target.starts_with(&canonical_root)`. Se o alvo não existir, retorna `commands.group.cwd_not_found`; se um symlink apontar para fora da raiz do projeto, retorna `commands.group.cwd_outside_project`.
7. **Truncamento UTF-8 Seguro de `output_tail`:**
   - `output_tail` no SQLite armazena no máximo `262_144` bytes (256 KiB) dos bytes mais recentes já sanitizados. O corte no início da string respeita fronteiras válidas de codepoints UTF-8 (`str::is_char_boundary`) para jamais gerar string inválida ou entrar em pânico ao cortar caracteres multibyte em pt-BR.
