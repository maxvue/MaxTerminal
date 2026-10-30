# Implementação 01 — Crate `mt-commands` e Descoberta Artisan com Cache Compatível (`spec.md`)

> **Fase:** 14 — Comandos Rápidos: Artisan (`>`) e Atalhos Universais (`/`)
> **Unidade:** `01 - Crate mt-commands e Descoberta Artisan com Cache Compativel`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §8.1, [`../instructions.md`](../instructions.md) (I-01 a I-04, CA-05 a CA-11, CA-33, CA-34, CA-37, CA-38) e [`../spec.md`](../spec.md) (§1 a §4).

---

## 1. Contexto, Objetivo e Escopo

Esta primeira implementação estabelece a fundação do crate nativo `mt-commands` e entrega integralmente o subsistema de descoberta, cache compatível com o MaxCode e invalidação de comandos PHP Artisan (`mt-commands::artisan`, exceto o submódulo `generator`, que pertence à Implementação 02, e o módulo `shortcuts`, que pertence à Implementação 03).

### 1.1. Objetivos Específicos
1. Configurar o crate `src-tauri/crates/mt-commands` no workspace Cargo, isolado de `tauri`, `mt-pty` e `mt-projects` por meio das traits de fronteira em `ports.rs`.
2. Definir todos os códigos de erro semânticos `artisan.*` e `shortcut.*` em `error.rs` com mensagens em Português do Brasil (`pt-BR`).
3. Implementar utilitários seguros de sistema de arquivos (`fs_util.rs`) com escrita atômica para cache, criação exclusiva (`O_EXCL` / `RENAME_NOREPLACE`), leitura limitada (`read_capped`), carimbo de arquivo (`file_stamp`) e verificação estrita de fronteira e ausência de symlinks (`is_inside` / travessia ancorada).
4. Implementar `RootResolver` (`artisan/root.rs`) capaz de localizar a raiz Laravel a partir do `cwd` do painel, respeitando fronteiras de Git Worktree (`.max-code-worktrees/`, `.worktrees/`, `.git`), detectando layout (`Modern`, `Legacy`, `Unknown`), versão do `laravel/framework` via `composer.lock` e calculando `invoker` e `invoker_tokens`.
5. Implementar os runners (`LocalPhpRunner`, `SailRunner` opcional e `RunnerSelector` em `artisan/runner.rs`) executando `php artisan list --format=json --no-interaction --no-ansi` fora do PTY com `tokio::process::Command` sem shell, `stdin` nulo, `env_clear()` + allowlist, novo grupo de processos (`process_group(0)`), drenagem concorrente de `stdout` (limite 8 MiB) e `stderr` (ring buffer 64 KiB, sanitizado via `mt_db::sanitizer::sanitize` e truncado a ≤ 4 KiB em diagnósticos) e extermínio garantido do grupo (`kill(-pgid, SIGKILL)`) em timeout (15 s) ou cancelamento.
6. Implementar o parser do JSON do Symfony Console (`artisan/parser.rs`), suportando mapas vazios serializados pelo PHP como `[]`, preservando a ordem posicional de argumentos via `IndexMap`, identificando opções globais por interseção, omitindo comandos `hidden: true` e normalizando prefixos `--` e atalhos.
7. Implementar a camada de cache em disco (`artisan/cache.rs`) gravando `<raiz>/.max-code/commands/artisan.json` (array JSON indentado com 2 espaços no schema exato do MaxCode acrescido de campos opcionais estendidos), o sidecar `<raiz>/.max-code/commands/artisan.meta.json` e garantindo `<raiz>/.max-code/.gitignore` com `*\n` sem sobrescrever.
8. Implementar o cálculo de fingerprint (`artisan/fingerprint.rs`) e o serviço `ArtisanService` (`artisan/service.rs`) com single-flight por raiz, *stale-while-revalidate* para caches legados/estrangeiros ou TTL expirado (24 h), backoff de falha (5 s) e invalidação explícita.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Modificações no Workspace Rust
- `src-tauri/Cargo.toml` — inclusão de `"crates/mt-commands"` em `[workspace.members]` e `[workspace.dependencies]` se aplicável.

