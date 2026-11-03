//! Pipeline incremental de sanitização para streams de terminal (`Utf8Decoder`,
//! `TerminalNormalizer` e `StreamSanitizer`).

use crate::sanitizer::{
    MAX_SECRET_SPAN,
    rules::{SanitizeReport, apply_rules},
};

/// Decodificador UTF-8 incremental que retém sufixos multibyte incompletos (1 a 3 bytes)
/// entre chamadas sucessivas de `push` (CA-06).
#[derive(Debug, Clone, Default)]
pub struct Utf8Decoder {
    pending: [u8; 4],
    pending_len: usize,
    decoded: String,
}

impl Utf8Decoder {
    /// Cria um novo `Utf8Decoder` vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Consome um chunk de bytes arbitrários, emitindo o prefixo UTF-8 válido decodificado
    /// e retendo até 3 bytes finais caso pertençam a um codepoint incompleto.
    pub fn push(&mut self, chunk: &[u8]) -> &str {
        self.decoded.clear();
        if chunk.is_empty() {
            return &self.decoded;
        }

        let mut slice = chunk;

        // Se havia bytes pendentes do chunk anterior, tenta completar o primeiro codepoint
        if self.pending_len > 0 {
            let mut combined = [0u8; 7];
            combined[..self.pending_len].copy_from_slice(&self.pending[..self.pending_len]);
            let take = slice.len().min(4 - self.pending_len);
            combined[self.pending_len..self.pending_len + take].copy_from_slice(&slice[..take]);
            let total = self.pending_len + take;

            match std::str::from_utf8(&combined[..total]) {
                Ok(valid) => {
                    self.decoded.push_str(valid);
                    self.pending_len = 0;
                    slice = &slice[take..];
                }
                Err(err) => {
                    let valid_up_to = err.valid_up_to();
                    if valid_up_to > 0 {
                        if let Ok(valid_prefix) = std::str::from_utf8(&combined[..valid_up_to]) {
                            self.decoded.push_str(valid_prefix);
                        }
                        let consumed_from_slice = valid_up_to.saturating_sub(self.pending_len);
                        self.pending_len = 0;
                        slice = &slice[consumed_from_slice..];
                    } else if let Some(error_len) = err.error_len() {
                        // Sequência genuinamente inválida
                        self.decoded.push('\u{FFFD}');
                        if error_len <= self.pending_len {
                            self.pending.copy_within(error_len..self.pending_len, 0);
                            self.pending_len -= error_len;
                        } else {
                            let skip_in_slice = error_len - self.pending_len;
                            self.pending_len = 0;
                            slice = &slice[skip_in_slice.min(slice.len())..];
                        }
                    } else {
                        // Ainda incompleto (chunk muito curto, ex.: 1 byte de um emoji de 4 bytes)
                        self.pending[..total].copy_from_slice(&combined[..total]);
                        self.pending_len = total;
                        return &self.decoded;
                    }
                }
            }
        }

        // Decodifica o restante de `slice`
        loop {
            if slice.is_empty() {
                break;
            }
            match std::str::from_utf8(slice) {
                Ok(valid) => {
                    self.decoded.push_str(valid);
                    break;
                }
                Err(err) => {
                    let valid_up_to = err.valid_up_to();
                    if valid_up_to > 0 {
                        if let Ok(valid_prefix) = std::str::from_utf8(&slice[..valid_up_to]) {
                            self.decoded.push_str(valid_prefix);
                        }
                    }
                    let rest = &slice[valid_up_to..];
                    if let Some(error_len) = err.error_len() {
                        self.decoded.push('\u{FFFD}');
                        slice = &rest[error_len..];
                    } else {
                        // Sufixo incompleto de 1..=3 bytes no final do chunk
                        self.pending[..rest.len()].copy_from_slice(rest);
                        self.pending_len = rest.len();
                        break;
                    }
                }
            }
        }

        &self.decoded
    }

