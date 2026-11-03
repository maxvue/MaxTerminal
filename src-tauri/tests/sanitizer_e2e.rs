//! Teste E2E de sanitização de segredos em todos os sinks (`CA-26` — Fase 17):
//! 1. Tabelas SQLite (`command_blocks`, `scrollback_chunks`, `search_metadata`, `search_index_fts`, `fts5vocab`)
//! 2. Bytes brutos em disco (`maxterminal.db`, `maxterminal.db-wal` e arquivo de backup `.db`)
//! 3. Sink de logs (`tracing_subscriber`)
//! 4. Payload externo do botão `"Inspecionar"` (`prepare_external_payload` / `format_plan_mode_inspect_prompt`)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::too_many_lines)]

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use mt_bridge::format_plan_mode_inspect_prompt;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandTextQuality, IntegrationMode, ObservationConfidence,
    ObservationSource, OutputKind, ShellKind,
};
use mt_core::workspace::ScrollbackChunkKind;
use mt_db::sanitizer::{
    SanitizedText, StreamSanitizer, prepare_external_payload, sanitize, sanitize_terminal,
};
use mt_db::{
    BackupKind, Db, DbConfig, FinishCommandBlock, NewSession, ProjectMutation, ScrollbackRecord,
    StoredNewProject, StoredProject, wal_path_for,
};
use tracing_subscriber::{
    fmt::{FmtContext, FormatEvent, FormatFields, format::Writer},
    layer::SubscriberExt,
    registry::LookupSpan,
};
use uuid::Uuid;

struct TempTestDir(PathBuf);

impl TempTestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mt-sanitizer-e2e-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[derive(Clone, Default)]
struct VecLogWriter {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl Write for VecLogWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.buf.lock().unwrap().write(data)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for VecLogWriter {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

struct RedactingFormatter;

impl<S, N> FormatEvent<S, N> for RedactingFormatter
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
        let clean = sanitize(&raw);
        writeln!(writer, "{clean}")
    }
}

