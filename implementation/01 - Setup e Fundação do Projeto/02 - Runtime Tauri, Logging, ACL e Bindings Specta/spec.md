# Implementação 02 — Runtime Tauri, Logging, ACL e Bindings Specta (`spec.md`)

> Fase: [`../instructions.md`](../instructions.md) | Especificação da Fase: [`../spec.md`](../spec.md)
> Depende de: [`../01 - Workspace Cargo e Crate mt-core/spec.md`](../01%20-%20Workspace%20Cargo%20e%20Crate%20mt-core/spec.md)
> Escopo desta sub-implementação: crate binário/biblioteca `maxterminal` (`src-tauri/`), `build.rs` com ACL de comandos próprios, `tauri.conf.json`, `capabilities/default.json`, `permissions/`, `src/main.rs`, `src/lib.rs`, `src/state.rs`, `src/logging.rs`, `src/commands/mod.rs`, `src/commands/app.rs`, `src/events/mod.rs`, `src/specta.rs`, `src/bin/export_bindings.rs`, `tests/bindings.rs` e `tests/capabilities.rs`.

---

## 1. Objetivo e Fronteira Arquitetural

Implementar a composição do backend nativo Tauri v2 no pacote `maxterminal` (`maxterminal_lib`), garantindo:

1. **ACL de Comandos Próprios e Segurança (`build.rs`, `permissions/`, `capabilities/default.json`, `tauri.conf.json`):**
   - Declarar no `build.rs` via `tauri_build::AppManifest::new().commands(&["app_info", "app_log_frontend"])` os comandos da aplicação para que o sistema de ACL do Tauri v2 gere as permissões (`allow-app-info`, `allow-app-log-frontend`) e **bloqueie por padrão** qualquer invocação fora da capability ativa.
   - Vincular a capability `foundation` exclusivamente à janela `main`, sem `core:default` e sem URLs remotas.
   - Configurar CSP de produção restritiva e `devCsp` restrita a `http://127.0.0.1:5177` e `ws://127.0.0.1:5177`.
2. **Logging Estruturado e Seguro (`src/logging.rs`):**
   - Writer não bloqueante (`tracing_appender::non_blocking`) com fila limitada (`buffered_lines_limit(4096)`), rotação diária (`maxterminal.log.<AAAA-MM-DD>`) em `$XDG_STATE_HOME/maxterminal/logs/` com permissão `0o600` e política `LogRedactor` (`FoundationRedactor`) que impede a gravação de mensagens livres, stacks ou segredos antes do sanitizador da Fase 03.
3. **Estado e Comandos IPC (`src/state.rs`, `src/commands/app.rs`):**
   - `AppState` imutável (`paths`, `info`) e `FrontendLogState` com rate-limiter monotônico (máximo de 200 eventos por janela de 1 segundo, contabilizando e reportando descartes de forma estática).
   - Comandos `app_info` e `app_log_frontend` anotados com `#[tauri::command]` e `#[specta::specta]`.
