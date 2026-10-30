# Implementação 01 — Crate `mt-git`: Descoberta, Status Porcelain v2, Cache, Watcher e Worktrees

> **Fase:** 08 — Indicadores Contextuais: Git, Worktree e Portas Ativas
> **Unidade:** `01 - Crate mt-git Descoberta Status Cache Watcher e Worktrees`
> **Referências Canônicas:** [`../instructions.md`](../instructions.md) §2.1, §4.1, §5.1–§5.4; [`../spec.md`](../spec.md) §2.1–§2.9, §9, §10, §11.1.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói integralmente o crate independente `mt-git` (`src-tauri/crates/mt-git/`), responsável por toda a inteligência de inspeção de repositórios Git e gerenciamento de worktrees no backend Rust do MaxTerminal, sem dependência direta do runtime Tauri (facilitando testes unitários e de integração puros via `cargo test -p mt-git`).

### 1.1. Objetivos Específicos
1. **Descoberta rápida sem subprocesso (`discover.rs`):** Localizar `.git` (diretório ou arquivo `gitdir: <path>`), resolver `commondir`, ler `HEAD` diretamente do sistema de arquivos para pintura inicial (< 2 ms) e comprovar se o diretório pertence a uma **worktree vinculada** (`WorktreeKind::MaxCode` para `/.max-code-worktrees/`, `WorktreeKind::Project` para `/.worktrees/` ou `WorktreeKind::Other`), distinguindo-a de submódulos, repositórios bare e diretórios com `--separate-git-dir`.
2. **Execução segura do Git CLI (`runner.rs`):** Executar `git` com argumentos em vetor (jamais via shell), ambiente higienizado (`GIT_OPTIONAL_LOCKS=0`, `GIT_TERMINAL_PROMPT=0`, `-c core.fsmonitor=false`), timeout de 5 s para status (com kill e wait do processo filho) e limite de saída de 16 MiB.
3. **Parser binário de Porcelain v2 (`porcelain.rs`):** Analisar a saída delimitada por NUL (`-z`) de `git status --porcelain=v2 --branch -z --untracked-files=all --ignore-submodules=none`, extraindo OID, branch, upstream, ahead/behind, contagens de *staged*, *unstaged*, *untracked* e *conflicted*, tratando renomeações (`2 XY` com consumo do caminho original seguinte), submódulos e estados `(detached)` / `(initial)`.
4. **Monitoramento eficiente via inotify (`watcher.rs`):** Observar de forma não-recursiva `git_dir`, `common_dir`, `refs/heads` e diretórios pais de refs aninhadas (ex.: `refs/heads/feature/foo`), filtrando `.lock`, `ORIG_HEAD` e eventos `Access`, aplicando debounce *trailing* de 300 ms com teto (*max wait*) de 1 s, e degradando graciosamente para polling de 5 s caso o limite de inotify (`ENOSPC`) seja atingido.
5. **Cache por repositório, Single-Flight e Reconciliação (`cache.rs`, `lib.rs`):** Coalescer leituras concorrentes do mesmo repositório (`RepoKey`), limitar execuções globais de `git` com `Semaphore(4)`, controlar `generation` e `revision` monotônica por painel, gerenciar contagem de referências (`refcount`) com expiração em 60 s após zero painéis e reconciliar periodicamente (30 s visível / 120 s oculto).
6. **API Pública de Worktrees (`worktree.rs`):** Implementar a trait `WorktreeManager` e `CliWorktreeManager` (`list`, `create`, `remove`, `prune`) com validações estritas de segurança, serialização de mutações por `common_dir`, proteção da worktree principal e de worktrees com painéis abertos (`panes_inside` + trava de reserva de `cwd`), preparando a fundação para as Fases 13 e 16.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Modificações no Workspace Cargo
- `src-tauri/Cargo.toml` — Adicionar `"crates/mt-git"` em `[workspace].members` e declarar dependência `mt-git = { path = "crates/mt-git" }` em `[workspace.dependencies]`.
- `src-tauri/crates/mt-core/src/error.rs` (se necessário) — Garantir suporte aos códigos de erro `git.*` no `MtError`.

### 2.2. Novos Arquivos em `src-tauri/crates/mt-git/`
- `src-tauri/crates/mt-git/Cargo.toml`
- `src-tauri/crates/mt-git/src/lib.rs`
- `src-tauri/crates/mt-git/src/types.rs`
- `src-tauri/crates/mt-git/src/error.rs`
- `src-tauri/crates/mt-git/src/discover.rs`
- `src-tauri/crates/mt-git/src/runner.rs`
- `src-tauri/crates/mt-git/src/porcelain.rs`
- `src-tauri/crates/mt-git/src/watcher.rs`
- `src-tauri/crates/mt-git/src/cache.rs`
- `src-tauri/crates/mt-git/src/worktree.rs`

### 2.3. Testes e Fixtures em `src-tauri/crates/mt-git/tests/`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/clean.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/mixed.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/rename.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/conflict.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/detached.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/initial.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/no_upstream.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/gone.bin`
- `src-tauri/crates/mt-git/tests/fixtures/porcelain/utf8_spaces.bin`
- `src-tauri/crates/mt-git/tests/discover.rs`
- `src-tauri/crates/mt-git/tests/watcher.rs`
- `src-tauri/crates/mt-git/tests/worktree.rs`

