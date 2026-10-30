# Implementação 01 — Domínio de Projetos, Caminhos, Detecção de Stack e Identidade (`spec.md`)

> Parte 01 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Referências canônicas: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) §2.1–§2.5, §2.9, §10 e §11.1.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação inaugura o crate `mt-projects` no workspace Cargo do MaxTerminal, estabelecendo o modelo de domínio puro (independente de Tauri e de SQLite), a normalização e validação segura de caminhos no Linux, o motor determinístico de auto-detecção de stack tecnológica (Laravel, Vue, Tauri, Extensão VS Code, Python, Rust), o mapeamento de identidade visual (ícone Iconify + cor hexadecimal com fallback determinístico FNV-1a de 64 bits) e os contratos de eventos do domínio.

### 1.1. Escopo Incluído
- Criação do crate `src-tauri/crates/mt-projects` e registro no workspace `src-tauri/Cargo.toml`.
- Tipos de domínio e DTOs compatíveis com `serde` e `specta` (`model.rs`).
- Resolução, expansão de `~`, canonicalização e validação de caminhos com proteção contra symlinks inválidos, caminhos proibidos (`/` e `$HOME`) e caracteres não-UTF-8 (`paths.rs`).
- Motor de inspeção de diretórios `FolderProbe` (sem execução de processos, leitura regular limitada a 1 MiB + 1 byte com `O_NOFOLLOW | O_NONBLOCK` no Linux) e `StackDetector` com 6 regras ordenadas por prioridade (`detect.rs`).
- Mapa de identidade visual por stack, fallback determinístico FNV-1a (`FALLBACK_ICONS` e `FALLBACK_COLORS`) e validadores de título, ícone e cor (`identity.rs`).
- Envelopes e payloads de eventos (`ProjectEvent<T>`) e trait `ProjectEventSink` (`events.rs`).
- Testes unitários exaustivos cobrindo caminhos, detecção de stacks isoladas e combinadas, limites de `package.json`, vetores de hash FNV-1a e serialização de eventos.

### 1.2. Fora de Escopo desta Implementação
- Persistência SQLite (`mt-db`), varredura de nível 1 (`scanner.rs`) e watcher `notify` (`watcher.rs`) — cobertos na **Implementação 02**.
- `ProjectService` e comandos/eventos Tauri — cobertos na **Implementação 03**.
- Stores Pinia, composables e componentes Vue — cobertos nas **Implementações 04 e 05**.

---

## 2. Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Modificar | `src-tauri/Cargo.toml` | Adicionar `"crates/mt-projects"` em `[workspace.members]` e dependências compartilhadas (`notify`, `notify-debouncer-full`, `dirs`, `async-trait`) se aplicável |
| Criar | `src-tauri/crates/mt-projects/Cargo.toml` | Manifesto do crate `mt-projects` |
| Criar | `src-tauri/crates/mt-projects/src/lib.rs` | Exportação pública dos módulos do crate |
| Criar | `src-tauri/crates/mt-projects/src/model.rs` | Entidades, enums, `ProjectRootId` e DTOs de entrada/saída |
| Criar | `src-tauri/crates/mt-projects/src/paths.rs` | Expansão de `~`, canonicalização, validação de caminhos proibidos e ocultos |
| Criar | `src-tauri/crates/mt-projects/src/detect.rs` | `FolderProbe`, `StackRule`, `StackDetector` e `DetectionReport` |
| Criar | `src-tauri/crates/mt-projects/src/identity.rs` | `IdentityMap`, `fnv1a_64`, `validate_icon`, `validate_color`, `validate_title` |
| Criar | `src-tauri/crates/mt-projects/src/events.rs` | Payloads de eventos e trait `ProjectEventSink` |
| Criar | `src-tauri/crates/mt-projects/tests/detect_and_identity.rs` | Testes de integração para detecção de stack, identidade, caminhos e serialização |

---

## 3. Especificação Técnica Detalhada (Rust)

### 3.1. Manifesto `src-tauri/crates/mt-projects/Cargo.toml`

