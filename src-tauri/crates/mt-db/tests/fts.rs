//! Testes de integração de índices FTS5, remoção de diacríticos, busca por prefixo e `proptest` (`mt-db`).

#![allow(clippy::unwrap_used)]

use mt_db::{
    DbConfig, FtsRegistry, FtsSpec, FtsTokenizer, build_match_query, format_safe_snippet,
    open_writer, verify_sqlite_capabilities,
};
use proptest::prelude::*;
use tempfile::TempDir;

const DOCS_FTS_SPEC: FtsSpec = FtsSpec {
    content_table: "docs",
    fts_table: "docs_fts",
    columns: &["title", "body"],
    tokenizer: FtsTokenizer::Text,
    prefix: &[2, 3],
};

#[test]
fn fts5_available() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let conn = open_writer(&cfg).unwrap().conn;
    let ver = verify_sqlite_capabilities(&conn, &cfg.db_path).unwrap();
    assert_ne!(ver, "");
}

#[test]
fn external_content_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let mut conn = open_writer(&cfg).unwrap().conn;

    conn.execute_batch(
        "CREATE TABLE docs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            body TEXT NOT NULL
        ) STRICT;",
    )
    .unwrap();
    conn.execute_batch(&DOCS_FTS_SPEC.ddl()).unwrap();

    let mut fts_reg = FtsRegistry::new();
    fts_reg.register(DOCS_FTS_SPEC).unwrap();

    // 1. INSERT
    conn.execute(
        "INSERT INTO docs (title, body) VALUES ('Deploy Laravel', 'php artisan migrate --force');",
        [],
    )
    .unwrap();

    let q_artisan = build_match_query("artisan").unwrap();
    let count_insert: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
            [&q_artisan],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_insert, 1);

    // 2. UPDATE
    conn.execute(
        "UPDATE docs SET body = 'cargo test --workspace' WHERE id = 1;",
        [],
    )
    .unwrap();

    let count_old: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
            [&q_artisan],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_old, 0);

    let q_cargo = build_match_query("cargo").unwrap();
    let count_new: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
            [&q_cargo],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_new, 1);

    // 3. ROLLBACK de transação
    {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO docs (title, body) VALUES ('Rollback Doc', 'termo_transiente');",
            [],
        )
        .unwrap();
        tx.rollback().unwrap();
    }

    let q_transient = build_match_query("termo_transiente").unwrap();
    let count_rb: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
            [&q_transient],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_rb, 0);

    // 4. DELETE
    conn.execute("DELETE FROM docs WHERE id = 1;", []).unwrap();
    let count_after_del: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
            [&q_cargo],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count_after_del, 0);

    // 5. Integrity check, optimize e rebuild
    fts_reg.integrity_check_all(&conn).unwrap();
    fts_reg.optimize_all(&conn).unwrap();
    fts_reg.rebuild_all(&conn).unwrap();
    fts_reg.integrity_check_all(&conn).unwrap();
}

#[test]
fn diacritics_removed_and_prefix_search() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let conn = open_writer(&cfg).unwrap().conn;

    conn.execute_batch(
        "CREATE TABLE docs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            body TEXT NOT NULL
        ) STRICT;",
    )
    .unwrap();
    conn.execute_batch(&DOCS_FTS_SPEC.ddl()).unwrap();

    conn.execute(
        "INSERT INTO docs (title, body) VALUES ('Execução Contínua', 'Configuração de AÇÃO rápida com npm run dev');",
        [],
    )
    .unwrap();

    // Busca sem acento deve encontrar texto com acento e caixa alta.
    let q_diacritics = build_match_query("execucao acao").unwrap();
    let found_diacritics: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
            [&q_diacritics],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(found_diacritics, 1);

    // Busca por prefixo ("npm ru" -> encontra "npm run dev").
    let q_prefix = build_match_query("npm ru").unwrap();
    let found_prefix: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
            [&q_prefix],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(found_prefix, 1);
}