4. **Geração Determinística de Bindings (`src/specta.rs`, `src/bin/export_bindings.rs`, `tests/bindings.rs`):**
   - Coleção única de comandos e registro explícito dos 8 IDs (`ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `TemplateId`, `CommandBlockId`, `CommandGroupId`) e tipos (`AppInfo`, `AppPathsDto`, `BuildProfile`, `MtError`, `ErrorCode`, `FrontendLogEvent`) com `ErrorHandlingMode::Result`.
   - Exportação sem abrir WebView/GTK e teste automatizado de drift sem sobrescrever `src/types/bindings.ts`.

---

## 2. Build Script, Configuração Tauri, Permissões e Capabilities

### 2.1. `src-tauri/build.rs`

```rust
//! Script de build do pacote `maxterminal`: declara o manifesto de comandos sujeitos à ACL do Tauri v2.

fn main() {
    let app_manifest = tauri_build::AppManifest::new().commands(&["app_info", "app_log_frontend"]);
    let attributes = tauri_build::Attributes::new().app_manifest(app_manifest);
    if let Err(err) = tauri_build::try_build(attributes) {
        eprintln!("Falha ao executar tauri_build no MaxTerminal: {err}");
        std::process::exit(1);
    }
}
```

### 2.2. `src-tauri/permissions/default.toml`

O `tauri_build::AppManifest::commands(&["app_info", "app_log_frontend"])` gera automaticamente no diretório de schemas as permissões `allow-app-info`, `deny-app-info`, `allow-app-log-frontend` e `deny-app-log-frontend`. Em `src-tauri/permissions/default.toml`, define-se o conjunto explícito da fundação:

```toml
"$schema" = "../gen/schemas/schema.json"

[default]
description = "Permissões mínimas de fundação do MaxTerminal (apenas app_info e app_log_frontend)."
permissions = ["allow-app-info", "allow-app-log-frontend"]
```

### 2.3. `src-tauri/capabilities/default.json`

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "foundation",
  "description": "Capability mínima da Fase 01 vinculada exclusivamente à janela principal.",
  "local": true,
  "windows": ["main"],
  "permissions": [
    "allow-app-info",
    "allow-app-log-frontend"
  ]
}
```

### 2.4. `src-tauri/tauri.conf.json`

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "MaxTerminal",
  "version": "0.1.0",
  "identifier": "com.johnattas.maxterminal",
  "build": {
    "beforeDevCommand": "npm run dev",
    "devUrl": "http://127.0.0.1:5177",
    "beforeBuildCommand": "npm run build",
    "frontendDist": "../dist"
  },
  "app": {
    "withGlobalTauri": false,
    "windows": [
      {
        "label": "main",
        "title": "MaxTerminal",
        "width": 1440,
        "height": 900,
        "minWidth": 960,
        "minHeight": 600,
        "center": true,
        "resizable": true,
        "decorations": true,
        "visible": true
      }
    ],
    "security": {
      "csp": "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'self'; frame-src 'none'",
      "devCsp": "default-src 'self'; script-src 'self' http://127.0.0.1:5177; style-src 'self' 'unsafe-inline' http://127.0.0.1:5177; img-src 'self' data: http://127.0.0.1:5177; font-src 'self' data: http://127.0.0.1:5177; connect-src 'self' ipc: http://ipc.localhost http://127.0.0.1:5177 ws://127.0.0.1:5177; object-src 'none'; base-uri 'self'; frame-src 'none'"
    }
  },
  "bundle": {
    "active": false,
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

---

## 3. Especificação dos Módulos Rust de `maxterminal` (`src-tauri/src/`)

### 3.1. `src-tauri/src/state.rs`

```rust
//! Estado gerenciado do MaxTerminal na Fase 01: metadados imutáveis e rate-limiter de logs do frontend.

use mt_core::{app::AppInfo, paths::AppPaths};
use std::{
    sync::Mutex,
    time::{Duration, Instant},
};

/// Limite máximo de eventos de log do frontend por janela de 1 segundo.
pub const FRONTEND_LOG_MAX_PER_WINDOW: u32 = 200;

/// Duração da janela de rate-limit de logs do frontend.
pub const FRONTEND_LOG_WINDOW: Duration = Duration::from_secs(1);

/// Estado global imutável compartilhado com os comandos IPC de fundação.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Caminhos XDG resolvidos e validados no startup.
    pub paths: AppPaths,
    /// Fotografia imutável de informações da aplicação.
    pub info: AppInfo,
}

impl AppState {
    /// Constrói o `AppState` a partir dos `AppPaths` já resolvidos.
    #[must_use]
    pub fn new(paths: AppPaths) -> Self {
        let build_profile = if cfg!(debug_assertions) {
            mt_core::app::BuildProfile::Debug
        } else {
            mt_core::app::BuildProfile::Release
        };

        let info = AppInfo {
            name: String::from("MaxTerminal"),
            version: String::from(env!("CARGO_PKG_VERSION")),
            build_profile,
            tauri_version: String::from(tauri::VERSION),
            os: String::from(std::env::consts::OS),
            arch: String::from(std::env::consts::ARCH),
            paths: (&paths).into(),
        };

        Self { paths, info }
    }
}

/// Resultado da avaliação do rate-limiter para um evento de log do frontend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// Evento aceito; se `dropped_in_previous_window > 0`, o chamador deve emitir primeiro
    /// um aviso agregado informando quantos eventos foram descartados na janela anterior.
    Allowed { dropped_in_previous_window: u64 },
    /// Evento descartado por exceder a cota de 200 eventos/s na janela atual.
    Dropped,
}

#[derive(Debug)]
struct RateWindow {
    window_start: Instant,
    accepted_in_window: u32,
    dropped_in_window: u64,
}

/// Estado concorrente de controle de taxa para o comando `app_log_frontend`.
#[derive(Debug)]
pub struct FrontendLogState {
    inner: Mutex<RateWindow>,
}

impl Default for FrontendLogState {
    fn default() -> Self {
        Self::new(Instant::now())
    }
}

impl FrontendLogState {
    /// Inicializa o controlador de taxa a partir de um instante monotônico inicial.
    #[must_use]
    pub fn new(now: Instant) -> Self {
        Self {
            inner: Mutex::new(RateWindow {
                window_start: now,
                accepted_in_window: 0,
                dropped_in_window: 0,
            }),
        }
    }

    /// Avalia se um novo evento no instante `now` pode ser gravado.
    pub fn check_and_record(&self, now: Instant) -> RateLimitDecision {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };

        if now.duration_since(guard.window_start) >= FRONTEND_LOG_WINDOW {
            let dropped_prev = guard.dropped_in_window;
            guard.window_start = now;
            guard.accepted_in_window = 1;
            guard.dropped_in_window = 0;
            return RateLimitDecision::Allowed {
                dropped_in_previous_window: dropped_prev,
            };
        }

        if guard.accepted_in_window < FRONTEND_LOG_MAX_PER_WINDOW {
            guard.accepted_in_window += 1;
            RateLimitDecision::Allowed {
                dropped_in_previous_window: 0,
            }
        } else {
            guard.dropped_in_window = guard.dropped_in_window.saturating_add(1);
            RateLimitDecision::Dropped
        }
    }
}
```

### 3.2. `src-tauri/src/logging.rs`

```rust
//! Subsistema de logging estruturado (`tracing`) com arquivo diário `0o600`, writer não bloqueante
//! de fila limitada e política `LogRedactor` que bloqueia texto livre na Fase 01.

use mt_core::{MtError, MtResult, error::codes};
use serde_json::json;
use std::{
    borrow::Cow,
    fmt,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use tracing::{Event, Subscriber};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::{
    EnvFilter, Layer,
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    layer::SubscriberExt,
    registry::LookupSpan,
};

/// Capacidade máxima do canal não bloqueante de logs antes de descartar linhas sob pressão.
pub const LOG_CHANNEL_BUFFERED_LINES: usize = 4096;

/// Marcador estático em pt-BR emitido quando um campo não pertence à allowlist da Fase 01.
pub const REDACTED_MARKER: &str = "[conteúdo omitido pela política de segurança da fundação]";

/// Contrato para redação/sanitização de campos textuais antes da serialização JSON do log.
/// Na Fase 03, `mt-db` fornecerá uma implementação conectada ao sanitizador de segredos.
pub trait LogRedactor: Send + Sync + 'static {
    /// Transforma um valor textual antes de serializá-lo no registro de log.
    fn redact<'a>(&self, field_name: &str, input: &'a str) -> Cow<'a, str>;
}

/// Política estrita da Fase 01: permite apenas mensagens e identificadores estáticos conhecidos.
#[derive(Clone, Copy, Debug, Default)]
pub struct FoundationRedactor;

impl FoundationRedactor {
    const ALLOWED_MESSAGES: &'static [&'static str] = &[
        "MaxTerminal iniciado",
        "Pânico detectado no backend Rust",
        "Eventos de log do frontend descartados por limite de taxa",
        "Erro capturado no runtime de componentes Vue",
        "Erro global não tratado capturado na janela",
        "Rejeição de Promise não tratada no frontend",
        "Falha durante o bootstrap inicial do frontend",
        "Falha ao executar comando IPC",
    ];

    const ALLOWED_EVENTS: &'static [&'static str] = &[
        "app_startup",
        "backend_panic",
        "frontend_log_rate_limited",
        "vue_error",
        "window_error",
        "unhandled_rejection",
        "bootstrap_failed",
        "ipc_command_error",
    ];
}

impl LogRedactor for FoundationRedactor {
    fn redact<'a>(&self, field_name: &str, input: &'a str) -> Cow<'a, str> {
        match field_name {
            "message" if Self::ALLOWED_MESSAGES.contains(&input) => Cow::Borrowed(input),
            "event" if Self::ALLOWED_EVENTS.contains(&input) => Cow::Borrowed(input),
            "version" | "code" | "command" | "dropped"
                if !input.is_empty()
                    && input.len() <= 64
                    && input
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-')) =>
            {
                Cow::Borrowed(input)
            }
            _ => Cow::Borrowed(REDACTED_MARKER),
        }
    }
}

/// Guard que mantém o worker de escrita assíncrona ativo até o encerramento de `run()`.
pub struct LogGuard {
    _file_guard: WorkerGuard,
    _stderr_guard: WorkerGuard,
    log_file_path: PathBuf,
    dropped_records: Arc<AtomicU64>,
}

impl LogGuard {
    /// Caminho do arquivo de log diário aberto no startup.
    #[must_use]
    pub fn log_file_path(&self) -> &Path {
        &self.log_file_path
    }

    /// Total de registros descartados ou rejeitados pelo writer subjacente.
    #[must_use]
    pub fn dropped_records(&self) -> u64 {
        self.dropped_records.load(Ordering::Relaxed)
    }
}

struct SafeFieldVisitor<'a> {
    redactor: &'a dyn LogRedactor,
    message: Cow<'static, str>,
    event: Option<String>,
    version: Option<String>,
    code: Option<String>,
    command: Option<String>,
    dropped: Option<u64>,
}

impl<'a> SafeFieldVisitor<'a> {
    fn new(redactor: &'a dyn LogRedactor) -> Self {
        Self {
            redactor,
            message: Cow::Borrowed(REDACTED_MARKER),
            event: None,
            version: None,
            code: None,
            command: None,
            dropped: None,
        }
    }
}

impl tracing::field::Visit for SafeFieldVisitor<'_> {
    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        if field.name() == "dropped" {
            self.dropped = Some(value);
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        let redacted = self.redactor.redact(field.name(), value);
        match field.name() {
            "message" => self.message = Cow::Owned(redacted.into_owned()),
            "event" => self.event = Some(redacted.into_owned()),
            "version" => self.version = Some(redacted.into_owned()),
            "code" => self.code = Some(redacted.into_owned()),
            "command" => self.command = Some(redacted.into_owned()),
            _ => {}
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let formatted = format!("{value:?}");
        let trimmed = formatted.trim_matches('"');
        self.record_str(field, trimmed);
    }
}

/// Formatador JSON estruturado que aplica `LogRedactor` campo a campo antes da serialização.
pub struct RedactedJsonFormatter {
    redactor: Arc<dyn LogRedactor>,
}

impl RedactedJsonFormatter {
    /// Cria um novo formatador JSON com o redator fornecido.
    #[must_use]
    pub fn new(redactor: Arc<dyn LogRedactor>) -> Self {
        Self { redactor }
    }
}

impl<S, N> FormatEvent<S, N> for RedactedJsonFormatter
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> fmt::Result {
        let meta = event.metadata();
        let target = meta.target();
        let allowed_target = matches!(target, "maxterminal" | "frontend" | "ipc");
        let safe_target = if allowed_target { target } else { "filtered" };

        let mut visitor = SafeFieldVisitor::new(self.redactor.as_ref());
        event.record(&mut visitor);

        let mut obj = serde_json::Map::new();
        obj.insert(String::from("level"), json!(meta.level().as_str()));
        obj.insert(String::from("target"), json!(safe_target));
        obj.insert(String::from("message"), json!(visitor.message));
        if let Some(ev) = visitor.event {
            obj.insert(String::from("event"), json!(ev));
        }
        if let Some(ver) = visitor.version {
            obj.insert(String::from("version"), json!(ver));
        }
        if let Some(code) = visitor.code {
            obj.insert(String::from("code"), json!(code));
        }
        if let Some(cmd) = visitor.command {
            obj.insert(String::from("command"), json!(cmd));
        }
        if let Some(dropped) = visitor.dropped {
            obj.insert(String::from("dropped"), json!(dropped));
        }

        let serialized =
            serde_json::to_string(&serde_json::Value::Object(obj)).map_err(|_| fmt::Error)?;
        writeln!(writer, "{serialized}")
    }
}

/// Calcula a data UTC `AAAA-MM-DD` a partir de `SystemTime::now()` (algoritmo civil de Howard Hinnant)
/// sem adicionar dependências extras apenas para nomear o arquivo diário.
#[must_use]
pub fn current_utc_date_ymd() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format_unix_days_ymd((secs / 86_400) as i64)
}

/// Converte dias desde `1970-01-01` para `AAAA-MM-DD`.
#[must_use]
pub fn format_unix_days_ymd(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = i64::from(yoe) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

/// Abre ou cria o arquivo `maxterminal.log.<AAAA-MM-DD>` dentro de `log_dir` com permissão `0o600`.
pub fn open_daily_log_file(log_dir: &Path, date_ymd: &str) -> MtResult<(fs::File, PathBuf)> {
    if !log_dir.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let mut builder = fs::DirBuilder::new();
            builder.recursive(true).mode(0o700);
            builder.create(log_dir)?;
        }
        #[cfg(not(unix))]
        {
            fs::create_dir_all(log_dir)?;
        }
    }

    let file_name = format!("maxterminal.log.{date_ymd}");
    let file_path = log_dir.join(file_name);

    let mut opts = OpenOptions::new();
    opts.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }

    let file = opts.open(&file_path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = file.metadata()?;
        let mut perms = meta.permissions();
        if perms.mode() & 0o777 != 0o600 {
            perms.set_mode(0o600);
            fs::set_permissions(&file_path, perms)?;
        }
    }

    Ok((file, file_path))
}

/// Constrói o `EnvFilter` restrito aos targets autorizados (`maxterminal`, `frontend`, `ipc`).
#[must_use]
pub fn build_env_filter(raw_level: Option<&str>) -> EnvFilter {
    let level = match raw_level.map(str::trim) {
        Some("trace") => "trace",
        Some("debug") => "debug",
        Some("warn") => "warn",
        Some("error") => "error",
        _ => "info",
    };
    let directive = format!("off,maxterminal={level},frontend={level},ipc={level}");
    EnvFilter::try_new(directive).unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Inicializa o subscriber global de logging e instala o panic hook estático.
pub fn init_logging(log_dir: &Path, redactor: Box<dyn LogRedactor>) -> MtResult<LogGuard> {
    let date_ymd = current_utc_date_ymd();
    let (file, log_file_path) = open_daily_log_file(log_dir, &date_ymd)?;
    let dropped_records = Arc::new(AtomicU64::new(0));

    let (file_writer, file_guard) = NonBlockingBuilder::default()
        .buffered_lines_limit(LOG_CHANNEL_BUFFERED_LINES)
        .lossy(true)
        .finish(file);

    let (stderr_writer, stderr_guard) = NonBlockingBuilder::default()
        .buffered_lines_limit(LOG_CHANNEL_BUFFERED_LINES)
        .lossy(true)
        .finish(std::io::stderr());

    let shared_redactor: Arc<dyn LogRedactor> = Arc::from(redactor);
    let env_level = std::env::var("MAXTERMINAL_LOG").ok();

    let file_layer = tracing_subscriber::fmt::layer()
        .event_format(RedactedJsonFormatter::new(Arc::clone(&shared_redactor)))
        .with_writer(file_writer)
        .with_filter(build_env_filter(env_level.as_deref()));

    let stderr_layer = tracing_subscriber::fmt::layer()
        .event_format(RedactedJsonFormatter::new(shared_redactor))
        .with_writer(stderr_writer)
        .with_filter(build_env_filter(env_level.as_deref()));

    let subscriber = tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer);

    tracing::subscriber::set_global_default(subscriber).map_err(|err| {
        MtError::new(
            codes::INTERNAL,
            "Não foi possível inicializar o subsistema de logs do MaxTerminal.",
        )
        .with_source(err)
    })?;

    install_safe_panic_hook();

    Ok(LogGuard {
        _file_guard: file_guard,
        _stderr_guard: stderr_guard,
        log_file_path,
        dropped_records,
    })
}

/// Instala um panic hook que registra apenas a ocorrência estática, sem gravar payload ou stack trace.
fn install_safe_panic_hook() {
    std::panic::set_hook(Box::new(|_panic_info| {
        tracing::error!(
            target: "maxterminal",
            event = "backend_panic",
            "Pânico detectado no backend Rust"
        );
    }));
}
```

### 3.3. `src-tauri/src/commands/mod.rs` e `src-tauri/src/commands/app.rs`

`src-tauri/src/commands/mod.rs`:
```rust
//! Módulos de comandos IPC Tauri organizados por domínio.

pub mod app;
```

`src-tauri/src/commands/app.rs`:
```rust
//! Comandos IPC de fundação: metadados da aplicação (`app_info`) e recepção de eventos de log (`app_log_frontend`).

use std::time::Instant;

use mt_core::{
    MtResult,
    app::{AppInfo, FrontendLogEvent},
};

use crate::state::{AppState, FrontendLogState, RateLimitDecision};

/// Retorna a fotografia imutável de metadados e caminhos resolvidos do MaxTerminal.
#[tauri::command]
#[specta::specta]
pub fn app_info(state: tauri::State<'_, AppState>) -> MtResult<AppInfo> {
    Ok(state.info.clone())
}

/// Registra um evento enumerado do frontend no log estruturado, respeitando o rate-limit de 200 eventos/s.
#[tauri::command]
#[specta::specta]
pub fn app_log_frontend(
    event: FrontendLogEvent,
    state: tauri::State<'_, FrontendLogState>,
) -> MtResult<()> {
    record_frontend_event_with_now(event, &state, Instant::now());
    Ok(())
}

/// Função interna testável com relógio injetado (`Instant`) para validação determinística do rate-limiter.
pub fn record_frontend_event_with_now(
    event: FrontendLogEvent,
    state: &FrontendLogState,
    now: Instant,
) {
    match state.check_and_record(now) {
        RateLimitDecision::Allowed {
            dropped_in_previous_window,
        } => {
            if dropped_in_previous_window > 0 {
                tracing::warn!(
                    target: "frontend",
                    event = "frontend_log_rate_limited",
                    dropped = dropped_in_previous_window,
                    "Eventos de log do frontend descartados por limite de taxa"
                );
            }
            tracing::error!(
                target: "frontend",
                event = event.as_str(),
                "{}",
                event.static_message()
            );
        }
        RateLimitDecision::Dropped => {}
    }
}
```

### 3.4. `src-tauri/src/events/mod.rs`

```rust
//! Registro de eventos tipados do backend para o frontend (vazio na Fase 01; estendido a partir da Fase 02).
```

### 3.5. `src-tauri/src/specta.rs`

```rust
//! Configuração central do `tauri-specta` para registro de comandos, tipos de domínio e exportação TypeScript.

use std::path::{Path, PathBuf};

use mt_core::{
    ErrorCode, MtError,
    app::{AppInfo, BuildProfile, FrontendLogEvent},
    ids::{
        CommandBlockId, CommandGroupId, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId,
    },
    paths::AppPathsDto,
};
use specta_typescript::Typescript;
use tauri_specta::{Builder, ErrorHandlingMode, collect_commands};

use crate::commands::app::{app_info, app_log_frontend};

/// Constrói o `tauri_specta::Builder<tauri::Wry>` com todos os comandos e tipos da Fase 01.
#[must_use]
pub fn builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .error_handling(ErrorHandlingMode::Result)
        .commands(collect_commands![app_info, app_log_frontend])
        .typ::<ProjectId>()
        .typ::<TabId>()
        .typ::<PaneId>()
        .typ::<PtyId>()
        .typ::<SessionId>()
        .typ::<TemplateId>()
        .typ::<CommandBlockId>()
        .typ::<CommandGroupId>()
        .typ::<ErrorCode>()
        .typ::<MtError>()
        .typ::<AppPathsDto>()
        .typ::<BuildProfile>()
        .typ::<AppInfo>()
        .typ::<FrontendLogEvent>()
}

/// Retorna o caminho canônico de `src/types/bindings.ts` resolvido a partir de `CARGO_MANIFEST_DIR`.
#[must_use]
pub fn canonical_bindings_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/types/bindings.ts")
}

/// Configura o exportador TypeScript com cabeçalho determinístico.
#[must_use]
pub fn typescript_exporter() -> Typescript {
    Typescript::default().header("// @ts-nocheck\n// Arquivo gerado automaticamente por tauri-specta (`npm run bindings`). Não edite manualmente.\n")
}

/// Exporta os bindings TypeScript para o caminho informado sem inicializar WebView ou Tauri runtime.
pub fn export_bindings_to(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("Falha ao criar diretório de bindings: {err}"))?;
    }
    builder()
        .export(typescript_exporter(), path)
        .map_err(|err| format!("Falha ao exportar bindings TypeScript via Specta: {err}"))
}

/// Exporta os bindings TypeScript para `src/types/bindings.ts`.
pub fn export_bindings() -> Result<(), String> {
    export_bindings_to(&canonical_bindings_path())
}
```

### 3.6. `src-tauri/src/bin/export_bindings.rs`

```rust
//! Utilitário de linha de comando para geração explícita de `src/types/bindings.ts`.

fn main() {
    if let Err(err) = maxterminal_lib::specta::export_bindings() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}
```

### 3.7. `src-tauri/src/lib.rs` e `src-tauri/src/main.rs`

`src-tauri/src/lib.rs`:
```rust
//! Ponto de composição da biblioteca nativa do MaxTerminal.

pub mod commands;
pub mod events;
pub mod logging;
pub mod specta;
pub mod state;

use mt_core::paths::{AppPaths, SystemEnv};
use state::{AppState, FrontendLogState};

use crate::logging::{FoundationRedactor, init_logging};

/// Executa o ciclo de vida completo do aplicativo desktop MaxTerminal.
pub fn run() {
    let paths = match AppPaths::resolve(&SystemEnv) {
        Ok(paths) => paths,
        Err(err) => {
            eprintln!("Falha ao resolver diretórios do MaxTerminal: {}", err.message);
            std::process::exit(1);
        }
    };

    if let Err(err) = paths.ensure_dirs() {
        eprintln!("Falha ao criar diretórios do MaxTerminal: {}", err.message);
        std::process::exit(1);
    };

    let _log_guard = match init_logging(&paths.log_dir, Box::new(FoundationRedactor)) {
        Ok(guard) => guard,
        Err(err) => {
            eprintln!("Falha ao inicializar logs do MaxTerminal: {}", err.message);
            std::process::exit(1);
        }
    };

    let app_state = AppState::new(paths);
    let version = app_state.info.version.clone();

    tracing::info!(
        target: "maxterminal",
        event = "app_startup",
        version = %version,
        "MaxTerminal iniciado"
    );

    let specta_builder = specta::builder();

    let app_result = tauri::Builder::default()
        .manage(app_state)
        .manage(FrontendLogState::default())
        .invoke_handler(specta_builder.invoke_handler())
        .run(tauri::generate_context!());

    if let Err(_err) = app_result {
        eprintln!("Falha crítica ao executar o runtime Tauri do MaxTerminal.");
        std::process::exit(1);
    }
}
```

`src-tauri/src/main.rs`:
```rust
//! Entrypoint do binário executável do MaxTerminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    maxterminal_lib::run();
}
```

---

## 4. Testes de Integração Backend (`src-tauri/tests/`)

### 4.1. `src-tauri/tests/bindings.rs`

1. **`export` (`#[test] #[ignore]`):** Chama `maxterminal_lib::specta::export_bindings()` para atualizar `../src/types/bindings.ts` quando invocado por `npm run bindings`.
2. **`bindings_are_up_to_date_and_deterministic` (`#[test]`):**
   - Exporta duas vezes seguidas para arquivos temporários dentro de `target/test-tmp/bindings/` e verifica igualdade byte a byte (determinismo).
   - Compara o conteúdo gerado com `../src/types/bindings.ts` sem modificar o arquivo versionado.
   - Verifica que uma cópia mutada em memória é detectada como divergente.
   - Verifica a presença das exportações obrigatórias: `commands`, `appInfo`, `appLogFrontend`, `AppInfo`, `AppPathsDto`, `BuildProfile`, `MtError`, `ErrorCode`, `FrontendLogEvent`, `ProjectId`, `TabId`, `PaneId`, `PtyId`, `SessionId`, `TemplateId`, `CommandBlockId`, `CommandGroupId` e a ausência do campo `source`.

### 4.2. `src-tauri/tests/capabilities.rs`

Testa a configuração de segurança, ACL e CSP sem adicionar comandos falsos ao binário de release:
1. **`capabilities_restrict_to_main_window_and_two_commands`:** Lê `capabilities/default.json` e `tauri.conf.json` garantindo:
   - `windows` == `["main"]`, `local` == `true`, sem `remote` ou `urls`.
   - `permissions` contém exclusivamente `allow-app-info` e `allow-app-log-frontend` (sem `core:default`).
   - `devUrl` == `"http://127.0.0.1:5177"`, janela `main` com `1440x900` e mínimo `960x600`, `bundle.active == false`.
   - CSP de produção não contém `127.0.0.1:5177`, `unsafe-eval` nem origens HTTP externas.
2. **`acl_runtime_allows_main_and_denies_unauthorized_window_or_command`:** Usando o runtime de teste do Tauri / inspeção da ACL compilada pelo `tauri_build::AppManifest`, comprova que:
   - Na janela `"main"`, `app_info` e `app_log_frontend` são autorizados.
   - Em uma janela `"secondary"` não listada na capability `foundation`, `app_info` é negado pela ACL.
   - Um comando não listado na allowlist é negado pela ACL.
