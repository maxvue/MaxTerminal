//! Captura incremental limitada e sanitizada de saída por bloco de comando (Fase 05).
//!
//! Mantém orçamento estrito de memória (`HEAD_BUDGET_BYTES = 16 KiB` + `TAIL_BUDGET_BYTES = 48 KiB`
//! = `64 KiB` por bloco) e aplica [`StreamSanitizer`] da Fase 03 antes de reter qualquer texto,
//! garantindo que segredos partidos entre chunks ou na fronteira `head`/`tail` nunca vazem.

use std::collections::VecDeque;

use mt_core::shell::OutputKind;
use mt_db::sanitizer::{SanitizedText, StreamSanitizer};

/// Orçamento máximo preservado para o início (`head`) da saída de um bloco (16 KiB).
pub const HEAD_BUDGET_BYTES: usize = 16 * 1024;

/// Orçamento máximo preservado para o final (`tail`) da saída de um bloco (48 KiB).
pub const TAIL_BUDGET_BYTES: usize = 48 * 1024;

/// Orçamento máximo total de texto armazenado em `command_blocks.output_text` (64 KiB).
pub const TOTAL_OUTPUT_BUDGET_BYTES: usize = HEAD_BUDGET_BYTES + TAIL_BUDGET_BYTES;

/// Orçamento máximo do excerto de erro (`error_excerpt`) para diagnóstico e envio ao `MaxCode` (16 KiB).
pub const ERROR_EXCERPT_BUDGET_BYTES: usize = 16 * 1024;

/// Limite máximo da prévia curta (`output_preview`) enviada no evento `shell://command-finished` (512 bytes).
pub const OUTPUT_PREVIEW_MAX_BYTES: usize = 512;

/// Marcador textual inserido entre `head` e `tail` quando a saída excede 64 KiB.
pub const TRUNCATION_MARKER: &str = "\n...[TRUNCATED]...\n";