---

## 3. Especificação Técnica Detalhada e Assinaturas Rust

### 3.1. Manifesto `src-tauri/crates/mt-git/Cargo.toml`

```toml
[package]
name = "mt-git"
version.workspace = true
edition.workspace = true
rust-version.workspace = true

[dependencies]
mt-core = { workspace = true }
async-trait = { workspace = true }
dashmap = { workspace = true }
dunce = { workspace = true }
futures = { workspace = true }
notify = { workspace = true }
parking_lot = { workspace = true }
regex = { workspace = true }
serde = { workspace = true, features = ["derive"] }
specta = { workspace = true, features = ["derive"] }
thiserror = { workspace = true }
tokio = { workspace = true, features = ["fs", "io-util", "macros", "process", "rt", "sync", "time"] }
tracing = { workspace = true }
uuid = { workspace = true, features = ["v4", "v7", "serde"] }
which = { workspace = true }

[dev-dependencies]
proptest = { workspace = true }
tempfile = { workspace = true }
tokio = { workspace = true, features = ["full", "test-util"] }
```

### 3.2. Tipos Públicos (`src-tauri/crates/mt-git/src/types.rs`)

```rust
use mt_core::{PaneId, TabId};
use serde::{Deserialize, Serialize};

/// Chave do cache: gitdir canônico da worktree (em worktrees vinculadas, `.git/worktrees/<nome>`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
pub struct RepoKey(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitContext {
    pub repo_key: RepoKey,
    /// Raiz do working tree atual (toplevel).
    pub work_tree: String,
    pub branch: GitBranch,
    /// `None` quando o status completo ainda não foi calculado ou falhou (modo degradado).
    pub changes: Option<GitChanges>,
    pub upstream: Option<GitUpstream>,
    pub worktree: Option<GitWorktree>,
    pub degraded: Option<DegradedReason>,
    /// Epoch ms do cálculo, apenas para exibição; ordenação usa o envelope versionado (`GitStatusChanged`).
    pub computed_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum GitBranch {
    Named { name: String, oid: Option<String> },
    Detached { short_oid: String, oid: String },
    Unborn { name: String },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitChanges {
    pub staged: u32,
    pub unstaged: u32,
    pub untracked: u32,
    pub conflicted: u32,
}

impl GitChanges {
    pub fn is_dirty(&self) -> bool {
        self.staged + self.unstaged + self.untracked + self.conflicted > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitUpstream {
    pub name: String,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub gone: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeKind {
    MaxCode,
    Project,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitWorktree {
    /// Último componente do gitdir (`.git/worktrees/<name>`).
    pub name: String,
    /// Classificação pelo caminho do working tree: `/.max-code-worktrees/` > `/.worktrees/` > `Other`.
    pub kind: WorktreeKind,
    /// Caminho do working tree principal resolvido pelo Git (`None` quando principal é bare ou desconhecida).
    pub main_work_tree: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum DegradedReason {
    GitNotFound,
    Timeout,
    UnsafeRepository,
    CommandFailed,
    WatchUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeEntry {
    pub path: String,
    pub head_oid: Option<String>,
    /// Nome da branch local sem o prefixo `refs/heads/`.
    pub branch: Option<String>,
    pub is_main: bool,
    pub is_detached: bool,
    pub is_locked: bool,
    pub is_prunable: bool,
    /// `None` para a worktree principal.
    pub kind: Option<WorktreeKind>,
}

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeCreateRequest {
    pub repo_path: String,
    /// Deve validar `^[A-Za-z0-9._-]{1,100}$`, sem `..`, `/` ou `\0`.
    pub name: String,
    pub location: WorktreeLocation,
    /// Se presente, passa `-b <branch>` após validar com `git check-ref-format --branch`.
    pub new_branch: Option<String>,
    /// Commit-ish base; default `HEAD`. Validado via `rev-parse --verify --end-of-options <base>^{commit}`.
    pub base: Option<String>,
    /// Se `true`, garante a regra no `.gitignore` dentro da nova worktree criada, sem tocar na principal.
    pub ensure_gitignore: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeLocation {
    /// `<main_work_tree>/.max-code-worktrees/<name>`
    MaxCode,
    /// `<main_work_tree>/.worktrees/<name>`
    Project,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum RefreshReason {
    CommandFinished,
    WindowFocus,
    Watcher,
    Periodic,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusChanged {
    pub epoch: String,
    pub revision: u32,
    pub generation: u32,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub cwd: String,
    pub context: Option<GitContext>,
}
```

### 3.3. Erros (`src-tauri/crates/mt-git/src/error.rs`)

