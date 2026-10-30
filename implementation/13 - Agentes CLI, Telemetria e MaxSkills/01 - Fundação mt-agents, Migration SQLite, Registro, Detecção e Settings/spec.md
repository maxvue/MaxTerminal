# Implementação 01 — Fundação `mt-agents`, Migration SQLite, Registro, Detecção e Settings (`spec.md`)

> Fase: **13 — Agentes CLI, Telemetria e MaxSkills**
> Referências: [`../instructions.md`](../instructions.md) (I-01, I-02, I-03, I-16; CA-01, CA-04, CA-05, CA-06, CA-07) e [`../spec.md`](../spec.md) (§1, §2.1–§2.5, §2.13, §3, §5, §7.4, §10).

---

## 1. Contexto, Objetivo e Escopo

Esta primeira unidade estabelece a fundação de backend da Fase 13 no workspace Rust (`src-tauri/`):

1. **Migration SQLite (`NNNN_13_agents_skills_memory.sql`)**: cria de forma atômica e com tabelas `STRICT` todas as 10 tabelas da Fase 13 (sementes e opções de templates de agentes, vínculos aba ↔ worktree, operações duráveis de lançamento, snapshots de telemetria, alvos e entradas de sincronização do MaxSkills, e tabelas de memória contextual de comandos com deduplicação at-least-once e tombstones).
2. **Crate `mt-agents`**: estrutura modular, catálogo completo de códigos de erro (`error.rs`) e portas de desacoplamento (`ports.rs`) que isolam `mt-agents` de Tauri, `mt-pty`, `mt-shell` e `mt-os`.
3. **Registro Declarativo dos 4 Agentes (`registry.rs`)**: descritores embutidos via `include_str!` para `claude-code` (binário `claude`), `codex`, `agy` e `opencode`, mesclados com overrides opcionais do usuário em `$XDG_CONFIG_HOME/maxterminal/agents.toml` (`#[serde(deny_unknown_fields)]`) sem derrubar o aplicativo caso o TOML seja inválido.
4. **Detecção Paralela e Matriz de Capacidades (`detect.rs`)**: captura do `PATH` real do *login shell* (`$SHELL -lic`), acréscimo dos diretórios extras (`~/.local/bin`, `~/.bun/bin`, `~/.npm-global/bin`, `~/.opencode/bin`), resolução dos binários e execução paralela de `--version` com timeout individual de 3 s (`kill_on_drop(true)`), além da construção do `CapabilityReport` verificável por versão/evidências (`S-01`..`S-14`).
5. **Configurações (`settings.rs`) e Comandos IPC Iniciais**: persistência de `agents.settings` e `skills.settings` no `SettingsRegistry` da Fase 03, validação rigorosa de sintaxe e colisão de atalhos (`agents.shortcut_invalid` / `agents.shortcut_conflict`) e exposição dos comandos Tauri `agent_list`, `agent_detect_refresh`, `agent_settings_get`, `agent_settings_update` e `agent_descriptors_reload`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Criados
- `src-tauri/crates/mt-db/migrations/0009_13_agents_skills_memory.sql` *(usar o próximo número sequencial livre em `mt-db/migrations/` sem lacunas)*
- `src-tauri/crates/mt-agents/Cargo.toml`
- `src-tauri/crates/mt-agents/src/lib.rs`
- `src-tauri/crates/mt-agents/src/error.rs`
- `src-tauri/crates/mt-agents/src/ports.rs`
- `src-tauri/crates/mt-agents/src/registry.rs`
- `src-tauri/crates/mt-agents/src/descriptors/claude.toml`
- `src-tauri/crates/mt-agents/src/descriptors/codex.toml`
- `src-tauri/crates/mt-agents/src/descriptors/agy.toml`
- `src-tauri/crates/mt-agents/src/descriptors/opencode.toml`
- `src-tauri/crates/mt-agents/src/detect.rs`
- `src-tauri/crates/mt-agents/src/settings.rs`
- `src-tauri/crates/mt-agents/tests/registry.rs`
- `src-tauri/crates/mt-agents/tests/detect.rs`
- `src-tauri/src/commands/agents.rs`

### 2.2. Arquivos Modificados
- `src-tauri/Cargo.toml` — inclusão de `crates/mt-agents` em `[workspace.members]` e `[workspace.dependencies]`.
- `src-tauri/src/commands/mod.rs` — exposição do submódulo `pub mod agents;`.
- `src-tauri/src/specta.rs` — registro inicial dos comandos `agent_list`, `agent_detect_refresh`, `agent_settings_get`, `agent_settings_update`, `agent_descriptors_reload` e eventos `agent://installations-changed`, `agent://settings-changed`.

