//! Subsistema de sanitização de segredos, normalização de streams de terminal (`StreamSanitizer`)
//! e preparação de payloads externos (`ExternalPayload`).

pub mod rules;
pub mod stream;

use std::borrow::Cow;
use std::fmt;
use std::ops::Deref;

use rusqlite::types::{ToSql, ToSqlOutput, ValueRef};
use serde::Serialize;

pub use rules::{SanitizeReport, SecretKind};
pub use stream::{StreamSanitizer, TerminalNormalizer, Utf8Decoder};

/// Comprimento máximo em bytes de um segredo suportado pela janela deslizante do `StreamSanitizer`.
pub const MAX_SECRET_SPAN: usize = 4096;

/// Versão atual do catálogo de regras de sanitização.
pub const SANITIZER_CATALOG_VERSION: u16 = 1;

/// Lista de nomes de chaves sensíveis proibidas em `inspect_settings_json`.
const SENSITIVE_SETTINGS_KEYS: &[&str] = &[
    "password",
    "passwd",
    "pwd",
    "secret",
    "token",
    "api_key",
    "apikey",
    "access_key",
    "private_key",
    "client_secret",
    "auth_token",
    "refresh_token",
    "session_id",
    "cookie",
    "jwt",
    "bearer",
    "app_key",
    "database_url",
    "db_password",
    "redis_password",
    "mail_password",
    "aws_secret_access_key",
    "stripe_secret",
    "github_token",
    "openai_api_key",
    "anthropic_api_key",
];

/// Tipo opaco representando texto que já passou obrigatoriamente pelo pipeline de sanitização.
///
/// O construtor interno é privado ao módulo `sanitizer`, garantindo que toda instância
/// persistida no SQLite via `ToSql` tenha sido previamente higienizada.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize)]
#[serde(transparent)]
pub struct SanitizedText(String);

impl SanitizedText {
    /// Sanitiza uma fatia `&str` e retorna um `SanitizedText`.
    #[must_use]
    pub fn new(raw: &str) -> Self {
        Self(sanitize(raw).into_owned())
    }

    /// Retorna o texto sanitizado como `&str`.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consome o `SanitizedText` e retorna a `String` interna.
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }

    /// Retorna o comprimento em bytes do texto sanitizado.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Indica se o texto sanitizado está vazio.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl AsRef<str> for SanitizedText {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Deref for SanitizedText {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for SanitizedText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq<str> for SanitizedText {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

impl PartialEq<&str> for SanitizedText {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}

impl PartialEq<String> for SanitizedText {
    fn eq(&self, other: &String) -> bool {
        &self.0 == other
    }
}

impl ToSql for SanitizedText {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::Borrowed(ValueRef::Text(self.0.as_bytes())))
    }
}

/// Payload textual sanitizado e truncado com segurança para envio a integrações externas.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalPayload {
    /// Texto sanitizado e truncado em fronteira válida de codepoint UTF-8.
    pub text: SanitizedText,
    /// Indica se o payload sofreu truncamento para respeitar `max_bytes`.
    pub truncated: bool,
    /// Tamanho original em bytes do texto bruto recebido.
    pub original_bytes: usize,
}

impl ExternalPayload {
    /// Retorna uma referência ao `SanitizedText` do payload.
    #[must_use]
    pub const fn text(&self) -> &SanitizedText {
        &self.text
    }

    /// Indica se o payload foi truncado.
    #[must_use]
    pub const fn truncated(&self) -> bool {
        self.truncated
    }

    /// Retorna a quantidade original de bytes antes do truncamento/sanitização.
    #[must_use]
    pub const fn original_bytes(&self) -> usize {
        self.original_bytes
    }
}

/// Sanitiza `input` com todas as regras do catálogo.
///
/// Retorna `Cow::Borrowed(input)` sem alocar memória quando nenhum segredo é detectado (CA-03).
#[must_use]
pub fn sanitize(input: &str) -> Cow<'_, str> {
    rules::apply_rules(input).0
}

/// Sanitiza `input` retornando o texto resultante e o `SanitizeReport` detalhado.
#[must_use]
pub fn sanitize_with(input: &str) -> (Cow<'_, str>, SanitizeReport) {
    rules::apply_rules(input)
}

/// Decodifica, normaliza sequências de terminal (`\r\n`, `\r`, `\x08`, preservando CSI/OSC)
/// e sanitiza bytes brutos de saída de PTY, retornando `SanitizedText`.
#[must_use]
pub fn sanitize_terminal(raw: &[u8]) -> SanitizedText {
    let mut decoder = Utf8Decoder::new();
    let mut normalizer = TerminalNormalizer::new();
    let mut clean_lines = normalizer.push(decoder.push(raw));
    let tail_utf8 = decoder.finish();
    if !tail_utf8.is_empty() {
        clean_lines.push_str(&normalizer.push(tail_utf8));
    }
    clean_lines.push_str(&normalizer.finish());
    SanitizedText(sanitize(&clean_lines).into_owned())
}

