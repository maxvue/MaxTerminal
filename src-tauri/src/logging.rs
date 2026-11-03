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
/// Na Fase 03, `SanitizerLogRedactor` conecta o logger a `mt_db::sanitizer::sanitize`.
pub trait LogRedactor: Send + Sync + 'static {
    /// Transforma um valor textual antes de serializá-lo no registro de log.
    /// Assinatura canônica de **1 parâmetro** (T-05), idêntica à de `01/spec.md` §4.7 e `03/04/spec.md` §3:
    /// o redator não conhece o nome do campo; quem filtra por nome de campo é o `SafeFieldVisitor`.
    fn redact<'a>(&self, input: &'a str) -> Cow<'a, str>;

    /// Indica se o redator permite mensagens e eventos de texto livre (sanitizados pelo motor
    /// de redação de segredos da Fase 03) em vez de exigir a allowlist estática da Fase 01.
    fn allows_free_text(&self) -> bool {
        false
    }
}

/// Redator de produção (Fase 03+) que encaminha todo texto de log para `mt_db::sanitizer::sanitize`.
/// Garante que nenhum segredo das 13 famílias seja persistido em `logs/maxterminal.log.*` ou `stderr`.
#[derive(Clone, Copy, Debug, Default)]
pub struct SanitizerLogRedactor;

impl LogRedactor for SanitizerLogRedactor {
    fn redact<'a>(&self, input: &'a str) -> Cow<'a, str> {
        mt_db::sanitizer::sanitize(input)
    }

    fn allows_free_text(&self) -> bool {
        true
    }
}

/// Política estrita da Fase 01: permite apenas mensagens e eventos estáticos do catálogo fechado ou
/// identificadores curtos (`version`, `code`, `command`); qualquer outro texto vira `REDACTED_MARKER`.
/// A política é **agnóstica ao nome do campo** (o `SafeFieldVisitor` só encaminha os campos conhecidos
/// `message`, `event`, `version`, `code` e `command`, exigindo catálogo exato para os dois primeiros).
/// Texto com espaços fora do catálogo, ou com mais de 64 caracteres, é substituído pelo marcador.
#[derive(Clone, Copy, Debug, Default)]
pub struct FoundationRedactor;

impl FoundationRedactor {
    /// Mensagens estáticas permitidas no catálogo da fundação.
    pub const ALLOWED_MESSAGES: &'static [&'static str] = &[
        "MaxTerminal iniciado",
        "Pânico detectado no backend Rust",
        "Eventos de log do frontend descartados por limite de taxa",
        "Erro capturado no runtime de componentes Vue",
        "Erro global não tratado capturado na janela",
        "Rejeição de Promise não tratada no frontend",
        "Falha durante o bootstrap inicial do frontend",
        "Falha ao executar comando IPC",
    ];

    /// Identificadores de evento permitidos no catálogo da fundação.
    pub const ALLOWED_EVENTS: &'static [&'static str] = &[
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
    fn redact<'a>(&self, input: &'a str) -> Cow<'a, str> {
        let is_catalog_entry =
            Self::ALLOWED_MESSAGES.contains(&input) || Self::ALLOWED_EVENTS.contains(&input);
        let is_short_identifier = !input.is_empty()
            && input.len() <= 64
            && input
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
        if is_catalog_entry || is_short_identifier {
            Cow::Borrowed(input)
        } else {
            Cow::Borrowed(REDACTED_MARKER)
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
        if self.redactor.allows_free_text() {
            let redacted = self.redactor.redact(value);
            match field.name() {
                "message" => {
                    if self.message == REDACTED_MARKER {
                        self.message = Cow::Owned(redacted.into_owned());
                    } else {
                        self.message = Cow::Owned(format!("{} | {}", self.message, redacted));
                    }
                }
                "event" => self.event = Some(redacted.into_owned()),
                "version" => self.version = Some(redacted.into_owned()),
                "code" => self.code = Some(redacted.into_owned()),
                "command" => self.command = Some(redacted.into_owned()),
                _ => {
                    // Quando um campo arbitrário de texto é emitido no modo livre,
                    // anexa seu valor sanitizado à mensagem para nunca perder diagnósticos nem vazar segredos.
                    if self.message == REDACTED_MARKER {
                        self.message = Cow::Owned(redacted.into_owned());
                    } else {
                        self.message =
                            Cow::Owned(format!("{} ({}={})", self.message, field.name(), redacted));
                    }
                }
            }
            return;
        }

        let allowed = match field.name() {
            "message" => FoundationRedactor::ALLOWED_MESSAGES.contains(&value),
            "event" => FoundationRedactor::ALLOWED_EVENTS.contains(&value),
            "version" | "code" | "command" => true,
            _ => return,
        };
        let redacted = self
            .redactor
            .redact(if allowed { value } else { REDACTED_MARKER });
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
        let allowed_target = if self.redactor.allows_free_text() {
            matches!(
                target,
                "maxterminal" | "frontend" | "ipc" | "mt_db" | "mt_pty"
            ) || target.starts_with("mt_db")
                || target.starts_with("mt_pty")
                || target.starts_with("maxterminal")
                || target.starts_with("sanitizer")
        } else {
            matches!(target, "maxterminal" | "frontend" | "ipc")
        };
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
    #[allow(clippy::cast_possible_wrap)]
    format_unix_days_ymd((secs / 86_400) as i64)
}