### 2.2. Novos Arquivos no Crate `src-tauri/crates/mt-commands/`
- `src-tauri/crates/mt-commands/Cargo.toml`
- `src-tauri/crates/mt-commands/src/lib.rs`
- `src-tauri/crates/mt-commands/src/error.rs`
- `src-tauri/crates/mt-commands/src/ports.rs`
- `src-tauri/crates/mt-commands/src/fs_util.rs`
- `src-tauri/crates/mt-commands/src/artisan/mod.rs`
- `src-tauri/crates/mt-commands/src/artisan/model.rs`
- `src-tauri/crates/mt-commands/src/artisan/root.rs`
- `src-tauri/crates/mt-commands/src/artisan/runner.rs`
- `src-tauri/crates/mt-commands/src/artisan/parser.rs`
- `src-tauri/crates/mt-commands/src/artisan/cache.rs`
- `src-tauri/crates/mt-commands/src/artisan/fingerprint.rs`
- `src-tauri/crates/mt-commands/src/artisan/service.rs`

### 2.3. Testes e Fixtures a Criar
- `src-tauri/crates/mt-commands/tests/fixtures/artisan_list_laravel11.json`
- `src-tauri/crates/mt-commands/tests/fixtures/artisan_list_laravel10.json`
- `src-tauri/crates/mt-commands/tests/fixtures/maxcode_cache_legacy.json`
- `src-tauri/crates/mt-commands/tests/artisan_cache.rs`
- `src-tauri/crates/mt-commands/tests/artisan_runner.rs`

---

## 3. Especificação Técnica e Assinaturas Exatas

### 3.1. Manifesto `src-tauri/crates/mt-commands/Cargo.toml`

```toml
[package]
name = "mt-commands"
version = "0.1.0"
edition = "2021"

[dependencies]
mt-core = { path = "../mt-core" }
mt-db = { path = "../mt-db", default-features = false }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
schemars = "1"
specta = { workspace = true }
tokio = { workspace = true, features = ["process", "time", "sync", "fs", "io-util", "macros", "rt"] }
async-trait = "0.1"
thiserror = { workspace = true }
tracing = { workspace = true }
sha2 = "0.10"
walkdir = "2"
dashmap = "6"
indexmap = { version = "2", features = ["serde"] }
libc = "0.2"

[dev-dependencies]
tempfile = "3"
insta = { version = "1", features = ["json"] }
pretty_assertions = "1"
tokio = { workspace = true, features = ["test-util", "rt-multi-thread"] }
```

### 3.2. Códigos de Erro e Construtores (`src/error.rs`)

