# Implementação 02 — Persistência SQLite, Settings, Varredura Nível 1 e Watcher (`spec.md`)

> Parte 02 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Referências canônicas: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) §2.6–§2.8, §5, §5.1 e §11.1.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói a camada de persistência transacional da Fase 06 sobre o `mt-db` (Fase 03), o registro das settings de seleção e proporção das colunas, o motor de varredura de subpastas de nível 1 (`scanner.rs`) e o subsistema de monitoramento de diretórios via `notify` + `notify-debouncer-full` (`watcher.rs`).

### 1.1. Escopo Incluído
- Migration SQL `src-tauri/crates/mt-db/migrations/NNNN_06_projects.sql` criando `project_roots`, `projects`, `project_root_entries` e `project_discovery_dismissed`.
- Repositório tipado interno `src-tauri/crates/mt-db/src/repos/projects.rs` (usando `Db::read`/`Db::write`, `SanitizedText::new`, `prepare_cached` e o ponto de extensão interno `pub(crate) trait ProjectInsertContributor`).
- Adaptador `SqliteProjectStore` e trait `ProjectStore` em `src-tauri/crates/mt-projects/src/store.rs`.
- Registro das settings `projects.selected_id` e `layout.columns.left_size` via `mt_projects::register_db(&mut DbSetup)` em `src-tauri/crates/mt-projects/src/settings.rs`.
- Varredura de nível 1 (`scan_level1`) e cálculo de diferenças (`diff`) em `src-tauri/crates/mt-projects/src/scanner.rs`, preservando aliases/symlinks, distinguindo ausência confirmada (`NotFound`/`NotADirectory`) de falha de leitura (`PermissionDenied`) e protegendo contra deleção de origens em scans parciais (`complete == false`).
- `RootWatcher` em `src-tauri/crates/mt-projects/src/watcher.rs` com `notify-debouncer-full` (`DEBOUNCE = 500ms`, `SETTLE = 1500ms`, `RecursiveMode::NonRecursive` na raiz e no pai) e fila `dirty_roots` resiliente a overflow.

### 1.2. Fora de Escopo desta Implementação
- Orquestração de alto nível (`ProjectService`), agendador de reconciliação periódica e comandos IPC Tauri — cobertos na **Implementação 03**.
- Stores e componentes frontend — cobertos nas **Implementações 04 e 05**.

---

## 2. Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Criar | `src-tauri/crates/mt-db/migrations/0003_06_projects.sql` | Migration SQL da Fase 06 (ou próximo `NNNN` sequencial livre após as migrations das Fases 03 e 05) |
| Criar | `src-tauri/crates/mt-db/src/repos/projects.rs` | Repositório tipado `ProjectsRepo`, rows de persistência e `ProjectInsertContributor` |
| Modificar | `src-tauri/crates/mt-db/src/repos/mod.rs` (e `src-tauri/crates/mt-db/src/lib.rs`) | Exportar `pub mod projects;` |
| Criar | `src-tauri/crates/mt-projects/src/store.rs` | Trait `ProjectStore`, `SqliteProjectStore` e tipos de mutação do catálogo |
| Criar | `src-tauri/crates/mt-projects/src/settings.rs` | `register_db(&mut DbSetup)` com descritores de `projects.selected_id` e `layout.columns.left_size` |
| Criar | `src-tauri/crates/mt-projects/src/scanner.rs` | `scan_level1`, `diff`, `ScanEntry`, `ScanSnapshot`, `ScanDiff` |
| Criar | `src-tauri/crates/mt-projects/src/watcher.rs` | `RootWatcher` e `WatchSignal` |
| Modificar | `src-tauri/crates/mt-projects/src/lib.rs` | Exportar `settings`, `store`, `scanner`, `watcher` e `register_db` |
| Criar | `src-tauri/crates/mt-projects/tests/store_and_scanner.rs` | Testes de integração para SQLite, migrations, settings e scanner |
| Criar | `src-tauri/crates/mt-projects/tests/watcher.rs` | Testes de integração de `RootWatcher` com `TempDir` |

---

## 3. Especificação Técnica Detalhada

### 3.1. Migration SQLite (`src-tauri/crates/mt-db/migrations/NNNN_06_projects.sql`)