```toml
[package]
name = "mt-projects"
version = "0.1.0"
edition = "2024"

[dependencies]
mt-core = { path = "../mt-core" }
mt-db = { path = "../mt-db" }
serde = { workspace = true, features = ["derive"] }
serde_json = { workspace = true }
specta = { workspace = true, features = ["derive", "uuid"] }
tokio = { workspace = true, features = ["rt", "sync", "time", "macros"] }
tracing = { workspace = true }
notify = "8"
notify-debouncer-full = "0.5"
dirs = "6"
async-trait = "0.1"
uuid = { workspace = true, features = ["v7", "serde"] }

[dev-dependencies]
tempfile = "3"
tokio = { workspace = true, features = ["test-util", "rt-multi-thread", "macros"] }
```

### 3.2. Modelos e DTOs (`src-tauri/crates/mt-projects/src/model.rs`)

```rust
use std::fmt;
use std::str::FromStr;
use mt_core::ids::ProjectId;
use serde::{Deserialize, Serialize};
use specta::Type;
use uuid::Uuid;

/// Identificador único de uma "Pasta de Projetos" (Uuid v7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, Type)]
#[serde(transparent)]
pub struct ProjectRootId(pub Uuid);

impl ProjectRootId {
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for ProjectRootId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for ProjectRootId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for ProjectRootId {
    type Err = uuid::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Uuid::parse_str(s)?))
    }
}

/// Stacks reconhecidas pelo detector de projetos em ordem de prioridade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum StackKind {
    Tauri,
    VscodeExtension,
    Laravel,
    Vue,
    Rust,
    Python,
    Unknown,
}

impl StackKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Tauri => "tauri",
            Self::VscodeExtension => "vscode_extension",
            Self::Laravel => "laravel",
            Self::Vue => "vue",
            Self::Rust => "rust",
            Self::Python => "python",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "tauri" => Some(Self::Tauri),
            "vscode_extension" => Some(Self::VscodeExtension),
            "laravel" => Some(Self::Laravel),
            "vue" => Some(Self::Vue),
            "rust" => Some(Self::Rust),
            "python" => Some(Self::Python),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

/// Origem da identidade visual (ícone e cor) do projeto.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IdentitySource {
    Auto,
    Manual,
}

impl IdentitySource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "manual" => Some(Self::Manual),
            _ => None,
        }
    }
}

/// Modo de reação quando uma nova subpasta é descoberta em uma Pasta de Projetos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DiscoveryMode {
    Suggest,
    AutoImport,
}

impl DiscoveryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::AutoImport => "auto_import",
        }
    }

    pub fn from_db_str(value: &str) -> Option<Self> {
        match value {
            "suggest" => Some(Self::Suggest),
            "auto_import" => Some(Self::AutoImport),
            _ => None,
        }
    }
}

/// Estado em runtime do monitoramento de uma Pasta de Projetos.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WatchStatus {
    Watching,
    Disabled,
    Missing,
    Error,
}

/// Identidade visual resolvida para uma pasta de projeto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectIdentity {
    pub icon: String,
    pub color: String,
    pub stack: StackKind,
    pub detected: Vec<StackKind>,
}

/// Entidade principal de projeto cadastrada no MaxTerminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: ProjectId,
    pub title: String,
    pub icon: String,
    pub color: String,
    pub path: String,
    pub root_id: Option<ProjectRootId>,
    pub stack: StackKind,
    pub identity_source: IdentitySource,
    pub sort_order: i64,
    pub missing: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Entidade de "Pasta de Projetos" monitorada/cadastrada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRoot {
    pub id: ProjectRootId,
    pub path: String,
    pub discovery_mode: DiscoveryMode,
    pub watch_enabled: bool,
    pub watch_status: WatchStatus,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Payload para criação de um projeto individual.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    pub path: String,
    pub title: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

/// Payload de atualização parcial de um projeto existente.
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectPatch {
    pub title: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub reset_identity: Option<bool>,
}

/// Payload para adição de uma nova "Pasta de Projetos".
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct NewProjectRoot {
    pub path: String,
    pub discovery_mode: DiscoveryMode,
    pub import_existing: bool,
    pub watch_enabled: bool,
}

/// Payload de atualização de uma "Pasta de Projetos".
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRootPatch {
    pub discovery_mode: Option<DiscoveryMode>,
    pub watch_enabled: Option<bool>,
}

/// Subpasta descoberta em uma raiz e ainda não importada nem dispensada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredFolder {
    pub root_id: ProjectRootId,
    pub path: String,
    pub suggested_title: String,
    pub identity: ProjectIdentity,
    pub discovered_at: i64,
}

/// Aviso não fatal ocorrido durante varredura ou leitura de marcador.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ScanWarning {
    pub path: String,
    pub code: String,
    pub message: String,
}

/// Relatório retornado ao adicionar ou re-escanear uma Pasta de Projetos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RootImportReport {
    pub root: ProjectRoot,
    pub imported: Vec<Project>,
    pub suggested: Vec<DiscoveredFolder>,
    pub skipped: u32,
    pub warnings: Vec<ScanWarning>,
    pub complete: bool,
}

/// Snapshot consistente de todo o catálogo de projetos em runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSnapshot {
    pub epoch: String,
    pub revision: u32,
    pub projects: Vec<Project>,
    pub roots: Vec<ProjectRoot>,
    pub discovered: Vec<DiscoveredFolder>,
}
```