---

## 3. Especificação Técnica Detalhada

### 3.1. Manifesto `src-tauri/crates/mt-agents/Cargo.toml`

```toml
[package]
name = "mt-agents"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"

[lib]
name = "mt_agents"
path = "src/lib.rs"

[dependencies]
mt-core = { path = "../mt-core" }
mt-db = { path = "../mt-db" }
mt-git = { path = "../mt-git" }
tokio = { workspace = true, features = ["rt", "macros", "time", "sync", "net", "process", "fs", "io-util"] }
tokio-util = { version = "0.7", features = ["rt"] }
async-trait = "0.1"
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
toml = "0.8"
specta = { workspace = true }
regex = "1"
strip-ansi-escapes = "0.2"
sha2 = "0.10"
subtle = "2"
rand = "0.8"
base64 = "0.22"
walkdir = "2"
notify = "6"
notify-debouncer-full = "0.3"
which = "6"
shell-words = "1"
dashmap = "6"
parking_lot = "0.12"
nix = { version = "0.29", features = ["user", "socket"] }
time = { workspace = true }
uuid = { workspace = true, features = ["v7"] }
tracing = { workspace = true }
thiserror = "1"

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true, features = ["full", "test-util"] }
proptest = "1"
```

---

### 3.2. Migration SQLite (`mt-db/migrations/NNNN_13_agents_skills_memory.sql`)