/// Prepara um payload textual para envio externo sanitizando segredos e truncando em
/// fronteira válida de codepoint UTF-8 até `max_bytes` (CA-08).
#[must_use]
pub fn prepare_external_payload(raw: &str, max_bytes: usize) -> ExternalPayload {
    let original_bytes = raw.len();
    if max_bytes == 0 {
        return ExternalPayload {
            text: SanitizedText(String::new()),
            truncated: !raw.is_empty(),
            original_bytes,
        };
    }

    // Se `raw` for muito maior que `max_bytes`, fatia com folga de `MAX_SECRET_SPAN` antes de sanitizar
    // para que qualquer segredo que comece antes de `max_bytes` seja integralmente detectado.
    let pre_cut_limit = max_bytes.saturating_add(MAX_SECRET_SPAN);
    let pre_slice = if raw.len() > pre_cut_limit {
        &raw[..floor_char_boundary(raw, pre_cut_limit)]
    } else {
        raw
    };

    let sanitized = sanitize(pre_slice);
    let mut truncated = raw.len() > max_bytes || sanitized.len() > max_bytes;

    let final_string = if sanitized.len() <= max_bytes {
        sanitized.into_owned()
    } else {
        truncated = true;
        let mut cut = floor_char_boundary(&sanitized, max_bytes);
        // Evita cortar um marcador `[REDACTED:...]` ao meio no limite de `max_bytes`
        if let Some(open_marker) = sanitized[..cut].rfind("[REDACTED:") {
            if !sanitized[open_marker..cut].contains(']') {
                cut = open_marker;
            }
        }
        sanitize(&sanitized[..cut]).into_owned()
    };

    ExternalPayload {
        text: SanitizedText(final_string),
        truncated,
        original_bytes,
    }
}

/// Inspeciona recursivamente chaves e strings de um `serde_json::Value` candidato à tabela `settings`.
///
/// Retorna `Err(String)` se qualquer chave bater na lista de nomes sensíveis ou se qualquer
/// string sofrer redação (`total_redactions > 0`).
///
/// # Errors
///
/// Retorna `Err` descrevendo a chave ou o tipo de segredo detectado quando o JSON contém
/// dados sensíveis.
pub fn inspect_settings_json(value: &serde_json::Value) -> Result<(), String> {
    inspect_json_node(value, "$")
}

fn inspect_json_node(value: &serde_json::Value, path: &str) -> Result<(), String> {
    match value {
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            Ok(())
        }
        serde_json::Value::String(s) => {
            let (_, report) = sanitize_with(s);
            if report.total_redactions > 0 {
                return Err(format!(
                    "segredo detectado no valor JSON em '{path}' ({:?})",
                    report.by_kind
                ));
            }
            Ok(())
        }
        serde_json::Value::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                let child_path = format!("{path}[{idx}]");
                inspect_json_node(item, &child_path)?;
            }
            Ok(())
        }
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if is_sensitive_settings_key(key) {
                    return Err(format!(
                        "chave sensível '{key}' em '{path}.{key}' não é permitida em settings"
                    ));
                }
                let child_path = format!("{path}.{key}");
                inspect_json_node(val, &child_path)?;
            }
            Ok(())
        }
    }
}

/// Normaliza uma chave JSON (`camelCase`, `kebab-case`, `snake_case`) e verifica se coincide
/// com a lista de chaves sensíveis proibidas em `settings`.
fn is_sensitive_settings_key(key: &str) -> bool {
    let normalized = normalize_key_identifier(key);
    if SENSITIVE_SETTINGS_KEYS.contains(&normalized.as_str()) {
        return true;
    }

    // Verifica também se algum segmento ou sufixo explícito corresponde a um termo sensível
    let tokens: Vec<&str> = normalized.split('_').filter(|s| !s.is_empty()).collect();
    for &sensitive in SENSITIVE_SETTINGS_KEYS {
        if sensitive.contains('_') {
            if normalized.contains(sensitive) {
                return true;
            }
        } else if tokens.contains(&sensitive) {
            return true;
        }
    }

    false
}

/// Converte identificadores `camelCase` ou `kebab-case` para `snake_case` em minúsculas.
fn normalize_key_identifier(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len() + 4);
    let mut prev_is_lower_or_digit = false;

    for ch in raw.trim().chars() {
        if ch == '-' || ch == '.' || ch == ' ' || ch == '_' {
            if !out.ends_with('_') && !out.is_empty() {
                out.push('_');
            }
            prev_is_lower_or_digit = false;
        } else if ch.is_ascii_uppercase() {
            if prev_is_lower_or_digit && !out.ends_with('_') {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            prev_is_lower_or_digit = false;
        } else {
            out.push(ch.to_ascii_lowercase());
            prev_is_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }

    out.trim_matches('_').to_string()
}

/// Encontra o maior índice `<= index` que é fronteira válida de codepoint UTF-8 em `s`.
#[must_use]
pub fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        s.len()
    } else {
        let mut i = index;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        i
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn fast_path_returns_borrowed_without_allocation() {
        let clean = "cargo test --workspace && echo http://localhost:5177";
        let out = sanitize(clean);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert_eq!(out, clean);
    }

    #[test]
    fn sanitized_text_implements_traits_and_tosql() {
        let text = SanitizedText::new("TOKEN=my_secret_value");
        assert_eq!(text.as_str(), "TOKEN=[REDACTED:SECRET]");
        assert_eq!(text.as_ref(), "TOKEN=[REDACTED:SECRET]");
        assert_eq!(&*text, "TOKEN=[REDACTED:SECRET]");
        assert_eq!(text.to_string(), "TOKEN=[REDACTED:SECRET]");
        assert!(!text.is_empty());
        assert_eq!(text.len(), "TOKEN=[REDACTED:SECRET]".len());
        assert!(text.to_sql().is_ok());
    }
}