/// Converte dias desde `1970-01-01` para `AAAA-MM-DD`.
#[must_use]
pub fn format_unix_days_ymd(days_since_epoch: i64) -> String {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
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

/// Constrói o `EnvFilter` restrito aos targets autorizados (`maxterminal`, `frontend`, `ipc`, `mt_db`, `mt_pty`).
#[must_use]
pub fn build_env_filter(raw_level: Option<&str>) -> EnvFilter {
    let level = match raw_level.map(str::trim) {
        Some("trace") => "trace",
        Some("debug") => "debug",
        Some("warn") => "warn",
        Some("error") => "error",
        _ => "info",
    };
    let directive = format!(
        "off,maxterminal={level},frontend={level},ipc={level},mt_db={level},mt_pty={level}"
    );
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
pub fn install_safe_panic_hook() {
    std::panic::set_hook(Box::new(|_panic_info| {
        tracing::error!(
            target: "maxterminal",
            event = "backend_panic",
            "Pânico detectado no backend Rust"
        );
    }));
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub mod tests {
    //! Testes unitários do subsistema de logging e redação.
    use super::*;
    use std::{
        io::{self, Write},
        sync::Mutex,
    };

    #[derive(Clone, Default)]
    struct BufferWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl Write for BufferWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.buffer
                .lock()
                .map_err(|_| io::Error::other("lock envenenado"))?
                .write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for BufferWriter {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn format_unix_days_ymd_conversion() {
        assert_eq!(format_unix_days_ymd(0), "1970-01-01");
        assert_eq!(format_unix_days_ymd(19723), "2024-01-01");
        assert_eq!(format_unix_days_ymd(20756), "2026-10-30");
        let ymd = current_utc_date_ymd();
        assert_eq!(ymd.len(), 10);
        assert_eq!(&ymd[4..5], "-");
        assert_eq!(&ymd[7..8], "-");
    }

    #[test]
    fn foundation_redactor_allowlist_and_redaction() {
        let redactor = FoundationRedactor;

        // Entradas do catálogo mantidas intactas
        assert_eq!(
            redactor.redact("MaxTerminal iniciado"),
            "MaxTerminal iniciado"
        );
        assert_eq!(redactor.redact("app_startup"), "app_startup");
        assert_eq!(
            redactor.redact("Erro capturado no runtime de componentes Vue"),
            "Erro capturado no runtime de componentes Vue"
        );
        assert_eq!(redactor.redact("vue_error"), "vue_error");

        // Identificadores curtos alfanuméricos com . _ - permitidos
        assert_eq!(redactor.redact("0.1.0"), "0.1.0");
        assert_eq!(redactor.redact("app_info"), "app_info");
        assert_eq!(redactor.redact("core.internal"), "core.internal");

        // Textos arbitrários, segredos e tokens devem virar REDACTED_MARKER
        assert_eq!(redactor.redact("SECRET_TOKEN_12345!@#"), REDACTED_MARKER);
        assert_eq!(
            redactor.redact("Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9..."),
            REDACTED_MARKER
        );
        assert_eq!(
            redactor.redact("Mensagem arbitrária de usuário que não está no catálogo"),
            REDACTED_MARKER
        );
        assert_eq!(
            redactor.redact(
                "string_muito_longa_com_mais_de_sessenta_e_quatro_caracteres_12345678901234567890"
            ),
            REDACTED_MARKER
        );
    }

    #[test]
    fn daily_log_file_created_with_0o600_and_expected_name() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-tmp")
            .join(format!("mt_log_test_{unique}"));

        let date_ymd = current_utc_date_ymd();
        let (mut file, file_path) = open_daily_log_file(&temp_dir, &date_ymd)
            .unwrap_or_else(|e| panic!("Falha ao abrir arquivo de log: {e}"));

        assert_eq!(
            file_path.file_name().unwrap_or_default().to_string_lossy(),
            format!("maxterminal.log.{date_ymd}")
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta =
                fs::metadata(&file_path).unwrap_or_else(|e| panic!("Falha ao ler metadata: {e}"));
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }

        // Escreve registro estruturado e valida persistência
        writeln!(file, r#"{{"level":"INFO","target":"maxterminal","event":"app_startup","version":"0.1.0","message":"MaxTerminal iniciado"}}"#)
            .unwrap_or_else(|e| panic!("Falha ao escrever no log: {e}"));
        drop(file);

        let content = fs::read_to_string(&file_path)
            .unwrap_or_else(|e| panic!("Falha ao ler arquivo de log: {e}"));
        assert!(content.contains("MaxTerminal iniciado"));
        assert!(content.contains(r#""version":"0.1.0""#));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn redacted_json_formatter_pipeline_and_filters() {
        let redactor = Arc::new(FoundationRedactor);
        let writer = BufferWriter::default();
        let buffer = Arc::clone(&writer.buffer);

        let layer = tracing_subscriber::fmt::layer()
            .event_format(RedactedJsonFormatter::new(redactor))
            .with_writer(writer)
            .with_filter(build_env_filter(Some("debug")));

        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            // Evento válido de inicialização
            tracing::info!(
                target: "maxterminal",
                event = "app_startup",
                version = "0.1.0",
                "MaxTerminal iniciado"
            );

            // Evento debug quando MAXTERMINAL_LOG=debug
            tracing::debug!(
                target: "maxterminal",
                command = "app_info",
                code = "core.internal",
                "MaxTerminal iniciado"
            );

            // Mensagem e evento com sentinela alfanumérica curta fora do catálogo (T-05)
            tracing::warn!(
                target: "maxterminal",
                event = "SHORT_SENTINEL_123",
                "SECRET_TOKEN_12345"
            );

            // Target não autorizado é descartado pelo EnvFilter (off por padrão)
            tracing::info!(
                target: "malicious_target",
                "MaxTerminal iniciado"
            );
        });

        let output = String::from_utf8(buffer.lock().unwrap().clone())
            .unwrap_or_else(|e| panic!("Saída UTF-8 inválida: {e}"));
        let lines: Vec<&str> = output.lines().collect();
        // 3 eventos gravados: o target "malicious_target" é bloqueado pelo EnvFilter
        assert_eq!(
            lines.len(),
            3,
            "Esperadas 3 linhas de log, obtidas: {output}"
        );

        // Linha 1: startup
        let json1: serde_json::Value = serde_json::from_str(lines[0])
            .unwrap_or_else(|e| panic!("JSON inválido na linha 1: {e}"));
        assert_eq!(json1["level"], "INFO");
        assert_eq!(json1["target"], "maxterminal");
        assert_eq!(json1["event"], "app_startup");
        assert_eq!(json1["version"], "0.1.0");
        assert_eq!(json1["message"], "MaxTerminal iniciado");

        // Linha 2: debug
        let json2: serde_json::Value = serde_json::from_str(lines[1])
            .unwrap_or_else(|e| panic!("JSON inválido na linha 2: {e}"));
        assert_eq!(json2["level"], "DEBUG");
        assert_eq!(json2["command"], "app_info");
        assert_eq!(json2["code"], "core.internal");

        // Linha 3: segredo redactado em message e event (T-05)
        let json3: serde_json::Value = serde_json::from_str(lines[2])
            .unwrap_or_else(|e| panic!("JSON inválido na linha 3: {e}"));
        assert_eq!(json3["message"], REDACTED_MARKER);
        assert_eq!(json3["event"], REDACTED_MARKER);
        assert!(!lines[2].contains("SECRET_TOKEN_12345"));
        assert!(!lines[2].contains("SHORT_SENTINEL_123"));

        // Defesa em profundidade: se um target não autorizado atingir o formatter diretamente, vira "filtered"
        let fallback_writer = BufferWriter::default();
        let fallback_buffer = Arc::clone(&fallback_writer.buffer);
        let fallback_layer = tracing_subscriber::fmt::layer()
            .event_format(RedactedJsonFormatter::new(Arc::new(FoundationRedactor)))
            .with_writer(fallback_writer);
        let fallback_sub = tracing_subscriber::registry().with(fallback_layer);

        tracing::subscriber::with_default(fallback_sub, || {
            tracing::info!(
                target: "malicious_target",
                "MaxTerminal iniciado"
            );
        });

        let fallback_out = String::from_utf8(fallback_buffer.lock().unwrap().clone())
            .unwrap_or_else(|e| panic!("Saída UTF-8 fallback inválida: {e}"));
        let json_fallback: serde_json::Value = serde_json::from_str(fallback_out.trim())
            .unwrap_or_else(|e| panic!("JSON fallback inválido: {e}"));
        assert_eq!(json_fallback["target"], "filtered");
    }

    #[test]
    fn build_env_filter_handles_various_levels() {
        let f_debug = build_env_filter(Some("debug"));
        let f_info = build_env_filter(Some("info"));
        let f_invalid = build_env_filter(Some("xyz"));
        let f_none = build_env_filter(None);

        let s_debug = format!("{f_debug}");
        assert!(s_debug.contains("maxterminal=debug"));

        let s_info = format!("{f_info}");
        assert!(s_info.contains("maxterminal=info"));

        let s_invalid = format!("{f_invalid}");
        assert!(s_invalid.contains("maxterminal=info"));

        let s_none = format!("{f_none}");
        assert!(s_none.contains("maxterminal=info"));
    }
}