```sql
-- Fase 13: agentes CLI, isolamento, telemetria, MaxSkills e memória contextual

-- Templates dedicados já semeados por projeto (idempotência de agent_templates_ensure, respeitando exclusões do usuário)
CREATE TABLE agent_template_seeds (
    project_id  TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    agent_id    TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    template_id TEXT    NULL REFERENCES tab_templates(id) ON DELETE SET NULL,
    seeded_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, agent_id)
) STRICT;

-- Opções específicas de agente por template (environment_kind = 'agent')
CREATE TABLE agent_template_options (
    template_id       TEXT    PRIMARY KEY REFERENCES tab_templates(id) ON DELETE CASCADE,
    model             TEXT    NULL CHECK (model IS NULL OR length(model) BETWEEN 1 AND 128),
    extra_args_json   TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(extra_args_json) AND json_type(extra_args_json) = 'array'),
    isolation_default INTEGER NOT NULL DEFAULT 0 CHECK (isolation_default IN (0, 1)),
    location          TEXT    NOT NULL DEFAULT 'project' CHECK (location IN ('project', 'max_code')),
    updated_at        INTEGER NOT NULL
) STRICT;

-- Vínculo aba ↔ worktree (CA-12, CA-14, CA-15)
CREATE TABLE agent_tab_links (
    id             INTEGER PRIMARY KEY,
    tab_id         TEXT    NOT NULL,
    project_id     TEXT    NOT NULL,
    agent_id       TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    worktree_path  TEXT    NULL,
    branch         TEXT    NULL,
    base_ref       TEXT    NULL,
    created_by_app INTEGER NOT NULL DEFAULT 0 CHECK (created_by_app IN (0, 1)),
    opened_at      INTEGER NOT NULL,
    tab_closed_at  INTEGER NULL,
    resolution     TEXT    NULL CHECK (resolution IN ('keep', 'remove', 'remove_and_delete_branch')),
    resolved_at    INTEGER NULL,
    CHECK ((resolution IS NULL) = (resolved_at IS NULL)),
    CHECK (resolution IS NULL OR tab_closed_at IS NOT NULL),
    CHECK (resolution <> 'remove_and_delete_branch' OR created_by_app = 1),
    CHECK (worktree_path IS NOT NULL OR (branch IS NULL AND created_by_app = 0))
) STRICT;
CREATE UNIQUE INDEX ux_agent_links_open_tab ON agent_tab_links(tab_id) WHERE tab_closed_at IS NULL;
CREATE INDEX ix_agent_links_pending ON agent_tab_links(project_id)
    WHERE tab_closed_at IS NOT NULL AND resolution IS NULL AND worktree_path IS NOT NULL;

-- Operações duráveis para reconciliar Git/SQLite/PTY após crash; não guardar tokens ou argv secretos
CREATE TABLE agent_launch_operations (
    launch_id     TEXT    PRIMARY KEY,
    project_id    TEXT    NOT NULL,
    tab_id        TEXT    NULL,
    agent_id      TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    repo_path     TEXT    NOT NULL,
    worktree_path TEXT    NULL,
    branch        TEXT    NULL,
    base_oid      TEXT    NULL,
    stage         TEXT    NOT NULL CHECK (stage IN ('reserved', 'worktree_ready', 'opening', 'opened', 'recovery_required', 'resolved')),
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
) STRICT;
CREATE INDEX ix_agent_launch_pending ON agent_launch_operations(updated_at)
    WHERE stage IN ('reserved', 'worktree_ready', 'opening', 'recovery_required');

-- Último snapshot de telemetria por aba (restauração da barra após reinício; sem conteúdo de conversa)
CREATE TABLE agent_telemetry_snapshots (
    tab_id     TEXT    PRIMARY KEY REFERENCES tabs(id) ON DELETE CASCADE,
    agent_id   TEXT    NOT NULL,
    payload    TEXT    NOT NULL CHECK (json_valid(payload)),
    updated_at INTEGER NOT NULL
) STRICT;

-- Alvos de sincronização de MaxSkills
CREATE TABLE skill_sync_targets (
    id             INTEGER PRIMARY KEY,
    agent_id       TEXT    NOT NULL CHECK (agent_id IN ('claude-code', 'codex', 'agy', 'opencode')),
    scope          TEXT    NOT NULL CHECK (scope IN ('project', 'global')),
    project_id     TEXT    NULL REFERENCES projects(id) ON DELETE CASCADE,
    mode           TEXT    NOT NULL CHECK (mode IN ('symlink', 'copy')),
    auto_sync      INTEGER NOT NULL DEFAULT 0 CHECK (auto_sync IN (0, 1)),
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    selection_json TEXT    NOT NULL DEFAULT '{"kind":"recommended"}' CHECK (json_valid(selection_json)),
    dest_canonical TEXT    NOT NULL,
    project_root   TEXT    NULL,
    dest_override  TEXT    NULL,
    last_synced_at INTEGER NULL,
    created_at     INTEGER NOT NULL,
    CHECK ((scope = 'project') = (project_id IS NOT NULL))
) STRICT;
CREATE UNIQUE INDEX ux_skill_targets_dest ON skill_sync_targets(dest_canonical);

-- Entradas gerenciadas por alvo (fonte da verdade do que o app criou)
CREATE TABLE skill_sync_entries (
    target_id   INTEGER NOT NULL REFERENCES skill_sync_targets(id) ON DELETE CASCADE,
    skill_name  TEXT    NOT NULL,
    dest_path   TEXT    NOT NULL,
    mode        TEXT    NOT NULL CHECK (mode IN ('symlink', 'copy')),
    source_hash TEXT    NOT NULL CHECK (length(source_hash) = 64),
    source_path TEXT    NOT NULL,
    synced_hash TEXT    NOT NULL CHECK (length(synced_hash) = 64),
    synced_at   INTEGER NOT NULL,
    PRIMARY KEY (target_id, skill_name)
) STRICT;

-- Memória contextual de comandos recorrentes (texto já sanitizado — Fase 05 — e re-sanitizado no INSERT)
CREATE TABLE command_memory (
    project_id     TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    command_norm   TEXT    NOT NULL CHECK (length(command_norm) BETWEEN 1 AND 1024),
    command        TEXT    NOT NULL,
    use_count      INTEGER NOT NULL DEFAULT 0,
    success_count  INTEGER NOT NULL DEFAULT 0,
    failure_count  INTEGER NOT NULL DEFAULT 0,
    first_used_at  INTEGER NOT NULL,
    last_used_at   INTEGER NOT NULL,
    last_cwd       TEXT    NULL,
    last_exit_code INTEGER NULL,
    pinned         INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    hidden         INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    PRIMARY KEY (project_id, command_norm),
    CHECK (instr(command, '[REDACTED:') = 0)
) STRICT, WITHOUT ROWID;
CREATE INDEX ix_command_memory_recent ON command_memory(project_id, last_used_at DESC) WHERE hidden = 0;

-- Replay at-least-once: atualizar memória somente quando o bloco for inserido aqui na mesma transação
CREATE TABLE command_memory_processed (
    block_id     TEXT    PRIMARY KEY,
    project_id   TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    processed_at INTEGER NOT NULL
) STRICT;
CREATE INDEX ix_memory_processed_project ON command_memory_processed(project_id);

-- Preferências sobrevivem ao rebuild; tombstone impede que "Esquecer" ressuscite histórico antigo
CREATE TABLE command_memory_preferences (
    project_id   TEXT    NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    command_hash TEXT    NOT NULL CHECK (length(command_hash) = 64),
    pinned       INTEGER NOT NULL DEFAULT 0 CHECK (pinned IN (0, 1)),
    hidden       INTEGER NOT NULL DEFAULT 0 CHECK (hidden IN (0, 1)),
    forgotten    INTEGER NOT NULL DEFAULT 0 CHECK (forgotten IN (0, 1)),
    updated_at   INTEGER NOT NULL,
    PRIMARY KEY (project_id, command_hash)
) STRICT, WITHOUT ROWID;
```

