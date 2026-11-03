//! Testes de integração de parsing FTS5, highlight seguro (`[REDACTED:*]`) e limites de `ParsedQuery` (`mt-db` — Fase 17).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use mt_db::search::{
    HighlightSegment, MAX_QUERY_BYTES, MAX_QUERY_CHARS, ParsedQuery, SearchError,
    highlight_tokens_safe,
};
use proptest::prelude::*;

#[test]
fn empty_and_whitespace_queries_return_none() {
    assert!(ParsedQuery::parse("", 1).unwrap().is_none());
    assert!(ParsedQuery::parse("   \t\n  ", 2).unwrap().is_none());
    assert!(ParsedQuery::parse("\"\"   **  ::", 3).unwrap().is_none());
}

#[test]
fn rejects_nul_byte_and_exceeding_limits() {
    let err_nul = ParsedQuery::parse("git\0status", 1).unwrap_err();
    assert!(matches!(err_nul, SearchError::InvalidQuery { .. }));

    let too_many_chars = "a".repeat(MAX_QUERY_CHARS + 1);
    let err_chars = ParsedQuery::parse(&too_many_chars, 2).unwrap_err();
    assert!(matches!(err_chars, SearchError::InvalidQuery { .. }));

    // 256 caracteres de 4 bytes cada = 1024 bytes (válido), +1 byte falha
    let max_emoji = "🦀".repeat(MAX_QUERY_CHARS);
    assert_eq!(max_emoji.chars().count(), MAX_QUERY_CHARS);
    assert_eq!(max_emoji.len(), MAX_QUERY_BYTES);
    assert!(ParsedQuery::parse(&max_emoji, 3).is_ok());
}

#[test]
fn hostile_fts5_syntax_never_causes_sqlite_match_error() {
    let hostile_inputs = [
        "\"unbalanced quote",
        "AND OR NOT NEAR",
        "foo*bar:baz^qux",
        "NEAR(a b, 5)",
        "title:secret OR body:token",
        "(((SELECT * FROM sqlite_master)))",
        "--- +++ *** ???",
        "app/Models/User.php:42",
        "ghp_1234567890abcdef",
    ];

    let conn = rusqlite::Connection::open_in_memory().unwrap();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE t_fts USING fts5(body, prefix='2 3');
         INSERT INTO t_fts(body) VALUES ('Erro em app/Models/User.php:42 durante migrate');",
    )
    .unwrap();

    for (idx, input) in hostile_inputs.iter().enumerate() {
        let parsed = ParsedQuery::parse(input, idx as u64 + 1).unwrap();
        if let Some(pq) = parsed {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM t_fts WHERE t_fts MATCH ?1;",
                    [&pq.fts_match_expr],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(count >= 0);
        }
    }
}

#[test]
fn sentinel_collision_avoidance_and_extraction() {
    let pq = ParsedQuery::parse("erro", 42).unwrap().unwrap();
    // Mesmo que o texto contenha bytes de controle \u{0001} e \u{0002}, extract_segments_from_marked
    // só interpreta o par exato de sentinelas com nonce.
    let raw_src = "Linha com \u{0001}lixo\u{0002} e erro real";
    let marked = format!(
        "Linha com \u{0001}lixo\u{0002} e {}erro{} real",
        pq.open_sentinel, pq.close_sentinel
    );
    let segments = pq.parse_snippet_segments(raw_src, &marked);
    let reconstructed: String = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(reconstructed, "Linha com \u{0001}lixo\u{0002} e erro real");
    assert!(segments.iter().any(|s| s.matched && s.text == "erro"));
}

#[test]
fn highlight_never_splits_or_matches_inside_redacted_placeholders() {
    let text =
        " Falha com Authorization: [REDACTED:Bearer] e [REDACTED:password] no comando bearer ";
    let tokens = vec![
        String::from("redacted"),
        String::from("bearer"),
        String::from("password"),
    ];
    let segments = highlight_tokens_safe(text, &tokens);

    let joined: String = segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(joined, text);

    for seg in &segments {
        if seg.matched {
            assert!(
                !seg.text.contains("REDACTED"),
                "Segmento marcado não pode conter REDACTED: {seg:?}"
            );
            assert_eq!(seg.text.to_lowercase(), "bearer");
        }
    }

    // Verifica que os placeholders aparecem inteiros dentro de segmentos unmatched
    assert!(
        segments
            .iter()
            .any(|s| !s.matched && s.text.contains("[REDACTED:Bearer]"))
    );
    assert!(
        segments
            .iter()
            .any(|s| !s.matched && s.text.contains("[REDACTED:password]"))
    );
}

proptest! {
    #[test]
    fn prop_highlight_preserves_exact_text_and_redacted_integrity(
        prefix in "[a-zA-Z0-9 _:/.-]{0,40}",
        tag in "[A-Za-z_]{1,12}",
        suffix in "[a-zA-Z0-9 _:/.-]{0,40}",
        query in "[a-zA-Z0-9]{1,10}",
    ) {
        let placeholder = format!("[REDACTED:{tag}]");
        let full_text = format!("{prefix}{placeholder}{suffix}");
        let tokens = vec![query.to_lowercase(), tag.to_lowercase(), String::from("redacted")];

        let segments: Vec<HighlightSegment> = highlight_tokens_safe(&full_text, &tokens);
        let reconstructed: String = segments.iter().map(|s| s.text.as_str()).collect();
        prop_assert_eq!(&reconstructed, &full_text);

        // O placeholder [REDACTED:...] deve estar integralmente contido em um único segmento não-marcado
        let holder_intact = segments
            .iter()
            .any(|s| !s.matched && s.text.contains(&placeholder));
        prop_assert!(holder_intact);
    }
}