```rust
use mt_core::{MtError, PaneId};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("Git não encontrado no PATH")]
    NotFound,
    #[error("Diretório não é um repositório Git")]
    NotARepo,
    #[error("Comando git excedeu o tempo limite")]
    Timeout,
    #[error("Repositório com proprietário divergente (safe.directory)")]
    UnsafeRepository,
    #[error("Falha ao executar git: {stderr}")]
    CommandFailed { code: Option<i32>, stderr: String },
    #[error("Saída do git excedeu o limite permitido de 16 MiB")]
    OutputLimit,
    #[error("Nome inválido: {0}")]
    InvalidName(String),
    #[error("Worktree já existe: {0}")]
    WorktreeExists(String),
    #[error("Worktree em uso por abas abertas")]
    WorktreeInUse(Vec<PaneId>),
    #[error("Não é possível remover a worktree principal")]
    WorktreeIsMain,
    #[error("Worktree está bloqueada (locked)")]
    WorktreeLocked(String),
    #[error("A pasta contêiner de worktrees não está ignorada no repositório principal")]
    WorktreeContainerNotIgnored(String),
    #[error("Raiz do repositório principal indisponível para criar worktree")]
    WorktreeRootUnavailable,
    #[error("Caminho contém codificação UTF-8 inválida")]
    PathEncoding,
    #[error("Falha de E/S: {0}")]
    Io(#[from] std::io::Error),
    #[error("Saída do git inválida: {0}")]
    Parse(String),
}

impl GitError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "git.not_found",
            Self::NotARepo => "git.not_a_repo",
            Self::Timeout => "git.timeout",
            Self::UnsafeRepository => "git.unsafe_repository",
            Self::CommandFailed { .. } => "git.command_failed",
            Self::OutputLimit => "git.output_limit",
            Self::InvalidName(_) => "git.invalid_name",
            Self::WorktreeExists(_) => "git.worktree_exists",
            Self::WorktreeInUse(_) => "git.worktree_in_use",
            Self::WorktreeIsMain => "git.worktree_is_main",
            Self::WorktreeLocked(_) => "git.worktree_locked",
            Self::WorktreeContainerNotIgnored(_) => "git.worktree_container_not_ignored",
            Self::WorktreeRootUnavailable => "git.worktree_root_unavailable",
            Self::PathEncoding => "git.path_encoding",
            Self::Io(_) => "git.io",
            Self::Parse(_) => "git.parse",
        }
    }

    /// Converte erros de consulta de status em `DegradedReason` para não falhar a UI.
    pub fn to_degraded_reason(&self) -> Option<crate::types::DegradedReason> {
        use crate::types::DegradedReason;
        match self {
            Self::NotFound => Some(DegradedReason::GitNotFound),
            Self::Timeout => Some(DegradedReason::Timeout),
            Self::UnsafeRepository => Some(DegradedReason::UnsafeRepository),
            Self::CommandFailed { .. } | Self::OutputLimit | Self::Parse(_) => {
                Some(DegradedReason::CommandFailed)
            }
            _ => None,
        }
    }
}

impl From<GitError> for MtError {
    fn from(err: GitError) -> Self {
        let code = err.code().to_string();
        let message = err.to_string();
        let details = match &err {
            GitError::CommandFailed { code: exit_code, stderr } => {
                let truncated: String = stderr.chars().take(2048).collect();
                Some(json!({ "exitCode": exit_code, "stderr": truncated }))
            }
            GitError::WorktreeInUse(panes) => Some(json!({ "paneIds": panes })),
            _ => None,
        };
        MtError::new(code, message, details)
    }
}
```

### 3.4. Descoberta e Leitura Rápida de `HEAD` (`src-tauri/crates/mt-git/src/discover.rs`)

```rust
use std::path::{Path, PathBuf};
use crate::error::GitError;
use crate::types::{GitBranch, GitWorktree, RepoKey, WorktreeKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoLocation {
    pub work_tree: PathBuf,
    /// `gitdir` específico da worktree atual.
    pub git_dir: PathBuf,
    /// Diretório comum (`common_dir == git_dir` na worktree principal).
    pub common_dir: PathBuf,
    pub worktree: Option<GitWorktree>,
}

impl RepoLocation {
    pub fn repo_key(&self) -> Result<RepoKey, GitError> {
        let s = self.git_dir.to_str().ok_or(GitError::PathEncoding)?;
        Ok(RepoKey(s.to_string()))
    }
}

/// Sobe a partir de `cwd` até achar `.git`; respeita `GIT_CEILING_DIRECTORIES` e para na raiz.
pub fn discover(cwd: &Path) -> Result<Option<RepoLocation>, GitError>;

/// Lê `HEAD` (simbólico ou SHA) e resolve o OID em `refs/` ou `packed-refs` sem executar subprocesso.
pub fn read_head(location: &RepoLocation) -> Result<GitBranch, GitError>;

/// Classifica o tipo de worktree vinculada com base nos componentes do caminho do `work_tree`.
/// Só deve ser chamada após comprovar que o repositório é uma worktree vinculada!
pub fn classify_worktree(work_tree: &Path) -> WorktreeKind;

/// Confirma os caminhos descobertos usando o binário `git` (`rev-parse` e `worktree list`),
/// resolvendo `main_work_tree` com precisão mesmo com gitdir separado ou principal bare.
pub async fn confirm_with_git(
    runner: &dyn crate::runner::GitRunner,
    provisional: RepoLocation,
) -> Result<Option<RepoLocation>, GitError>;
```