```sql
-- Fase 06: projetos, pastas de projetos, origens observadas e sugestões dispensadas
CREATE TABLE project_roots (
    id              TEXT    PRIMARY KEY NOT NULL,
    path            TEXT    NOT NULL UNIQUE,
    discovery_mode  TEXT    NOT NULL DEFAULT 'suggest'
                    CHECK (discovery_mode IN ('suggest', 'auto_import')),
    watch_enabled   INTEGER NOT NULL DEFAULT 1 CHECK (watch_enabled IN (0, 1)),
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL
) STRICT;

CREATE TABLE projects (
    id               TEXT    PRIMARY KEY NOT NULL,
    title            TEXT    NOT NULL CHECK (length(title) BETWEEN 1 AND 120),
    icon             TEXT    NOT NULL,
    color            TEXT    NOT NULL CHECK (color GLOB '#[0-9A-F][0-9A-F][0-9A-F][0-9A-F][0-9A-F][0-9A-F]'),
    path             TEXT    NOT NULL UNIQUE,
    root_id          TEXT    NULL REFERENCES project_roots(id) ON DELETE SET NULL,
    stack            TEXT    NOT NULL DEFAULT 'unknown'
                     CHECK (stack IN ('tauri','vscode_extension','laravel','vue','rust','python','unknown')),
    identity_source  TEXT    NOT NULL DEFAULT 'auto' CHECK (identity_source IN ('auto', 'manual')),
    sort_order       INTEGER NOT NULL,
    missing          INTEGER NOT NULL DEFAULT 0 CHECK (missing IN (0, 1)),
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
) STRICT;

CREATE INDEX idx_projects_sort_order ON projects (sort_order, id);
CREATE INDEX idx_projects_root_id    ON projects (root_id) WHERE root_id IS NOT NULL;

CREATE TABLE project_root_entries (
    root_id        TEXT    NOT NULL REFERENCES project_roots(id) ON DELETE CASCADE,
    entry_path     TEXT    NOT NULL,
    canonical_path TEXT    NOT NULL,
    observed_at    INTEGER NOT NULL,
    PRIMARY KEY (root_id, entry_path)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_root_entries_canonical ON project_root_entries (canonical_path, root_id);

CREATE TABLE project_discovery_dismissed (
    root_id      TEXT    NOT NULL REFERENCES project_roots(id) ON DELETE CASCADE,
    path         TEXT    NOT NULL,
    dismissed_at INTEGER NOT NULL,
    PRIMARY KEY (root_id, path)
) STRICT, WITHOUT ROWID;

CREATE INDEX idx_discovery_dismissed_path ON project_discovery_dismissed (path);
```

### 3.2. Repositório Tipado em `mt-db` (`src-tauri/crates/mt-db/src/repos/projects.rs`)

