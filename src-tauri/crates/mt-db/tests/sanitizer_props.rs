//! Testes baseados em propriedades (`proptest`) para idempotência, ausência de vazamento
//! literal do segredo e equivalência sob chunking em pontos arbitrários de bytes UTF-8.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::borrow::Cow;

use mt_db::sanitizer::{SecretKind, StreamSanitizer, sanitize, sanitize_with};
use proptest::prelude::*;

fn sample_secret_for_kind(kind: SecretKind) -> (&'static str, &'static str) {
    match kind {
        SecretKind::PemBlock => (
            "-----BEGIN RSA PRIVATE KEY-----\nMIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn/ygWyF8PbnGy0AHB7MhgHcTz6sE2I2yPB\n-----END RSA PRIVATE KEY-----",
            "MIIEpAIBAAKCAQEA0Z3VS5JJcds3xfn",
        ),
        SecretKind::JwtToken => (
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvYW8ifQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
            "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
        ),
        SecretKind::GitHubToken => (
            concat!("ghp", "_1234567890abcdefghijklmnopqrstuvwxyzABCD"),
            concat!("ghp", "_1234567890abcdefghijklmnopqrstuvwxyzABCD"),
        ),
        SecretKind::GitLabToken => (
            concat!("glpat", "-1234567890abcdefghij"),
            concat!("glpat", "-1234567890abcdefghij"),
        ),
        SecretKind::SlackToken => (
            concat!("xoxb", "-123456789012-1234567890123-abcdefghijklmnopqrstuvwx"),
            concat!("xoxb", "-123456789012-1234567890123-abcdefghijklmnopqrstuvwx"),
        ),
        SecretKind::StripeKey => (
            concat!("sk_live", "_51Abcdefghijklmnopqrstuvwxyz123456"),
            concat!("sk_live", "_51Abcdefghijklmnopqrstuvwxyz123456"),
        ),
        SecretKind::AnthropicKey => (
            concat!("sk-ant", "-api03-1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"),
            concat!("sk-ant", "-api03-1234567890abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"),
        ),
        SecretKind::OpenAiKey => (
            concat!("sk-proj", "-1234567890abcdefghijklmnopqrstuvwxyzABCD"),
            concat!("sk-proj", "-1234567890abcdefghijklmnopqrstuvwxyzABCD"),
        ),
        SecretKind::GoogleApiKey => (
            concat!("AIza", "SyDaGmWKa4JsXZ-HjGw7ISLn_3namBGewQe"),
            concat!("AIza", "SyDaGmWKa4JsXZ-HjGw7ISLn_3namBGewQe"),
        ),
        SecretKind::AwsAccessKeyId => (
            concat!("AKIA", "IOSFODNN7EXAMPLE"),
            concat!("AKIA", "IOSFODNN7EXAMPLE"),
        ),
        SecretKind::AuthHeader => (
            "Authorization: Bearer super_secret_token_value_987654321",
            "super_secret_token_value_987654321",
        ),
        SecretKind::UrlUserinfo => (
            "https://deploy_user:MyUrlPassw0rd99@registry.example.com/v2/",
            "MyUrlPassw0rd99",
        ),
        SecretKind::SensitiveKeyValue => (
            "DB_PASSWORD=super_secret_db_value_2026",
            "super_secret_db_value_2026",
        ),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn prop_sanitize_is_idempotent_for_arbitrary_strings(input in ".*") {
        let once = sanitize(&input);
        let twice = sanitize(&once);
        prop_assert_eq!(once.as_ref(), twice.as_ref());
        prop_assert!(matches!(twice, Cow::Borrowed(_)));
    }

    #[test]
    fn prop_no_secret_leaks_when_embedded_in_safe_context(
        prefix in "[a-zA-Z0-9_ /.,:-]{0,60}",
        suffix in "[a-zA-Z0-9_ /.,:-]{0,60}",
        kind_idx in 0usize..13usize,
    ) {
        let kind = SecretKind::ALL[kind_idx];
        let (secret_expr, raw_secret_needle) = sample_secret_for_kind(kind);
        let text = format!("{prefix} {secret_expr} {suffix}");

        let (sanitized, report) = sanitize_with(&text);
        prop_assert!(report.total_redactions >= 1);
        prop_assert!(!sanitized.contains(raw_secret_needle));
        prop_assert!(sanitized.contains(kind.marker()));

        // Segunda passagem deve ser idempotente e Borrowed
        let again = sanitize(&sanitized);
        prop_assert_eq!(sanitized.as_ref(), again.as_ref());
        prop_assert!(matches!(again, Cow::Borrowed(_)));
    }

    #[test]
    fn prop_stream_sanitizer_arbitrary_split_points_never_leak(
        prefix in "[a-zA-Z0-9_]{1,40}",
        suffix in "[a-zA-Z0-9_]{1,40}",
        kind_idx in 0usize..13usize,
        split_points in proptest::collection::vec(1usize..120usize, 1..15),
    ) {
        let kind = SecretKind::ALL[kind_idx];
        let (secret_expr, raw_secret_needle) = sample_secret_for_kind(kind);
        let full_text = format!("inicio {prefix} {secret_expr} {suffix} fim\n");
        let bytes = full_text.as_bytes();

        let mut stream = StreamSanitizer::new();
        let mut collected = String::new();
        let mut cursor = 0usize;

        for step in split_points {
            if cursor >= bytes.len() {
                break;
            }
            let end = (cursor + step).min(bytes.len());
            collected.push_str(&stream.push(&bytes[cursor..end]));
            cursor = end;
        }
        if cursor < bytes.len() {
            collected.push_str(&stream.push(&bytes[cursor..]));
        }
        collected.push_str(&stream.finish());

        let expected = sanitize(&full_text);
        prop_assert_eq!(&collected, expected.as_ref());
        prop_assert!(!collected.contains(raw_secret_needle));
    }
}

#[test]
fn exhaustive_two_chunk_split_for_all_13_families() {
    for kind in SecretKind::ALL {
        let (secret_expr, needle) = sample_secret_for_kind(kind);
        let line = format!("prefixo 🚀 {secret_expr} sufixo çãõ\n");
        let bytes = line.as_bytes();
        let expected = sanitize(&line);

        for split_at in 0..=bytes.len() {
            let mut stream = StreamSanitizer::new();
            let mut out = stream.push(&bytes[..split_at]);
            out.push_str(&stream.push(&bytes[split_at..]));
            out.push_str(&stream.finish());

            assert_eq!(
                out,
                expected.as_ref(),
                "divergência em split_at={split_at} para a família {kind:?}"
            );
            assert!(
                !out.contains(needle),
                "vazamento em split_at={split_at} para a família {kind:?}"
            );
        }
    }
}