#### Regras de Negócio de `discover.rs`
1. **Canonicalização e Ceiling:** Canonicalizar `cwd` com `dunce::canonicalize(cwd)` (retorna `Ok(None)` se `cwd` não existir no disco). Ler `GIT_CEILING_DIRECTORIES` (separado por `:` no Linux) e interromper a subida caso o ancestral alcance um diretório limite. Limitar subida a 128 níveis.
2. **Inspeção de `.git`:**
   - Se `<ancestral>/.git` for um **diretório**: verificar se contém `HEAD` e `objects` (ou `commondir`). Se for um repositório *bare* (ex.: `config` tem `bare = true` sem working tree), retornar `Ok(None)`. Caso contrário, `work_tree = ancestral`, `git_dir = canonicalize(<ancestral>/.git)`.
   - Se `<ancestral>/.git` for um **arquivo**: ler até 4 KiB; deve iniciar com `gitdir:` (ASCII case-insensitive opcional, padrão `gitdir: `). Resolver o caminho relativo ao diretório `<ancestral>` e canonicalizar com `dunce::canonicalize`.
3. **Resolução de `commondir` e Comprovação de Worktree Vinculada:**
   - Verificar se existe `<git_dir>/commondir`. Se existir, ler a linha, remover quebras de linha e resolver relativamente a `git_dir` (canonicalizando para obter `common_dir`). Se não existir, `common_dir = git_dir`.
   - Uma **worktree vinculada** é comprovada **somente** quando todas as condições abaixo são verdadeiras:
     a. `.git` na raiz do `work_tree` é um arquivo `gitdir: ...`;
     b. `git_dir != common_dir`;
     c. `git_dir` é filho de `<common_dir>/worktrees/<name>` (portanto tem arquivo `<git_dir>/commondir`);
     d. Existe o arquivo reverso `<git_dir>/gitdir` cujo caminho resolvido e canonicalizado aponta de volta para o arquivo `<work_tree>/.git` encontrado.
   - **Submódulos** (onde `git_dir` fica em `<super>/.git/modules/<sub_name>` e não possui estrutura `<common_dir>/worktrees/<name>`) e **repositórios com `--separate-git-dir`** (onde `git_dir == common_dir`) **não** recebem `worktree: Some(...)` (`worktree = None`).
   - **Falso positivo de nome de pasta:** Um repositório comum (`git init`) criado dentro de uma pasta chamada `.worktrees/foo` ou `.max-code-worktrees/bar` tem `git_dir == common_dir` e **não** é classificado como worktree (`worktree = None`).
4. **Classificação (`classify_worktree`):**
   - Percorre os componentes de `work_tree`: se contiver `.max-code-worktrees`, retorna `WorktreeKind::MaxCode`; se contiver `.worktrees`, retorna `WorktreeKind::Project`; caso contrário, `WorktreeKind::Other`.
   - O `name` de `GitWorktree` é sempre o nome da entrada administrativa (`git_dir.file_name()`), não o nome da pasta se divergente.
5. **Leitura de `HEAD` (`read_head`):**
   - Lê `<git_dir>/HEAD` (máx. 1 KiB).
   - Se começar com `ref: `: extrai o caminho da ref (ex.: `refs/heads/main`). Se começar com `refs/heads/`, o nome da branch é o sufixo após `refs/heads/` (preservando barras internas como `feature/foo`).
   - Para resolver o OID da ref simbólica (máximo de 5 redirecionamentos `ref:` para evitar ciclos):
     1. Procura `<git_dir>/<ref>` (para refs específicas de worktree como `refs/bisect/*`) e depois `<common_dir>/<ref>`.
     2. Se não achar arquivo solto, lê `<common_dir>/packed-refs` e busca linha `<oid> <ref>`.
     3. Se encontrar OID hexadecimal válido (40 ou 64 chars): retorna `GitBranch::Named { name, oid: Some(oid) }`.
     4. Se não encontrar OID: verifica `<common_dir>/config` ou `<git_dir>/config` por `extensions.refStorage = reftable` (ou diretório `<common_dir>/reftable`). Se o armazenamento for `reftable`/desconhecido, retorna `GitBranch::Named { name, oid: None }` (não finge `Unborn`). Caso seja armazenamento `files` padrão, retorna `GitBranch::Unborn { name }`.
   - Se `<git_dir>/HEAD` contiver 40 ou 64 caracteres hexadecimais: retorna `GitBranch::Detached { short_oid: oid[..7].to_string(), oid }`.

### 3.5. Executor `GitRunner` (`src-tauri/crates/mt-git/src/runner.rs`)

```rust
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;
use crate::error::GitError;
use crate::porcelain::PorcelainStatus;

#[async_trait::async_trait]
pub trait GitRunner: Send + Sync + 'static {
    async fn status(&self, work_tree: &Path) -> Result<PorcelainStatus, GitError>;
    async fn run(&self, cwd: &Path, args: &[&OsStr], timeout: Duration) -> Result<Output, GitError>;
    fn is_available(&self) -> bool;
}

pub struct CliGitRunner {
    git_bin: Option<PathBuf>,
    status_timeout: Duration,
    max_output_bytes: usize,
}

impl CliGitRunner {
    pub fn new() -> Self {
        let git_bin = which::which("git").ok();
        Self {
            git_bin,
            status_timeout: Duration::from_secs(5),
            max_output_bytes: 16 * 1024 * 1024, // 16 MiB
        }
    }

    pub fn with_bin_and_timeout(git_bin: Option<PathBuf>, status_timeout: Duration) -> Self {
        Self {
            git_bin,
            status_timeout,
            max_output_bytes: 16 * 1024 * 1024,
        }
    }
}
```