```rust
use mt_core::{ErrorCode, MtError};
use serde_json::json;
use std::path::Path;

pub mod codes {
    use super::ErrorCode;

    pub const NOT_LARAVEL: ErrorCode = ErrorCode("artisan.not_laravel");
    pub const ROOT_FORBIDDEN: ErrorCode = ErrorCode("artisan.root_forbidden");
    pub const PHP_NOT_FOUND: ErrorCode = ErrorCode("artisan.php_not_found");
    pub const TIMEOUT: ErrorCode = ErrorCode("artisan.timeout");
    pub const EXIT_NONZERO: ErrorCode = ErrorCode("artisan.exit_nonzero");
    pub const INVALID_JSON: ErrorCode = ErrorCode("artisan.invalid_json");
    pub const OUTPUT_TOO_LARGE: ErrorCode = ErrorCode("artisan.output_too_large");
    pub const INVALID_FORM: ErrorCode = ErrorCode("artisan.invalid_form");
    pub const FILE_EXISTS: ErrorCode = ErrorCode("artisan.file_exists");
    pub const UNSAFE_PATH: ErrorCode = ErrorCode("artisan.unsafe_path");
    pub const WRITE_FAILED: ErrorCode = ErrorCode("artisan.write_failed");
    pub const CONSOLE_ROUTES_FAILED: ErrorCode = ErrorCode("artisan.console_routes_failed");
    pub const PREVIEW_STALE: ErrorCode = ErrorCode("artisan.preview_stale");
    pub const PARTIAL_WRITE: ErrorCode = ErrorCode("artisan.partial_write");
    pub const RUNNER_UNSUPPORTED: ErrorCode = ErrorCode("artisan.runner_unsupported");
    pub const SHORTCUT_INVALID_CWD: ErrorCode = ErrorCode("shortcut.invalid_cwd");
    pub const SHORTCUT_UNRESOLVED: ErrorCode = ErrorCode("shortcut.unresolved");
    pub const MANAGER_CONFLICT: ErrorCode = ErrorCode("shortcut.manager_conflict");
    pub const UNSUPPORTED_CAPABILITY: ErrorCode = ErrorCode("shortcut.unsupported_capability");
}

pub fn not_laravel(path: &Path) -> MtError {
    MtError::new(
        codes::NOT_LARAVEL,
        format!("O diretório '{}' não é uma raiz de projeto Laravel válida.", path.display()),
    )
}

pub fn root_forbidden(path: &Path) -> MtError {
    MtError::new(
        codes::ROOT_FORBIDDEN,
        format!("O caminho '{}' não pertence a nenhum projeto autorizado.", path.display()),
    )
}

pub fn php_not_found() -> MtError {
    MtError::new(
        codes::PHP_NOT_FOUND,
        "PHP não encontrado no PATH. Instale o PHP ou inicie o Sail (`./vendor/bin/sail up -d`).",
    )
}

pub fn timeout(secs: u64) -> MtError {
    MtError::new(
        codes::TIMEOUT,
        format!("A descoberta de comandos Artisan excedeu {secs} s e foi interrompida."),
    )
}

pub fn exit_nonzero(status: Option<i32>, stderr_tail: String) -> MtError {
    MtError::new(
        codes::EXIT_NONZERO,
        format!(
            "A execução de `php artisan list` falhou (código {}).",
            status.map(|c| c.to_string()).unwrap_or_else(|| "sinal".to_string())
        ),
    )
    .with_details(json!({ "exitCode": status, "stderrTail": stderr_tail }))
}

pub fn invalid_json(reason: impl Into<String>) -> MtError {
    MtError::new(
        codes::INVALID_JSON,
        "A saída de `php artisan list --format=json` não contém um JSON válido.",
    )
    .with_details(json!({ "reason": reason.into() }))
}

pub fn output_too_large(max_bytes: usize) -> MtError {
    MtError::new(
        codes::OUTPUT_TOO_LARGE,
        format!("A saída do comando Artisan excedeu o limite seguro de {} MiB.", max_bytes / (1024 * 1024)),
    )
}

pub fn unsafe_path(path: &Path, reason: &str) -> MtError {
    MtError::new(
        codes::UNSAFE_PATH,
        format!("Caminho inseguro rejeitado ('{}'): {reason}", path.display()),
    )
}

pub fn runner_unsupported(reason: &str) -> MtError {
    MtError::new(
        codes::RUNNER_UNSUPPORTED,
        format!("Runner Artisan não suportado neste ambiente: {reason}"),
    )
}
```

### 3.3. Portas e Limites (`src/ports.rs`)

```rust
use crate::artisan::model::{
    CatalogUpdatedPayload, CommandCreatedPayload, DiscoveryFailedPayload, RunnerKind,
};
use mt_core::{MtResult, ProjectId};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug)]
pub struct RunLimits {
    pub timeout: Duration,
    pub max_stdout: usize,
    pub max_stderr: usize,
}

impl Default for RunLimits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(15),
            max_stdout: 8 * 1024 * 1024,
            max_stderr: 64 * 1024,
        }
    }
}

#[async_trait::async_trait]
pub trait ArtisanRunner: Send + Sync + 'static {
    fn kind(&self) -> RunnerKind;
    /// Prefixo inserido no input (relativo ao cwd da aba): "php artisan", "php ../artisan", "./vendor/bin/sail artisan".
    fn invoker(&self, root: &Path, cwd: &Path) -> String;
    /// Tokens estruturados do invocador relativos ao cwd.
    fn invoker_tokens(&self, root: &Path, cwd: &Path) -> Vec<String>;
    async fn list_json(&self, root: &Path, limits: RunLimits) -> MtResult<Vec<u8>>;
}

pub trait CommandsEventSink: Send + Sync + 'static {
    fn catalog_updated(&self, payload: CatalogUpdatedPayload);
    fn discovery_failed(&self, payload: DiscoveryFailedPayload);
    fn command_created(&self, payload: CommandCreatedPayload);
}

pub trait ProjectLookup: Send + Sync + 'static {
    /// Verifica se o caminho canônico pertence a algum projeto cadastrado (incluindo worktrees vinculadas).
    /// Se `expected_project` for informado, exige correspondência com o projeto proprietário.
    fn owning_project(&self, path: &Path) -> Option<(ProjectId, PathBuf)>;
}

pub trait MaxCodeConfigLocator: Send + Sync + 'static {
    /// Retorna os candidatos ou o arquivo conhecido de configuração `config-trabalho.json` do MaxCode.
    fn config_files(&self) -> Vec<PathBuf>;
}

pub trait Clock: Send + Sync + 'static {
    fn now_ms(&self) -> i64;
}

#[derive(Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}
```