### 3.3. Resolução e Validação de Caminhos (`src-tauri/crates/mt-projects/src/paths.rs`)

```rust
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use mt_core::MtError;

/// Códigos de erro de caminho do domínio `project.*`:
/// - `project.path_not_found`
/// - `project.path_not_directory`
/// - `project.path_forbidden`
/// - `project.path_not_utf8`
/// - `project.invalid_argument`

/// Expande "~" ou "~/..." usando `dirs::home_dir()`.
/// Rejeita caminhos vazios, contendo NUL (`\0`), `~outro_usuario`, variáveis `$VAR` não expandidas ou caminhos relativos.
pub fn expand_home(input: &str) -> Result<PathBuf, MtError>;

/// Expande `~`, resolve symlinks com `std::fs::canonicalize`, valida que existe, é diretório e é UTF-8 válido.
pub fn canonical_dir(input: &str) -> Result<PathBuf, MtError>;

/// Valida e retorna o caminho canônico em UTF-8 para uso como Projeto Individual:
/// proíbe `/` e o próprio `$HOME` canonicalizado (`project.path_forbidden`).
pub fn validate_project_dir(input: &str) -> Result<PathBuf, MtError>;

/// Valida e retorna o caminho canônico em UTF-8 para uso como Pasta de Projetos (`ProjectRoot`):
/// proíbe `/` (`project.path_forbidden`), mas permite `$HOME` se selecionado explicitamente.
pub fn validate_root_dir(input: &str) -> Result<PathBuf, MtError>;

/// Retorna `true` se o nome da entrada começa com `.` (ex.: `.git`, `.worktrees`, `.max-code-worktrees`).
pub fn is_hidden(name: &OsStr) -> bool;

/// Verifica se um caminho canônico está localizado dentro de um diretório oculto de controle
/// (`.git`, `.worktrees`, `.max-code-worktrees`), usado para filtrar destinos de symlinks na descoberta automática.
pub fn is_inside_excluded_discovery_dir(path: &Path) -> bool;

/// Retorna o nome do último componente do caminho sem alterar caixa.
pub fn default_title(path: &Path) -> String;
```

**Regras de Negócio em `paths.rs`:**
1. Se `input` contiver `'\0'` ou for vazio após trim de espaços externos → se começar com `~` ou `/`, não fazer trim que altere nomes reais de pasta com espaço, apenas rejeitar string vazia `""` com `project.invalid_argument`.
2. Se `input == "~"` → retorna `dirs::home_dir()`. Se `input.starts_with("~/")` → concatena o sufixo ao `home_dir()`. Se `input.starts_with('~')` (ex.: `~root`) → retorna `MtError` com código `project.invalid_argument` e mensagem `"Não é permitido expandir o diretório de outro usuário."`.
3. Se o caminho resultante não for absoluto (`!path.is_absolute()`) → retorna `project.invalid_argument` (`"O caminho do projeto deve ser absoluto."`).
4. Antes de `std::fs::canonicalize(&expanded)`, inspecionar `std::fs::metadata(&expanded)`:
   - `Err(e)` com `ErrorKind::NotFound` → `project.path_not_found` (`"A pasta informada não foi encontrada no disco."`).
   - `Ok(meta)` onde `!meta.is_dir()` → `project.path_not_directory` (`"O caminho informado não é um diretório."`).