    /// Finaliza o stream UTF-8, emitindo `U+FFFD` caso ainda restem bytes incompletos no EOF.
    pub fn finish(&mut self) -> &str {
        self.decoded.clear();
        if self.pending_len > 0 {
            self.decoded.push('\u{FFFD}');
            self.pending_len = 0;
        }
        &self.decoded
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AnsiScanState {
    #[default]
    Normal,
    Esc,
    Csi,
    Osc,
    OscEsc,
}

/// Normalizador incremental de saída de terminal que converte `\r\n -> \n`, aplica `\r`
/// isolado sobrescrevendo desde o início da linha corrente, resolve `\x08` (BS) apagando
/// o último escalar Unicode da linha corrente e preserva sequências ANSI CSI/OSC intactas (CA-07).
#[derive(Debug, Clone, Default)]
pub struct TerminalNormalizer {
    current_line: String,
    cursor_byte: usize,
    pending_cr: bool,
    ansi_state: AnsiScanState,
}

impl TerminalNormalizer {
    /// Cria um novo `TerminalNormalizer` vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Alimenta um fragmento de texto UTF-8 e retorna todas as linhas já consolidadas (`\n`).
    #[allow(clippy::too_many_lines)]
    pub fn push(&mut self, text: &str) -> String {
        let mut emitted = String::new();

        for ch in text.chars() {
            // Preserva sequências ANSI CSI e OSC intactas sem tratar caracteres internos como controle de linha
            match self.ansi_state {
                AnsiScanState::Esc => {
                    self.current_line.push(ch);
                    self.cursor_byte = self.current_line.len();
                    match ch {
                        '[' => self.ansi_state = AnsiScanState::Csi,
                        ']' => self.ansi_state = AnsiScanState::Osc,
                        _ => self.ansi_state = AnsiScanState::Normal,
                    }
                    continue;
                }
                AnsiScanState::Csi => {
                    self.current_line.push(ch);
                    self.cursor_byte = self.current_line.len();
                    if ('\x40'..='\x7e').contains(&ch) {
                        self.ansi_state = AnsiScanState::Normal;
                    }
                    continue;
                }
                AnsiScanState::Osc => {
                    self.current_line.push(ch);
                    self.cursor_byte = self.current_line.len();
                    if ch == '\x07' {
                        self.ansi_state = AnsiScanState::Normal;
                    } else if ch == '\x1b' {
                        self.ansi_state = AnsiScanState::OscEsc;
                    }
                    continue;
                }
                AnsiScanState::OscEsc => {
                    self.current_line.push(ch);
                    self.cursor_byte = self.current_line.len();
                    if ch == '\\' || ch == '\x07' {
                        self.ansi_state = AnsiScanState::Normal;
                    } else {
                        self.ansi_state = AnsiScanState::Osc;
                    }
                    continue;
                }
                AnsiScanState::Normal => {}
            }

            if ch == '\n' {
                self.pending_cr = false;
                emitted.push_str(&self.current_line);
                emitted.push('\n');
                self.current_line.clear();
                self.cursor_byte = 0;
                continue;
            }

            if ch == '\r' {
                self.pending_cr = true;
                continue;
            }

            if self.pending_cr {
                self.pending_cr = false;
                self.cursor_byte = 0;
            }

            if ch == '\x08' {
                if self.cursor_byte > 0 {
                    // Se o cursor está no final da linha, remove o último escalar Unicode
                    if self.cursor_byte >= self.current_line.len() {
                        self.current_line.pop();
                        self.cursor_byte = self.current_line.len();
                    } else if let Some((prev_idx, _)) = self.current_line[..self.cursor_byte]
                        .char_indices()
                        .next_back()
                    {
                        self.current_line.remove(prev_idx);
                        self.cursor_byte = prev_idx;
                    }
                }
                continue;
            }

            if ch == '\x1b' {
                if self.cursor_byte < self.current_line.len() {
                    self.current_line.truncate(self.cursor_byte);
                }
                self.current_line.push(ch);
                self.cursor_byte = self.current_line.len();
                self.ansi_state = AnsiScanState::Esc;
                continue;
            }

            // Escrita normal ou sobrescrita a partir do início da linha corrente após `\r`
            if self.cursor_byte >= self.current_line.len() {
                self.current_line.push(ch);
                self.cursor_byte = self.current_line.len();
            } else if let Some(existing_ch) = self.current_line[self.cursor_byte..].chars().next() {
                let end_byte = self.cursor_byte + existing_ch.len_utf8();
                let mut buf = [0u8; 4];
                let repl = ch.encode_utf8(&mut buf);
                self.current_line
                    .replace_range(self.cursor_byte..end_byte, repl);
                self.cursor_byte += repl.len();
            }
        }

        // Se a linha sem `\n` crescer muito além de 2 * MAX_SECRET_SPAN sem `\r` pendente,
        // emite o prefixo antigo preservando os últimos MAX_SECRET_SPAN bytes na linha corrente.
        let max_unflushed = MAX_SECRET_SPAN * 2;
        if !self.pending_cr
            && self.ansi_state == AnsiScanState::Normal
            && self.current_line.len() > max_unflushed
            && self.cursor_byte == self.current_line.len()
        {
            let keep_from = crate::sanitizer::floor_char_boundary(
                &self.current_line,
                self.current_line.len() - MAX_SECRET_SPAN,
            );
            if keep_from > 0 {
                emitted.push_str(&self.current_line[..keep_from]);
                self.current_line.drain(..keep_from);
                self.cursor_byte = self.current_line.len();
            }
        }

        emitted
    }

    /// Finaliza o normalizador no EOF, retornando o conteúdo pendente da linha corrente.
    pub fn finish(&mut self) -> String {
        self.pending_cr = false;
        self.ansi_state = AnsiScanState::Normal;
        self.cursor_byte = 0;
        std::mem::take(&mut self.current_line)
    }
}

/// Sanitizador incremental de streams com janela deslizante de `tail_overlap = MAX_SECRET_SPAN`
/// (4096 bytes) em fronteira válida de codepoint UTF-8, garantindo que qualquer segredo
/// `<= MAX_SECRET_SPAN` partido entre múltiplos chunks seja 100% redigido antes de ser emitido (CA-05).
#[derive(Debug, Clone)]
pub struct StreamSanitizer {
    decoder: Utf8Decoder,
    normalizer: TerminalNormalizer,
    buffer: String,
    tail_overlap: usize,
    report: SanitizeReport,
}

impl Default for StreamSanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamSanitizer {
    /// Cria um novo `StreamSanitizer` com `tail_overlap = MAX_SECRET_SPAN` (4096 bytes).
    #[must_use]
    pub fn new() -> Self {
        Self::with_tail_overlap(MAX_SECRET_SPAN)
    }

    /// Cria um `StreamSanitizer` com janela de retenção (`tail_overlap`) customizada.
    #[must_use]
    pub fn with_tail_overlap(tail_overlap: usize) -> Self {
        Self {
            decoder: Utf8Decoder::new(),
            normalizer: TerminalNormalizer::new(),
            buffer: String::new(),
            tail_overlap: tail_overlap.max(64),
            report: SanitizeReport::default(),
        }
    }

    /// Alimenta um chunk de bytes brutos no pipeline (`Utf8Decoder -> TerminalNormalizer -> janela deslizante`),
    /// retornando a porção já sanitizada e segura para emissão imediata.
    pub fn push(&mut self, chunk: &[u8]) -> String {
        let decoded = self.decoder.push(chunk);
        let normalized = self.normalizer.push(decoded);
        if !normalized.is_empty() {
            self.buffer.push_str(&normalized);
        }
        self.emit_safe_prefix()
    }

    /// Alimenta uma fatia `&str` diretamente no `StreamSanitizer`.
    pub fn push_str(&mut self, text: &str) -> String {
        self.push(text.as_bytes())
    }

    /// Finaliza o stream no EOF, processando bytes remanescentes de `Utf8Decoder` e `TerminalNormalizer`
    /// e sanitizando toda a janela residual retida.
    pub fn finish(&mut self) -> String {
        let tail_utf8 = self.decoder.finish();
        if !tail_utf8.is_empty() {
            let norm = self.normalizer.push(tail_utf8);
            self.buffer.push_str(&norm);
        }
        let tail_norm = self.normalizer.finish();
        if !tail_norm.is_empty() {
            self.buffer.push_str(&tail_norm);
        }

        if self.buffer.is_empty() {
            return String::new();
        }

        let raw = std::mem::take(&mut self.buffer);
        let (sanitized, chunk_report) = apply_rules(&raw);
        self.report.merge(&chunk_report);
        sanitized.into_owned()
    }

    /// Retorna o relatório acumulado de redações realizadas pelo `StreamSanitizer`.
    #[must_use]
    pub const fn report(&self) -> &SanitizeReport {
        &self.report
    }

    /// Calcula e emite apenas o prefixo anterior à janela `tail_overlap` em uma fronteira
    /// de delimitador seguro, evitando cortar tokens em crescimento no final do chunk.
    fn emit_safe_prefix(&mut self) -> String {
        if self.buffer.len() <= self.tail_overlap {
            return String::new();
        }

        // Se houver um bloco PEM aberto sem `-----END`, aguarda até `tail_overlap * 4`
        if let Some(begin_pos) = self.buffer.find("-----BEGIN ") {
            let after_begin = &self.buffer[begin_pos..];
            if !after_begin.contains("-----END ") && after_begin.len() <= self.tail_overlap * 4 {
                if begin_pos == 0 {
                    return String::new();
                }
                let prefix = self.buffer[..begin_pos].to_string();
                self.buffer.drain(..begin_pos);
                let (sanitized, chunk_report) = apply_rules(&prefix);
                self.report.merge(&chunk_report);
                return sanitized.into_owned();
            }
        }

        let max_cut = crate::sanitizer::floor_char_boundary(
            &self.buffer,
            self.buffer.len() - self.tail_overlap,
        );
        if max_cut == 0 {
            return String::new();
        }

        // Prioriza cortar em quebra de linha `\n` dentro de `..=max_cut`
        let safe_cut = if let Some(nl_pos) = self.buffer[..max_cut].rfind('\n') {
            nl_pos + 1
        } else {
            // Se não houver `\n`, procura um espaço seguro que não suceda imediatamente um operador/header
            find_safe_whitespace_cut(&self.buffer[..max_cut]).unwrap_or(0)
        };

        if safe_cut == 0 {
            return String::new();
        }

        let to_sanitize = self.buffer[..safe_cut].to_string();
        self.buffer.drain(..safe_cut);
        let (sanitized, chunk_report) = apply_rules(&to_sanitize);
        self.report.merge(&chunk_report);
        sanitized.into_owned()
    }
}

/// Localiza o último espaço em branco em `prefix` que não seja o separador imediato de um
/// cabeçalho (`Bearer `, `Basic `, `Authorization: `) ou flag CLI (`--password `, `-p `).
fn find_safe_whitespace_cut(prefix: &str) -> Option<usize> {
    for (idx, ch) in prefix.char_indices().rev() {
        if ch == ' ' || ch == '\t' {
            let before = prefix[..idx].trim_end();
            let lower = before.to_ascii_lowercase();
            let unsafe_predecessor = lower.ends_with(':')
                || lower.ends_with('=')
                || lower.ends_with("bearer")
                || lower.ends_with("basic")
                || lower.ends_with("token")
                || lower.ends_with("--password")
                || lower.ends_with("--passwd")
                || lower.ends_with("--secret")
                || lower.ends_with("--api-key")
                || lower.ends_with("--access-token")
                || lower.ends_with("-p");
            if !unsafe_predecessor {
                return Some(idx + ch.len_utf8());
            }
        }
    }
    None
}