#[test]
fn ddl_matches_handwritten_and_safe_snippet() {
    let ddl = DOCS_FTS_SPEC.ddl();
    assert!(ddl.contains("CREATE VIRTUAL TABLE IF NOT EXISTS docs_fts USING fts5("));
    assert!(ddl.contains("content='docs'"));
    assert!(ddl.contains("content_rowid='rowid'"));
    assert!(ddl.contains("tokenize=\"unicode61 remove_diacritics 2\""));
    assert!(ddl.contains("prefix='2 3'"));
    assert!(ddl.contains("CREATE TRIGGER IF NOT EXISTS docs_fts_ai AFTER INSERT ON docs"));
    assert!(ddl.contains("CREATE TRIGGER IF NOT EXISTS docs_fts_ad AFTER DELETE ON docs"));
    assert!(ddl.contains("CREATE TRIGGER IF NOT EXISTS docs_fts_au AFTER UPDATE ON docs"));

    let raw_text = "Falha ao executar comando artisan com [REDACTED:GITHUB_TOKEN] no ambiente";
    let snippet = format_safe_snippet(raw_text, "artisan", 120);
    assert!(snippet.contains("«artisan»"));
    assert!(snippet.contains("[REDACTED:GITHUB_TOKEN]"));
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn command_blocks_fts_searches_paths_flags_and_errors() {
    use mt_core::ids::{CommandBlockId, PtyId, SessionId};
    use mt_core::shell::{
        BlockEndReason, BlockStatus, CommandBlockSearchQuery, CommandTextQuality, IntegrationMode,
        ObservationConfidence, ObservationSource, OutputKind, ShellKind,
    };
    use mt_db::{Db, FinishCommandBlock, NewSession, sanitizer::SanitizedText};

    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    let session_id = SessionId::new();
    let pty_id = PtyId::new();
    db.sessions()
        .prepare(NewSession {
            id: session_id,
            tab_id: None,
            pane_id: None,
            project_id: None,
            tab_session_id: None,
            pty_id,
            shell_kind: ShellKind::Zsh,
            shell_executable: SanitizedText::new("/bin/zsh"),
            initial_cwd: SanitizedText::new("/home/dev/engeapp"),
            integration_mode: IntegrationMode::Osc133,
            started_at: 1_000,
        })
        .await
        .unwrap();

    let block_id = CommandBlockId::new();
    db.command_blocks()
        .finish(FinishCommandBlock {
            id: block_id,
            session_id,
            tab_id: None,
            pane_id: None,
            pty_id,
            command_seq: 1,
            command_text: Some(SanitizedText::new("php artisan migrate --no-interaction")),
            command_text_quality: CommandTextQuality::Exact,
            cwd: Some(SanitizedText::new("/home/dev/engeapp")),
            started_at: 1_100,
            ended_at: 1_900,
            duration_ms: 800,
            exit_code: Some(1),
            signal: None,
            status: BlockStatus::Failed,
            end_reason: BlockEndReason::ExitNonZero,
            is_continuous: false,
            source: ObservationSource::Osc133,
            confidence: ObservationConfidence::Exact,
            output_text: Some(SanitizedText::new(
                "SQLSTATE[42S02] em app/Models/User.php:42 tabela ausente",
            )),
            output_kind: OutputKind::Full,
            output_bytes: 58,
            output_lines: 1,
            error_excerpt: Some(SanitizedText::new(
                "SQLSTATE[42S02] em app/Models/User.php:42 tabela ausente",
            )),
            inspectable: true,
            start_pty_seq: 1,
            start_byte_offset: 0,
            output_start_pty_seq: Some(2),
            output_start_byte_offset: Some(10),
            end_pty_seq: 3,
            end_byte_offset: 68,
        })
        .await
        .unwrap();

    // Busca por caminho com número de linha
    let res_path = db
        .command_blocks()
        .search(CommandBlockSearchQuery {
            query: "app/Models/User.php:42".to_string(),
            session_id: Some(session_id),
            tab_id: None,
            only_errors: Some(true),
            limit: Some(10),
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(res_path.items.len(), 1);
    assert_eq!(res_path.items[0].block.id, block_id);

    // Busca por flag CLI
    let res_flag = db
        .command_blocks()
        .search(CommandBlockSearchQuery {
            query: "--no-interaction".to_string(),
            session_id: Some(session_id),
            tab_id: None,
            only_errors: None,
            limit: Some(10),
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(res_flag.items.len(), 1);
    assert_eq!(res_flag.items[0].block.id, block_id);

    // Query inválida (apenas pontuação) deve retornar DbError::QueryInvalid
    let err_invalid = db
        .command_blocks()
        .search(CommandBlockSearchQuery {
            query: "   *** ---   ".to_string(),
            session_id: None,
            tab_id: None,
            only_errors: None,
            limit: None,
            cursor: None,
        })
        .await
        .unwrap_err();
    assert!(matches!(err_invalid, mt_db::DbError::QueryInvalid { .. }));

    db.shutdown().await.unwrap();
}

proptest! {
    #[test]
    fn build_match_query_never_syntax_error(raw in ".*") {
        let tmp = TempDir::new().unwrap();
        let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
        let conn = open_writer(&cfg).unwrap().conn;

        conn.execute_batch(
            "CREATE TABLE docs (id INTEGER PRIMARY KEY, title TEXT NOT NULL, body TEXT NOT NULL) STRICT;",
        ).unwrap();
        conn.execute_batch(&DOCS_FTS_SPEC.ddl()).unwrap();
        conn.execute(
            "INSERT INTO docs (id, title, body) VALUES (1, 'hello world', 'foo bar baz');",
            [],
        ).unwrap();

        if let Some(query) = build_match_query(&raw) {
            let res: Result<i64, _> = conn.query_row(
                "SELECT COUNT(*) FROM docs_fts WHERE docs_fts MATCH ?1;",
                [&query],
                |r| r.get(0),
            );
            prop_assert!(res.is_ok(), "Query FTS5 gerou erro de sintaxe: query={query:?}, err={res:?}");
        }
    }
}