```rust
use std::sync::{Arc, OnceLock, RwLock};
use crate::{Db, DbError, sanitizer::SanitizedText};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProject {
    pub id: String,
    pub title: String,
    pub icon: String,
    pub color: String,
    pub path: String,
    pub root_id: Option<String>,
    pub stack: String,
    pub identity_source: String,
    pub sort_order: i64,
    pub missing: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredNewProject {
    pub id: String,
    pub title: SanitizedText,
    pub icon: String,
    pub color: String,
    pub path: String,
    pub root_id: Option<String>,
    pub stack: String,
    pub identity_source: String,
    pub missing: bool,
    pub created_at: i64,
    pub updated_at: i64,
    /// Se `true`, conflito em `UNIQUE(path)` não falha a transação e reutiliza a linha existente (fluxos auto_import/accept).
    pub allow_existing_path: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProjectUpdate {
    pub id: String,
    pub title: SanitizedText,
    pub icon: String,
    pub color: String,
    pub stack: String,
    pub identity_source: String,
    pub missing: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRoot {
    pub id: String,
    pub path: String,
    pub discovery_mode: String,
    pub watch_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRootEntry {
    pub root_id: String,
    pub entry_path: String,
    pub canonical_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDismissed {
    pub root_id: String,
    pub path: String,
    pub dismissed_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredCatalog {
    pub projects: Vec<StoredProject>,
    pub roots: Vec<StoredRoot>,
    pub entries: Vec<StoredRootEntry>,
    pub dismissed: Vec<StoredDismissed>,
}

#[derive(Debug, Clone, Default)]
pub struct StoredReconcileBatch {
    pub root_id: Option<String>,
    pub upserted_entries: Vec<StoredRootEntry>,
    pub removed_entry_paths: Vec<(String, String)>, // (root_id, entry_path)
    pub auto_imported: Vec<StoredNewProject>,
    pub auto_identity_updates: Vec<StoredProjectUpdate>,
    pub missing_updates: Vec<(String, bool, i64)>,  // (project_id, missing, updated_at)
}

pub enum ProjectMutation {
    Add {
        project: StoredNewProject,
        clear_dismissed_path: String,
    },
    Update(StoredProjectUpdate),
    Remove {
        id: String,
        dismiss: Vec<StoredDismissed>,
    },
    Reorder(Vec<String>),
    AddRoot {
        root: StoredRoot,
        reconciliation: StoredReconcileBatch,
    },
    UpdateRoot(StoredRoot),
    RemoveRoot(String),
    Reconcile(StoredReconcileBatch),
    Accept {
        projects: Vec<StoredNewProject>,
        clear_dismissed_paths: Vec<String>,
    },
    Dismiss(Vec<StoredDismissed>),
}

#[derive(Debug, Clone, Default)]
pub struct ProjectCommit {
    pub catalog: StoredCatalog,
    pub upserted_projects: Vec<StoredProject>,
    pub deleted_project_ids: Vec<String>,
    pub reordered_project_ids: Vec<String>,
    pub upserted_root: Option<StoredRoot>,
    pub deleted_root_id: Option<String>,
}

/// Extensão transacional interna a `mt-db` (ativada pela Fase 07 para criação do Template Padrão).
pub(crate) trait ProjectInsertContributor: Send + Sync + 'static {
    fn after_insert(
        &self,
        tx: &rusqlite::Transaction<'_>,
        project: &StoredProject,
    ) -> Result<(), DbError>;
}

#[derive(Clone)]
pub struct ProjectsRepo {
    db: Db,
}

impl ProjectsRepo {
    pub fn new(db: Db) -> Self;
    pub async fn load_catalog(&self) -> Result<StoredCatalog, DbError>;
    pub async fn find_project_by_path(&self, path: &str) -> Result<Option<StoredProject>, DbError>;
    pub async fn commit(&self, mutation: ProjectMutation) -> Result<ProjectCommit, DbError>;
}
```

**Regras Críticas de `ProjectsRepo`:**
1. **Sanitização antes da escrita**: Todo `title` passa por `SanitizedText::new(&validated_title)` e, se após a sanitização exceder 120 caracteres ou ficar vazio, retorna erro de validação (`project.invalid_title`).
2. **Alocação de `sort_order`**: Em toda inserção nova em `projects`, calcula `COALESCE((SELECT MAX(sort_order) + 1 FROM projects), 0)` dentro da transação `IMMEDIATE`.
3. **Sem `INSERT OR REPLACE`**:
   - Para `ProjectMutation::Add` (`allow_existing_path == false`), se `path` já existir em `projects`, retorna `DbError::Constraint("project.duplicate_path".into())`.
   - Para `AddRoot`, se `path` já existir em `project_roots`, retorna `DbError::Constraint("project_root.duplicate_path".into())`.
   - Para `Accept` ou `auto_imported` (`allow_existing_path == true`), usa `INSERT INTO projects (...) VALUES (...) ON CONFLICT(path) DO NOTHING`, verificando `changes() > 0`. Somente quando uma linha nova é efetivamente inserida, invoca os `ProjectInsertContributor` registrados no mesmo `tx`! Se já existia, relê a linha existente sem invocar contributors.
4. **Reordenação Transacional (`ProjectMutation::Reorder`)**:
   - Lê os `id`s atuais de `projects` ordenados por `(sort_order, id)`.
   - Verifica se o vetor `ids` recebido não possui duplicatas e contém exatamente o mesmo conjunto de `id`s cadastrados. Se divergir, retorna `DbError::Constraint("project.reorder_mismatch".into())` sem alterar nenhuma linha.
   - Atualiza `UPDATE projects SET sort_order = ?1, updated_at = ?2 WHERE id = ?3` para `0..n-1`.
5. **Reconciliação Condicional de Identidade**:
   - Em `auto_identity_updates`, a query SQL filtra `WHERE id = ? AND identity_source = 'auto' AND stack = 'unknown'`, garantindo que edições manuais feitas concorrentemente nunca sejam sobrescritas por um scan em voo.
