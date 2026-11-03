//! Construção blindada de consultas FTS5 (`ParsedQuery`) e extração segura de `HighlightSegment`
//! com sentinelas livres de colisão.

use std::fmt::Write as _;

use crate::fts::build_match_query;
use crate::search::error::SearchError;
use crate::search::model::HighlightSegment;

/// Limite máximo de caracteres Unicode em uma consulta de busca global.
pub const MAX_QUERY_CHARS: usize = 256;

/// Limite máximo de bytes UTF-8 em uma consulta de busca global.
pub const MAX_QUERY_BYTES: usize = 1024;

/// Limite máximo de caracteres em um título destacado.
pub const MAX_TITLE_CHARS: usize = 240;

/// Limite máximo de caracteres em um excerto (snippet) destacado.
pub const MAX_EXCERPT_CHARS: usize = 320;

/// Consulta validada e pronta para execução parametrizada nos índices FTS5.
#[derive(Debug, Clone)]
pub struct ParsedQuery {
    /// Texto original após `trim()`.
    pub raw_trimmed: String,
    /// Expressão `MATCH` segura para `FtsTokenizer::Code` (`command_blocks_fts`).
    pub fts_match_code: String,
    /// Expressão `MATCH` segura para `FtsTokenizer::Text` (`session_scrollback_fts`, `search_metadata_fts`).
    pub fts_match_text: String,
    /// Expressão `MATCH` canônica compatível com ambos os tokenizadores.
    pub fts_match_expr: String,
    /// Tokens normalizados em minúsculas para destaque e fallback seguro.
    pub tokens_lower: Vec<String>,
    /// Sentinela de abertura passada para `snippet()`.
    pub open_sentinel: String,
    /// Sentinela de fechamento passada para `snippet()`.
    pub close_sentinel: String,
}

impl ParsedQuery {
    /// Valida a consulta do usuário e constrói expressões `MATCH` blindadas contra `SQLITE_ERROR`.
    ///
    /// Retorna:
    /// - `Err(SearchError::InvalidQuery)` quando contém byte NUL (`\0`) ou excede `256` chars / `1024` bytes.
    /// - `Ok(None)` quando a consulta é vazia ou contém apenas pontuação/separadores sem termos pesquisáveis.
    /// - `Ok(Some(ParsedQuery))` quando há ao menos um termo válido.
    pub fn parse(input: &str, seed_nonce: u64) -> Result<Option<Self>, SearchError> {
        if input.as_bytes().contains(&0) {
            return Err(SearchError::InvalidQuery {
                reason: "caractere nulo não permitido",
            });
        }
        if input.len() > MAX_QUERY_BYTES || input.chars().count() > MAX_QUERY_CHARS {
            return Err(SearchError::InvalidQuery {
                reason: "consulta excede o limite de 256 caracteres ou 1024 bytes",
            });
        }

        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        let Some(fts_match_code) = build_match_query(trimmed) else {
            return Ok(None);
        };

        let tokens_lower = extract_search_tokens(trimmed);
        if tokens_lower.is_empty() {
            return Ok(None);
        }

        let fts_match_text = build_text_match_expr(&tokens_lower);
        if fts_match_text.is_empty() {
            return Ok(None);
        }

        let (open_sentinel, close_sentinel) = generate_sentinel_pair(trimmed, seed_nonce);

        Ok(Some(Self {
            raw_trimmed: trimmed.to_owned(),
            fts_match_expr: fts_match_code.clone(),
            fts_match_code,
            fts_match_text,
            tokens_lower,
            open_sentinel,
            close_sentinel,
        }))
    }

    /// Converte um texto sanitizado de título em `Vec<HighlightSegment>` destacando os tokens da busca.
    #[must_use]
    pub fn highlight_title(&self, sanitized_title: &str) -> Vec<HighlightSegment> {
        let safe = truncate_preserving_redacted(sanitized_title, MAX_TITLE_CHARS);
        highlight_tokens_safe(&safe, &self.tokens_lower)
    }