### 3.4. Utilitários de Sistema de Arquivos (`src/fs_util.rs`)

```rust
use crate::artisan::fingerprint::FileStamp;
use crate::error;
use mt_core::{MtError, MtResult};
use std::io;
use std::path::{Path, PathBuf};

/// Verifica que `root` é um diretório real sem symlinks na cadeia ancestral de `candidate` dentro de `root`,
/// garantindo que `candidate` permanece estritamente dentro de `root` canônico.
pub fn ensure_safe_under_root(root: &Path, relative_or_abs: &Path) -> MtResult<PathBuf>;

/// Verifica se um caminho existente está estritamente dentro da raiz canônica sem atravessar symlinks relativos.
pub fn is_inside(root: &Path, candidate: &Path) -> bool;

/// Cria diretórios filhos dentro de `root` validando componente a componente que nenhum segmento é symlink.
pub fn ensure_dir_anchored(root: &Path, rel_dir: &Path) -> MtResult<PathBuf>;

/// Escrita atômica para arquivos de cache regeneráveis (`.max-code/commands/artisan.json` e `artisan.meta.json`).
/// Grava `path.tmp-<pid>-<counter>` no mesmo diretório validado, executa `sync_all()` e faz `rename`.
pub fn atomic_write_under_root(root: &Path, target: &Path, bytes: &[u8]) -> MtResult<()>;

/// Criação estritamente exclusiva (`O_EXCL` / `create_new(true)` + `sync_all()`).
/// Mapeia `AlreadyExists` para `artisan.file_exists`.
pub fn create_new_file_under_root(root: &Path, target: &Path, bytes: &[u8]) -> MtResult<()>;

/// Lê um arquivo como UTF-8 até `max_bytes`. Retorna `Ok(None)` se `NotFound`, ou erro se exceder `max_bytes`
/// ou se for um symlink apontando para fora da raiz.
pub fn read_capped(path: &Path, max_bytes: usize) -> io::Result<Option<String>>;

/// Calcula o `FileStamp` (`mtime_ns`, `size`, `content_sha256`) de um arquivo regular (não segue symlink).
pub fn file_stamp(path: &Path, max_bytes: usize) -> io::Result<Option<FileStamp>>;

/// Formata um token POSIX seguro com aspas simples quando contiver espaços ou metacaracteres shell.
pub fn posix_quote_token(raw: &str) -> String;
```

### 3.5. Modelos de Dados (`src/artisan/model.rs`)

```rust
use mt_core::ProjectId;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanCommand {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<ArtisanOption>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arguments: Vec<ArtisanArgument>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanOption {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shortcut: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accept_value: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_value_required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_multiple: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_global: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanArgument {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_array: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    Local,
    Sail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LaravelLayout {
    Modern,
    Legacy,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CatalogSource {
    Fresh,
    Cache,
    ForeignCache,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanRoot {
    pub root: String,
    pub project_id: Option<ProjectId>,
    pub is_worktree: bool,
    pub invoker: String,
    pub invoker_tokens: Vec<String>,
    pub runner: RunnerKind,
    pub layout: LaravelLayout,
    pub laravel_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ArtisanCatalog {
    pub root: String,
    pub commands: Vec<ArtisanCommand>,
    pub source: CatalogSource,
    pub stale: bool,
    pub generated_at_ms: Option<i64>,
    pub last_error: Option<CatalogErrorInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogErrorInfo {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CatalogUpdatedPayload {
    pub root: String,
    pub project_id: Option<ProjectId>,
    pub revision: u64,
    pub request_id: String,
    pub command_count: u32,
    pub source: CatalogSource,
    pub generated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryFailedPayload {
    pub root: String,
    pub project_id: Option<ProjectId>,
    pub revision: u64,
    pub request_id: String,
    pub error: CatalogErrorInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandCreatedPayload {
    pub root: String,
    pub project_id: Option<ProjectId>,
    pub revision: u64,
    pub request_id: String,
    pub path: String,
    pub class_name: String,
    pub signature_name: String,
}
```