#[tokio::test]
async fn synthetic_secrets_never_leak_to_db_wal_fts5_backup_logs_or_inspect_prompt() {
    let secret_bearer = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";
    let secret_app_key = "base64:dGhpc2lzYXN5bnRoZXRpY2xhcmF2ZWxhcHBrZXk0MjA=";
    let secret_cookie = "eyJpdiI6ImZyYWdtZW50ZWQiLCJ2YWx1ZSI6InN5bnRoZXRpYyJ9";
    let secret_url_pass = "Synthet1cP@ssw0rd!99";

    let raw_lines = format!(
        "Authorization: Bearer {secret_bearer}\n\
         APP_KEY={secret_app_key}\n\
         Set-Cookie: laravel_session={secret_cookie}; Path=/; HttpOnly\n\
         DATABASE_URL=postgres://admin:{secret_url_pass}@db.internal:5432/prod\n"
    );

    // 1. Alimenta via StreamSanitizer com chunks propositalmente fragmentados no meio dos segredos
    let mut stream_sanitizer = StreamSanitizer::new();
    let mut assembled = String::new();
    for chunk in raw_lines.as_bytes().chunks(13) {
        let out = stream_sanitizer.push(chunk);
        if !out.is_empty() {
            assembled.push_str(out.as_str());
        }
    }
    let tail = stream_sanitizer.finish();
    assembled.push_str(tail.as_str());

    for needle in [
        secret_bearer,
        secret_app_key,
        secret_cookie,
        secret_url_pass,
    ] {
        assert!(
            !assembled.contains(needle),
            "StreamSanitizer vazou segredo fragmentado: {needle} em {assembled}"
        );
    }

    // 2. Persiste em command_blocks, scrollback_chunks, search_metadata, search_index_fts e cria backup .db
    let tmp = TempTestDir::new();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg.clone()).unwrap();

    let proj_id = ProjectId::new();
    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let ts_id = TabSessionId::new();
    let sess_id = SessionId::new();
    let pty_id = PtyId::new();

    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj_id,
                title: SanitizedText::new("Sanitizer E2E Project"),
                icon: String::from("logos:laravel"),
                color: String::from("#FF2D20"),
                path: String::from("/workspace/sanitizer-e2e"),
                root_id: None,
                stack: String::from("laravel"),
                identity_source: String::from("auto"),
                sort_order: 0,
                missing: false,
                created_at: 1_000,
                updated_at: 1_000,
            },
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap();

    db.sessions()
        .prepare(NewSession {
            id: sess_id,
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
            project_id: Some(proj_id),
            tab_session_id: Some(ts_id),
            pty_id,
            shell_kind: ShellKind::Zsh,
            shell_executable: SanitizedText::new("/bin/zsh"),
            initial_cwd: SanitizedText::new("/workspace/sanitizer-e2e"),
            integration_mode: IntegrationMode::Osc133,
            started_at: 2_000,
        })
        .await
        .unwrap();

    let sanitized_cmd = sanitize_terminal(raw_lines.as_bytes());
    let sanitized_err = sanitize_terminal(raw_lines.as_bytes());

    db.command_blocks()
        .finish(FinishCommandBlock {
            id: CommandBlockId::new(),
            session_id: sess_id,
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
            pty_id,
            command_seq: 1,
            command_text: Some(sanitized_cmd.clone()),
            command_text_quality: CommandTextQuality::Exact,
            cwd: Some(SanitizedText::new("/workspace/sanitizer-e2e")),
            started_at: 2_100,
            ended_at: 2_500,
            duration_ms: 400,
            exit_code: Some(1),
            signal: None,
            status: BlockStatus::Failed,
            end_reason: BlockEndReason::ExitNonZero,
            is_continuous: false,
            source: ObservationSource::Osc133,
            confidence: ObservationConfidence::Exact,
            output_text: Some(sanitized_err.clone()),
            output_kind: OutputKind::Full,
            output_bytes: i64::try_from(sanitized_err.as_str().len()).unwrap_or(0),
            output_lines: 4,
            error_excerpt: Some(sanitized_err.clone()),
            inspectable: true,
            start_pty_seq: 1,
            start_byte_offset: 0,
            output_start_pty_seq: Some(2),
            output_start_byte_offset: Some(10),
            end_pty_seq: 3,
            end_byte_offset: 100,
        })
        .await
        .unwrap();

    db.scrollback()
        .append_chunk(ScrollbackRecord::new(
            sess_id,
            1,
            3_000,
            sanitized_err.clone(),
            ScrollbackChunkKind::Output,
        ))
        .await
        .unwrap();

    let backup_entry = db
        .maintenance()
        .create_backup(BackupKind::Manual)
        .await
        .unwrap();

    // 3. Verifica zero ocorrências via SQL nas tabelas e no vocabulário FTS5 (fts5vocab)
    let needles = [
        secret_bearer,
        secret_app_key,
        secret_cookie,
        secret_url_pass,
    ];

    db.write(move |conn| {
        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS temp.search_vocab USING fts5vocab(main, 'search_metadata_fts', 'row');
             CREATE VIRTUAL TABLE IF NOT EXISTS temp.blocks_vocab USING fts5vocab(main, 'command_blocks_fts', 'row');
             CREATE VIRTUAL TABLE IF NOT EXISTS temp.scrollback_vocab USING fts5vocab(main, 'session_scrollback_fts', 'row');",
        )
        .unwrap();

        for secret in needles {
            let count_blocks: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM command_blocks WHERE instr(COALESCE(command_text, ''), ?1) > 0 OR instr(COALESCE(output_text, ''), ?1) > 0 OR instr(COALESCE(error_excerpt, ''), ?1) > 0;",
                    [secret],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count_blocks, 0, "Segredo vazou em command_blocks: {secret}");

            let count_scrollback: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM session_scrollback WHERE instr(text, ?1) > 0;",
                    [secret],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(
                count_scrollback, 0,
                "Segredo vazou em session_scrollback: {secret}"
            );

            let count_meta: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM search_metadata WHERE instr(title, ?1) > 0 OR instr(body, ?1) > 0;",
                    [secret],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(count_meta, 0, "Segredo vazou em search_metadata: {secret}");
        }

        for vocab_token in [
            "dghpc2lzYXN5bnRoZXRpY2xhcmF2ZWxhcHBrZXk0MjA".to_ascii_lowercase(),
            "synthet1cp".to_string(),
            "ssw0rd".to_string(),
        ] {
            for vocab_table in ["temp.search_vocab", "temp.blocks_vocab", "temp.scrollback_vocab"] {
                let sql = format!("SELECT COUNT(*) FROM {vocab_table} WHERE term = ?1;");
                let count_vocab: i64 = conn
                    .query_row(&sql, [&vocab_token], |r| r.get(0))
                    .unwrap();
                assert_eq!(
                    count_vocab, 0,
                    "Token de segredo vazou no dicionário FTS5 ({vocab_table}): {vocab_token}"
                );
            }
        }

        Ok(())
    })
    .await
    .unwrap();

    // 4. Inspeciona bytes brutos em disco de maxterminal.db, maxterminal.db-wal e do backup .db
    let db_bytes = fs::read(&cfg.db_path).unwrap();
    let wal_bytes = fs::read(wal_path_for(&cfg.db_path)).unwrap_or_default();
    let backup_bytes = fs::read(&backup_entry.path).unwrap();

    for secret in needles {
        let b = secret.as_bytes();
        assert!(
            !db_bytes.windows(b.len()).any(|w| w == b),
            "Bytes brutos de maxterminal.db contêm segredo: {secret}"
        );
        if !wal_bytes.is_empty() {
            assert!(
                !wal_bytes.windows(b.len()).any(|w| w == b),
                "Bytes brutos de maxterminal.db-wal contêm segredo: {secret}"
            );
        }
        assert!(
            !backup_bytes.windows(b.len()).any(|w| w == b),
            "Bytes brutos do backup .db contêm segredo: {secret}"
        );
    }

    // 5. Sink de logs e payload externo do botão "Inspecionar"
    let writer = VecLogWriter::default();
    let log_buf = Arc::clone(&writer.buf);
    let layer = tracing_subscriber::fmt::layer()
        .event_format(RedactingFormatter)
        .with_writer(writer);
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        tracing::error!(payload = %raw_lines, "erro com segredos");
    });
    let log_str = String::from_utf8(log_buf.lock().unwrap().clone()).unwrap();

    let external_cmd = prepare_external_payload(&raw_lines, 16 * 1024);
    let external_err = prepare_external_payload(&raw_lines, 16 * 1024);
    let inspect_prompt =
        format_plan_mode_inspect_prompt(external_cmd.text().as_str(), external_err.text().as_str());

    for secret in needles {
        assert!(
            !log_str.contains(secret),
            "Sink de log vazou segredo: {secret}"
        );
        assert!(
            !inspect_prompt.contains(secret),
            "Prompt do botão Inspecionar vazou segredo: {secret}"
        );
    }

    db.shutdown().await.unwrap();
}