---

### 3.3. Códigos de Erro (`crates/mt-agents/src/error.rs`)

```rust
use mt_core::{ErrorCode, MtError};
use serde_json::json;

pub mod codes {
    pub const NOT_INSTALLED: &str = "agents.not_installed";
    pub const UNKNOWN_AGENT: &str = "agents.unknown_agent";
    pub const SETTINGS_INVALID: &str = "agents.settings_invalid";
    pub const SHORTCUT_CONFLICT: &str = "agents.shortcut_conflict";
    pub const SHORTCUT_INVALID: &str = "agents.shortcut_invalid";
    pub const NOT_A_REPO: &str = "agents.not_a_repo";
    pub const WORKTREE_FOREIGN: &str = "agents.worktree_foreign";
    pub const WORKTREE_DIRTY: &str = "agents.worktree_dirty";
    pub const LINK_NOT_FOUND: &str = "agents.link_not_found";
    pub const LINK_RESOLVED: &str = "agents.link_already_resolved";
    pub const RELAY_UNAVAILABLE: &str = "agents.relay_unavailable";
    pub const TAB_NOT_AGENT: &str = "agents.tab_not_agent";
    pub const MODEL_UNSUPPORTED: &str = "agents.model_unsupported";
    pub const INSPECTION_STALE: &str = "agents.inspection_stale";
    pub const EXECUTION_IN_USE: &str = "agents.execution_in_use";
    pub const TEMPLATE_DELETED: &str = "agents.template_deleted";

    pub const SKILLS_SOURCE_MISSING: &str = "skills.source_missing";
    pub const SKILLS_INDEX_INVALID: &str = "skills.index_invalid";
    pub const SKILLS_PLAN_STALE: &str = "skills.plan_stale";
    pub const SKILLS_TARGET_INVALID: &str = "skills.target_invalid";
    pub const SKILLS_CAPABILITY_UNVERIFIED: &str = "skills.capability_unverified";
    pub const SKILLS_LIMIT_EXCEEDED: &str = "skills.limit_exceeded";
    pub const SKILLS_IO: &str = "skills.io";

    pub const MEMORY_NOT_FOUND: &str = "memory.not_found";
    pub const MEMORY_PIN_LIMIT: &str = "memory.pin_limit";
}

pub fn not_installed(agent_display: &str) -> MtError {
    MtError::new(
        ErrorCode::from(codes::NOT_INSTALLED),
        format!("Agente {agent_display} não encontrado no PATH")
    )
}

pub fn unknown_agent(raw: &str) -> MtError {
    MtError::with_details(
        ErrorCode::from(codes::UNKNOWN_AGENT),
        format!("Agente desconhecido: {raw}"),
        json!({ "agentId": raw })
    )
}

pub fn settings_invalid(message: impl Into<String>, line: Option<usize>, column: Option<usize>) -> MtError {
    let msg = message.into();
    MtError::with_details(
        ErrorCode::from(codes::SETTINGS_INVALID),
        format!("Configuração de agentes inválida: {msg}"),
        json!({ "line": line, "column": column, "message": msg })
    )
}

pub fn shortcut_invalid(shortcut: &str) -> MtError {
    MtError::with_details(
        ErrorCode::from(codes::SHORTCUT_INVALID),
        format!("Atalho de teclado inválido: {shortcut}"),
        json!({ "shortcut": shortcut })
    )
}

pub fn shortcut_conflict(shortcut: &str, conflicts_with: &str) -> MtError {
    MtError::with_details(
        ErrorCode::from(codes::SHORTCUT_CONFLICT),
        format!("O atalho '{shortcut}' conflita com '{conflicts_with}'"),
        json!({ "shortcut": shortcut, "conflictsWith": conflicts_with })
    )
}
```

---

### 3.4. Portas de Domínio (`crates/mt-agents/src/ports.rs`)

