//! Testes de streaming (`StreamSanitizer`, `Utf8Decoder`, `TerminalNormalizer`),
//! `prepare_external_payload` e `inspect_settings_json` (CA-05, CA-06, CA-07, CA-08).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::assert_is_empty,
    clippy::format_push_string
)]

use mt_db::sanitizer::{
    MAX_SECRET_SPAN, StreamSanitizer, TerminalNormalizer, Utf8Decoder, inspect_settings_json,
    prepare_external_payload, sanitize, sanitize_terminal,
};
use serde_json::json;

#[test]
fn utf8_decoder_preserves_multibyte_codepoints_split_byte_by_byte() {
    let original =
        "Olá, coração! 🦀🚀 Ação de sanitização em português: áéíóú âêô ãõ ç Ümlaut 日本語";
    let bytes = original.as_bytes();

    for chunk_size in [1usize, 2, 3, 5, 7, 13] {
        let mut decoder = Utf8Decoder::new();
        let mut reconstructed = String::new();

        for chunk in bytes.chunks(chunk_size) {
            reconstructed.push_str(decoder.push(chunk));
        }
        reconstructed.push_str(decoder.finish());

        assert_eq!(
            reconstructed, original,
            "falha ao reconstruir UTF-8 com chunk_size={chunk_size}"
        );
    }
}

#[test]
fn terminal_normalizer_resolves_crlf_isolated_cr_backspace_and_preserves_csi_osc() {
    let mut norm = TerminalNormalizer::new();

    // 1. \r\n -> \n
    let out1 = norm.push("linha 1\r\nlinha 2\r\n");
    assert_eq!(out1, "linha 1\nlinha 2\n");

    // 2. \r isolado sobrescreve desde o início da linha corrente
    let out2 = norm.push("Carregando 10%\rCarregando 100%\n");
    assert_eq!(out2, "Carregando 100%\n");

    // 3. \x08 (BS) apaga o último escalar Unicode (incluindo multibyte 'ã' / emoji '🦀')
    let out3 = norm.push("ab\x08c Coraçã\x08o 🦀\x08🚀\n");
    assert_eq!(out3, "ac Coraço 🚀\n");

    // 4. Sequências CSI e OSC preservadas intactas
    let ansi_line = "\x1b]0;MaxTerminal - Projeto\x07\x1b[32m✔ Build concluído\x1b[0m\r\n";
    let out4 = norm.push(ansi_line);
    assert_eq!(
        out4,
        "\x1b]0;MaxTerminal - Projeto\x07\x1b[32m✔ Build concluído\x1b[0m\n"
    );
    assert!(norm.finish().is_empty());
}

#[test]
fn stream_sanitizer_redacts_secrets_across_prime_and_boundary_chunk_sizes() {
    let mut big_doc = String::new();
    for i in 0..80 {
        big_doc.push_str(&format!(
            "[{i:03}] Log normal de compilação em /home/user/project/src/lib.rs:{i}\r\n"
        ));
        if i == 10 {
            big_doc
                .push_str("export GITHUB_TOKEN=ghp_1234567890abcdefghijklmnopqrstuvwxyzABCD\r\n");
        }
        if i == 25 {
            big_doc.push_str("Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c\r\n");
        }
        if i == 40 {
            big_doc.push_str("git clone https://admin:SuperSecretPass99@gitlab.com/org/repo.git && echo glpat-1234567890abcdefghij\r\n");
        }
        if i == 55 {
            big_doc.push_str(
                "OPENAI_API_KEY=sk-proj-1234567890abcdefghijklmnopqrstuvwxyzABCD DB_PASSWORD=\"minha_senha_123\"\r\n",
            );
        }
        if i == 70 {
            big_doc.push_str("-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n-----END OPENSSH PRIVATE KEY-----\r\n");
        }
    }

    let expected = sanitize_terminal(big_doc.as_bytes());
    assert!(
        big_doc.len() > MAX_SECRET_SPAN,
        "documento de teste deve exceder MAX_SECRET_SPAN (4096 bytes)"
    );

    for chunk_size in [1usize, 2, 3, 5, 7, 13, 64, 512, 4097] {
        let mut stream = StreamSanitizer::new();
        let mut actual = String::new();

        for chunk in big_doc.as_bytes().chunks(chunk_size) {
            actual.push_str(&stream.push(chunk));
        }
        actual.push_str(&stream.finish());

        assert_eq!(
            actual,
            expected.as_str(),
            "divergência no StreamSanitizer para chunk_size={chunk_size}"
        );
        assert!(!actual.contains("ghp_1234567890abcdefghijklmnopqrstuvwxyzABCD"));
        assert!(!actual.contains("SuperSecretPass99"));
        assert!(!actual.contains("minha_senha_123"));
        assert!(!actual.contains("b3BlbnNzaC1rZXktdjE"));
    }
}