5. Converter `canonical.to_str()`: se `None` → `project.path_not_utf8` (`"O caminho contém caracteres que não são UTF-8 válidos."`).
6. Para `validate_project_dir`: se `canonical == Path::new("/")` ou `Some(&canonical) == home_canonical.as_ref()` → `project.path_forbidden` (`"A raiz do sistema e a pasta pessoal ($HOME) não podem ser cadastradas diretamente como um projeto."`).
7. Para `validate_root_dir`: se `canonical == Path::new("/")` → `project.path_forbidden` (`"A raiz do sistema (/) não pode ser cadastrada como Pasta de Projetos."`).

### 3.4. Auto-detecção de Stack (`src-tauri/crates/mt-projects/src/detect.rs`)

```rust
use std::path::Path;
use crate::model::{ScanWarning, StackKind};

/// Limite máximo para leitura de `package.json`: 1 MiB (1_048_576 bytes).
pub const MAX_PACKAGE_JSON_BYTES: usize = 1024 * 1024;

/// Snapshot pré-lido dos arquivos marcadores na raiz de uma pasta de projeto.
#[derive(Debug, Clone, Default)]
pub struct FolderProbe {
    pub has_artisan: bool,
    pub has_composer_json: bool,
    pub package_json: Option<serde_json::Value>,
    pub has_tauri_conf: bool,
    pub has_pyproject: bool,
    pub has_requirements: bool,
    pub has_cargo_toml: bool,
    pub complete: bool,
    pub warnings: Vec<ScanWarning>,
}

impl FolderProbe {
    /// Realiza leitura segura e única dos marcadores na pasta `dir`.
    /// Nunca segue symlinks para arquivos marcadores nem bloqueia em FIFOs/devices.
    pub fn read(dir: &Path) -> std::io::Result<Self>;
}

pub trait StackRule: Send + Sync {
    fn kind(&self) -> StackKind;
    fn matches(&self, probe: &FolderProbe) -> bool;
}

#[derive(Debug, Clone)]
pub struct DetectionReport {
    pub detected: Vec<StackKind>,
    pub complete: bool,
    pub warnings: Vec<ScanWarning>,
}

pub struct StackDetector {
    rules: Vec<Box<dyn StackRule>>,
}

impl StackDetector {
    pub fn default_rules() -> Self;
    pub fn detect(&self, dir: &Path) -> DetectionReport;
    pub fn detect_from_probe(&self, probe: FolderProbe) -> DetectionReport;
}
```

**Regras Estritas de Segurança e Prioridade em `detect.rs`:**
1. **Verificação de marcador regular (`has_regular_file`)**:
   - Usa `std::fs::symlink_metadata(&marker_path)`.
   - Retorna `true` somente se `meta.file_type().is_file()` (rejeita diretórios, symlinks, FIFOs, sockets, block/char devices).
   - Se `Err(e)` for `NotFound` ou `NotADirectory`, retorna `false` sem warning.
   - Se `Err(e)` for `PermissionDenied` ou outro erro de I/O, marca `probe.complete = false` e adiciona `ScanWarning { path, code: "project.probe_io_error".into(), message: "Não foi possível ler marcadores da pasta devido a erro de permissão ou E/S.".into() }`.
2. **Marcadores Tauri (`has_tauri_conf`)**:
   - Verifica `src-tauri` com `symlink_metadata`: deve ser um diretório real (`is_dir()`). Se `PermissionDenied`, marca `complete = false` e registra warning.
   - Dentro de `src-tauri/`, verifica se existe pelo menos um arquivo regular entre: `tauri.conf.json`, `tauri.conf.json5` ou `Tauri.toml`.