#### Regras de Execução e Segurança (`CliGitRunner`)
1. **Sem Shell:** Jamais invocar `sh -c`. Usar `tokio::process::Command::new(git_bin)`.
2. **Higienização de Ambiente:**
   - Remover obrigatoriamente variáveis herdadas que desviam repositório/configuração: `GIT_DIR`, `GIT_WORK_TREE`, `GIT_COMMON_DIR`, `GIT_INDEX_FILE`, `GIT_OBJECT_DIRECTORY`, `GIT_ALTERNATE_OBJECT_DIRECTORIES`, `GIT_CEILING_DIRECTORIES`, `GIT_CONFIG_PARAMETERS`, `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `GIT_ASKPASS`, `SSH_ASKPASS`.
   - Se `GIT_CONFIG_COUNT` existir no ambiente do processo, remover `GIT_CONFIG_COUNT` e todas as variáveis `GIT_CONFIG_KEY_<i}` e `GIT_CONFIG_VALUE_<i>` (varrendo `std::env::vars_os()` onde a chave começa com `GIT_CONFIG_`).
   - Definir explicitamente:
     - `GIT_OPTIONAL_LOCKS = "0"`
     - `GIT_TERMINAL_PROMPT = "0"`
     - `LC_ALL = "C"`
     - `LANG = "C"`
     - `GIT_PAGER = "cat"`
3. **Argumentos de `status`:**
   ```text
   -C <work_tree> --no-optional-locks -c core.fsmonitor=false -c color.status=false
   status --porcelain=v2 --branch -z --untracked-files=all --ignore-submodules=none
   ```
4. **Drenagem Limitada e Timeout:**
   - Configurar `stdin(Stdio::null())`, `stdout(Stdio::piped())`, `stderr(Stdio::piped())`, `kill_on_drop(true)`.
   - Ler `stdout` e `stderr` concorrentemente com `tokio::io::AsyncReadExt::take(max_output_bytes as u64 + 1)`. Se `stdout.len() > max_output_bytes`, matar o filho, aguardar `child.wait().await` e retornar `Err(GitError::OutputLimit)`.
   - Se `tokio::time::timeout(timeout, ...)` expirar: chamar `child.kill().await` **e** `child.wait().await` antes de retornar `Err(GitError::Timeout)` (garantindo que nenhum processo `git` órfão ou zumbi fique pendente).
   - Se `status.success()` for `false`: inspecionar `stderr` (UTF-8 lossy). Se contiver `"detected dubious ownership"` ou `"unsafe repository"`, retornar `Err(GitError::UnsafeRepository)`. Se código for `128` e contiver `"not a git repository"`, retornar `Err(GitError::NotARepo)`. Caso contrário, retornar `Err(GitError::CommandFailed { code: status.code(), stderr })`.

### 3.6. Parser Porcelain v2 (`src-tauri/crates/mt-git/src/porcelain.rs`)

```rust
use crate::error::GitError;
use crate::types::GitChanges;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PorcelainStatus {
    /// `None` quando `# branch.oid (initial)`.
    pub oid: Option<String>,
    /// `None` quando `# branch.head (detached)`.
    pub head: Option<String>,
    pub upstream: Option<String>,
    pub ahead_behind: Option<(u32, u32)>,
    pub changes: GitChanges,
}

pub fn parse(bytes: &[u8]) -> Result<PorcelainStatus, GitError>;
```

#### Regras de Parsing de `porcelain.rs`
- Dividir `bytes` pelo byte NUL (`b'\0'`), iterando com um iterador mutável (`let mut entries = bytes.split(|&b| b == 0)`), ignorando fatias vazias de terminação.
- Cabeçalhos `# `:
  - `# branch.oid <val>`: se `val != "(initial)"`, `oid = Some(val.to_string())`.
  - `# branch.head <val>`: se `val != "(detached)"`, `head = Some(val.to_string())`.
  - `# branch.upstream <val>`: `upstream = Some(val.to_string())`.
  - `# branch.ab +<A> -<B>`: converte `A` e `B` (`u32`) e define `ahead_behind = Some((a, b))`.
- Entradas de arquivos modificados ordinários (`1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>`):
  - Os primeiros 5 bytes são `b"1 XY "` (onde `X = entry[2]`, `Y = entry[3]`). O campo `<sub>` tem 4 bytes (`entry[5..9]`, ex.: `N...` ou `SCMU`).
  - Se `X != b'.'`, incrementa `changes.staged += 1`.
  - Se `Y != b'.'` **ou** (`sub.len() == 4 && sub[0] == b'S' && (sub[2] == b'M' || sub[3] == b'U')`), incrementa `changes.unstaged += 1` (uma única vez por registro!).
- Entradas de arquivos renomeados/copiados (`2 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <X><score> <path>\0<origPath>`):
  - Mesma regra de `X`, `Y` e `<sub>` da entrada `1`.
  - **Obrigatório:** Consumir imediatamente o próximo item do iterador (`let _orig_path = entries.next();`) para que `<origPath>` não seja interpretado como um novo registro!