/// Resultado final higienizado e limitado produzido por [`OutputCapture::finish`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinalizedOutput {
    /// Texto completo ou `head + marcador + tail` já sanitizado (`None` quando vazio).
    pub output_text: Option<SanitizedText>,
    /// Classificação do formato armazenado (`None`, `Full`, `HeadTailTruncated`, `TailOnly`).
    pub output_kind: OutputKind,
    /// Total de bytes brutos de saída recebidos pelo bloco.
    pub output_bytes: i64,
    /// Total de quebras de linha `\n` observadas na saída do bloco.
    pub output_lines: i64,
    /// Prévia curta sanitizada (até 512 bytes) para exibição rápida na UI.
    pub output_preview: Option<String>,
    /// Excerto sanitizado das últimas linhas da saída (até 16 KiB) quando `include_error_excerpt == true`.
    pub error_excerpt: Option<SanitizedText>,
    /// Indica se houve truncamento por orçamento de 64 KiB ou perda de bytes (`dropped_bytes > 0`).
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum StripState {
    #[default]
    Normal,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

/// Capturador incremental de saída de um bloco de comando em execução.
#[derive(Debug, Clone)]
pub struct OutputCapture {
    sanitizer: StreamSanitizer,
    strip_state: StripState,
    head_buf: String,
    tail_chunks: VecDeque<String>,
    tail_bytes: usize,
    raw_bytes_total: u64,
    raw_lines_total: u64,
    dropped_bytes: u64,
    overflowed_head: bool,
}

impl Default for OutputCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputCapture {
    /// Cria um novo [`OutputCapture`] com orçamentos canônicos (16 KiB head + 48 KiB tail).
    #[must_use]
    pub fn new() -> Self {
        Self {
            sanitizer: StreamSanitizer::new(),
            strip_state: StripState::Normal,
            head_buf: String::with_capacity(4096),
            tail_chunks: VecDeque::new(),
            tail_bytes: 0,
            raw_bytes_total: 0,
            raw_lines_total: 0,
            dropped_bytes: 0,
            overflowed_head: false,
        }
    }

    /// Quantidade total de bytes brutos alimentados na captura até o momento.
    #[must_use]
    pub const fn raw_bytes_total(&self) -> u64 {
        self.raw_bytes_total
    }

    /// Quantidade de linhas acumuladas até o momento.
    #[must_use]
    pub const fn raw_lines_total(&self) -> u64 {
        self.raw_lines_total
    }

    /// Registra uma lacuna (`gap`) de bytes descartados sob pressão extrema de fila,
    /// reiniciando o estado parcial do sanitizador e inserindo marcador explícito.
    pub fn record_gap(&mut self, dropped: u64) {
        if dropped == 0 {
            return;
        }
        self.dropped_bytes = self.dropped_bytes.saturating_add(dropped);
        self.raw_bytes_total = self.raw_bytes_total.saturating_add(dropped);
        // Descarta carry parcial inseguro reiniciando a instância de StreamSanitizer
        self.sanitizer = StreamSanitizer::new();
        self.strip_state = StripState::Normal;
        self.append_sanitized_segment("\n...[TRUNCATED]...\n");
    }

    /// Alimenta um chunk de bytes de saída do comando (já sem marcadores OSC 133).
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.raw_bytes_total = self.raw_bytes_total.saturating_add(bytes.len() as u64);
        let newlines = bytecount_newlines(bytes);
        self.raw_lines_total = self.raw_lines_total.saturating_add(newlines);

        let sanitized_piece = self.sanitizer.push(bytes);
        if !sanitized_piece.is_empty() {
            let clean = self.strip_ansi_sequences(&sanitized_piece);
            self.append_sanitized_segment(&clean);
        }
    }

    /// Finaliza a captura do bloco, drenando o buffer residual do [`StreamSanitizer`]
    /// e montando o [`FinalizedOutput`] dentro dos orçamentos de 64 KiB e 16 KiB.
    #[must_use]
    #[allow(clippy::cast_possible_wrap)]
    pub fn finish(mut self, include_error_excerpt: bool) -> FinalizedOutput {
        let tail_sanitized = self.sanitizer.finish();
        if !tail_sanitized.is_empty() {
            let clean = self.strip_ansi_sequences(&tail_sanitized);
            self.append_sanitized_segment(&clean);
        }

        let mut tail_string = String::with_capacity(self.tail_bytes);
        for chunk in &self.tail_chunks {
            tail_string.push_str(chunk);
        }

        let truncated = self.overflowed_head || self.dropped_bytes > 0;
        let (combined_raw, output_kind) = if self.head_buf.is_empty() && tail_string.is_empty() {
            (String::new(), OutputKind::None)
        } else if !self.overflowed_head && tail_string.is_empty() {
            (self.head_buf.clone(), OutputKind::Full)
        } else {
            let marker_budget = TRUNCATION_MARKER.len();
            let max_tail = TAIL_BUDGET_BYTES.saturating_sub(marker_budget);
            let trimmed_tail = take_utf8_suffix(&tail_string, max_tail);
            let mut full =
                String::with_capacity(self.head_buf.len() + marker_budget + trimmed_tail.len());
            full.push_str(&self.head_buf);
            full.push_str(TRUNCATION_MARKER);
            full.push_str(trimmed_tail);
            (full, OutputKind::HeadTailTruncated)
        };

        let output_text = if combined_raw.is_empty() {
            None
        } else {
            // Re-sanitiza o texto consolidado para garantir invariante `SanitizedText`
            Some(SanitizedText::new(&combined_raw))
        };

        let output_preview = output_text.as_ref().and_then(|txt| {
            let trimmed = txt.as_str().trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(take_utf8_prefix(trimmed, OUTPUT_PREVIEW_MAX_BYTES).to_string())
            }
        });

        let error_excerpt = if include_error_excerpt {
            let source_str = if tail_string.is_empty() {
                combined_raw.as_str()
            } else {
                tail_string.as_str()
            };
            let trimmed = source_str.trim();
            if trimmed.is_empty() {
                None
            } else {
                let suffix = take_utf8_suffix(trimmed, ERROR_EXCERPT_BUDGET_BYTES);
                Some(SanitizedText::new(suffix))
            }
        } else {
            None
        };

        // Se houve saída mas sem `\n` final, contabiliza ao menos 1 linha
        let effective_lines = if self.raw_bytes_total > 0 && self.raw_lines_total == 0 {
            1
        } else {
            self.raw_lines_total
        };

        FinalizedOutput {
            output_text,
            output_kind,
            output_bytes: self.raw_bytes_total.min(i64::MAX as u64) as i64,
            output_lines: effective_lines.min(i64::MAX as u64) as i64,
            output_preview,
            error_excerpt,
            truncated,
        }
    }

    fn append_sanitized_segment(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        if self.overflowed_head {
            self.push_to_tail_ring(text);
        } else {
            let remaining_head = HEAD_BUDGET_BYTES.saturating_sub(self.head_buf.len());
            if text.len() <= remaining_head {
                self.head_buf.push_str(text);
                return;
            }

            let prefix = take_utf8_prefix(text, remaining_head);
            self.head_buf.push_str(prefix);
            self.overflowed_head = true;
            let rest = &text[prefix.len()..];
            if !rest.is_empty() {
                self.push_to_tail_ring(rest);
            }
        }
    }

    fn push_to_tail_ring(&mut self, text: &str) {
        if text.len() >= TAIL_BUDGET_BYTES {
            let suffix = take_utf8_suffix(text, TAIL_BUDGET_BYTES);
            self.tail_chunks.clear();
            self.tail_chunks.push_back(suffix.to_string());
            self.tail_bytes = suffix.len();
            return;
        }

        self.tail_chunks.push_back(text.to_string());
        self.tail_bytes += text.len();

        while self.tail_bytes > TAIL_BUDGET_BYTES {
            if let Some(front) = self.tail_chunks.pop_front() {
                let excess = self.tail_bytes - TAIL_BUDGET_BYTES;
                if front.len() <= excess {
                    self.tail_bytes -= front.len();
                } else {
                    let keep_len = front.len() - excess;
                    let suffix = take_utf8_suffix(&front, keep_len).to_string();
                    self.tail_bytes = self.tail_bytes - front.len() + suffix.len();
                    self.tail_chunks.push_front(suffix);
                    break;
                }
            } else {
                self.tail_bytes = 0;
                break;
            }
        }
    }

    /// Remove sequências de escape ANSI (CSI/OSC) do texto já normalizado, preservando `\n` e `\t`.
    fn strip_ansi_sequences(&mut self, input: &str) -> String {
        let mut out = String::with_capacity(input.len());
        for ch in input.chars() {
            match self.strip_state {
                StripState::Normal => {
                    if ch == '\x1b' {
                        self.strip_state = StripState::Esc;
                    } else if ch == '\n' || ch == '\t' || ch >= ' ' {
                        out.push(ch);
                    }
                }
                StripState::Esc => match ch {
                    '[' => self.strip_state = StripState::Csi,
                    ']' => self.strip_state = StripState::Osc,
                    _ => self.strip_state = StripState::Normal,
                },
                StripState::Csi => {
                    if ('\x40'..='\x7e').contains(&ch) {
                        self.strip_state = StripState::Normal;
                    }
                }
                StripState::Osc => {
                    if ch == '\x07' {
                        self.strip_state = StripState::Normal;
                    } else if ch == '\x1b' {
                        self.strip_state = StripState::OscEsc;
                    }
                }
                StripState::OscEsc => {
                    if ch == '\\' || ch == '\x07' {
                        self.strip_state = StripState::Normal;
                    } else {
                        self.strip_state = StripState::Osc;
                    }
                }
            }
        }
        out
    }
}