```rust
use std::path::{Path, PathBuf};
use async_trait::async_trait;
use mt_core::{MtResult, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId};
use mt_git::{GitContext, WorktreeCreateRequest, WorktreeEntry};
use serde::{Deserialize, Serialize};
use crate::registry::AgentKind;

/// Identidade imutável de uma execução de agente dentro de um painel/sessão.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunBinding {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub project_id: ProjectId,
    pub run_id: String,
    pub agent: AgentKind,
    pub cwd: PathBuf,
    pub started_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ForegroundProc {
    pub pid: u32,
    pub comm: String,
    pub exe: Option<PathBuf>,
    pub cmdline: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ProjectRecord {
    pub id: ProjectId,
    pub title: String,
    pub path: PathBuf,
    pub stack: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttentionKind {
    AwaitingApproval,
    AwaitingInput,
}

/// Porta para a máquina de estados da Fase 05 com suporte a overlay por episódio.
pub trait TabStatePort: Send + Sync {
    /// Aplica sobreposição `AttentionRequired` para `(run, episode)` e retorna `true` se o estado mudou.
    fn attention_required(&self, run: &RunBinding, episode: u64) -> bool;
    /// Remove apenas a sobreposição do episódio informado e recompõe o estado subjacente real do painel.
    fn attention_resolved(&self, tab: TabId, pane: PaneId, session: SessionId, run_id: &str, episode: u64);
}

/// Porta para o `AttentionRegistry` e `Notifier` da Fase 09.
#[async_trait]
pub trait NotifyPort: Send + Sync {
    fn set_attention(&self, tab: TabId, kind: AttentionKind);
    fn clear_attention(&self, tab: TabId);
    fn is_tab_focused_and_active(&self, tab: TabId) -> bool;
    async fn notify(&self, tab: TabId, title: &str, body: &str);
}

/// Porta para consulta de processo em foreground na Fase 02.
pub trait PtyPort: Send + Sync {
    fn foreground(&self, pty: PtyId) -> Option<ForegroundProc>;
    fn has_active_processes(&self, tab: TabId) -> bool;
}

/// Porta para operações de projeto, templates e abas (Fases 06 e 07).
#[async_trait]
pub trait WorkspacePort: Send + Sync {
    async fn project(&self, id: ProjectId) -> MtResult<ProjectRecord>;
    fn project_of_tab(&self, tab: TabId) -> Option<ProjectId>;
    fn tabs_using_cwd(&self, dir: &Path) -> Vec<TabId>;
    async fn ensure_agent_templates(&self, project: ProjectId) -> MtResult<u32>;
}

/// Porta para operações Git e Worktree (Fase 08).
#[async_trait]
pub trait GitPort: Send + Sync {
    async fn create_worktree(&self, req: WorktreeCreateRequest) -> MtResult<WorktreeEntry>;
    async fn remove_worktree(&self, repo_root: &Path, worktree_path: &Path, force: bool) -> MtResult<()>;
    async fn status(&self, cwd: &Path) -> MtResult<Option<GitContext>>;
    async fn run(&self, cwd: &Path, args: &[&str]) -> MtResult<String>;
}
```

---

### 3.5. Registro e Descritores (`crates/mt-agents/src/registry.rs` e `descriptors/*.toml`)

```rust
use std::collections::BTreeMap;
use std::sync::Arc;
use mt_core::{AppPaths, MtError, MtResult};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use crate::error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
pub enum AgentKind {
    #[serde(rename = "claude-code")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "agy")]
    Agy,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl AgentKind {
    pub const ALL: [AgentKind; 4] = [Self::Claude, Self::Codex, Self::Agy, Self::OpenCode];

    pub fn as_ref_id(&self) -> &'static str {
        match self {
            Self::Claude => "claude-code",
            Self::Codex => "codex",
            Self::Agy => "agy",
            Self::OpenCode => "opencode",
        }
    }

    pub fn from_ref_str(s: &str) -> MtResult<Self> {
        match s {
            "claude-code" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "agy" => Ok(Self::Agy),
            "opencode" => Ok(Self::OpenCode),
            other => Err(error::unknown_agent(other)),
        }
    }

    pub fn short(&self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Agy => "agy",
            Self::OpenCode => "opencode",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub statusline_hook: bool,
    pub native_attention_hooks: bool,
    pub session_files: bool,
    pub osc_notifications: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentDescriptor {
    pub kind: AgentKind,
    pub display_name: String,
    pub icon: String,
    pub color_token: String,
    pub binary: String,
    pub process_names: Vec<String>,
    pub version_args: Vec<String>,
    pub version_regex: String,
    pub default_args: Vec<String>,
    #[serde(default)]
    pub extra_args: Vec<String>,
    pub model_flag: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub shortcut: String,
    pub capabilities: AgentCapabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum CapabilitySupport {
    Available,
    Unsupported,
    Unverified,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityEvidence {
    pub support: CapabilitySupport,
    pub source: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityReport {
    pub agent: AgentKind,
    pub binary_version: Option<String>,
    pub adapter_revision: String,
    pub checked_at_ms: i64,
    pub features: BTreeMap<String, CapabilityEvidence>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentUserOverride {
    pub binary: Option<String>,
    pub extra_args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub shortcut: Option<String>,
    pub model_flag: Option<Option<String>>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserAgentsToml {
    pub claude: Option<AgentUserOverride>,
    pub codex: Option<AgentUserOverride>,
    pub agy: Option<AgentUserOverride>,
    pub opencode: Option<AgentUserOverride>,
}

pub struct AgentRegistry {
    descriptors: RwLock<Arc<[AgentDescriptor; 4]>>,
    enabled: RwLock<BTreeMap<AgentKind, bool>>,
}

impl AgentRegistry {
    pub fn load(paths: &AppPaths) -> (Self, Option<MtError>);
    pub fn reload(&self, paths: &AppPaths) -> MtResult<()>;
    pub fn descriptors(&self) -> Arc<[AgentDescriptor; 4]>;
    pub fn get(&self, kind: AgentKind) -> AgentDescriptor;
    pub fn is_enabled(&self, kind: AgentKind) -> bool;
}
```