- Entradas de conflito (`u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>`):
  - Se o registro começa com `b"u "`: incrementa `changes.conflicted += 1` (não soma em `staged`/`unstaged`).
- Entradas não rastreadas (`? <path>`):
  - Se começa com `b"? "`: incrementa `changes.untracked += 1`.
- Entradas ignoradas (`! <path>`) ou desconhecidas:
  - Ignorar silenciosamente sem erro e sem *panic*.

### 3.7. Watcher de Repositório (`src-tauri/crates/mt-git/src/watcher.rs`)

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use crate::discover::RepoLocation;
use crate::error::GitError;
use crate::types::RepoKey;

#[derive(Debug, Clone)]
pub enum WatchNotification {
    /// Mudança no gitdir específico de uma worktree (ex.: `HEAD`, `index`).
    RepoChanged(RepoKey),
    /// Mudança no `common_dir` compartilhado (ex.: `refs/heads/*`, `packed-refs`) afetando todas as worktrees que compartilham o mesmo `common_dir`.
    CommonDirChanged(PathBuf),
}

pub struct RepoWatcher {
    inner: Mutex<WatcherState>,
    tx: mpsc::UnboundedSender<WatchNotification>,
}

impl RepoWatcher {
    pub fn new(tx: mpsc::UnboundedSender<WatchNotification>) -> Result<Self, GitError>;
    pub fn watch(&self, key: &RepoKey, loc: &RepoLocation, branch_ref: Option<&str>, upstream_remote: Option<&str>) -> Result<(), GitError>;
    pub fn unwatch(&self, key: &RepoKey);
}
```

#### Regras do Watcher e Debounce
1. **Caminhos Observados (`RecursiveMode::NonRecursive`):**
   - `loc.git_dir` (observa `HEAD`, `index`, `MERGE_HEAD`, `REBASE_HEAD`, `CHERRY_PICK_HEAD`, `gitdir`, `commondir`).
   - `loc.common_dir` (observa `packed-refs`, `FETCH_HEAD`, `config`).
   - `<loc.common_dir>/refs/heads`.
   - Se a branch atual contiver subdiretórios (ex.: `feature/auth/login`), observar não-recursivamente cada diretório pai intermediário existente sob `<loc.common_dir>/refs/heads/` (`refs/heads/feature`, `refs/heads/feature/auth`).
   - Se houver `upstream_remote` (ex.: `origin` extraído de `origin/main`), observar `<loc.common_dir>/refs/remotes/<remote>` e seus pais intermediários existentes.
2. **Filtro de Eventos Inotify:**
   - Descartar `notify::EventKind::Access(_)`.
   - Descartar eventos onde **todos** os caminhos afetados terminam em `.lock` (ex.: `index.lock`, `HEAD.lock`) ou cujo `file_name` seja `ORIG_HEAD`.
3. **Coalescência e Debounce por Repo (300 ms trailing, teto 1 s):**
   - No coordenador do watcher dentro de `GitService`: ao receber o primeiro evento de um `RepoKey`, registrar `first_event_at = Instant::now()` e agendar timer para `min(now + 300ms, first_event_at + 1000ms)`.
   - Novos eventos para o mesmo `RepoKey` antes do disparo empurram o deadline para `min(now + 300ms, first_event_at + 1000ms)`. Ao disparar, invoca a recomputação do `RepoKey`.
4. **Degradação em Falha de Watch (`ENOSPC` / `max_user_watches`):**
   - Se `watcher.watch(...)` falhar, `RepoWatcher::watch` retorna `Err(GitError::Io(...))` e `GitService` marca a entrada do repositório com `watch_failed = true` (`DegradedReason::WatchUnavailable`), ativando polling de 5 s para o repositório enquanto houver painel visível (e 30 s quando oculto).

### 3.8. Cache, Single-Flight e `GitService` (`src-tauri/crates/mt-git/src/cache.rs`, `src/lib.rs`)

```rust
use std::path::{Path, PathBuf};
use std::sync::Arc;
use dashmap::DashMap;
use mt_core::{PaneId, PtyId, TabId};
use tokio::sync::Semaphore;
use crate::error::GitError;
use crate::runner::GitRunner;
use crate::types::{GitContext, GitStatusChanged, RefreshReason, RepoKey};

pub trait GitEventSink: Send + Sync + 'static {
    fn status_changed(&self, payload: GitStatusChanged);
}

pub struct GitService {
    epoch: String,
    runner: Arc<dyn GitRunner>,
    emitter: Arc<dyn GitEventSink>,
    panes: DashMap<PaneId, PaneBinding>,
    repos: DashMap<RepoKey, Arc<tokio::sync::Mutex<RepoEntry>>>,
    /// Semáforo global limitando a 4 subprocessos `git` simultâneos.
    sem: Arc<Semaphore>,
    /// Mutex por `common_dir` para serializar operações de worktree (`add`/`remove`/`prune`) e reserva de `cwd`.
    worktree_locks: DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>,
    /// Reserva temporária de `cwd` durante abertura de aba/painel para impedir `worktree remove` concorrente.
    reserved_cwds: DashMap<u64, PathBuf>,
    // ... canais de debounce, watcher e task handles
}

impl GitService {
    pub fn new(runner: Arc<dyn GitRunner>, emitter: Arc<dyn GitEventSink>) -> Arc<Self>;