### 3.6. Resolução de Raiz Laravel (`src/artisan/root.rs`)

```rust
pub struct RootResolver {
    lookup: Arc<dyn ProjectLookup>,
    clock: Arc<dyn Clock>,
    memo: DashMap<(Option<ProjectId>, PathBuf), (i64, Option<ArtisanRoot>)>,
    ttl_ms: i64, // 30_000 ms
}

impl RootResolver {
    pub fn new(lookup: Arc<dyn ProjectLookup>, clock: Arc<dyn Clock>) -> Self;
    pub fn resolve(
        &self,
        cwd: &Path,
        expected_project: Option<ProjectId>,
        runner: &dyn ArtisanRunner,
    ) -> MtResult<Option<ArtisanRoot>>;
    pub fn ensure_authorized_laravel_root(&self, root: &Path) -> MtResult<(ProjectId, PathBuf)>;
    pub fn invalidate_all(&self);
}

pub fn is_laravel_composer(composer_json_path: &Path) -> MtResult<bool>;
pub fn is_worktree(dir: &Path) -> bool;
pub fn has_git_boundary(dir: &Path) -> bool;
pub fn detect_layout(root: &Path) -> LaravelLayout;
pub fn detect_laravel_version(root: &Path) -> Option<String>;
pub fn detect_console_routes_loaded(root: &Path) -> bool;
```

**Regras de Negócio da Resolução (`root.rs`):**
- `cwd` inexistente retorna `MtError` com `ErrorCode("core.not_found")`.
- Se `lookup.owning_project(&canonical_cwd)` retornar `None` ou se `expected_project` for `Some(pid)` divergente do `owner_id` retornado, retorna `Ok(None)` em `resolve` e `Err(error::root_forbidden(root))` em `ensure_authorized_laravel_root`.
- Sobe diretório a diretório a partir de `canonical_cwd`:
  - Em cada ancestral `dir`, se `dir.join("artisan").is_file()` (e não for symlink para fora) **e** `is_laravel_composer(&dir.join("composer.json"))?` for `true` (`require` ou `require-dev` contém `"laravel/framework"`), retorna `Some(ArtisanRoot)`. Projetos Lumen (`"laravel/lumen-framework"` sem `"laravel/framework"`) retornam `false`.
  - Se `dir == project_path` **ou** `has_git_boundary(dir)` (`dir.join(".git")` existe como arquivo ou pasta), interrompe a subida imediatamente e retorna `Ok(None)`. Isso impede que uma worktree sem `artisan` atravesse `.git` e capture a raiz principal (CA-10, CA-11).
- `is_worktree(dir)`: `true` se qualquer componente do caminho for `.max-code-worktrees` ou `.worktrees`, ou se `dir.join(".git")` for um arquivo regular começando com `gitdir:`.
- `invoker_tokens` para `RunnerKind::Local`: `["php", "<rel_artisan>"]`, onde `<rel_artisan>` é `"artisan"` se `cwd == root` ou o caminho relativo de `cwd` para `root.join("artisan")` (ex.: `"../artisan"`). `invoker` é a junção com cada token escapado via `posix_quote_token` quando necessário (ex.: `"php '../Meu Projeto/artisan'"`).

### 3.7. Runners e Supervisor de Processos (`src/artisan/runner.rs`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunnerPreference {
    #[default]
    Auto,
    Local,
    Sail,
}

pub struct LocalPhpRunner {
    php_bin: Option<PathBuf>,
}

pub struct SailRunner {
    docker_bin: Option<PathBuf>,
}

pub struct RunnerSelector {
    preference: RunnerPreference,
    local: Arc<LocalPhpRunner>,
    sail: Arc<SailRunner>,
}