#### Arquivos `descriptors/*.toml`:
- `descriptors/claude.toml`:
  - `kind = "claude-code"`, `display_name = "Claude Code"`, `icon = "simple-icons:claude"`, `color_token = "--max-warning-500"`, `binary = "claude"`, `process_names = ["claude", "node"]`, `version_args = ["--version"]`, `version_regex = '(\d+\.\d+\.\d+)'`, `default_args = []`, `model_flag = "--model"`, `shortcut = "Ctrl+Alt+C"`, `capabilities = { statusline_hook = true, native_attention_hooks = true, session_files = true, osc_notifications = false }`.
- `descriptors/codex.toml`:
  - `kind = "codex"`, `display_name = "Codex"`, `icon = "simple-icons:openai"`, `color_token = "--max-success-500"`, `binary = "codex"`, `process_names = ["codex", "node"]`, `version_args = ["--version"]`, `version_regex = '(\d+\.\d+\.\d+)'`, `default_args = []`, `model_flag = "--model"`, `shortcut = "Ctrl+Alt+X"`, `capabilities = { statusline_hook = false, native_attention_hooks = false, session_files = false, osc_notifications = true }`.
- `descriptors/agy.toml`:
  - `kind = "agy"`, `display_name = "Antigravity (agy)"`, `icon = "simple-icons:google"`, `color_token = "--max-info-500"`, `binary = "agy"`, `process_names = ["agy", "antigravity", "node"]`, `version_args = ["--version"]`, `version_regex = '(\d+\.\d+\.\d+)'`, `default_args = []`, `shortcut = "Ctrl+Alt+A"`, `capabilities = { statusline_hook = false, native_attention_hooks = false, session_files = false, osc_notifications = false }`. *(sem `model_flag`)*
- `descriptors/opencode.toml`:
  - `kind = "opencode"`, `display_name = "OpenCode"`, `icon = "mdi:code-braces"`, `color_token = "--max-primary-500"`, `binary = "opencode"`, `process_names = ["opencode", "bun", "node"]`, `version_args = ["--version"]`, `version_regex = '(\d+\.\d+\.\d+)'`, `default_args = []`, `shortcut = "Ctrl+Alt+O"`, `capabilities = { statusline_hook = false, native_attention_hooks = false, session_files = false, osc_notifications = false }`. *(sem `model_flag`)*

---

### 3.6. Detecção e Relatório de Capacidades (`crates/mt-agents/src/detect.rs`)