6. **Leitura Consistente Pós-Mutação**:
   - Ao final do mesmo `tx` em `commit`, carrega o `StoredCatalog` atualizado para que o `ProjectService` recompute sugestões e eventos com consistência transacional.

### 3.3. Store de Domínio (`src-tauri/crates/mt-projects/src/store.rs`)

```rust
use mt_core::{ids::ProjectId, MtError};
use crate::model::{
    DiscoveryMode, IdentitySource, Project, ProjectRoot, ProjectRootId, StackKind,
};

#[derive(Debug, Clone)]
pub struct ProjectRootRow {
    pub id: ProjectRootId,
    pub path: String,
    pub discovery_mode: DiscoveryMode,
    pub watch_enabled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub struct RootEntryRow {
    pub root_id: ProjectRootId,
    pub entry_path: String,
    pub canonical_path: String,
    pub observed_at: i64,
}

#[derive(Debug, Clone)]
pub struct DismissedRow {
    pub root_id: ProjectRootId,
    pub path: String,
    pub dismissed_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewProjectRow {
    pub project: Project,
    pub allow_existing_path: bool,
    pub clear_dismissed: bool,
}

#[derive(Debug, Clone, Default)]
pub struct ReconcileBatch {
    pub root_id: Option<ProjectRootId>,
    pub upserted_entries: Vec<RootEntryRow>,
    pub removed_entry_paths: Vec<(ProjectRootId, String)>,
    pub auto_imported: Vec<NewProjectRow>,
    pub auto_identity_updates: Vec<Project>,
    pub missing_updates: Vec<(ProjectId, bool, i64)>,
}

#[derive(Debug, Clone, Default)]
pub struct CatalogRows {
    pub projects: Vec<Project>,
    pub roots: Vec<ProjectRootRow>,
    pub entries: Vec<RootEntryRow>,
    pub dismissed: Vec<DismissedRow>,
}

#[derive(Debug, Clone)]
pub enum CatalogMutation {
    Add(NewProjectRow),
    Update(Project),
    Remove {
        id: ProjectId,
        dismiss: Vec<DismissedRow>,
    },
    Reorder(Vec<ProjectId>),
    AddRoot {
        root: ProjectRootRow,
        reconciliation: ReconcileBatch,
    },
    UpdateRoot(ProjectRootRow),
    RemoveRoot(ProjectRootId),
    Reconcile(ReconcileBatch),
    Accept(Vec<NewProjectRow>),
    Dismiss(Vec<DismissedRow>),
}

#[derive(Debug, Clone, Default)]
pub struct CommitResult {
    pub catalog: CatalogRows,
    pub upserted_projects: Vec<Project>,
    pub deleted_project_ids: Vec<ProjectId>,
    pub reordered_project_ids: Vec<ProjectId>,
    pub upserted_root: Option<ProjectRootRow>,
    pub deleted_root_id: Option<ProjectRootId>,
}

#[async_trait::async_trait]
pub trait ProjectStore: Send + Sync + 'static {
    async fn load_catalog(&self) -> Result<CatalogRows, MtError>;
    async fn find_project_by_path(&self, path: &str) -> Result<Option<Project>, MtError>;
    async fn commit(&self, mutation: CatalogMutation) -> Result<CommitResult, MtError>;
}

pub struct SqliteProjectStore {
    repo: mt_db::repos::projects::ProjectsRepo,
}

impl SqliteProjectStore {
    pub fn new(db: mt_db::Db) -> Self {
        Self {
            repo: mt_db::repos::projects::ProjectsRepo::new(db),
        }
    }
}
```

### 3.4. Registro de Settings (`src-tauri/crates/mt-projects/src/settings.rs`)