impl RunnerSelector {
    pub fn new(preference: RunnerPreference) -> Self;
    pub fn with_local_php_bin(php_bin: PathBuf) -> Self;
    pub async fn pick(&self, root: &Path) -> Arc<dyn ArtisanRunner>;
}

pub async fn run_bounded(mut cmd: tokio::process::Command, limits: RunLimits) -> MtResult<Vec<u8>>;
```

**Regras Críticas de `run_bounded` (CA-09, CA-38):**
1. Antes de `spawn()`, configura:
   - `stdin(Stdio::null())`, `stdout(Stdio::piped())`, `stderr(Stdio::piped())`, `kill_on_drop(true)`.
   - Em Unix (`#[cfg(unix)]`), chama `cmd.process_group(0)` para que o processo filho seja líder de um novo grupo de processos (`pgid == child_pid`).
2. Se `cmd.spawn()` falhar com `io::ErrorKind::NotFound`, retorna `error::php_not_found()`.
3. Duas tasks assíncronas drenam simultaneamente `stdout` e `stderr`:
   - `stdout`: lê chunks de 16 KiB até `limits.max_stdout` (8 MiB). Se chegar 1 byte além de `max_stdout`, interrompe imediatamente sinalizando `Overflow`.
   - `stderr`: lê continuamente até EOF em um ring buffer circular de `limits.max_stderr` (64 KiB), descartando os bytes mais antigos sem jamais fechar ou parar de ler o pipe enquanto o processo estiver vivo (evita deadlock quando o processo escreve muito no `stderr`).
4. `tokio::time::timeout(limits.timeout, ...)` supervisiona a conclusão:
   - Em caso de timeout, overflow de stdout ou cancelamento/drop do futuro (via `Drop` guard que armazena `pgid: Option<i32>`), envia ` unsafe { libc::kill(-(pgid as libc::pid_t), libc::SIGKILL) } `, aguarda `child.wait().await` para evitar processos zumbis e aguarda o encerramento das tasks de leitura.
5. Ao terminar com `exit != 0`:
   - Extrai o conteúdo do ring buffer de `stderr` (descartando a primeira linha parcial se houve rotação no buffer para não quebrar sequências UTF-8 nem vazar fragmentos de segredos).
   - Aplica `mt_db::sanitizer::sanitize(&stderr_str)` **antes** de truncar os últimos 4 KiB (`4096` bytes em fronteira de caractere UTF-8).
   - Retorna `error::exit_nonzero(status.code(), sanitized_tail)`.

### 3.8. Parser Symfony Console (`src/artisan/parser.rs`)

```rust
pub fn parse_artisan_list(bytes: &[u8]) -> MtResult<Vec<ArtisanCommand>>;
```

