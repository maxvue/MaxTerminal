# Implementação 04 — Sincronizador MaxSkills e Memória Contextual de Comandos (`spec.md`)

> Fase: **13 — Agentes CLI, Telemetria e MaxSkills**
> Referências: [`../instructions.md`](../instructions.md) (I-13, I-14, I-17; CA-02, CA-26, CA-27, CA-28, CA-29, CA-30, CA-31, CA-32, CA-33, CA-35) e [`../spec.md`](../spec.md) (§2.11, §2.12, §2.13, §3, §5, §7.5, §7.6, §9).

---

## 1. Contexto, Objetivo e Escopo

Esta quarta unidade completa o backend da Fase 13 implementando a sincronização local de skills do repositório `~/GitHub/MaxSkills` para os diretórios de cada agente CLI e o motor de memória contextual de comandos recorrentes no SQLite:

1. **Catálogo e Sincronizador `MaxSkills` (`crates/mt-agents/src/skills/`)**:
   - Descoberta a partir de `index.json` (quando presente e com schema/versão suportados) com fallback automático para varredura recursiva de `**/SKILL.md` (ignorando `.git`, `node_modules` e pastas de worktrees), respeitando limites de segurança (profundidade ≤ 12, ≤ 10 000 arquivos e ≤ 64 MiB por skill — `skills.limit_exceeded`).
   - Cálculo de hash determinístico SHA-256 da árvore da skill e classificação de relevância (`Recommended` vs `Neutral`) cruzando `stacks` com o `StackKind` do projeto (Fase 06).
   - Resolução canônica de destinos por agente e escopo (`Project` vs `Global`), suportando `project_root` de worktree isolada, exigindo `dest_override` para `agy`, recusando symlinks ancestrais que escapem do escopo sem autorização explícita e deduplicando alvos por `dest_canonical` (CA-35).
   - Planejamento *dry-run* puro (`skills_plan`) que não realiza nenhuma escrita em disco (CA-27), classifica conflitos (`ForeignSymlink`, `Unmanaged`, `LocallyModified`), identifica `Prune` e retém o plano em memória por 5 min com `fingerprint`.
   - Aplicação atômica (`skills_apply`) validando o `fingerprint` sob lock do destino (`skills.plan_stale` se mudou), operando por staging irmão + rename atômico, gravando `.maxterminal-skill.json` apenas em modo `copy` (nunca dentro de symlink — CA-28), gerando backup completo em `$XDG_STATE_HOME/maxterminal/skills-backup/` antes de `Overwrite` (CA-29) e reconciliando operações interrompidas.
    - Monitoramento em tempo real (`watch.rs`) com `notify-debouncer-full` 0.5 e `notify` 8 herdados via `workspace = true` (debounce de 500 ms), emitindo `skills://changed` em ≤ 2 s e executando `auto_sync` somente para ações sem conflito (CA-30, E-10).
2. **Memória Contextual de Comandos (`crates/mt-agents/src/memory.rs`)**:
   - Consumo incremental e idempotente (`at-least-once` via `command_memory_processed`) dos blocos concluídos e persistidos na tabela `command_blocks` da Fase 05, lidos por `CommandBlockId` (com retry curto caso o evento broadcast chegue antes do commit e reconciliação de blocos pendentes no startup).
   - Normalização rigorosa (`CommandMemory::normalize`) que re-sanitiza com `mt_db::sanitizer::sanitize()`, rejeita comandos com `[REDACTED:...]`, comandos iniciados por espaço, multiline, caracteres de controle/ANSI, comprimento > 1024 ou comandos gerados internamente pelo launcher/relay, preservando espaços internos e aspas (`echo 'a  b'` ≠ `echo 'a b'` — CA-31, CA-33).
   - Cálculo de *frecency* `score`, busca literal por prefixo (`LIKE ... ESCAPE '\'`), ações `pin` (máx. 100 por projeto → `memory.pin_limit`), `hide`, `forget` (com tombstone SHA-256 em `command_memory_preferences` que impede ressurreição futura) e `rebuild` transacional que produz exatamente o mesmo estado do fluxo incremental (CA-32, CA-33).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Arquivos Criados