```rust
use std::path::PathBuf;
use std::time::Duration;
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use crate::registry::{AgentDescriptor, AgentKind, AgentRegistry, CapabilityReport};
use crate::settings::AgentsSettings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentInstallation {
    pub kind: AgentKind,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub reason: Option<String>,
    pub checked_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub descriptor: AgentDescriptor,
    pub installation: AgentInstallation,
    pub enabled: bool,
    pub capabilities: CapabilityReport,
}

pub struct Detector {
    login_path: OnceCell<Vec<PathBuf>>,
    cache: DashMap<AgentKind, AgentInstallation>,
}

impl Detector {
    pub const VERSION_TIMEOUT: Duration = Duration::from_secs(3);
    pub const SHELL_PATH_TIMEOUT: Duration = Duration::from_secs(2);
    pub const EXTRA_DIRS: [&'static str; 4] = [
        "~/.local/bin",
        "~/.bun/bin",
        "~/.npm-global/bin",
        "~/.opencode/bin",
    ];

    pub fn new() -> Self;
    pub async fn login_path(&self) -> &[PathBuf];
    pub async fn detect_all(&self, reg: &AgentRegistry) -> Vec<AgentInstallation>;
    pub fn cached(&self, kind: AgentKind) -> Option<AgentInstallation>;
    pub fn build_capability_report(
        descriptor: &AgentDescriptor,
        installation: &AgentInstallation,
        settings: &AgentsSettings,
    ) -> CapabilityReport;
    pub fn list_agent_info(&self, reg: &AgentRegistry, settings: &AgentsSettings) -> Vec<AgentInfo>;
}
```

#### Regras de Negócio da Detecção e `CapabilityReport`:
- `login_path`: executa `$SHELL -lic 'printf %s "$PATH"'` com `stdin(Stdio::null())`, `env("MAXTERMINAL_PATH_PROBE", "1")`, `kill_on_drop(true)` e timeout de 2 s; mescla com `std::env::var_os("PATH")` e com `EXTRA_DIRS` expandidos para `$HOME` (ou `MAXCODE_REAL_HOME` se definido), preservando a ordem e removendo duplicatas.
- Se `descriptor.binary` contiver `/`, valida `Path::is_file` e permissão de execução (`mode & 0o111 != 0`); caso contrário, resolve via `which::which_in(&descriptor.binary, Some(joined_path), ".")`.
- Se o binário não existir: `installed = false`, `path = None`, `version = None`, `reason = Some("Binário não encontrado no PATH".into())`.
- Se existir: executa `<path> <version_args...>` em paralelo (`futures::future::join_all` / `tokio::join!`) com `kill_on_drop(true)` e `tokio::time::timeout(Duration::from_secs(3), ...)`.
  - Timeout → `installed = false`, `path = Some(...)`, `version = None`, `reason = Some("Tempo esgotado ao executar --version".into())`.
  - Exit != 0 → `installed = false`, `reason = Some(format!("Falha ao executar --version (código {})", code))`.
  - Sucesso → extrai `version` via `descriptor.version_regex` sobre `stdout + stderr`; se a regex não casar, `version = None`, mas `installed = true`.
- `build_capability_report` gera sempre as 9 chaves canônicas em `features`:
  `launch`, `modelSelection`, `modelTelemetry`, `contextTelemetry`, `quota5h`, `quotaWeekly`, `attention`, `skillsProject`, `skillsGlobal`, respeitando rigorosamente a matriz S-01..S-14 (`spec.md` §4 e §7.4):
  - Quando `!installation.installed`: `launch` é `Unsupported` ("Binário não instalado"), e demais chaves refletem a capacidade declarada ou `Unsupported`.
  - Para `Claude`: `modelSelection`, `modelTelemetry`, `contextTelemetry`, `quota5h`, `quotaWeekly`, `attention`, `skillsProject`, `skillsGlobal` são `Available` (ou `Disabled` se `settings.inject[Claude] == false` ou `settings.telemetry.enabled == false`).
  - Para `Codex`: `modelSelection` (`Available`), `modelTelemetry` (`Available` via configuração rotulada), `contextTelemetry` / `quota5h` / `quotaWeekly` (`Unverified` até sessão rollout validada), `attention` (`Unverified` — `notify` é apenas fim de turno e padrões/OSC são heurísticos), `skillsProject` / `skillsGlobal` (`Available` em `.agents/skills`).
  - Para `Agy`: `launch` (`Available` se instalado), `modelSelection` / `modelTelemetry` / `contextTelemetry` / `quota5h` / `quotaWeekly` / `attention` / `skillsProject` / `skillsGlobal` (`Unverified` com razão pt-BR "Sem contrato documentado validado para esta versão do CLI").
  - Para `OpenCode`: `launch` (`Available` se instalado), `skillsProject` / `skillsGlobal` (`Available` em `.opencode/skills` e `$XDG_CONFIG_HOME/opencode/skills`), e `modelSelection` / `modelTelemetry` / `contextTelemetry` / `quota5h` / `quotaWeekly` / `attention` (`Unverified`).

---

### 3.7. Configurações (`crates/mt-agents/src/settings.rs`)