**Regras Críticas do Parser (CA-06):**
- Procura o primeiro `{` que inicia o JSON do Symfony Console (tolerando warnings/notices emitidos pelo PHP no stdout antes do JSON); se não houver `{` ou `serde_json::from_slice` falhar, retorna `error::invalid_json(...)`.
- Usa um deserializer customizado `map_or_empty_array` para `definition.arguments` (`IndexMap<String, RawArgument>`) e `definition.options` (`IndexMap<String, RawOption>`), pois o PHP serializa arrays associativos vazios `[]` como lista JSON `[]` em vez de `{}`.
- Filtra comandos com `hidden == true` ou `name.trim().is_empty()`, e rejeita nomes que contenham caracteres de controle ou metacaracteres shell (`;`, `|`, `&`, `` ` ``, `$`, `\n`, `\r`).
- Calcula o conjunto de opções globais (`globals: HashSet<String>`) como a interseção das chaves de opções presentes em todos os comandos visíveis (quando houver ≥ 2 comandos visíveis; caso haja 1 comando, compara com a lista padrão `["help", "quiet", "verbose", "version", "ansi", "no-ansi", "no-interaction", "env"]`).
- Normaliza cada comando:
  - `name` da opção sempre com prefixo `--` (se vier `"database"`, vira `"--database"`).
  - `shortcut`: `trim()`, se vazio → `None`; se não começar com `-`, prefixa `-` em cada segmento separado por `|`.
  - `description`: sanitizada via `mt_db::sanitizer::sanitize`, `trim()`, vazia → `None`.
  - `default`: se `Value::Null`, `Value::Bool(false)` (quando `accept_value == false`) ou array vazio `[]` → `None`.
- Ordena o vetor resultante por `a.name.cmp(&b.name)`.

### 3.9. Cache Compatível com MaxCode e Fingerprint (`src/artisan/cache.rs`, `src/artisan/fingerprint.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileStamp {
    pub mtime_ns: i64,
    pub size: u64,
    pub content_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirStamp {
    pub file_count: u32,
    pub entries_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fingerprint {
    pub artisan: Option<FileStamp>,
    pub composer_json: Option<FileStamp>,
    pub composer_lock: Option<FileStamp>,
    pub bootstrap_app: Option<FileStamp>,
    pub console_routes: Option<FileStamp>,
    pub commands_dir: DirStamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheMeta {
    pub schema: u32,
    pub generator: String,
    pub generator_version: String,
    pub generated_at_ms: i64,
    pub artisan_json_sha256: String,
    pub fingerprint: Fingerprint,
    pub runner: RunnerKind,
    pub laravel_version: Option<String>,
    pub command_count: u32,
}

pub enum DiskCacheState {
    Fresh {
        commands: Vec<ArtisanCommand>,
        meta: CacheMeta,
    },
    StaleForeign {
        commands: Vec<ArtisanCommand>,
    },
    StaleTtl {
        commands: Vec<ArtisanCommand>,
        meta: CacheMeta,
    },
    Outdated {
        commands: Vec<ArtisanCommand>,
        meta: CacheMeta,
    },
    Missing,
}

pub fn compute_fingerprint(root: &Path) -> MtResult<Fingerprint>;
pub fn read_disk_cache(
    root: &Path,
    current_fp: &Fingerprint,
    now_ms: i64,
    ttl_ms: i64,
) -> MtResult<DiskCacheState>;
pub fn write_disk_cache(
    root: &Path,
    commands: &[ArtisanCommand],
    fingerprint: Fingerprint,
    runner: RunnerKind,
    laravel_version: Option<String>,
    now_ms: i64,
) -> MtResult<(String, CacheMeta)>;
```

**Regras Críticas do Cache e Fingerprint (CA-05, CA-06, CA-07, CA-08, CA-37):**
- `write_disk_cache`:
  1. Garante `<raiz>/.max-code` e `<raiz>/.max-code/commands` via `ensure_dir_anchored`.
  2. Tenta criar `<raiz>/.max-code/.gitignore` com conteúdo `"*\n"` usando `create_new(true)` (se já existir, ignora silenciosamente sem sobrescrever).
  3. Serializa `commands` como array JSON indentado com 2 espaços (`serde_json::to_vec_pretty`), calcula `artisan_json_sha256` (SHA-256 hex minúsculo dos bytes exatos), grava `artisan.json` via `atomic_write_under_root` e em seguida grava `artisan.meta.json` via `atomic_write_under_root`.
- `compute_fingerprint`:
  - Percorre `app/Console/Commands` com `walkdir::WalkDir::new(...).follow_links(false).max_depth(6)`.
  - Se o número de arquivos `.php` exceder `5_000` ou se encontrar erro de leitura/limite, retorna erro em vez de produzir um `DirStamp` truncado.
  - Ordena os caminhos relativos e acumula no SHA-256 `rel_path \0 mtime_ns \0 size \0 content_sha256 \n` de **cada** arquivo `.php` (detecta edições em qualquer arquivo, inclusive quando mtime ou tamanho não mudam ou em arquivos que não tinham o maior `mtime`).
- `read_disk_cache`:
  - Lê `artisan.json` com limite de 8 MiB e calcula o `sha256` dos bytes em disco.
  - Valida cada `ArtisanCommand` lido (rejeita nomes vazios ou com metacaracteres shell; normaliza `--` nas opções).
  - Se `artisan.meta.json` não existir, for inválido, `meta.generator != "maxterminal"` ou `meta.artisan_json_sha256 != disk_sha256`: retorna `DiskCacheState::StaleForeign { commands }` (produz `source: CatalogSource::ForeignCache, stale: true` e dispara revalidação em background — CA-07).
  - Se `meta.fingerprint != *current_fp`: retorna `DiskCacheState::Outdated { commands, meta }` (força redescoberta síncrona, caindo para o cache antigo com `stale: true` + `last_error` se a redescoberta falhar — CA-08).
  - Se `now_ms - meta.generated_at_ms > ttl_ms`: retorna `DiskCacheState::StaleTtl { commands, meta }`.
  - Caso contrário: retorna `DiskCacheState::Fresh { commands, meta }`.

### 3.10. Serviço `ArtisanService` (`src/artisan/service.rs`)

```rust
pub struct ArtisanService {
    roots: Arc<RootResolver>,
    runners: Arc<RunnerSelector>,
    sink: Arc<dyn CommandsEventSink>,
    clock: Arc<dyn Clock>,
    locks: DashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>,
    memory: DashMap<PathBuf, Arc<MemEntry>>,
    revisions: DashMap<PathBuf, Arc<AtomicU64>>,
    backoff_until: DashMap<PathBuf, (i64, MtError)>,
    revalidating: DashMap<PathBuf, tokio::task::JoinHandle<()>>,
    limits: RunLimits,
    ttl: Duration,
    backoff_duration_ms: i64, // 5_000 ms
}

impl ArtisanService {
    pub fn new(
        roots: Arc<RootResolver>,
        runners: Arc<RunnerSelector>,
        sink: Arc<dyn CommandsEventSink>,
        clock: Arc<dyn Clock>,
    ) -> Arc<Self>;

    pub fn with_limits(self: Arc<Self>, limits: RunLimits, ttl: Duration, backoff_ms: i64) -> Arc<Self>;
    pub async fn get(self: &Arc<Self>, root: &Path) -> MtResult<ArtisanCatalog>;
    pub async fn refresh(self: &Arc<Self>, root: &Path) -> MtResult<ArtisanCatalog>;
    pub fn invalidate(&self, root: &Path);
    pub fn next_revision(&self, root: &Path) -> u64;
    pub async fn shutdown(&self);
}
```

**Regras de Concorrência do `ArtisanService` (CA-05, CA-37):**
- `fresh_from_memory(root)` verifica se há `MemEntry` na memória **e** recalcula em `tokio::task::spawn_blocking` o `compute_fingerprint(root)` + hash rápido do `artisan.json` e `artisan.meta.json`. Assim, se o MaxCode regravar `artisan.json` externamente enquanto a memória do MaxTerminal está quente, a divergência é detectada imediatamente (CA-37).
- `get(root)` adquire o `tokio::sync::Mutex<()>` específico de `root`: 10 chamadas concorrentes para a mesma raiz resultam em **exatamente 1** execução do runner (CA-05).
- Quando não há cache e a descoberta falha, `backoff_until` armazena `(now_ms + 5000, err.clone())` e emite `discovery_failed`. Chamadas subsequentes a `get(root)` dentro da janela de 5 s retornam o erro imediatamente sem disparar novos subprocessos `php` a cada tecla digitada. `refresh(root)` limpa o backoff e força nova tentativa sob o mesmo lock single-flight.

---

## 4. Tratamento de Erros e Casos de Borda

1. **Warnings PHP antes do JSON (`display_errors=On`):** `Deprecated: ... \n{"application":...}` é parseado normalmente localizando o primeiro `{` válido; caso não seja JSON válido, retorna `artisan.invalid_json`.
2. **Processo filho que faz fork e mantém stdout aberto:** `process_group(0)` + `kill(-pgid, SIGKILL)` no fim ou timeout garante que nenhum sub-processo segure o pipe ou permaneça órfão (CA-09, CA-38).
3. **Disco somente leitura ou falha ao gravar `.max-code/commands/artisan.json`:** Registrado com `tracing::warn!`; `discover_locked` ainda atualiza `memory`, emite `artisan://catalog-updated` e retorna o `ArtisanCatalog` para não impedir o uso do popup.
4. **Symlinks maliciosos em `.max-code` ou `app/Console/Commands`:** `ensure_dir_anchored` rejeita symlinks com `artisan.unsafe_path` e `walkdir` roda com `follow_links(false)`.
5. **Segredos no `stderr` (ex.: `APP_KEY=base64:...`, `Bearer ...`, senhas em DSN):** Passam obrigatoriamente por `mt_db::sanitizer::sanitize` antes do truncamento de 4 KiB e antes de serem anexados a `MtError.details` ou eventos IPC.