3. **Leitura Segura de `package.json`**:
   - Verifica `symlink_metadata(dir.join("package.json"))`. Se não existir ou não for `is_file()`, `package_json = None`.
   - Se `meta.len() > MAX_PACKAGE_JSON_BYTES as u64`, ignora e adiciona `ScanWarning` (`project.package_json_too_large`).
   - No Linux (`#[cfg(unix)]`), abre com `std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)` (ou flags equivalentes via `rustix`/`nix`/constantes POSIX padrão do target Linux) e valida `file.metadata()?.file_type().is_file()` no descritor aberto antes de ler.
   - Lê com `std::io::Read::take(file, (MAX_PACKAGE_JSON_BYTES + 1) as u64)` para um buffer `Vec<u8>`. Se `buf.len() > MAX_PACKAGE_JSON_BYTES`, descarta (`None`) e emite warning (`project.package_json_too_large`), cobrindo arquivo que cresceu após `metadata`.
   - Faz parse com `serde_json::from_slice(&buf)`. Se inválido, define `package_json = None` e adiciona warning (`project.package_json_invalid`), sem pânico e sem marcar `complete = false` (JSON malformado é estado determinístico do arquivo, não falha transitória de I/O).
4. **Ordem Canônica de Regras (`StackDetector::default_rules`)**:
   - **Prioridade 1 — `StackKind::Tauri`**: `probe.has_tauri_conf == true`.
   - **Prioridade 2 — `StackKind::VscodeExtension`**: `probe.package_json` possui objeto `"engines"` contendo chave `"vscode"` cujo valor é string não vazia (`as_str().is_some_and(|s| !s.trim().is_empty())`).
   - **Prioridade 3 — `StackKind::Laravel`**: `probe.has_artisan && probe.has_composer_json`.
   - **Prioridade 4 — `StackKind::Vue`**: `probe.package_json` possui a chave `"vue"` dentro de `"dependencies"`, `"devDependencies"` ou `"peerDependencies"` (como objeto JSON).
   - **Prioridade 5 — `StackKind::Rust`**: `probe.has_cargo_toml == true`.
   - **Prioridade 6 — `StackKind::Python`**: `probe.has_pyproject || probe.has_requirements`.
   - Se nenhuma regra casar, `detected` recebe `vec![StackKind::Unknown]`.

### 3.5. Identidade Visual e Validações (`src-tauri/crates/mt-projects/src/identity.rs`)

```rust
use std::path::Path;
use mt_core::MtError;
use crate::model::{ProjectIdentity, StackKind};

pub const FALLBACK_ICONS: [&str; 12] = [
    "mdi:folder",
    "mdi:folder-star",
    "mdi:folder-code",
    "mdi:folder-heart",
    "mdi:folder-cog",
    "mdi:folder-wrench",
    "mdi:folder-table",
    "mdi:folder-text",
    "mdi:folder-multiple",
    "mdi:folder-network",
    "mdi:folder-zip",
    "mdi:folder-file",
];

pub const FALLBACK_COLORS: [&str; 12] = [
    "#14B8A6",
    "#0EA5E9",
    "#6366F1",
    "#A855F7",
    "#EC4899",
    "#F43F5E",
    "#F97316",
    "#EAB308",
    "#84CC16",
    "#22C55E",
    "#06B6D4",
    "#64748B",
];

pub struct IdentityMap;

impl IdentityMap {
    /// Retorna `(icon, color)` para stacks conhecidas ou `None` para `StackKind::Unknown`.
    pub fn for_stack(stack: StackKind) -> Option<(&'static str, &'static str)> {
        match stack {
            StackKind::Tauri => Some(("logos:tauri", "#FFC131")),
            StackKind::VscodeExtension => Some(("logos:visual-studio-code", "#007ACC")),
            StackKind::Laravel => Some(("logos:laravel", "#FF2D20")),
            StackKind::Vue => Some(("logos:vue", "#42B883")),
            StackKind::Rust => Some(("logos:rust", "#CE422B")),
            StackKind::Python => Some(("logos:python", "#3776AB")),
            StackKind::Unknown => None,
        }
    }

    /// Resolve a identidade final a partir do caminho canônico e das stacks detectadas.
    pub fn resolve(canonical_path: &Path, detected: Vec<StackKind>) -> ProjectIdentity {
        let detected = if detected.is_empty() {
            vec![StackKind::Unknown]
        } else {
            detected
        };
        let primary = detected[0];
        let (icon, color) = Self::for_stack(primary)
            .unwrap_or_else(|| Self::fallback(canonical_path));
        ProjectIdentity {
            icon: icon.to_string(),
            color: color.to_string(),
            stack: primary,
            detected,
        }
    }

    /// Calcula `(icon, color)` determinísticos via FNV-1a 64 bits sobre os bytes UTF-8 do caminho canônico.
    pub fn fallback(canonical_path: &Path) -> (&'static str, &'static str) {
        let bytes = canonical_path.to_string_lossy();
        let h = fnv1a_64(bytes.as_bytes());
        let icon = FALLBACK_ICONS[(h % 12) as usize];
        let color = FALLBACK_COLORS[((h >> 32) % 12) as usize];
        (icon, color)
    }
}

/// Hash FNV-1a de 64 bits determinístico.
/// Offset basis: `14695981039346656037` (`0xcbf29ce484222325`), Prime: `1099511628211` (`0x100000001b3`).
pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}

/// Valida nome de ícone Iconify: `^[a-z0-9]+(-[a-z0-9]+)*:[a-z0-9]+(-[a-z0-9]+)*$` e tamanho total <= 64 bytes.
/// Erro: `project.invalid_icon`.
pub fn validate_icon(icon: &str) -> Result<(), MtError>;

/// Valida cor hexadecimal `^#[0-9A-Fa-f]{6}$` e retorna normalizada em maiúsculas (`#RRGGBB`).
/// Erro: `project.invalid_color`.
pub fn validate_color(color: &str) -> Result<String, MtError>;