```rust
use std::collections::{BTreeMap, HashSet};
use mt_core::MtResult;
use serde::{Deserialize, Serialize};
use crate::error;
use crate::registry::{AgentKind, AgentRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeLocationDto {
    Project,
    MaxCode,
}

impl Default for WorktreeLocationDto {
    fn default() -> Self {
        Self::Project
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct IsolationSettings {
    pub default_location: WorktreeLocationDto,
    pub branch_prefix: String,
    pub open_isolated_by_default: bool,
}

impl Default for IsolationSettings {
    fn default() -> Self {
        Self {
            default_location: WorktreeLocationDto::Project,
            branch_prefix: "agent".to_string(),
            open_isolated_by_default: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AttentionLevel {
    Notify,
    Ignore,
}

impl Default for AttentionLevel {
    fn default() -> Self {
        Self::Ignore
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AttentionSettings {
    pub enabled: bool,
    pub on_turn_complete: AttentionLevel,
    pub silence_ms: u32,
    pub notify: bool,
}

impl Default for AttentionSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            on_turn_complete: AttentionLevel::Ignore,
            silence_ms: 300,
            notify: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct TelemetrySettings {
    pub enabled: bool,
    pub stale_after_s: u32,
    pub read_session_files: bool,
}

impl Default for TelemetrySettings {
    fn default() -> Self {
        Self {
            enabled: true,
            stale_after_s: 60,
            read_session_files: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", default)]
pub struct AgentsSettings {
    pub shortcuts_enabled: bool,
    pub shortcuts: BTreeMap<AgentKind, String>,
    pub isolation: IsolationSettings,
    pub attention: AttentionSettings,
    pub telemetry: TelemetrySettings,
    pub inject: BTreeMap<AgentKind, bool>,
}

impl Default for AgentsSettings {
    fn default() -> Self {
        let mut inject = BTreeMap::new();
        inject.insert(AgentKind::Claude, true);
        inject.insert(AgentKind::Codex, true);
        inject.insert(AgentKind::Agy, false);
        inject.insert(AgentKind::OpenCode, false);
        Self {
            shortcuts_enabled: true,
            shortcuts: BTreeMap::new(),
            isolation: IsolationSettings::default(),
            attention: AttentionSettings::default(),
            telemetry: TelemetrySettings::default(),
            inject,
        }
    }
}

/// Normaliza um atalho para a forma canônica `Ctrl+Alt+Shift+Super+<KEY>` e valida a regex
/// `^(Ctrl\+)?(Alt\+)?(Shift\+)?(Super\+)?[A-Z0-9]$` com pelo menos 1 modificador.
pub fn normalize_and_validate_shortcut(raw: &str) -> MtResult<String>;

/// Valida `AgentsSettings` contra atalhos inválidos, colisões entre agentes (incluindo variante `+Shift`)
/// e colisões com atalhos reservados das Fases 04/07/09 (`reserved_shortcuts`).
pub fn validate_settings(
    settings: &AgentsSettings,
    reg: &AgentRegistry,
    reserved_shortcuts: &HashSet<String>,
) -> MtResult<AgentsSettings>;
```

---

## 4. Casos de Borda e Tratamento de Erros

1. **`agents.toml` corrompido ou com chave desconhecida (`#[serde(deny_unknown_fields)]`)**:
   - Na inicialização (`AgentRegistry::load`), o erro `agents.settings_invalid` é retornado ao lado da instância com os 4 descritores embutidos intactos (nunca causa `panic` nem impede o app de abrir — CA-07).
   - Em `agent_descriptors_reload`, se o arquivo for inválido, retorna `Err(MtError)` com código `agents.settings_invalid` (incluindo `line` e `column` do `toml::de::Error`) e mantém os descritores anteriores em memória.
2. **Binário que trava ou demora em `--version`**:
   - Cada processo filho é criado com `kill_on_drop(true)` e envolvido em `tokio::time::timeout(Duration::from_secs(3), ...)`. Um agente travado termina em ≤ 3,05 s sem bloquear os outros 3 agentes, pois `detect_all` roda os 4 em paralelo (CA-06).
3. **Colisão com variante `Shift`**:
   - Como `<Atalho>+Shift` é reservado para abrir o agente isolado em worktree (ex.: `Ctrl+Alt+C` implica `Ctrl+Alt+Shift+C`), configurar um agente com `Ctrl+Alt+Shift+C` quando outro usa `Ctrl+Alt+C` deve ser rejeitado com `agents.shortcut_conflict` (CA-11).
4. **Proibição de `unsafe` e IO bloqueante em runtime async**:
   - `#![forbid(unsafe_code)]` em `crates/mt-agents/src/lib.rs` (CA-04).