#[test]
fn prepare_external_payload_truncates_on_valid_utf8_boundary_and_redacts_secrets() {
    let raw =
        "Erro ao conectar com sk-proj-1234567890abcdefghijklmnopqrstuvwxyzABCD na ação 🦀🚀 final";
    let full_payload = prepare_external_payload(raw, 1024);
    assert!(!full_payload.truncated);
    assert_eq!(full_payload.original_bytes, raw.len());
    assert!(full_payload.text.contains("[REDACTED:OPENAI_KEY]"));
    assert!(!full_payload.text.contains("sk-proj-"));

    // Testa truncamento em todos os limites de bytes ao redor de caracteres multibyte ('ç', 'ã', '🦀', '🚀')
    let utf8_text =
        "Prefixo áéíóú çãõ 🦀🚀 e token ghp_1234567890abcdefghijklmnopqrstuvwxyzABCD fim";
    for max_bytes in 0..=utf8_text.len() {
        let payload = prepare_external_payload(utf8_text, max_bytes);
        assert!(payload.text.len() <= max_bytes);
        assert_eq!(payload.original_bytes, utf8_text.len());
        assert!(!payload.text.contains("ghp_1234567890"));
        // Garante que é UTF-8 válido e não possui marcador `[REDACTED:...` cortado pela metade
        if let Some(pos) = payload.text.rfind("[REDACTED:") {
            assert!(
                payload.text[pos..].contains(']'),
                "marcador [REDACTED: cortado ao meio em max_bytes={max_bytes}: {}",
                payload.text
            );
        }
    }

    let sanitized_full = sanitize(utf8_text);
    assert_eq!(
        prepare_external_payload(utf8_text, 4096).text.as_str(),
        sanitized_full.as_ref()
    );
}

#[test]
fn inspect_settings_json_rejects_sensitive_keys_and_secret_values() {
    let clean = json!({
        "ui": {
            "theme": "max-navy-teal",
            "fontSize": 14,
            "ligatures": true
        },
        "terminal": {
            "scrollbackLines": 10000,
            "shell": "/bin/zsh"
        }
    });
    assert!(inspect_settings_json(&clean).is_ok());

    let bad_keys = [
        json!({ "password": "plain_value" }),
        json!({ "nested": { "apiKey": "123" } }),
        json!({ "auth": { "client_secret": "abc" } }),
        json!({ "db_password": "secret" }),
        json!({ "github_token": "tok" }),
        json!({ "openai_api_key": "key" }),
        json!({ "anthropic_api_key": "key" }),
        json!({ "aws_secret_access_key": "key" }),
    ];

    for bad in &bad_keys {
        assert!(
            inspect_settings_json(bad).is_err(),
            "deveria rejeitar JSON com chave sensível: {bad}"
        );
    }

    let bad_value = json!({
        "terminal": {
            "startupCommand": "export OPENAI_API_KEY=sk-proj-1234567890abcdefghijklmnopqrstuvwxyzABCD"
        }
    });
    assert!(
        inspect_settings_json(&bad_value).is_err(),
        "deveria rejeitar JSON contendo segredo em string interna"
    );
}