    /// Converte a saída da função SQL `snippet()` (com `open_sentinel` / `close_sentinel`) em
    /// `Vec<HighlightSegment>`, fazendo fallback seguro caso `raw_source` já contivesse as sentinelas
    /// ou caso um marcador `[REDACTED:*]` tenha sido afetado.
    #[must_use]
    pub fn parse_snippet_segments(
        &self,
        raw_source: &str,
        snippet_out: &str,
    ) -> Vec<HighlightSegment> {
        if raw_source.contains(&self.open_sentinel)
            || raw_source.contains(&self.close_sentinel)
            || snippet_out.is_empty()
        {
            let safe = truncate_preserving_redacted(raw_source, MAX_EXCERPT_CHARS);
            return highlight_tokens_safe(&safe, &self.tokens_lower);
        }

        let mut segments: Vec<HighlightSegment> = Vec::new();
        let mut cursor = snippet_out;

        while !cursor.is_empty() {
            if let Some(open_pos) = cursor.find(&self.open_sentinel) {
                if open_pos > 0 {
                    push_segment(&mut segments, &cursor[..open_pos], false);
                }
                let after_open = &cursor[open_pos + self.open_sentinel.len()..];
                if let Some(close_pos) = after_open.find(&self.close_sentinel) {
                    let matched_text = &after_open[..close_pos];
                    let is_redacted =
                        matched_text.contains("[REDACTED:") || matched_text.contains("REDACTED");
                    push_segment(&mut segments, matched_text, !is_redacted);
                    cursor = &after_open[close_pos + self.close_sentinel.len()..];
                } else {
                    // Sentinela sem fechamento (ex.: truncada pelo SQLite): fallback seguro
                    let safe = truncate_preserving_redacted(raw_source, MAX_EXCERPT_CHARS);
                    return highlight_tokens_safe(&safe, &self.tokens_lower);
                }
            } else {
                push_segment(&mut segments, cursor, false);
                break;
            }
        }

        // Verifica se a junção dos segmentos cortou algum marcador [REDACTED:...] ao meio
        let joined: String = segments.iter().map(|s| s.text.as_str()).collect();
        if has_broken_redaction_marker(&joined) {
            let safe = truncate_preserving_redacted(raw_source, MAX_EXCERPT_CHARS);
            return highlight_tokens_safe(&safe, &self.tokens_lower);
        }

        if segments.is_empty() {
            vec![HighlightSegment {
                text: String::new(),
                matched: false,
            }]
        } else {
            segments
        }
    }
}

/// Verifica se um caractere é classificado como palavra/dígito pelo tokenizador `unicode61` do SQLite.
#[inline]
fn is_unicode61_word_char(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_alphanumeric()
    } else {
        // unicode61 trata letras e números Unicode padrão como tokens, mas ignora caracteres de controle/formato
        (ch.is_alphabetic() || ch.is_numeric()) && !ch.is_whitespace() && !ch.is_control()
    }
}

fn extract_search_tokens(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in input.chars().take(MAX_QUERY_CHARS) {
        if is_unicode61_word_char(ch) {
            for lower_ch in ch.to_lowercase() {
                current.push(lower_ch);
            }
        } else if !current.is_empty() {
            let tok = std::mem::take(&mut current);
            if !tok.is_empty() {
                tokens.push(tok);
                if tokens.len() >= 16 {
                    break;
                }
            }
        }
    }

    if !current.is_empty() && tokens.len() < 16 {
        tokens.push(current);
    }

    tokens
}

fn build_text_match_expr(tokens: &[String]) -> String {
    if tokens.is_empty() {
        return String::new();
    }
    let last_idx = tokens.len() - 1;
    let mut out = String::new();
    for (idx, token) in tokens.iter().enumerate() {
        if idx > 0 {
            out.push_str(" AND ");
        }
        let escaped = token.replace('"', "\"\"");
        if idx == last_idx {
            let _ = write!(&mut out, "\"{escaped}\"*");
        } else {
            let _ = write!(&mut out, "\"{escaped}\"");
        }
    }
    out
}

fn generate_sentinel_pair(input: &str, mut nonce: u64) -> (String, String) {
    loop {
        let open = format!("\u{E010}MT{nonce:08x}\u{E011}");
        let close = format!("\u{E012}MT{nonce:08x}\u{E013}");
        if !input.contains(&open) && !input.contains(&close) {
            return (open, close);
        }
        nonce = nonce.wrapping_add(0x9E37_79B9_7F4A_7C15);
    }
}

