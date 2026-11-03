//! Testes orientados por tabela carregando `tests/fixtures/sanitizer_cases.toml`
//! para validar todas as 13 famílias de `SecretKind`, casos negativos (`Cow::Borrowed`)
//! e idempotência (`sanitize(&sanitize(x)) == sanitize(x)`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::borrow::Cow;
use std::collections::HashMap;

use mt_db::sanitizer::{SecretKind, sanitize, sanitize_with};

#[derive(Debug, Clone)]
struct SanitizerCase {
    name: String,
    kind: SecretKind,
    positive: bool,
    input: String,
    expected: String,
}

fn unescape_toml_basic_string(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') | None => out.push('\\'),
                Some('"') => out.push('"'),
                Some('_') => out.push('_'),
                Some('-') => out.push('-'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn parse_cases_toml(content: &str) -> Vec<SanitizerCase> {
    let mut cases = Vec::new();
    let mut current: HashMap<String, String> = HashMap::new();
    let mut in_case = false;

    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[case]]" {
            if in_case {
                cases.push(build_case(&current));
                current.clear();
            }
            in_case = true;
            continue;
        }
        if let Some((key, val)) = line.split_once('=') {
            let key = key.trim().to_string();
            let val = val.trim();
            let parsed_val = if let Some(inner) =
                val.strip_prefix('"').and_then(|s| s.strip_suffix('"'))
            {
                unescape_toml_basic_string(inner)
            } else if let Some(inner) = val.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')) {
                inner.to_string()
            } else {
                val.to_string()
            };
            current.insert(key, parsed_val);
        }
    }

    if in_case && !current.is_empty() {
        cases.push(build_case(&current));
    }

    cases
}

fn build_case(map: &HashMap<String, String>) -> SanitizerCase {
    let name = map.get("name").expect("campo 'name' ausente").clone();
    let kind_str = map.get("kind").expect("campo 'kind' ausente");
    let kind = SecretKind::from_name(kind_str)
        .unwrap_or_else(|| panic!("SecretKind desconhecido '{kind_str}' no caso '{name}'"));
    let positive = map
        .get("positive")
        .expect("campo 'positive' ausente")
        .parse::<bool>()
        .unwrap_or_else(|_| panic!("booleano inválido em 'positive' no caso '{name}'"));
    let input = map.get("input").expect("campo 'input' ausente").clone();
    let expected = map
        .get("expected")
        .expect("campo 'expected' ausente")
        .clone();

    SanitizerCase {
        name,
        kind,
        positive,
        input,
        expected,
    }
}

#[test]
fn table_cases_cover_all_13_families_and_match_expected() {
    let toml_raw = include_str!("fixtures/sanitizer_cases.toml");
    let cases = parse_cases_toml(toml_raw);

    assert!(
        cases.len() >= 65,
        "esperado >= 65 casos no catálogo, encontrado {}",
        cases.len()
    );

    let mut pos_counts: HashMap<SecretKind, usize> = HashMap::new();
    let mut neg_counts: HashMap<SecretKind, usize> = HashMap::new();

    for case in &cases {
        let (out, report) = sanitize_with(&case.input);
        assert_eq!(
            out.as_ref(),
            case.expected.as_str(),
            "falha no caso '{}' ({:?}, positive={})",
            case.name,
            case.kind,
            case.positive
        );

        if case.positive {
            *pos_counts.entry(case.kind).or_insert(0) += 1;
            assert!(
                report.total_redactions >= 1,
                "caso positivo '{}' deveria registrar pelo menos 1 redação",
                case.name
            );
            assert!(
                report.count_for(case.kind) >= 1,
                "caso positivo '{}' deveria registrar a família {:?}, mas reportou {:?}",
                case.name,
                case.kind,
                report.by_kind
            );
            assert!(
                out.contains(case.kind.marker()),
                "saída do caso positivo '{}' deveria conter o marcador {}",
                case.name,
                case.kind.marker()
            );
        } else {
            *neg_counts.entry(case.kind).or_insert(0) += 1;
            assert_eq!(
                report.total_redactions, 0,
                "caso negativo '{}' não deveria sofrer redações",
                case.name
            );
            assert!(
                matches!(out, Cow::Borrowed(_)),
                "caso negativo '{}' deveria retornar Cow::Borrowed sem alocar",
                case.name
            );
        }

        // Invariante de idempotência: sanitize(&sanitize(x)) == sanitize(x)
        let second = sanitize(&out);
        assert_eq!(
            second.as_ref(),
            out.as_ref(),
            "quebra de idempotência no caso '{}'",
            case.name
        );
        assert!(
            matches!(second, Cow::Borrowed(_)),
            "segunda passagem em texto já sanitizado ('{}') deve retornar Cow::Borrowed",
            case.name
        );
    }

    for kind in SecretKind::ALL {
        let pos = pos_counts.get(&kind).copied().unwrap_or(0);
        let neg = neg_counts.get(&kind).copied().unwrap_or(0);
        assert!(
            pos >= 3,
            "família {kind:?} possui apenas {pos} casos positivos (mínimo exigido: 3)"
        );
        assert!(
            neg >= 2,
            "família {kind:?} possui apenas {neg} casos negativos (mínimo exigido: 2)"
        );
    }
}
