//! Teste unificado dos 3 sinks de segurança de segredos (CA-47):
//! 1. Persistência SQLite (`SanitizedText` + `.db` + `.db-wal`)
//! 2. `ExternalPayload` (`prepare_external_payload`)
//! 3. `LogRedactor` (`mt_db::sanitizer::sanitize` acoplado a `Subscriber` de teste)

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::similar_names)]

use std::{
    fs,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use mt_db::{
    DbConfig,
    connection::open_writer,
    sanitizer::{SecretKind, prepare_external_payload, sanitize, sanitize_terminal},
};
use rusqlite::params;
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    layer::SubscriberExt,
    registry::LookupSpan,
};

struct RepresentativeCase {
    kind: SecretKind,
    secret: &'static str,
    input: String,
}

fn representative_cases() -> Vec<RepresentativeCase> {
    let pem_secret = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n-----END OPENSSH PRIVATE KEY-----";
    let jwt_secret = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4ifQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let gh_secret = concat!("ghp", "_1234567890abcdefghijklmnopqrstuvwxyzABCD");
    let gl_secret = concat!("glpat", "-1234567890abcdefghij1234");
    let slack_secret = concat!(
        "xoxb",
        "-123456789012-123456789012-abcdefghijklmnopqrstuvwx"
    );
    let stripe_secret = concat!("sk_live", "_1234567890abcdefghijklmnop");
    let anthropic_secret = concat!("sk-ant", "-api03-1234567890abcdefghijklmnopqrstuvwxyz");
    let openai_secret = concat!("sk-proj", "-1234567890abcdefghijklmnopqrstuvwxyz");
    let google_secret = concat!("AIza", "SyA1234567890abcdefghijklmnopqrstuv");
    let aws_secret = concat!("AKIA", "IOSFODNN7EXAMPLE");
    let auth_secret = "superSecretBearerTokenValue987654321";
    let url_secret = "myUrlPass987654";
    let kv_secret = "superSecretAppKeyValue1234567890";

    vec![
        RepresentativeCase {
            kind: SecretKind::PemBlock,
            secret: "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW",
            input: format!("Chave privada carregada:\n{pem_secret}\nfim"),
        },
        RepresentativeCase {
            kind: SecretKind::JwtToken,
            secret: jwt_secret,
            input: format!("Token JWT emitido: {jwt_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::GitHubToken,
            secret: gh_secret,
            input: format!("export GITHUB_PAT={gh_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::GitLabToken,
            secret: gl_secret,
            input: format!("glab auth login --token {gl_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::SlackToken,
            secret: slack_secret,
            input: format!("slack_webhook_token {slack_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::StripeKey,
            secret: stripe_secret,
            input: format!("stripe key: {stripe_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::AnthropicKey,
            secret: anthropic_secret,
            input: format!("claude key {anthropic_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::OpenAiKey,
            secret: openai_secret,
            input: format!("openai key {openai_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::GoogleApiKey,
            secret: google_secret,
            input: format!("gemini key {google_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::AwsAccessKeyId,
            secret: aws_secret,
            input: format!("aws access key {aws_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::AuthHeader,
            secret: auth_secret,
            input: format!("Authorization: Bearer {auth_secret}"),
        },
        RepresentativeCase {
            kind: SecretKind::UrlUserinfo,
            secret: url_secret,
            input: format!("postgres://admin:{url_secret}@db.internal:5432/prod"),
        },
        RepresentativeCase {
            kind: SecretKind::SensitiveKeyValue,
            secret: kv_secret,
            input: format!("APP_KEY={kv_secret}"),
        },
    ]
}

#[derive(Clone, Default)]
struct TestVecWriter {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl Write for TestVecWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.lock().unwrap().write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for TestVecWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

struct SanitizedEventFormatter;

impl<S, N> FormatEvent<S, N> for SanitizedEventFormatter
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let mut raw = String::new();
        let mut raw_writer = Writer::new(&mut raw);
        ctx.field_format()
            .format_fields(raw_writer.by_ref(), event)?;
        let sanitized = sanitize(&raw);
        writeln!(writer, "{sanitized}")
    }
}

#[test]
fn all_13_secret_families_are_redacted_across_sqlite_external_payload_and_logs() {
    let cases = representative_cases();
    assert_eq!(cases.len(), SecretKind::ALL.len());

    let tmp = tempfile::tempdir().expect("tempdir");
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let opened = open_writer(&cfg).expect("open_writer");
    let conn = opened.conn;

    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _sink_probe (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            content TEXT NOT NULL
        ) STRICT;",
    )
    .expect("create _sink_probe");

    for case in &cases {
        let expected_marker = case.kind.marker();

        // 1. Sink 1: Persistência SQLite (SanitizedText + inspeção binária de .db e .db-wal)
        let sanitized_text = sanitize_terminal(case.input.as_bytes());
        assert!(
            !sanitized_text.as_str().contains(case.secret),
            "SanitizedText vazou segredo da família {:?}",
            case.kind
        );
        assert!(
            sanitized_text.as_str().contains(expected_marker),
            "SanitizedText não incluiu marcador {expected_marker} para {:?}",
            case.kind
        );

        conn.execute(
            "INSERT INTO _sink_probe (content) VALUES (?1)",
            params![sanitized_text],
        )
        .expect("insert _sink_probe");

        // 2. Sink 2: ExternalPayload (prepare_external_payload)
        let external = prepare_external_payload(&case.input, 16 * 1024);
        assert!(
            !external.text().as_str().contains(case.secret),
            "ExternalPayload vazou segredo da família {:?}",
            case.kind
        );
        assert!(
            external.text().as_str().contains(expected_marker),
            "ExternalPayload não incluiu marcador {expected_marker} para {:?}",
            case.kind
        );

        // 3. Sink 3: LogRedactor acoplado a Subscriber de teste
        let writer = TestVecWriter::default();
        let buf = Arc::clone(&writer.buf);
        let layer = tracing_subscriber::fmt::layer()
            .event_format(SanitizedEventFormatter)
            .with_writer(writer);
        let subscriber = tracing_subscriber::registry().with(layer);

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(message = %case.input, "falha simulada");
        });

        let logged = String::from_utf8(buf.lock().unwrap().clone()).expect("utf8 log");
        assert!(
            !logged.contains(case.secret),
            "Sink de log vazou segredo da família {:?}: {logged}",
            case.kind
        );
        assert!(
            logged.contains(expected_marker),
            "Sink de log não incluiu marcador {expected_marker} para {:?}: {logged}",
            case.kind
        );
    }

    // Força flush/checkpoint e inspeciona bytes crus tanto do .db quanto do .db-wal (CA-33, CA-47)
    let _ = conn.query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |_| Ok(()));

    let db_bytes = fs::read(&cfg.db_path).expect("read .db");
    let wal_bytes = fs::read(mt_db::wal_path_for(&cfg.db_path)).unwrap_or_default();

    for case in &cases {
        let needle = case.secret.as_bytes();
        assert!(
            !db_bytes.windows(needle.len()).any(|w| w == needle),
            "Bytes crus do arquivo .db contêm segredo da família {:?}!",
            case.kind
        );
        if !wal_bytes.is_empty() {
            assert!(
                !wal_bytes.windows(needle.len()).any(|w| w == needle),
                "Bytes crus do arquivo .db-wal contêm segredo da família {:?}!",
                case.kind
            );
        }
    }
}