- `src-tauri/crates/mt-agents/src/skills/mod.rs`
- `src-tauri/crates/mt-agents/src/skills/catalog.rs`
- `src-tauri/crates/mt-agents/src/skills/targets.rs`
- `src-tauri/crates/mt-agents/src/skills/conflict.rs`
- `src-tauri/crates/mt-agents/src/skills/sync.rs`
- `src-tauri/crates/mt-agents/src/skills/watch.rs`
- `src-tauri/crates/mt-agents/src/memory.rs`
- `src-tauri/src/commands/skills.rs`
- `src-tauri/src/commands/memory.rs`
- `src-tauri/crates/mt-agents/tests/skills_sync.rs`
- `src-tauri/crates/mt-agents/tests/memory.rs`

### 2.2. Arquivos Modificados
- `src-tauri/crates/mt-agents/src/lib.rs` — conclusão da fachada `AgentService` integrando `skills: Arc<SkillsService>` e `memory: Arc<CommandMemory>`.
- `src-tauri/src/commands/mod.rs` — exposição de `pub mod skills;` e `pub mod memory;`.
- `src-tauri/src/specta.rs` e `src-tauri/src/lib.rs` — registro dos comandos de skills e memória, dos eventos `skills://changed`, `skills://sync-completed` e `memory://updated`, e assinatura do `ShellService::subscribe()` (`CommandEvent::Finished`).

---

## 3. Especificação Técnica Detalhada

### 3.1. Catálogo, Alvos, Conflitos e Sincronização (`crates/mt-agents/src/skills/`)

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use mt_core::{MtResult, ProjectId};
use serde::{Deserialize, Serialize};
use crate::ports::ProjectRecord;
use crate::registry::AgentKind;