    /// Associa ou atualiza o painel ao `cwd` informado (e opcionalmente `pty_id`).
    /// Retorna imediatamente o envelope `GitStatusChanged` com o contexto em cache ou lido de `HEAD` (`changes: None`),
    /// e agenda o cálculo completo de `status` em background.
    pub async fn track_pane(
        self: &Arc<Self>,
        tab_id: TabId,
        pane_id: PaneId,
        pty_id: Option<PtyId>,
        cwd: PathBuf,
        visible: bool,
    ) -> Result<GitStatusChanged, GitError>;

    pub fn untrack_pane(&self, pane_id: PaneId);

    pub fn set_visible(&self, tab_id: Option<TabId>);

    /// Agenda recomputação (debounced 150 ms para gatilhos de UI/comando) do repo do painel.
    pub fn request_refresh(self: &Arc<Self>, pane_id: PaneId, reason: RefreshReason);

    /// Calcula o status completo uma única vez para o `cwd` (usado por `git_status` e Fase 13).
    pub async fn status_once(&self, cwd: &Path) -> Result<Option<GitContext>, GitError>;

    /// Lista todos os `PaneId` rastreados cujo `cwd` (ou `work_tree`) está dentro de `target_path`.
    pub fn panes_inside(&self, target_path: &Path) -> Vec<PaneId>;

    /// Reserva um `cwd` durante o ciclo de spawn de aba/painel da Fase 07; o guard remove a reserva no `Drop`.
    pub fn reserve_cwd(self: &Arc<Self>, cwd: PathBuf) -> CwdReservationGuard;

    /// Verifica se há algum painel rastreado ou `cwd` reservado dentro de `target_path`.
    pub fn has_active_or_reserved_inside(&self, target_path: &Path) -> Result<(), Vec<PaneId>>;
}
```

#### Regras de Concorrência, Single-Flight e Versionamento (`GitService`)
1. **Envelope Versionado (`epoch`, `revision`, `generation`):**
   - `epoch`: UUID v4 gerado na criação de `GitService`.
   - `generation`: incrementa em `PaneBinding` toda vez que `(cwd, pty_id, repo_key)` do painel muda ou quando `untrack_pane` é chamado. Qualquer task assíncrona em andamento captura a `generation` inicial e descarta seu resultado se `pane_binding.generation != captured_generation`.
   - `revision`: inteiro monotônico (`u32`) por `PaneId`, incrementado a **cada** resposta de `track_pane` ou emissão de `git://status-changed` (inclusive quando `context` passa a ser `None` ao sair de um repositório).
2. **Single-Flight com `dirty_again`:**
   - Quando um cálculo de status já está `inflight` para um `RepoKey` e chega um novo gatilho (`Watcher`, `CommandFinished`, `Manual`), a flag `dirty_again = true` é marcada no estado do repositório.
   - Assim que o cálculo em voo termina, se `dirty_again` estiver `true`, uma nova execução é disparada imediatamente (sem janela cega de supressão de 500 ms).
3. **Verificação de Upstream `gone`:**
   - Quando `# branch.upstream <u>` está presente no porcelain v2 mas `# branch.ab` está ausente: verificar se a ref remota `<common_dir>/refs/remotes/<u>` existe (em arquivo solto ou `packed-refs`). Se **não** existir e a branch atual já possuir commit (`oid.is_some()`), definir `upstream.gone = true`; caso contrário `gone = false`.
4. **Emissão apenas em Mudança Estrutural:**
   - Comparar o novo `GitContext` com o anterior ignorando `computed_at`. Só incrementar `revision` e emitir `git://status-changed` para os painéis vinculados ao `RepoKey` se houver diferença em `(repo_key, work_tree, branch, changes, upstream, worktree, degraded)` ou quando era `changes: None` (pintura inicial) e passou a ter `changes: Some(...)`.
5. **Expiração de Cache e Reconciliação:**
   - Quando o último painel sai de um `RepoKey` (`refcount == 0`), remover os watches do `RepoWatcher` imediatamente e registrar `released_at = Some(Instant::now())`. Uma tarefa de manutenção remove entradas com `refcount == 0` após 60 s.
   - Reconciliação periódica: a cada 30 s atualiza repositórios com pelo menos um painel `visible == true`; a cada 120 s atualiza repositórios apenas com painéis ocultos (`visible == false`). Sem painéis rastreados, nenhuma tarefa de reconciliação executa subprocessos.

### 3.9. Gerenciamento de Worktrees (`src-tauri/crates/mt-git/src/worktree.rs`)

```rust
use std::path::Path;
use std::sync::{Arc, Weak};
use crate::error::GitError;
use crate::runner::GitRunner;
use crate::types::{WorktreeCreateRequest, WorktreeEntry};
use crate::GitService;

#[async_trait::async_trait]
pub trait WorktreeManager: Send + Sync + 'static {
    async fn list(&self, repo_path: &Path) -> Result<Vec<WorktreeEntry>, GitError>;
    async fn create(&self, req: WorktreeCreateRequest) -> Result<WorktreeEntry, GitError>;
    async fn remove(&self, path: &Path, force: bool) -> Result<(), GitError>;
    async fn prune(&self, repo_path: &Path) -> Result<(), GitError>;
}

pub struct CliWorktreeManager {
    runner: Arc<dyn GitRunner>,
    service: Weak<GitService>,
}

impl CliWorktreeManager {
    pub fn new(runner: Arc<dyn GitRunner>, service: Weak<GitService>) -> Self {
        Self { runner, service }
    }
}
```