fn truncate_preserving_redacted(text: &str, max_chars: usize) -> String {
    let limit = max_chars.max(32);
    let truncated: String = text.chars().take(limit).collect();
    if let Some(open_idx) = truncated.rfind("[REDACTED:") {
        if !truncated[open_idx..].contains(']') {
            return truncated[..open_idx].to_string();
        }
    }
    truncated
}

fn has_broken_redaction_marker(text: &str) -> bool {
    if let Some(open_idx) = text.rfind("[REDACTED:") {
        !text[open_idx..].contains(']')
    } else {
        false
    }
}

/// Realiza destaque seguro por casamento case-insensitive de tokens sem jamais destacar dentro de `[REDACTED:*]`.
#[must_use]
pub fn highlight_tokens_safe(text: &str, tokens_lower: &[String]) -> Vec<HighlightSegment> {
    if text.is_empty() {
        return vec![HighlightSegment {
            text: String::new(),
            matched: false,
        }];
    }

    let valid_tokens: Vec<&str> = tokens_lower
        .iter()
        .map(String::as_str)
        .filter(|t| !t.is_empty() && !t.eq_ignore_ascii_case("redacted"))
        .collect();

    if valid_tokens.is_empty() {
        return vec![HighlightSegment {
            text: text.to_owned(),
            matched: false,
        }];
    }

    // Identifica intervalos protegidos `[REDACTED:...]`
    let mut protected_ranges: Vec<(usize, usize)> = Vec::new();
    let mut search_from = 0usize;
    while let Some(rel_start) = text[search_from..].find("[REDACTED:") {
        let abs_start = search_from + rel_start;
        if let Some(rel_end) = text[abs_start..].find(']') {
            let abs_end = abs_start + rel_end + 1;
            protected_ranges.push((abs_start, abs_end));
            search_from = abs_end;
        } else {
            protected_ranges.push((abs_start, text.len()));
            break;
        }
    }

    let mut segments: Vec<HighlightSegment> = Vec::new();
    let mut byte_idx = 0usize;
    let mut plain_start = 0usize;

    while byte_idx < text.len() {
        if let Some(&(_, prot_end)) = protected_ranges
            .iter()
            .find(|&&(s, e)| byte_idx >= s && byte_idx < e)
        {
            byte_idx = prot_end;
            continue;
        }

        let rest = &text[byte_idx..];
        let mut best_match_len = 0usize;

        for &tok in &valid_tokens {
            if let Some(candidate) = prefix_case_insensitive_len(rest, tok) {
                let end_pos = byte_idx + candidate;
                let overlaps_protected = protected_ranges
                    .iter()
                    .any(|&(s, e)| byte_idx < e && end_pos > s);
                if !overlaps_protected && candidate > best_match_len {
                    best_match_len = candidate;
                }
            }
        }

        if best_match_len > 0 {
            if plain_start < byte_idx {
                push_segment(&mut segments, &text[plain_start..byte_idx], false);
            }
            push_segment(
                &mut segments,
                &text[byte_idx..byte_idx + best_match_len],
                true,
            );
            byte_idx += best_match_len;
            plain_start = byte_idx;
        } else if let Some(ch) = rest.chars().next() {
            byte_idx += ch.len_utf8();
        } else {
            break;
        }
    }

    if plain_start < text.len() {
        push_segment(&mut segments, &text[plain_start..], false);
    }

    if segments.is_empty() {
        vec![HighlightSegment {
            text: text.to_owned(),
            matched: false,
        }]
    } else {
        segments
    }
}

fn prefix_case_insensitive_len(haystack: &str, needle_lower: &str) -> Option<usize> {
    let mut hay_chars = haystack.chars();
    let mut consumed_bytes = 0usize;
    let mut lowered_buf = String::new();

    while lowered_buf.len() < needle_lower.len() {
        let ch = hay_chars.next()?;
        consumed_bytes += ch.len_utf8();
        for lc in ch.to_lowercase() {
            lowered_buf.push(lc);
        }
    }

    if lowered_buf == needle_lower {
        Some(consumed_bytes)
    } else {
        None
    }
}

fn push_segment(segments: &mut Vec<HighlightSegment>, text: &str, matched: bool) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = segments.last_mut()
        && last.matched == matched
    {
        last.text.push_str(text);
        return;
    }
    segments.push(HighlightSegment {
        text: text.to_owned(),
        matched,
    });
}