/// Faz `trim()`, valida que possui entre 1 e 120 caracteres (`chars().count()`) e não possui caracteres de controle (`c.is_control()`).
/// Erro: `project.invalid_title`.
pub fn validate_title(title: &str) -> Result<String, MtError>;
```

### 3.6. Eventos e Sink (`src-tauri/crates/mt-projects/src/events.rs`)

```rust
use mt_core::ids::ProjectId;
use serde::{Deserialize, Serialize};
use specta::Type;
use crate::model::{DiscoveredFolder, DiscoveryMode, Project, ProjectRoot, ProjectRootId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiscoveredPayload {
    pub root_id: ProjectRootId,
    pub mode: DiscoveryMode,
    pub folders: Vec<DiscoveredFolder>,
    pub imported: Vec<Project>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RemovedKind {
    ProjectMissing,
    SuggestionGone,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRemovedPayload {
    pub root_id: Option<ProjectRootId>,
    pub kind: RemovedKind,
    pub path: String,
    pub project_id: Option<ProjectId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectChangedPayload {
    Upserted { projects: Vec<Project> },
    Deleted { ids: Vec<ProjectId> },
    Reordered { ids: Vec<ProjectId> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectRootChangedPayload {
    Upserted { root: ProjectRoot },
    Deleted { id: ProjectRootId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEvent<T> {
    pub epoch: String,
    pub revision: u32,
    pub payload: T,
}

pub trait ProjectEventSink: Send + Sync + 'static {
    fn discovered(&self, event: ProjectEvent<ProjectDiscoveredPayload>);
    fn removed(&self, event: ProjectEvent<ProjectRemovedPayload>);
    fn changed(&self, event: ProjectEvent<ProjectChangedPayload>);
    fn root_changed(&self, event: ProjectEvent<ProjectRootChangedPayload>);
}
```

---

## 4. Casos de Borda e Segurança

- **Proibição absoluta de execução de subprocessos**: `mt-projects` jamais invoca `std::process::Command` ou `tokio::process::Command`.
- **Proteção contra DoS em `package.json`**: Um `package.json` que seja uma FIFO (`mkfifo`), symlink para `/dev/zero` ou arquivo de 50 MiB é barrado antes da leitura completa gracias a `symlink_metadata`, `O_NOFOLLOW | O_NONBLOCK` e `Read::take(MAX_PACKAGE_JSON_BYTES + 1)`.
- **Determinismo multi-release do Fallback**: O cálculo `fnv1a_64` não utiliza `std::collections::hash_map::DefaultHasher` (que muda de semente por processo/versão do Rust). Vetores de teste fixam valores exatos de hash, ícone e cor para caminhos conhecidos.