fn bytecount_newlines(bytes: &[u8]) -> u64 {
    let mut count = 0u64;
    let mut slice = bytes;
    while let Some(pos) = memchr::memchr(b'\n', slice) {
        count += 1;
        slice = &slice[pos + 1..];
    }
    count
}

fn take_utf8_prefix(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut idx = max_bytes;
    while idx > 0 && !text.is_char_boundary(idx) {
        idx -= 1;
    }
    &text[..idx]
}

fn take_utf8_suffix(text: &str, max_bytes: usize) -> &str {
    if text.len() <= max_bytes {
        return text;
    }
    let mut idx = text.len() - max_bytes;
    while idx < text.len() && !text.is_char_boundary(idx) {
        idx += 1;
    }
    &text[idx..]
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn sanitizes_secrets_and_strips_ansi_codes() {
        let mut cap = OutputCapture::new();
        cap.push_bytes(
            b"\x1b[31mErro fatal:\x1b[0m Authorization: Bearer sk-1234567890abcdef12345678\r\n",
        );
        let fin = cap.finish(true);
        let text = fin.output_text.unwrap();
        assert!(!text.contains("sk-1234567890abcdef12345678"));
        assert!(text.contains("[REDACTED:"));
        assert!(!text.contains("\x1b[31m"));
        assert!(fin.error_excerpt.is_some());
    }

    #[test]
    fn enforces_64kib_head_tail_budget_on_large_output() {
        let mut cap = OutputCapture::new();
        let line = b"linha de output repetida para testar orcamento de 64KiB\n";
        for _ in 0..2500 {
            cap.push_bytes(line);
        }
        let fin = cap.finish(true);
        assert_eq!(fin.output_kind, OutputKind::HeadTailTruncated);
        assert!(fin.truncated);
        let text = fin.output_text.unwrap();
        assert!(text.len() <= TOTAL_OUTPUT_BUDGET_BYTES);
        assert!(text.contains(TRUNCATION_MARKER));
        let excerpt = fin.error_excerpt.unwrap();
        assert!(excerpt.len() <= ERROR_EXCERPT_BUDGET_BYTES);
    }
}