```rust
use mt_db::{DbSetup, SettingDescriptor};
use serde_json::Value;
use uuid::Uuid;

pub const SETTING_SELECTED_PROJECT_ID: &str = "projects.selected_id";
pub const SETTING_LEFT_COLUMN_SIZE: &str = "layout.columns.left_size";

pub fn register_db(setup: &mut DbSetup) {
    setup.settings.push(SettingDescriptor {
        key: SETTING_SELECTED_PROJECT_ID,
        default: || Value::Null,
        validate: validate_selected_project_id,
        sensitive: false,
        description: "ID do projeto atualmente selecionado na coluna esquerda",
    });

    setup.settings.push(SettingDescriptor {
        key: SETTING_LEFT_COLUMN_SIZE,
        default: || serde_json::json!(22),
        validate: validate_left_column_size,
        sensitive: false,
        description: "Largura percentual da coluna esquerda de projetos (15% a 45%)",
    });
}

fn validate_selected_project_id(val: &Value) -> Result<(), String> {
    match val {
        Value::Null => Ok(()),
        Value::String(s) => Uuid::parse_str(s)
            .map(|_| ())
            .map_err(|_| "O identificador do projeto selecionado deve ser um UUID válido ou nulo.".to_string()),
        _ => Err("O identificador do projeto selecionado deve ser uma string UUID ou nulo.".to_string()),
    }
}

fn validate_left_column_size(val: &Value) -> Result<(), String> {
    match val.as_f64() {
        Some(n) if n.is_finite() && (15.0..=45.0).contains(&n) => Ok(()),
        _ => Err("A largura da coluna de projetos deve ser um número entre 15 e 45.".to_string()),
    }
}
```

### 3.5. Varredura de Nível 1 (`src-tauri/crates/mt-projects/src/scanner.rs`)

```rust
use std::path::{Path, PathBuf};
use mt_core::MtError;
use crate::model::ScanWarning;
use crate::store::RootEntryRow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanEntry {
    pub entry_path: PathBuf,
    pub canonical_path: PathBuf,
    pub name: String,
}

#[derive(Debug, Clone, Default)]
pub struct ScanSnapshot {
    pub entries: Vec<ScanEntry>,
    pub warnings: Vec<ScanWarning>,
    pub complete: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanDiff {
    pub upserted_entries: Vec<ScanEntry>,
    pub removed_entry_paths: Vec<String>,
}

/// Lista subpastas de nível 1 de uma raiz canônica.
pub fn scan_level1(root: &Path) -> Result<ScanSnapshot, MtError>;

/// Calcula o diff entre o snapshot atual e as origens anteriormente registradas para a raiz.
/// Quando `current.complete == false`, `removed_entry_paths` é sempre vazio para evitar falsas remoções.
pub fn diff(current: &ScanSnapshot, previous: &[RootEntryRow]) -> ScanDiff;
```

**Regras de Negócio em `scanner.rs`:**
1. `scan_level1(root)`:
   - Canonicaliza `root` (se falhar com `NotFound`, retorna erro `project_root.missing`; outros erros retornam `project_root.scan_failed`).
   - Chama `std::fs::read_dir(&canonical_root)`. Se `read_dir` falhar na raiz, retorna `project_root.scan_failed`.
   - Para cada item de `read_dir`:
     - Se `entry` for `Err(e)` → marca `complete = false`, registra `ScanWarning` (`project_root.entry_io_error`) e prossegue.
     - Obtém `file_name()`. Se `paths::is_hidden(&file_name)` → ignora silenciosamente.
     - Se `file_name` não for UTF-8 → registra `ScanWarning` (`project.path_not_utf8`) e ignora.
     - Constrói `entry_path = canonical_root.join(&name_str)`.
     - Inspeciona `std::fs::metadata(&entry_path)` (que segue symlinks):
       - Se `Ok(meta)`:
         - Se `!meta.is_dir()` (arquivo comum, FIFO, socket) → ignora.
         - Se `meta.is_dir()`: chama `std::fs::canonicalize(&entry_path)`.
           - Verifica se `canonical_path` não é UTF-8 → warning e ignora.
           - Verifica se `canonical_path == canonical_root` ou `canonical_root.starts_with(&canonical_path)` (symlink apontando para a própria raiz ou ancestral) → ignora para evitar autoimportação/ciclo.
           - Verifica se `canonical_path == Path::new("/")` ou igual a `$HOME` → ignora.
           - Verifica `paths::is_inside_excluded_discovery_dir(&canonical_path)` (destino dentro de `.git`, `.worktrees` ou `.max-code-worktrees`) → ignora.
           - Caso contrário, adiciona `ScanEntry { entry_path, canonical_path, name: name_str }`.
       - Se `Err(e)`:
         - Se `e.kind() == ErrorKind::NotFound` ou `e.raw_os_error() == Some(libc::ELOOP)` (symlink quebrado ou circular): não marca `complete = false` (assim `diff` pode remover essa origem se existia antes) e registra warning informativo `project_root.broken_symlink`.
         - Se `e.kind() == ErrorKind::PermissionDenied` ou outro erro de I/O: marca `complete = false` e registra `ScanWarning` (`project_root.entry_permission_denied`).
   - Ordena `entries` de forma estável por `(entry.name.as_str(), entry.entry_path.as_os_str())`.