pub const SKILL_MANIFEST_FILENAME: &str = ".maxterminal-skill.json";
pub const MAX_SKILL_DEPTH: usize = 12;
pub const MAX_SKILL_FILES: usize = 10_000;
pub const MAX_SKILL_BYTES: u64 = 64 * 1024 * 1024; // 64 MiB
pub const PLAN_TTL_MS: i64 = 5 * 60 * 1000;        // 5 minutos

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SkillRelevance {
    Recommended,
    Neutral,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub source_dir: String,
    pub tags: Vec<String>,
    pub stacks: Vec<String>,
    pub hash: String,
    pub relevance: SkillRelevance,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillCatalog {
    pub source_dir: String,
    pub from_index: bool,
    pub skills: Vec<SkillEntry>,
    pub scanned_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SyncScope {
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SyncMode {
    Symlink,
    Copy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SkillSelection {
    Recommended,
    All,
    Explicit { names: Vec<String> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SkillSyncTarget {
    pub id: Option<i64>,
    pub agent: AgentKind,
    pub scope: SyncScope,
    pub project_id: Option<ProjectId>,
    pub mode: SyncMode,
    pub auto_sync: bool,
    pub enabled: bool,
    pub selection: SkillSelection,
    pub project_root: Option<String>,
    pub dest_override: Option<String>,
    pub dest_dir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConflictKind {
    ForeignSymlink { target: String },
    Unmanaged,
    LocallyModified { expected: String, actual: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "action", rename_all = "camelCase")]
pub enum SyncAction {
    Create { skill: String, dest: String },
    Update { skill: String, dest: String },
    Unchanged { skill: String, dest: String },
    Conflict { skill: String, dest: String, conflict: ConflictKind },
    Prune { skill: String, dest: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    pub plan_id: String,
    pub target_id: i64,
    pub dest_dir: String,
    pub actions: Vec<SyncAction>,
    pub warnings: Vec<String>,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    Keep,
    Overwrite,
    Adopt,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub target_id: i64,
    pub applied: u32,
    pub skipped: u32,
    pub backups: Vec<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCopyManifest {
    pub version: u8,
    pub skill_name: String,
    pub source_path: String,
    pub source_hash: String,
    pub synced_hash: String,
    pub operation_id: String,
    pub synced_at_ms: i64,
}

pub fn agent_skill_dir(
    agent: AgentKind,
    scope: SyncScope,
    project_root: Option<&Path>,
    dest_override: Option<&Path>,
    home: &Path,
    xdg_config_home: &Path,
) -> MtResult<PathBuf>;

pub fn compute_skill_tree_hash(dir: &Path) -> MtResult<String>;
```

#### Regras de Negócio de `MaxSkills` (`spec.md` §7.5):
1. **Hash Determinístico da Árvore (`compute_skill_tree_hash`)**:
   - Percorre a pasta da skill ignorando apenas `.maxterminal-skill.json` e `.DS_Store`.
   - Rejeita imediatamente se a profundidade exceder 12 níveis, mais de 10 000 arquivos ou total > 64 MiB (`skills.limit_exceeded`), ou se encontrar symlink que aponte para fora da pasta da skill / loop cíclico (`skills.io`).
   - Ordena todas as entradas por caminho relativo UTF-8 (`BTreeMap`), alimentando o `Sha256` com: `rel_path + "\0" + kind ("F"|"D"|"L") + "\0" + exec_bit ("1"|"0") + "\0" + len + "\0" + bytes`.
2. **Resolução de Diretório-Alvo (`targets.rs`)**:
   - `Claude`: projeto `<root>/.claude/skills`, global `<home>/.claude/skills`.
   - `Codex`: projeto `<root>/.agents/skills`, global `<home>/.agents/skills`.
   - `OpenCode`: projeto `<root>/.opencode/skills`, global `<xdg_config_home>/opencode/skills`.
   - `Agy`: exige `dest_override` explícito; sem ele retorna `Err(skills.capability_unverified)`.
   - Quando `scope == SyncScope::Project`, valida que `project_root` (se fornecido, como o caminho de uma worktree) pertence ao mesmo repositório Git do `project_id` (via `mt_git::discover`). Inspeciona cada componente ancestral com `symlink_metadata`: se algum ancestral dentro do projeto for um symlink apontando para fora de `project_root`, recusa com `skills.target_invalid` a menos que o usuário tenha cadastrado `dest_override` explícito (CA-35).
3. **Classificação de Conflitos (`conflict.rs`)**:
   - Para cada skill selecionada e cada entrada existente no `dest_dir`:
     - Se `dest_skill_path` é um symlink:
       - Se aponta exatamente para `skill.source_dir`: `SyncAction::Unchanged` (se já consta em `skill_sync_entries` em modo `Symlink`) ou `ConflictKind::ForeignSymlink` (passível de `Adopt`).
       - Se aponta para outro caminho (ex.: `~/.max-code/skills/x`): `ConflictKind::ForeignSymlink { target }` (CA-29).
     - Se `dest_skill_path` é um diretório regular:
       - Se não possui `.maxterminal-skill.json` **e** não consta em `skill_sync_entries`: `ConflictKind::Unmanaged`.
       - Se possui `.maxterminal-skill.json` (ou consta em `skill_sync_entries`): calcula `actual_hash = compute_skill_tree_hash(&dest_skill_path)`:
         - Se `actual_hash != entry.synced_hash`: `ConflictKind::LocallyModified { expected: entry.synced_hash, actual: actual_hash }` (CA-29).
         - Se `actual_hash == entry.synced_hash`: se `skill.hash == actual_hash` e `mode == Copy`, `Unchanged`; caso contrário `Update`.
   - Entradas presentes em `skill_sync_entries` que não estão mais na seleção atual:
     - Se não foram modificadas localmente: `SyncAction::Prune`.
     - Se foram modificadas localmente: `SyncAction::Conflict { conflict: LocallyModified }` (prune nunca apaga edição local — CA-35).
4. **Execução Atômica (`skills_apply`) e Backups**:
   - Revalida o `fingerprint` do plano antes de escrever; se o estado da fonte ou do destino mudou desde `skills_plan`, retorna `Err(skills.plan_stale)` (CA-35).
   - Para `Conflict`:
     - `Keep` (ou sem resolução informada): incrementa `skipped` e não altera o item.
     - `Adopt`: permitido somente se o symlink já aponta para `skill.source_dir` ou se o hash do diretório existente é idêntico a `skill.hash`; registra em `skill_sync_entries` sem reescrever arquivos. Se o conteúdo diferir, rejeita com erro no item.
     - `Overwrite`: move/copia primeiro o item existente para `$XDG_STATE_HOME/maxterminal/skills-backup/<timestamp>-<target_id>-<skill_name>` (se o backup falhar, aborta aquele item sem tocar no destino — CA-29).
   - Modo `Copy`: copia para diretório irmão temporário `.staging-<skill>-<uuid>` dentro de `dest_dir`, grava `.maxterminal-skill.json`, verifica o hash SHA-256 e faz `fs::rename` atômico.
   - Modo `Symlink`: cria symlink temporário `.staging-<skill>-<uuid>` apontando para o caminho absoluto de `skill.source_dir` e faz `fs::rename` atômico (jamais escreve `.maxterminal-skill.json` dentro de symlink — CA-28).

---

### 3.2. Memória Contextual de Comandos (`crates/mt-agents/src/memory.rs`)

```rust
use std::sync::Arc;
use mt_core::{CommandBlockId, MtResult, ProjectId, TabId};
use serde::{Deserialize, Serialize};
use crate::ports::WorkspacePort;

pub const MAX_MEMORY_ENTRIES_PER_PROJECT: u32 = 2_000;
pub const MAX_PINNED_PER_PROJECT: u32 = 100;
pub const PRUNE_INTERVAL_INSERTIONS: u32 = 200;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryEntry {
    pub project_id: ProjectId,
    pub command: String,
    pub use_count: u32,
    pub success_count: u32,
    pub failure_count: u32,
    pub last_used_at_ms: i64,
    pub last_cwd: Option<String>,
    pub last_exit_code: Option<i32>,
    pub pinned: bool,
    pub hidden: bool,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MemoryQuery {
    pub project_id: ProjectId,
    pub prefix: Option<String>,
    pub limit: u16,
    pub include_hidden: bool,
    pub cwd: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PersistedCommandBlock {
    pub block_id: CommandBlockId,
    pub project_id: ProjectId,
    pub tab_id: TabId,
    pub command: String,
    pub cwd: Option<String>,
    pub exit_code: Option<i32>,
    pub started_at_ms: i64,
    pub ended_at_ms: i64,
    pub is_agent_launcher: bool,
}

pub struct CommandMemory {
    db: mt_db::Db,
    workspace: Arc<dyn WorkspacePort>,
}

impl CommandMemory {
    pub fn new(db: mt_db::Db, workspace: Arc<dyn WorkspacePort>) -> Self;

    /// Normaliza um comando para indexação na memória contextual.
    /// Retorna `None` se não memorizável (vazio, iniciado por espaço, multiline, controle/ANSI,
    /// > 1024 caracteres ou contendo `[REDACTED:`).
    pub fn normalize(raw_cmd: &str) -> Option<String>;

    /// Calcula o hash SHA-256 hex de `command_norm` para indexação em `command_memory_preferences`.
    pub fn command_hash(command_norm: &str) -> String;

    /// Calcula o score de frecency:
    /// `(pinned ? 1000.0 : 0.0) + ln(1.0 + use_count as f32) * exp(-age_days / 14.0) + (same_cwd ? 0.25 : 0.0)`
    pub fn score(entry: &MemoryEntry, query_cwd: Option<&str>, now_ms: i64) -> f32;

    pub async fn process_block(&self, block: &PersistedCommandBlock, now_ms: i64) -> MtResult<bool>;
    pub async fn on_command_finished(&self, block_id: CommandBlockId, now_ms: i64) -> MtResult<bool>;
    pub async fn reconcile_unprocessed_on_startup(&self, now_ms: i64) -> MtResult<u32>;
    pub async fn suggestions(&self, query: &MemoryQuery, now_ms: i64) -> MtResult<Vec<MemoryEntry>>;
    pub async fn list(&self, project_id: ProjectId, include_hidden: bool, limit: u16, now_ms: i64) -> MtResult<Vec<MemoryEntry>>;
    pub async fn set_pinned(&self, project_id: ProjectId, command: &str, pinned: bool, now_ms: i64) -> MtResult<MemoryEntry>;
    pub async fn set_hidden(&self, project_id: ProjectId, command: &str, hidden: bool, now_ms: i64) -> MtResult<MemoryEntry>;
    pub async fn forget(&self, project_id: ProjectId, command: &str, now_ms: i64) -> MtResult<bool>;
    pub async fn rebuild(&self, project_id: Option<ProjectId>, now_ms: i64) -> MtResult<u32>;
}
```

#### Regras de Negócio da Memória Contextual (`spec.md` §7.6):
1. **Privacidade e Normalização (`normalize`)**:
   - Se `raw_cmd` começa com espaço (`raw_cmd.starts_with(' ')` ou `'\t'`), retorna `None` (convenção `HISTCONTROL=ignorespace`).
   - Passa por `mt_db::sanitizer::sanitize(raw_cmd)`. Se o resultado contiver `[REDACTED:`, retorna `None` (CA-31).
   - Se contiver `\n`, `\r`, `\0` ou qualquer caractere de controle (`c.is_control()`), retorna `None`.
   - Faz `trim()` apenas nas extremidades externas, preservando integralmente espaços internos e aspas (`echo 'a  b'` e `echo 'a b'` geram `command_norm` distintos — CA-33).
   - Se `trimmed.is_empty()` ou `trimmed.len() > 1024`, retorna `None`.
2. **Transação Idempotente (`process_block`)**:
   - Na mesma transação SQLite (`Db::write`):
     1. Executa `INSERT OR IGNORE INTO command_memory_processed (block_id, project_id, processed_at) VALUES (?1, ?2, ?3)`. Se `changes() == 0`, o bloco já foi processado → retorna `Ok(false)` sem incrementar contadores (CA-33).
     2. Se `block.is_agent_launcher` ou `normalize(&block.command)` for `None`, conclui a transação retornando `Ok(false)`.
     3. Calcula `cmd_hash = command_hash(&cmd_norm)` e consulta `command_memory_preferences`: se `forgotten == 1`, não insere em `command_memory` e retorna `Ok(false)` (CA-33).
     4. Faz UPSERT em `command_memory`:
        - `use_count = use_count + 1`
        - `success_count = success_count + (exit_code == Some(0) ? 1 : 0)`
        - `failure_count = failure_count + (matches!(exit_code, Some(c) if c != 0) ? 1 : 0)` *(quando `exit_code == None`, não soma nem em sucesso nem em falha)*
        - Atualiza `command`, `last_used_at`, `last_cwd` e `last_exit_code` somente se `(block.ended_at_ms, &block.block_id)` for maior ou igual ao registro atual (desempate determinístico contra eventos fora de ordem — CA-32, CA-33).
        - Aplica `pinned` e `hidden` salvos em `command_memory_preferences`.
3. **Equivalência Estrita do `rebuild` (CA-32)**:
   - `rebuild` lê todos os blocos concluídos retidos em `command_blocks` (ordenados por `ended_at ASC, id ASC`), limpa `command_memory` e `command_memory_processed` do(s) projeto(s) alvo dentro de uma única transação writer e reaplica exatamente a mesma lógica de agregação + preferências/tombstones de `command_memory_preferences`, produzindo estado idêntico ao incremental.
4. **Escape de Prefixo Literal em `suggestions`**:
   - Escapa `\`, `%` e `_` no `prefix` (`prefix.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")`) e consulta `command_norm LIKE ? || '%' ESCAPE '\'`, ordenando após cálculo do `score` por `score DESC, last_used_at DESC, command ASC` com `LIMIT <= 50`.