#### Regras de Negócio e Segurança em `worktree.rs`
1. **`list(repo_path)`:**
   - Executa `git -C <repo_path> worktree list --porcelain -z` (timeout 10 s).
   - Faz o parse dos blocos separados por NUL duplo (`\0\0`):
     - `worktree <path>`
     - `HEAD <oid>`
     - `branch refs/heads/<name>` (remove prefixo `refs/heads/`)
     - `detached` ⇒ `is_detached = true`
     - `locked [reason]` ⇒ `is_locked = true`
     - `prunable [reason]` ⇒ `is_prunable = true`
     - `bare` ⇒ tratado quando principal é bare.
   - A primeira entrada da lista é sempre `is_main = true` e `kind = None`. As demais têm `is_main = false` e `kind = Some(classify_worktree(Path::new(&path)))`.
2. **`create(req)`:**
   - Valida `req.name`: regex `^[A-Za-z0-9._-]{1,100}$`, rejeita `"."`, `".."`, strings contendo `'/'`, `'\\'` ou `'\0'` com `GitError::InvalidName(req.name)`.
   - Descobre o repositório a partir de `req.repo_path` e obtém a worktree principal via `list`. Se a principal for `bare` ou não tiver `work_tree` válido, retorna `GitError::WorktreeRootUnavailable`.
   - Adquire o mutex por `common_dir` (`service.worktree_lock(&common_dir)`).
   - Determina a pasta contêiner: `.max-code-worktrees` (`WorktreeLocation::MaxCode`) ou `.worktrees` (`WorktreeLocation::Project`).
   - Verifica se o contêiner já está ignorado no repositório principal (`git -C <main_work_tree> check-ignore -q <container>/`). Se `req.ensure_gitignore == false` e não estiver ignorado, ou se a política exigir contêiner ignorado na principal sem modificá-la, tratamos conforme `spec.md` §2.8:
     - **Nunca** editar silenciosamente `.gitignore` da worktree principal! Verificar `git check-ignore`; se `req.ensure_gitignore == true`, após criar a worktree, garantir a linha `/.max-code-worktrees/` ou `/.worktrees/` no `.gitignore` **dentro da nova worktree criada** (de forma idempotente), caso a principal ainda não a ignore (ou retornar `GitError::WorktreeContainerNotIgnored` se a principal bloquear).
   - Calcula `target_path = main_work_tree.join(container).join(&req.name)`. Verifica se `target_path` já existe no disco ou na lista de worktrees ⇒ `GitError::WorktreeExists`.
   - Se `req.new_branch` for `Some(branch)`: valida com `git -C <main_work_tree> check-ref-format --branch <branch>`; falha ⇒ `GitError::InvalidName(branch)`.
   - Se `req.base` for `Some(base)`: resolve previamente para OID de commit com `git -C <main_work_tree> rev-parse --verify --end-of-options <base>^{commit}`; falha ⇒ `GitError::InvalidName(base)`.
   - Executa com hooks desabilitados e timeout de 120 s:
     `git -C <main_work_tree> -c core.hooksPath=/dev/null worktree add [-b <branch>] -- <target_path> [<resolved_base_oid>]`.
   - Pós-canonicalização: verifica que `dunce::canonicalize(&target_path)` é estritamente filho de `dunce::canonicalize(main_work_tree.join(container))`.
   - Notifica `service.refresh_repo_by_common_dir(&common_dir, RefreshReason::Manual)` e retorna o `WorktreeEntry` recém-criado consultando `list`.
3. **`remove(path, force)`:**
   - Canonicaliza `path` e localiza o repositório e a lista de worktrees (`list`).
   - Adquire o mutex por `common_dir`.
   - Se a entrada correspondente for `is_main == true` ⇒ retorna `Err(GitError::WorktreeIsMain)`.
   - Se `is_locked == true` ⇒ retorna `Err(GitError::WorktreeLocked(path))`.
   - Verifica `service.has_active_or_reserved_inside(&canonical_path)`: se houver qualquer painel aberto ou `cwd` reservado em abertura dentro da worktree, retorna `Err(GitError::WorktreeInUse(pane_ids))` **mesmo quando `force == true`**.
   - Executa `git -C <main_work_tree> worktree remove [--force] -- <canonical_path>` (timeout 30 s). Jamais usar `std::fs::remove_dir_all` em caminho arbitrário.
   - Dispara refresh manual nos painéis do repositório.
4. **`prune(repo_path)`:**
   - Adquire o mutex por `common_dir`.
   - Verifica entradas `is_prunable` e garante que nenhuma está em uso ou bloqueada; executa `git -C <repo_path> worktree prune --verbose` e dispara refresh.
5. **Doc-test Obrigatório (`src/worktree.rs`):**
   - Documentar `WorktreeManager` com exemplo compilável em doc-test (`cargo test --doc -p mt-git`) demonstrando `WorktreeCreateRequest` com `WorktreeLocation::MaxCode`, conforme critério de aceite §5.4.
