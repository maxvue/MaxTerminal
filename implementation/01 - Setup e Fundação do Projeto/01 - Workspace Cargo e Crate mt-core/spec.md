# Implementação 01 — Workspace Cargo e Crate `mt-core` (`spec.md`)

> Fase: [`../instructions.md`](../instructions.md) | Especificação da Fase: [`../spec.md`](../spec.md)
> Escopo desta sub-implementação: configuração de repositório base (`.gitignore`, `.editorconfig`), configuração de toolchain e formatação Rust (`src-tauri/rust-toolchain.toml`, `src-tauri/rustfmt.toml`), manifesto do workspace Cargo (`src-tauri/Cargo.toml`) e implementação integral do crate de domínio base `src-tauri/crates/mt-core/` com seus testes unitários.

---

## 1. Objetivo e Fronteira Arquitetural

Estabelecer a base do repositório e o crate `mt-core`, que concentra todos os tipos fundamentais e invariantes de domínio compartilhados do MaxTerminal sem nenhuma dependência de `tauri` ou WebView:

1. **Higiene de Repositório (`.gitignore`, `.editorconfig`):** Bloquear o versionamento acidental de worktrees (`.worktrees/`, `.max-code-worktrees/`), `node_modules/`, `dist/`, `src-tauri/target/`, `src-tauri/gen/` (preservando `src-tauri/gen/schemas/`), `coverage/` e `*.log`, além de padronizar recuo (4 espaços em Rust/TS/Vue/SCSS, 2 em JSON/YAML/TOML), UTF-8 e finais de linha `lf`.
2. **Workspace Cargo (`src-tauri/`):** Configurar `resolver = "3"`, edição `2024`, `rust-version = "1.85"`, lints estritos (`unsafe_code = "forbid"`, `unwrap_used = "deny"`, `clippy::all = "deny"`, `clippy::pedantic = "warn"`) e declarar exclusivamente os membros `.` (`maxterminal`) e `crates/mt-core`.
3. **Crate `mt-core` (`src-tauri/crates/mt-core/`):**
   - `ids.rs`: 8 newtypes de identificadores de domínio baseados em `Uuid` v7 (`ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `TemplateId`, `CommandBlockId`, `CommandGroupId`), com validação estrita da versão 7 em `try_from_uuid`, `TryFrom<Uuid>`, `FromStr` e `serde::Deserialize`.
   - `error.rs`: `ErrorCode(&'static str)`, módulo `codes` com as 9 constantes `core.*`, struct `MtError` serializável (`{ code, message, details? }`) com `source` omitido de Serde/Specta e conversões `From<std::io::Error>` e `From<serde_json::Error>`, além do alias `MtResult<T>`.
   - `paths.rs`: trait `EnvSource`, struct `SystemEnv`, struct `AppPaths` (resolução pura sem I/O segundo a XDG Base Directory Specification + override `MAXTERMINAL_HOME` e criação segura com modo `0o700` em `ensure_dirs()` sem criar `db_path`) e DTO `AppPathsDto`.
   - `app.rs`: `AppInfo`, `BuildProfile` (`Debug`, `Release`) e `FrontendLogEvent` (`VueError`, `WindowError`, `UnhandledRejection`, `BootstrapFailed`).

---

## 2. Arquivos de Configuração da Raiz e Toolchain Rust

### 2.1. `.gitignore`

```gitignore
# Worktrees isoladas (obrigatório conforme AGENTS.md)
.worktrees/
.max-code-worktrees/

# Dependências e artefatos frontend
node_modules/
dist/
coverage/

# Artefatos Rust / Tauri
src-tauri/target/
src-tauri/gen/*
!src-tauri/gen/schemas/

# Logs e temporários
*.log
*.log.*
.DS_Store
.env
.env.*
!.env.example
.tmp/
```

### 2.2. `.editorconfig`

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
insert_final_newline = true
trim_trailing_whitespace = true
indent_style = space
indent_size = 4

[*.{json,yml,yaml,toml,md}]
indent_size = 2
```

### 2.3. `src-tauri/rust-toolchain.toml`

```toml
[toolchain]
channel = "1.85.0"
components = ["rustfmt", "clippy"]
profile = "minimal"
```

> Nota: caso o `Cargo.lock` resolvido exija um patch estável superior (ex.: `1.86.0`), o canal deve ser atualizado para a versão exata validada na máquina CachyOS mantendo `rustfmt` e `clippy`.

### 2.4. `src-tauri/rustfmt.toml`

```toml
edition = "2024"
max_width = 100
tab_spaces = 4
newline_style = "Unix"
use_field_init_shorthand = true
use_try_shorthand = true
```

---

## 3. Manifesto do Workspace Cargo (`src-tauri/Cargo.toml`)

```toml
[workspace]
resolver = "3"
members = [".", "crates/mt-core"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
license = "UNLICENSED"
authors = ["Johnattas Santana"]

[workspace.dependencies]
mt-core = { path = "crates/mt-core" }
tauri = { version = "2", features = [] }
tauri-build = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
uuid = { version = "1", features = ["v4", "v7", "serde"] }
specta = { version = "=2.0.0-rc.27", features = ["derive", "function", "uuid", "serde_json"] }
specta-typescript = "=0.0.14"
tauri-specta = { version = "=2.0.0-rc.27", features = ["typescript"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt", "json"] }
tracing-appender = "0.2"

[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"
unused_must_use = "deny"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"
missing_errors_doc = "allow"
unwrap_used = "deny"
expect_used = "warn"

[package]
name = "maxterminal"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true

[lib]
name = "maxterminal_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[dependencies]
mt-core.workspace = true
tauri.workspace = true
serde.workspace = true
serde_json.workspace = true
specta.workspace = true
specta-typescript.workspace = true
tauri-specta.workspace = true
tracing.workspace = true
tracing-subscriber.workspace = true
tracing-appender.workspace = true

[build-dependencies]
tauri-build.workspace = true

[lints]
workspace = true

[profile.release]
lto = "thin"
codegen-units = 1
strip = "symbols"
```

---

## 4. Especificação Técnica do Crate `mt-core`

### 4.1. `src-tauri/crates/mt-core/Cargo.toml`

```toml
[package]
name = "mt-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
authors.workspace = true
publish = false

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
uuid.workspace = true
specta.workspace = true

[lints]
workspace = true
```

### 4.2. `src-tauri/crates/mt-core/src/lib.rs`

```rust
//! Tipos de domínio compartilhados do MaxTerminal: identificadores UUID v7,
//! erros estruturados, resolução de caminhos XDG e metadados da aplicação.

pub mod app;
pub mod error;
pub mod ids;
pub mod paths;

pub use error::{ErrorCode, MtError, MtResult};
```

### 4.3. `src-tauri/crates/mt-core/src/ids.rs`

Assinaturas e regras de implementação para os 8 newtypes (`ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `TemplateId`, `CommandBlockId`, `CommandGroupId`):

```rust
//! Identificadores de domínio tipados baseados em UUID v7 (ordenáveis temporalmente).

use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

use crate::{MtError, MtResult};

/// Gera um newtype de identificador de domínio baseado em `Uuid` v7.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type,
        )]
        #[serde(transparent)]
        #[specta(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Cria um novo identificador com timestamp atual (UUID v7).
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Reconstrói o identificador validando que o UUID é estritamente da versão 7.
            pub fn try_from_uuid(uuid: Uuid) -> MtResult<Self> {
                if uuid.get_version_num() != 7 {
                    return Err(MtError::invalid_argument(
                        stringify!($name),
                        "identificador inválido: esperado UUID v7",
                    ));
                }
                Ok(Self(uuid))
            }

            /// Retorna a referência ao `Uuid` subjacente.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = MtError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let uuid = Uuid::parse_str(s).map_err(|_| {
                    MtError::invalid_argument(
                        stringify!($name),
                        "identificador inválido: esperado UUID v7",
                    )
                })?;
                Self::try_from_uuid(uuid)
            }
        }

        impl TryFrom<Uuid> for $name {
            type Error = MtError;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                Self::try_from_uuid(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Self::from_str(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(
    /// Identificador único de um projeto cadastrado (Fase 06).
    ProjectId
);
define_id!(
    /// Identificador único de uma aba do workspace (Fase 07).
    TabId
);
define_id!(
    /// Identificador único de um painel dentro de uma aba (Fase 07).
    PaneId
);
define_id!(
    /// Identificador único de uma instância de pseudo-terminal (Fase 02).
    PtyId
);
define_id!(
    /// Identificador único de uma sessão persistida no SQLite (Fases 03/07).
    SessionId
);
define_id!(
    /// Identificador único de um template de aba (Fase 07).
    TemplateId
);
define_id!(
    /// Identificador único de um bloco de comando OSC 133 (Fase 05).
    CommandBlockId
);
define_id!(
    /// Identificador único de um grupo de comandos (Fase 15).
    CommandGroupId
);
```

**Regras de Negócio e Invariantes (`ids.rs`):**
- O campo interno `Uuid` é privado; nenhum código externo ao módulo pode instanciar `$name(uuid)` ignorando `try_from_uuid`.
- `#[specta(transparent)]` garante que no TypeScript gerado cada ID seja exportado como `export type ProjectId = string`.
- UUIDs `nil`, `v4`, strings malformadas ou valores JSON não-string (`123`, `null`, `{}`) são rejeitados tanto em `FromStr` quanto em `serde_json::from_str`.
- Dois UUIDs v7 criados a partir de timestamps distintos (`ts1 < ts2` via `Uuid::from_u64_pair` ou `uuid::Timestamp::from_unix`) respeitam `id1 < id2`.

### 4.4. `src-tauri/crates/mt-core/src/error.rs`

```rust
//! Contrato único de erro (`MtError`) e códigos de erro (`ErrorCode`) para fronteira IPC e domínio.

use serde::Serialize;
use serde_json::json;
use std::{borrow::Cow, fmt, io};

/// Código estável legível por máquina no formato `<domínio>.<motivo>`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, specta::Type)]
#[serde(transparent)]
#[specta(transparent)]
pub struct ErrorCode(pub &'static str);

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

/// Códigos canônicos do domínio `core`.
pub mod codes {
    use super::ErrorCode;

    /// Falha interna inesperada.
    pub const INTERNAL: ErrorCode = ErrorCode("core.internal");
    /// Erro de entrada/saída do sistema operacional.
    pub const IO: ErrorCode = ErrorCode("core.io");
    /// Entidade ou recurso não encontrado.
    pub const NOT_FOUND: ErrorCode = ErrorCode("core.not_found");
    /// Argumento de entrada inválido.
    pub const INVALID_ARGUMENT: ErrorCode = ErrorCode("core.invalid_argument");
    /// Conflito de estado ou recurso duplicado.
    pub const CONFLICT: ErrorCode = ErrorCode("core.conflict");
    /// Permissão negada pelo sistema operacional ou política interna.
    pub const PERMISSION_DENIED: ErrorCode = ErrorCode("core.permission_denied");
    /// Recurso ou operação não suportada.
    pub const UNSUPPORTED: ErrorCode = ErrorCode("core.unsupported");
    /// Erro de serialização ou desserialização de dados.
    pub const SERIALIZATION: ErrorCode = ErrorCode("core.serialization");
    /// Impossível resolver diretórios base XDG/HOME da aplicação.
    pub const PATHS_UNRESOLVED: ErrorCode = ErrorCode("core.paths_unresolved");
}

/// Erro único retornado por todos os comandos IPC do MaxTerminal.
#[derive(Debug, Serialize, specta::Type, thiserror::Error)]
#[error("[{code}] {message}", code = .code.0)]
pub struct MtError {
    /// Código estável legível por máquina.
    pub code: ErrorCode,
    /// Mensagem descritiva em português do Brasil (pt-BR) segura para exibição na UI.
    pub message: Cow<'static, str>,
    /// Metadados estruturados opcionais (< 4 KiB, sem dados sensíveis).
    #[serde(skip_serializing_if = "Option::is_none")]
    #[specta(optional)]
    pub details: Option<serde_json::Value>,
    /// Causa original interna mantida apenas em memória (nunca serializada ou logada bruta).
    #[serde(skip)]
    #[specta(skip)]
    #[source]
    pub source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

/// Alias padrão de resultado para operações de domínio e comandos IPC.
pub type MtResult<T> = Result<T, MtError>;

impl MtError {
    /// Cria um novo `MtError` com código e mensagem em pt-BR.
    #[must_use]
    pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
            source: None,
        }
    }

    /// Anexa detalhes estruturados seguros ao erro.
    #[must_use]
    pub fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }

    /// Anexa a causa interna em memória (omitida na serialização).
    #[must_use]
    pub fn with_source(mut self, err: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(err));
        self
    }

    /// Constrói erro interno genérico (`core.internal`).
    #[must_use]
    pub fn internal(message: impl Into<Cow<'static, str>>) -> Self {
        Self::new(codes::INTERNAL, message)
    }

    /// Constrói erro de recurso não encontrado (`core.not_found`) com `entity` e `id` em `details`.
    #[must_use]
    pub fn not_found(entity: &'static str, id: impl fmt::Display) -> Self {
        Self::new(
            codes::NOT_FOUND,
            format!("Recurso '{entity}' não encontrado."),
        )
        .with_details(json!({
            "entity": entity,
            "id": id.to_string(),
        }))
    }

    /// Constrói erro de argumento inválido (`core.invalid_argument`) com `field` em `details`.
    #[must_use]
    pub fn invalid_argument(field: &'static str, reason: impl Into<Cow<'static, str>>) -> Self {
        Self::new(codes::INVALID_ARGUMENT, reason).with_details(json!({
            "field": field,
        }))
    }

    /// Constrói erro de operação não suportada (`core.unsupported`).
    #[must_use]
    pub fn unsupported(feature: &'static str) -> Self {
        Self::new(
            codes::UNSUPPORTED,
            format!("Recurso não suportado: {feature}."),
        )
        .with_details(json!({
            "feature": feature,
        }))
    }
}

impl From<io::Error> for MtError {
    fn from(err: io::Error) -> Self {
        let (code, msg) = match err.kind() {
            io::ErrorKind::PermissionDenied => (
                codes::PERMISSION_DENIED,
                "Permissão negada ao acessar recurso do sistema.",
            ),
            io::ErrorKind::NotFound => (
                codes::NOT_FOUND,
                "Arquivo ou diretório não encontrado no sistema.",
            ),
            _ => (
                codes::IO,
                "Falha de entrada/saída ao acessar o sistema de arquivos.",
            ),
        };
        Self::new(code, msg).with_source(err)
    }
}

impl From<serde_json::Error> for MtError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(
            codes::SERIALIZATION,
            "Falha ao processar dados em formato JSON.",
        )
        .with_source(err)
    }
}
```

### 4.5. `src-tauri/crates/mt-core/src/paths.rs`

```rust
//! Resolução e criação segura dos diretórios XDG do MaxTerminal.

use serde::Serialize;
use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{MtError, MtResult, error::codes};

/// Nome do diretório da aplicação dentro das bases XDG.
pub const APP_DIR_NAME: &str = "maxterminal";

/// Abstração de leitura de variáveis de ambiente para permitir testes puros e determinísticos.
pub trait EnvSource {
    /// Obtém o valor de uma variável de ambiente, se definida.
    fn var(&self, key: &str) -> Option<String>;
}

/// Implementação real de `EnvSource` consultando `std::env::var`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemEnv;

impl EnvSource for SystemEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }
}

/// Diretórios resolvidos do MaxTerminal conforme a especificação XDG Base Directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppPaths {
    /// `$XDG_CONFIG_HOME/maxterminal` ou `~/.config/maxterminal`.
    pub config_dir: PathBuf,
    /// `$XDG_DATA_HOME/maxterminal` ou `~/.local/share/maxterminal`.
    pub data_dir: PathBuf,
    /// `$XDG_STATE_HOME/maxterminal` ou `~/.local/state/maxterminal`.
    pub state_dir: PathBuf,
    /// `$XDG_CACHE_HOME/maxterminal` ou `~/.cache/maxterminal`.
    pub cache_dir: PathBuf,
    /// `$XDG_RUNTIME_DIR/maxterminal` quando `XDG_RUNTIME_DIR` é um caminho absoluto.
    pub runtime_dir: Option<PathBuf>,
    /// `state_dir/logs` para gravação dos arquivos rotativos de log.
    pub log_dir: PathBuf,
    /// `data_dir/maxterminal.db` (apenas exposto na Fase 01; criado somente na Fase 03).
    pub db_path: PathBuf,
}

impl AppPaths {
    /// Resolve todos os caminhos sem tocar no disco.
    ///
    /// - Se `MAXTERMINAL_HOME` estiver presente: exige que seja absoluto (caso seja vazio ou relativo,
    ///   retorna `core.invalid_argument`) e resolve `config`, `data`, `state`, `cache`, `state/logs` e
    ///   `data/maxterminal.db` dentro dele, com `runtime_dir = None`.
    /// - Caso contrário, avalia `XDG_CONFIG_HOME`, `XDG_DATA_HOME`, `XDG_STATE_HOME`, `XDG_CACHE_HOME`
    ///   e `XDG_RUNTIME_DIR`. Valores vazios ou relativos são ignorados conforme a especificação XDG.
    /// - `HOME` absoluto só é consultado se ao menos um dos 4 diretórios persistentes precisar de fallback.
    ///   Se um fallback for necessário e `HOME` estiver ausente ou relativo, retorna `core.paths_unresolved`.
    pub fn resolve(env: &impl EnvSource) -> MtResult<Self> {
        if let Some(override_raw) = env.var("MAXTERMINAL_HOME") {
            let trimmed = override_raw.trim();
            let base = PathBuf::from(trimmed);
            if trimmed.is_empty() || !base.is_absolute() {
                return Err(MtError::invalid_argument(
                    "MAXTERMINAL_HOME",
                    "O caminho em MAXTERMINAL_HOME deve ser absoluto.",
                ));
            }
            let config_dir = base.join("config");
            let data_dir = base.join("data");
            let state_dir = base.join("state");
            let cache_dir = base.join("cache");
            let log_dir = state_dir.join("logs");
            let db_path = data_dir.join("maxterminal.db");

            return Ok(Self {
                config_dir,
                data_dir,
                state_dir,
                cache_dir,
                runtime_dir: None,
                log_dir,
                db_path,
            });
        }

        let xdg_config = Self::parse_abs_dir(env.var("XDG_CONFIG_HOME"));
        let xdg_data = Self::parse_abs_dir(env.var("XDG_DATA_HOME"));
        let xdg_state = Self::parse_abs_dir(env.var("XDG_STATE_HOME"));
        let xdg_cache = Self::parse_abs_dir(env.var("XDG_CACHE_HOME"));
        let xdg_runtime = Self::parse_abs_dir(env.var("XDG_RUNTIME_DIR"));

        let needs_home = xdg_config.is_none()
            || xdg_data.is_none()
            || xdg_state.is_none()
            || xdg_cache.is_none();

        let home_dir = if needs_home {
            Some(Self::parse_abs_dir(env.var("HOME")).ok_or_else(|| {
                MtError::new(
                    codes::PATHS_UNRESOLVED,
                    "Não foi possível determinar o diretório HOME absoluto do usuário.",
                )
            })?)
        } else {
            None
        };

        let resolve_with_fallback = |xdg: Option<PathBuf>, rel_fallback: &str| -> PathBuf {
            if let Some(dir) = xdg {
                dir.join(APP_DIR_NAME)
            } else if let Some(ref home) = home_dir {
                home.join(rel_fallback).join(APP_DIR_NAME)
            } else {
                unreachable!("home_dir validado quando needs_home é verdadeiro")
            }
        };

        let config_dir = resolve_with_fallback(xdg_config, ".config");
        let data_dir = resolve_with_fallback(xdg_data, ".local/share");
        let state_dir = resolve_with_fallback(xdg_state, ".local/state");
        let cache_dir = resolve_with_fallback(xdg_cache, ".cache");
        let runtime_dir = xdg_runtime.map(|dir| dir.join(APP_DIR_NAME));
        let log_dir = state_dir.join("logs");
        let db_path = data_dir.join("maxterminal.db");

        Ok(Self {
            config_dir,
            data_dir,
            state_dir,
            cache_dir,
            runtime_dir,
            log_dir,
            db_path,
        })
    }

    /// Cria os diretórios persistentes (`config_dir`, `data_dir`, `state_dir`, `cache_dir`, `log_dir`)
    /// garantindo permissão `0o700` no Unix. Não cria `runtime_dir` nem o arquivo `db_path`.
    pub fn ensure_dirs(&self) -> MtResult<()> {
        let dirs = [
            &self.config_dir,
            &self.data_dir,
            &self.state_dir,
            &self.cache_dir,
            &self.log_dir,
        ];
        for dir in dirs {
            Self::ensure_single_dir(dir)?;
        }
        Ok(())
    }

    fn parse_abs_dir(val: Option<String>) -> Option<PathBuf> {
        let raw = val?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return None;
        }
        let path = PathBuf::from(trimmed);
        if path.is_absolute() {
            Some(path)
        } else {
            None
        }
    }

    fn ensure_single_dir(dir: &Path) -> MtResult<()> {
        if dir.exists() {
            let meta = fs::metadata(dir)?;
            if !meta.is_dir() {
                return Err(MtError::new(
                    codes::IO,
                    "O caminho configurado para diretório da aplicação não é uma pasta válida.",
                ));
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = meta.permissions();
                if perms.mode() & 0o777 != 0o700 {
                    perms.set_mode(0o700);
                    fs::set_permissions(dir, perms)?;
                }
            }
            return Ok(());
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder.create(dir)?;
        }
        #[cfg(not(unix))]
        {
            fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

/// DTO serializável exposto ao frontend contendo os caminhos resolvidos da aplicação.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppPathsDto {
    /// Caminho do diretório de configuração.
    pub config_dir: String,
    /// Caminho do diretório de dados.
    pub data_dir: String,
    /// Caminho do diretório de estado.
    pub state_dir: String,
    /// Caminho do diretório de cache.
    pub cache_dir: String,
    /// Caminho do diretório de logs.
    pub log_dir: String,
    /// Caminho onde o banco SQLite será armazenado (Fase 03).
    pub db_path: String,
}

impl From<&AppPaths> for AppPathsDto {
    fn from(paths: &AppPaths) -> Self {
        Self {
            config_dir: paths.config_dir.to_string_lossy().into_owned(),
            data_dir: paths.data_dir.to_string_lossy().into_owned(),
            state_dir: paths.state_dir.to_string_lossy().into_owned(),
            cache_dir: paths.cache_dir.to_string_lossy().into_owned(),
            log_dir: paths.log_dir.to_string_lossy().into_owned(),
            db_path: paths.db_path.to_string_lossy().into_owned(),
        }
    }
}
```

### 4.6. `src-tauri/crates/mt-core/src/app.rs`

```rust
//! Metadados estáticos da aplicação e catálogo fechado de eventos de log do frontend.

use serde::{Deserialize, Serialize};

use crate::paths::AppPathsDto;

/// Perfil de compilação do binário Rust.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum BuildProfile {
    /// Compilação de desenvolvimento (`debug_assertions` ativo).
    Debug,
    /// Compilação otimizada de produção.
    Release,
}

/// Fotografia imutável de metadados do MaxTerminal retornada por `app_info`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// Nome oficial da aplicação (`"MaxTerminal"`).
    pub name: String,
    /// Versão semântica do pacote (`CARGO_PKG_VERSION`).
    pub version: String,
    /// Perfil de compilação (`debug` ou `release`).
    pub build_profile: BuildProfile,
    /// Versão do runtime Tauri em uso.
    pub tauri_version: String,
    /// Sistema operacional (`std::env::consts::OS`).
    pub os: String,
    /// Arquitetura da CPU (`std::env::consts::ARCH`).
    pub arch: String,
    /// Diretórios XDG resolvidos no boot.
    pub paths: AppPathsDto,
}

/// Catálogo fechado de eventos de erro reportáveis pelo frontend antes do sanitizador da Fase 03.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FrontendLogEvent {
    /// Erro capturado pelo `app.config.errorHandler` do Vue.
    VueError,
    /// Erro global capturado no listener `window.addEventListener('error', ...)`.
    WindowError,
    /// Promise rejeitada sem tratamento capturada em `unhandledrejection`.
    UnhandledRejection,
    /// Falha ao executar o bootstrap inicial da store `useAppStore`.
    BootstrapFailed,
}

impl FrontendLogEvent {
    /// Identificador estático snake_case do evento para gravação estruturada no log.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::VueError => "vue_error",
            Self::WindowError => "window_error",
            Self::UnhandledRejection => "unhandled_rejection",
            Self::BootstrapFailed => "bootstrap_failed",
        }
    }

    /// Mensagem estática em pt-BR associada ao evento (sem interpolação de payload externo).
    #[must_use]
    pub const fn static_message(self) -> &'static str {
        match self {
            Self::VueError => "Erro capturado no runtime de componentes Vue",
            Self::WindowError => "Erro global não tratado capturado na janela",
            Self::UnhandledRejection => "Rejeição de Promise não tratada no frontend",
            Self::BootstrapFailed => "Falha durante o bootstrap inicial do frontend",
        }
    }
}
```

---

## 5. Casos de Teste Unitários Obrigatórios em `mt-core`

| Módulo | Teste | Asserção |
|--------|-------|----------|
| `ids.rs` | `roundtrip_display_and_serde_all_eight_ids` | Para `ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `TemplateId`, `CommandBlockId`, `CommandGroupId`: `new()` gera versão 7; `Display` -> `FromStr` retorna ID idêntico; `serde_json::to_string` gera string JSON `"\"...\""` e `serde_json::from_str` reconstrói o mesmo valor. |
| `ids.rs` | `rejects_nil_v4_invalid_text_and_non_string_json` | `Uuid::nil()`, `Uuid::new_v4()`, `"nao-e-uuid"` e JSON `123` ou `null` falham para todos os 8 tipos com `codes::INVALID_ARGUMENT`. |
| `ids.rs` | `temporal_ordering_between_distinct_timestamps` | Dois UUIDs v7 construídos com timestamps Unix distintos (`1_700_000_000` vs `1_700_000_001` s) satisfazem `id_earlier < id_later`. |
| `error.rs` | `serializes_mt_error_contract_and_omits_source` | `MtError::not_found("project", id)` serializa exatamente `{"code":"core.not_found","message":"Recurso 'project' não encontrado.","details":{"entity":"project","id":"..."}}`; quando `details` é `None`, a chave `"details"` não existe no JSON; `with_source(...)` preserva `std::error::Error::source(&err).is_some()`, mas a chave `"source"` jamais aparece no JSON. |
| `error.rs` | `maps_io_and_serde_errors` | `io::ErrorKind::PermissionDenied` -> `core.permission_denied`; `io::ErrorKind::NotFound` -> `core.not_found`; `io::ErrorKind::Other` -> `core.io`; `serde_json::Error` -> `core.serialization`. |
| `paths.rs` | `resolves_xdg_defaults_from_home` | `FakeEnv` apenas com `HOME=/home/tester` resolve `.config/maxterminal`, `.local/share/maxterminal`, `.local/state/maxterminal`, `.cache/maxterminal`, `runtime_dir = None`, `log_dir = .../state/logs`, `db_path = .../data/maxterminal.db`. |
| `paths.rs` | `resolves_explicit_xdg_without_home` | `FakeEnv` com todos os 4 `XDG_*_HOME` absolutos e `HOME` ausente resolve com sucesso sem exigir `HOME`. |
| `paths.rs` | `ignores_relative_or_empty_xdg_and_fails_without_home` | `XDG_CONFIG_HOME=relativo` é ignorado e usa fallback `HOME`; se `HOME` estiver ausente ou relativo quando um fallback é necessário, retorna `core.paths_unresolved`. |
| `paths.rs` | `resolves_and_validates_maxterminal_home_override` | `MAXTERMINAL_HOME=/tmp/mt-test` resolve subpastas `config`, `data`, `state`, `cache`, `state/logs` sem precisar de `HOME`; `MAXTERMINAL_HOME=relativo` ou `""` retorna `core.invalid_argument`. |
| `paths.rs` | `ensure_dirs_creates_0700_dirs_and_never_creates_db_file` | Dentro de um diretório temporário sob a worktree (`target/test-tmp/...`), `ensure_dirs()` cria os 5 diretórios com permissão `0o700` (`mode & 0o777 == 0o700`) e `paths.db_path.exists()` permanece `false`. |
| `app.rs` | `frontend_log_event_serde_snake_case` | `FrontendLogEvent::VueError` serializa/desserializa como `"vue_error"`, `UnhandledRejection` como `"unhandled_rejection"`, e string desconhecida falha no Serde. |