2. `diff(current, previous)`:
   - `upserted_entries` contém todas as entradas presentes em `current.entries`.
   - Se `current.complete == true`, `removed_entry_paths` contém todo `prev.entry_path` que não aparece em `current.entries`.
   - Se `current.complete == false`, `removed_entry_paths` é vazio (`Vec::new()`).

### 3.6. Auto-descoberta com `notify` (`src-tauri/crates/mt-projects/src/watcher.rs`)

```rust
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use mt_core::MtError;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{Debouncer, RecommendedCache};
use tokio::sync::mpsc;
use crate::model::ProjectRootId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchSignal {
    RootChanged(ProjectRootId),
    RootMissing(ProjectRootId),
    WatchError(ProjectRootId, String),
}

pub struct RootWatcher {
    debouncers: HashMap<ProjectRootId, RootWatchHandle>,
    tx: mpsc::Sender<WatchSignal>,
    dirty_roots: Arc<Mutex<HashSet<ProjectRootId>>>,
}

struct RootWatchHandle {
    root_path: PathBuf,
    parent_path: Option<PathBuf>,
    debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

impl RootWatcher {
    pub const DEBOUNCE: Duration = Duration::from_millis(500);
    pub const SETTLE: Duration = Duration::from_millis(1500);

    pub fn new(
        tx: mpsc::Sender<WatchSignal>,
        dirty_roots: Arc<Mutex<HashSet<ProjectRootId>>>,
    ) -> Self;

    /// Inicia ou substitui o monitoramento `NonRecursive` da raiz e do seu diretório pai.
    pub fn watch(&mut self, id: ProjectRootId, path: &Path) -> Result<(), MtError>;

    /// Remove e descarta o debouncer associado à raiz.
    pub fn unwatch(&mut self, id: ProjectRootId);

    /// Remove todos os watchers ativos (usado no shutdown).
    pub fn unwatch_all(&mut self);

    /// Drena o conjunto `dirty_roots` acumulado (inclusive quando o canal de 64 posições encher).
    pub fn take_dirty_roots(&self) -> Vec<ProjectRootId>;
}
```

**Regras de Funcionamento de `RootWatcher`:**
1. Ao chamar `watch(id, path)`:
   - Primeiro chama `self.unwatch(id)` caso já exista watcher para `id` (evita watchers duplicados).
   - Se `!path.exists()`, retorna `MtError` com código `project_root.watch_failed`.
   - Cria `new_debouncer(Self::DEBOUNCE, None, move |result| { ... })`.
   - Registra `debouncer.watch(path, RecursiveMode::NonRecursive)` e, se `path.parent()` existir e for diferente de `path`, registra `debouncer.watch(parent, RecursiveMode::NonRecursive)` para detectar remoção ou rename da própria raiz.
   - Em falha de `new_debouncer` ou `watch` (incluindo `ENOSPC` / limite de inotify), retorna `MtError` com código `project_root.watch_failed` e mensagem em pt-BR orientando sobre `fs.inotify.max_user_watches` quando aplicável.
2. No callback do debouncer:
   - Se `Err(errors)`: insere `id` em `dirty_roots` e tenta `tx.try_send(WatchSignal::RootChanged(id))` (ou `WatchError` se fatal).
   - Se `Ok(events)`:
     - Se qualquer evento tiver `need_rescan()`, marca `id` em `dirty_roots` e envia `RootChanged(id)`.
     - Se a própria raiz deixou de existir (`!root_path.exists()`), envia `WatchSignal::RootMissing(id)`.
     - Para eventos cujos `paths` tocam a própria `root_path` ou filhos diretos (`p.parent() == Some(&root_path)`) cujo nome não começa com `.` (`!is_hidden`): insere `id` em `dirty_roots` **antes** de `let _ = tx.try_send(WatchSignal::RootChanged(id))`. Assim, mesmo sob rajadas que lotem o canal de 64 posições, a raiz permanece marcada em `dirty_roots`.
3. O módulo também exporta uma trait `FileSystemProbe` ou ponto de injeção interno em `scanner.rs` para permitir simular `PermissionDenied` determinístico em testes quando o container de CI executar como `root`.
