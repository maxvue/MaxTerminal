//! Scanner incremental em `O(n)` para sequências `OSC 133`, `OSC 633` e `OSC 7`.
//!
//! Opera sobre fatias arbitrárias de bytes (`&[u8]`) sem exigir alinhamento de linha ou UTF-8
//! nos limites de cada chunk, preservando coordenadas exatas de [`StreamPosition`] e separando
//! os segmentos de texto intercalados com os marcadores semânticos.

use memchr::memchr;
use mt_core::shell::{CommandTextQuality, ObservationSource, StreamPosition};

use crate::osc::escape::{
    decode_base64_utf8, decode_file_uri_cwd, normalize_and_validate_cwd, unescape_osc_value,
};

/// Limite padrão de bytes acumulados em um único payload OSC antes de entrar em descarte seguro.
pub const DEFAULT_MAX_OSC_PAYLOAD: usize = 16 * 1024;

/// Tipo semântico decodificado a partir de uma sequência `OSC 133`, `OSC 633` ou `OSC 7`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscKind {
    /// `OSC 133 ; A` ou `OSC 633 ; A` — início da renderização do prompt.
    PromptStart {
        /// Nonce de sessão informado pelo hook, se presente.
        nonce: Option<String>,
        /// Geração opcional do hook (`g=<n>`).
        generation: Option<u64>,
        /// Identificador opcional de aplicação (`aid`).
        aid: Option<String>,
        /// Modo de linha de comando (`cl`).
        command_line_mode: Option<String>,
        /// Fonte do marcador (`Osc133` ou `Osc633`).
        source: ObservationSource,
    },
    /// `OSC 133 ; B` ou `OSC 633 ; B` — fim do prompt / início da entrada do usuário.
    PromptEnd {
        /// Nonce de sessão informado pelo hook, se presente.
        nonce: Option<String>,
        /// Geração opcional do hook (`g=<n>`).
        generation: Option<u64>,
        /// Fonte do marcador (`Osc133` ou `Osc633`).
        source: ObservationSource,
    },
    /// `OSC 133 ; C` ou `OSC 633 ; C` — início da execução do comando e da sua saída.
    CommandExecuted {
        /// Nonce de sessão informado pelo hook, se presente.
        nonce: Option<String>,
        /// Geração opcional do hook (`g=<n>`).
        generation: Option<u64>,
        /// Texto decodificado do comando (`cmd_b64` ou inline).
        command: Option<String>,
        /// Diretório de trabalho decodificado (`cwd_b64` ou inline).
        cwd: Option<String>,
        /// Fonte do marcador (`Osc133` ou `Osc633`).
        source: ObservationSource,
    },
    /// `OSC 133 ; D [; exit_code]` ou `OSC 633 ; D [; exit_code]` — término da execução do comando.
    CommandFinished {
        /// Código numérico de saída reportado pelo shell, se presente e válido.
        exit_code: Option<i32>,
        /// Nonce de sessão informado pelo hook, se presente.
        nonce: Option<String>,
        /// Geração opcional do hook (`g=<n>`).
        generation: Option<u64>,
        /// Diretório de trabalho decodificado após o comando, se reportado.
        cwd: Option<String>,
        /// Fonte do marcador (`Osc133` ou `Osc633`).
        source: ObservationSource,
    },
    /// `OSC 633 ; E ; <cmd> [; <nonce>]` ou `OSC 133 ; E` — linha de comando explícita pré-execução.
    CommandLine {
        /// Linha de comando decodificada (ou `None` se omitida/inválida).
        command: Option<String>,
        /// Qualidade declarada do texto do comando.
        quality: CommandTextQuality,
        /// Nonce de sessão informado pelo hook, se presente.
        nonce: Option<String>,
        /// Geração opcional do hook (`g=<n>`).
        generation: Option<u64>,
        /// Fonte do marcador.
        source: ObservationSource,
    },
    /// `OSC 133 ; P ; <k=v>` ou `OSC 633 ; P ; <k=v>` — propriedade de contexto (ex.: `Cwd`, `shell`, `MT=1`).
    Property {
        /// Novo diretório de trabalho validado, se a propriedade contiver `Cwd` ou `cwd_b64`.
        cwd: Option<String>,
        /// Nome do shell reportado (`shell=...`), se presente.
        shell: Option<String>,
        /// Flag `MT=1` de handshake explícito, se presente.
        handshake_mt: bool,
        /// Nonce opcional enviado na propriedade.
        nonce: Option<String>,
        /// Geração opcional enviada na propriedade.
        generation: Option<u64>,
        /// Fonte do marcador.
        source: ObservationSource,
    },
    /// `OSC 7 ; file://host/path` — notificação de diretório de trabalho padrão do terminal.
    Osc7Cwd {
        /// Caminho absoluto local decodificado e validado.
        cwd: String,
    },
}

/// Evento OSC semântico bruto acompanhado de suas coordenadas exatas no stream do PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawOscEvent {
    /// Variante semântica decodificada.
    pub kind: OscKind,
    /// Posição no stream do primeiro byte (`ESC`) que iniciou a sequência OSC.
    pub start_pos: StreamPosition,
    /// Posição no stream imediatamente após o último byte terminador (`BEL` ou `ST`) da sequência.
    pub end_pos: StreamPosition,
}

/// Fatia de bytes de saída do terminal não pertencente a sequências OSC semânticas reconhecidas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSpan {
    /// Sequência do frame `Data` do PTY onde a fatia ocorreu.
    pub pty_seq: u64,
    /// Offset inicial acumulado no stream bruto do PTY.
    pub start_byte_offset: u64,
    /// Offset final acumulado no stream bruto do PTY (`start_byte_offset + bytes.len()`).
    pub end_byte_offset: u64,
    /// Bytes de saída preservados (incluindo sequências CSI/SGR e texto UTF-8 normal).
    pub bytes: Vec<u8>,
}

/// Item ordenado produzido durante a varredura de um chunk, preservando a intercalação exata
/// entre bytes de saída (`Text`) e marcadores semânticos (`Event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanSegment {
    /// Segmento de bytes de saída de terminal.
    Text(TextSpan),
    /// Marcador semântico OSC reconhecido.
    Event(RawOscEvent),
}

/// Resultado consolidado do processamento de um chunk pelo [`OscScanner`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScanOutput {
    /// Eventos OSC semânticos extraídos na ordem em que terminaram.
    pub events: Vec<RawOscEvent>,
    /// Fatias de texto intercaladas fora dos marcadores OSC reconhecidos.
    pub text_spans: Vec<TextSpan>,
    /// Sequência ordenada exata de segmentos (`Text` e `Event`) dentro do chunk.
    pub segments: Vec<ScanSegment>,
}

impl ScanOutput {
    /// Concatena todos os [`TextSpan`]s em um único vetor de bytes sem as sequências OSC reconhecidas.
    #[must_use]
    pub fn concatenated_text(&self) -> Vec<u8> {
        let total: usize = self.text_spans.iter().map(|s| s.bytes.len()).sum();
        let mut out = Vec::with_capacity(total);
        for span in &self.text_spans {
            out.extend_from_slice(&span.bytes);
        }
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    AfterEsc,
    OscPayload,
    OscAfterEsc,
    OverflowDiscard,
    OverflowAfterEsc,
    IgnoredControlString,
    IgnoredControlStringAfterEsc,
}

/// Scanner incremental stateful para extração de `OSC 133`, `OSC 633` e `OSC 7`.
#[derive(Debug, Clone)]
pub struct OscScanner {
    state: State,
    max_payload_bytes: usize,
    payload_buf: Vec<u8>,
    raw_seq_buf: Vec<u8>,
    osc_start_pos: StreamPosition,
    osc_utf8_cont_remaining: u8,
    total_bytes_seen: u64,
    overflow_discards: u64,
}

impl Default for OscScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl OscScanner {
    /// Cria um novo [`OscScanner`] com limite padrão de 16 KiB por payload OSC.
    #[must_use]
    pub fn new() -> Self {
        Self::with_max_payload_bytes(DEFAULT_MAX_OSC_PAYLOAD)
    }

    /// Cria um [`OscScanner`] com limite customizado de bytes por payload OSC.
    #[must_use]
    pub fn with_max_payload_bytes(max_payload_bytes: usize) -> Self {
        let cap = max_payload_bytes.clamp(64, 64 * 1024);
        Self {
            state: State::Ground,
            max_payload_bytes: max_payload_bytes.max(16),
            payload_buf: Vec::with_capacity(cap.min(4096)),
            raw_seq_buf: Vec::with_capacity(cap.min(4096)),
            osc_start_pos: StreamPosition::default(),
            osc_utf8_cont_remaining: 0,
            total_bytes_seen: 0,
            overflow_discards: 0,
        }
    }

    /// Retorna o total acumulado de bytes processados pelo scanner desde sua criação.
    #[must_use]
    pub const fn total_bytes_seen(&self) -> u64 {
        self.total_bytes_seen
    }

    /// Retorna a quantidade de sequências OSC descartadas por excederem o orçamento de tamanho.
    #[must_use]
    pub const fn overflow_discards(&self) -> u64 {
        self.overflow_discards
    }

    /// Limpa qualquer estado parcial pendente quando o stream chega a EOF (`finish`).
    pub fn finish(&mut self) {
        self.state = State::Ground;
        self.payload_buf.clear();
        self.raw_seq_buf.clear();
        self.osc_utf8_cont_remaining = 0;
    }

    /// Processa um chunk de bytes vindo do PTY (`pty_seq`) e retorna os eventos e fatias de texto.
    #[allow(clippy::too_many_lines)]
    pub fn scan_chunk(&mut self, pty_seq: u64, chunk: &[u8]) -> ScanOutput {
        let mut out = ScanOutput::default();
        if chunk.is_empty() {
            return out;
        }

        let chunk_base_offset = self.total_bytes_seen;
        let mut idx = 0;
        let len = chunk.len();

        while idx < len {
            match self.state {
                State::Ground => {
                    // Fast-path O(n) vetorizado via memchr procurando ESC (0x1B)
                    if let Some(rel_esc) = memchr(0x1b, &chunk[idx..]) {
                        if rel_esc > 0 {
                            let start_off = chunk_base_offset + idx as u64;
                            let end_off = start_off + rel_esc as u64;
                            Self::push_text_span(
                                &mut out,
                                pty_seq,
                                start_off,
                                end_off,
                                &chunk[idx..idx + rel_esc],
                            );
                        }
                        let esc_idx = idx + rel_esc;
                        self.osc_start_pos = StreamPosition {
                            pty_seq,
                            byte_offset: chunk_base_offset + esc_idx as u64,
                        };
                        self.raw_seq_buf.clear();
                        self.raw_seq_buf.push(0x1b);
                        self.state = State::AfterEsc;
                        idx = esc_idx + 1;
                    } else {
                        let start_off = chunk_base_offset + idx as u64;
                        let end_off = chunk_base_offset + len as u64;
                        Self::push_text_span(&mut out, pty_seq, start_off, end_off, &chunk[idx..]);
                        idx = len;
                    }
                }
                State::AfterEsc => {
                    let b = chunk[idx];
                    match b {
                        b']' => {
                            // Início de sequência OSC (`ESC ]`)
                            self.raw_seq_buf.push(b']');
                            self.payload_buf.clear();
                            self.osc_utf8_cont_remaining = 0;
                            self.state = State::OscPayload;
                            idx += 1;
                        }
                        b'P' | b'X' | b'^' | b'_' => {
                            // DCS (`ESC P`), SOS (`ESC X`), PM (`ESC ^`), APC (`ESC _`):
                            // ignora falsos prefixos OSC internos até ST, preservando os bytes no texto.
                            let esc_off = self.osc_start_pos.byte_offset;
                            let cur_off = chunk_base_offset + idx as u64 + 1;
                            Self::push_text_span(&mut out, pty_seq, esc_off, esc_off + 1, &[0x1b]);
                            Self::push_text_span(&mut out, pty_seq, cur_off - 1, cur_off, &[b]);
                            self.raw_seq_buf.clear();
                            self.state = State::IgnoredControlString;
                            idx += 1;
                        }
                        0x1b => {
                            // Novo ESC consecutivo: emite o ESC anterior como texto normal e permanece em AfterEsc
                            let prev_esc_off = self.osc_start_pos.byte_offset;
                            Self::push_text_span(
                                &mut out,
                                pty_seq,
                                prev_esc_off,
                                prev_esc_off + 1,
                                &[0x1b],
                            );
                            self.osc_start_pos = StreamPosition {
                                pty_seq,
                                byte_offset: chunk_base_offset + idx as u64,
                            };
                            self.raw_seq_buf.clear();
                            self.raw_seq_buf.push(0x1b);
                            idx += 1;
                        }
                        _ => {
                            // Qualquer outra sequência (ex.: CSI `ESC [`), preserva como texto
                            let esc_off = self.osc_start_pos.byte_offset;
                            let cur_off = chunk_base_offset + idx as u64 + 1;
                            Self::push_text_span(&mut out, pty_seq, esc_off, esc_off + 1, &[0x1b]);
                            Self::push_text_span(&mut out, pty_seq, cur_off - 1, cur_off, &[b]);
                            self.raw_seq_buf.clear();
                            self.state = State::Ground;
                            idx += 1;
                        }
                    }
                }
                State::IgnoredControlString => {
                    let b = chunk[idx];
                    let cur_off = chunk_base_offset + idx as u64;
                    Self::push_text_span(&mut out, pty_seq, cur_off, cur_off + 1, &[b]);
                    idx += 1;
                    if b == 0x1b {
                        self.state = State::IgnoredControlStringAfterEsc;
                    } else if b == 0x07 || b == 0x18 || b == 0x1a {
                        self.state = State::Ground;
                    }
                }
                State::IgnoredControlStringAfterEsc => {
                    let b = chunk[idx];
                    let cur_off = chunk_base_offset + idx as u64;
                    Self::push_text_span(&mut out, pty_seq, cur_off, cur_off + 1, &[b]);
                    idx += 1;
                    if b == b'\\' {
                        self.state = State::Ground;
                    } else if b == 0x1b {
                        self.state = State::IgnoredControlStringAfterEsc;
                    } else {
                        self.state = State::IgnoredControlString;
                    }
                }
                State::OscPayload => {
                    let b = chunk[idx];
                    if b == 0x07 {
                        // Terminador BEL
                        self.raw_seq_buf.push(b);
                        let end_pos = StreamPosition {
                            pty_seq,
                            byte_offset: chunk_base_offset + idx as u64 + 1,
                        };
                        self.finalize_osc_candidate(&mut out, pty_seq, end_pos);
                        self.state = State::Ground;
                        idx += 1;
                    } else if b == 0x1b {
                        self.raw_seq_buf.push(b);
                        self.state = State::OscAfterEsc;
                        idx += 1;
                    } else if b == 0x9c && self.osc_utf8_cont_remaining == 0 {
                        // Terminador C1 ST (0x9C) fora de sequência UTF-8 multibyte
                        self.raw_seq_buf.push(b);
                        let end_pos = StreamPosition {
                            pty_seq,
                            byte_offset: chunk_base_offset + idx as u64 + 1,
                        };
                        self.finalize_osc_candidate(&mut out, pty_seq, end_pos);
                        self.state = State::Ground;
                        idx += 1;
                    } else if b == 0x18 || b == 0x1a {
                        // CAN (0x18) ou SUB (0x1A) aborta sequência OSC sem emitir marca
                        self.payload_buf.clear();
                        self.raw_seq_buf.clear();
                        self.osc_utf8_cont_remaining = 0;
                        self.state = State::Ground;
                        idx += 1;
                    } else if self.payload_buf.len() >= self.max_payload_bytes {
                        // Excedeu limite de segurança: descarta o restante até ST/BEL sem alocar
                        self.payload_buf.clear();
                        self.raw_seq_buf.clear();
                        self.osc_utf8_cont_remaining = 0;
                        self.overflow_discards += 1;
                        self.state = State::OverflowDiscard;
                        idx += 1;
                    } else {
                        self.update_utf8_tracking(b);
                        self.payload_buf.push(b);
                        if self.raw_seq_buf.len() < self.max_payload_bytes + 8 {
                            self.raw_seq_buf.push(b);
                        }
                        idx += 1;
                    }
                }
                State::OscAfterEsc => {
                    let b = chunk[idx];
                    if b == b'\\' {
                        // Terminador ST (`ESC \`)
                        self.raw_seq_buf.push(b);
                        let end_pos = StreamPosition {
                            pty_seq,
                            byte_offset: chunk_base_offset + idx as u64 + 1,
                        };
                        self.finalize_osc_candidate(&mut out, pty_seq, end_pos);
                        self.state = State::Ground;
                        idx += 1;
                    } else {
                        // ESC seguido de outro byte cancela o OSC incompleto e reprocessa a nova sequência ESC!
                        self.payload_buf.clear();
                        self.raw_seq_buf.clear();
                        self.osc_utf8_cont_remaining = 0;
                        self.osc_start_pos = StreamPosition {
                            pty_seq,
                            byte_offset: (chunk_base_offset + idx as u64).saturating_sub(1),
                        };
                        self.raw_seq_buf.push(0x1b);
                        self.state = State::AfterEsc;
                        // Não incrementa `idx` para reprocessar `b` no estado `AfterEsc`
                    }
                }
                State::OverflowDiscard => {
                    let b = chunk[idx];
                    idx += 1;
                    if b == 0x07 || b == 0x18 || b == 0x1a {
                        self.state = State::Ground;
                    } else if b == 0x1b {
                        self.state = State::OverflowAfterEsc;
                    }
                }
                State::OverflowAfterEsc => {
                    let b = chunk[idx];
                    if b == b'\\' {
                        self.state = State::Ground;
                        idx += 1;
                    } else {
                        // Cancela descarte e reprocessa a nova sequência iniciada pelo ESC
                        self.osc_start_pos = StreamPosition {
                            pty_seq,
                            byte_offset: (chunk_base_offset + idx as u64).saturating_sub(1),
                        };
                        self.raw_seq_buf.clear();
                        self.raw_seq_buf.push(0x1b);
                        self.state = State::AfterEsc;
                    }
                }
            }
        }

        self.total_bytes_seen += len as u64;
        out
    }

    fn update_utf8_tracking(&mut self, b: u8) {
        if self.osc_utf8_cont_remaining > 0 {
            if (0x80..=0xbf).contains(&b) {
                self.osc_utf8_cont_remaining -= 1;
            } else {
                self.osc_utf8_cont_remaining = 0;
            }
        } else if (0xc2..=0xdf).contains(&b) {
            self.osc_utf8_cont_remaining = 1;
        } else if (0xe0..=0xef).contains(&b) {
            self.osc_utf8_cont_remaining = 2;
        } else if (0xf0..=0xf4).contains(&b) {
            self.osc_utf8_cont_remaining = 3;
        }
    }

    fn finalize_osc_candidate(
        &mut self,
        out: &mut ScanOutput,
        pty_seq: u64,
        end_pos: StreamPosition,
    ) {
        if let Some(kind) = parse_osc_payload(&self.payload_buf) {
            let ev = RawOscEvent {
                kind,
                start_pos: self.osc_start_pos,
                end_pos,
            };
            out.events.push(ev.clone());
            out.segments.push(ScanSegment::Event(ev));
        } else if !self.raw_seq_buf.is_empty() {
            // Sequência OSC desconhecida (ex.: OSC 0 título da janela) é preservada nos spans de texto
            let start_off = self.osc_start_pos.byte_offset;
            let end_off = end_pos.byte_offset;
            Self::push_text_span(out, pty_seq, start_off, end_off, &self.raw_seq_buf);
        }
        self.payload_buf.clear();
        self.raw_seq_buf.clear();
        self.osc_utf8_cont_remaining = 0;
    }

    fn push_text_span(
        out: &mut ScanOutput,
        pty_seq: u64,
        start_offset: u64,
        end_offset: u64,
        bytes: &[u8],
    ) {
        if bytes.is_empty() {
            return;
        }
        if let Some(last) = out.text_spans.last_mut()
            && last.pty_seq == pty_seq
            && last.end_byte_offset == start_offset
            && matches!(out.segments.last(), Some(ScanSegment::Text(_)))
        {
            last.bytes.extend_from_slice(bytes);
            last.end_byte_offset = end_offset;
            if let Some(ScanSegment::Text(seg_last)) = out.segments.last_mut() {
                seg_last.bytes.extend_from_slice(bytes);
                seg_last.end_byte_offset = end_offset;
            }
            return;
        }

        let span = TextSpan {
            pty_seq,
            start_byte_offset: start_offset,
            end_byte_offset: end_offset,
            bytes: bytes.to_vec(),
        };
        out.text_spans.push(span.clone());
        out.segments.push(ScanSegment::Text(span));
    }
}

fn parse_osc_payload(payload: &[u8]) -> Option<OscKind> {
    let text = std::str::from_utf8(payload).ok()?;
    let (code_str, rest) = text.split_once(';')?;
    match code_str.trim() {
        "133" => parse_osc_133_or_633(rest, ObservationSource::Osc133),
        "633" => parse_osc_133_or_633(rest, ObservationSource::Osc633),
        "7" => {
            let cwd = decode_file_uri_cwd(rest)?;
            Some(OscKind::Osc7Cwd { cwd })
        }
        _ => None,
    }
}

#[allow(clippy::too_many_lines)]
fn parse_osc_133_or_633(rest: &str, source: ObservationSource) -> Option<OscKind> {
    let mut parts = rest.split(';');
    let marker = parts.next()?.trim();

    match marker {
        "A" => {
            let mut nonce = None;
            let mut generation = None;
            let mut aid = None;
            let mut cl = None;
            for param in parts {
                if let Some((k, v)) = param.split_once('=') {
                    match k.trim() {
                        "nonce" | "n" => nonce = non_empty_trimmed(v),
                        "g" | "gen" => generation = v.trim().parse::<u64>().ok(),
                        "aid" => aid = non_empty_trimmed(v),
                        "cl" => cl = non_empty_trimmed(v),
                        _ => {}
                    }
                }
            }
            Some(OscKind::PromptStart {
                nonce,
                generation,
                aid,
                command_line_mode: cl,
                source,
            })
        }
        "B" => {
            let mut nonce = None;
            let mut generation = None;
            for param in parts {
                if let Some((k, v)) = param.split_once('=') {
                    match k.trim() {
                        "nonce" | "n" => nonce = non_empty_trimmed(v),
                        "g" | "gen" => generation = v.trim().parse::<u64>().ok(),
                        _ => {}
                    }
                }
            }
            Some(OscKind::PromptEnd {
                nonce,
                generation,
                source,
            })
        }
        "C" => {
            let mut nonce = None;
            let mut generation = None;
            let mut command = None;
            let mut cwd = None;
            for param in parts {
                if let Some((k, v)) = param.split_once('=') {
                    match k.trim() {
                        "nonce" | "n" => nonce = non_empty_trimmed(v),
                        "g" | "gen" => generation = v.trim().parse::<u64>().ok(),
                        "cmd_b64" => {
                            if let Some(decoded) = decode_base64_utf8(v)
                                && !decoded.trim().is_empty()
                            {
                                command = Some(decoded);
                            }
                        }
                        "cmd" | "cmdline" => {
                            let unescaped = unescape_osc_value(v);
                            if !unescaped.trim().is_empty() {
                                command = Some(unescaped);
                            }
                        }
                        "cwd_b64" => {
                            if let Some(decoded) = decode_base64_utf8(v) {
                                cwd = normalize_and_validate_cwd(&decoded);
                            }
                        }
                        "cwd" | "Cwd" => {
                            let unescaped = unescape_osc_value(v);
                            cwd = normalize_and_validate_cwd(&unescaped);
                        }
                        _ => {}
                    }
                }
            }
            Some(OscKind::CommandExecuted {
                nonce,
                generation,
                command,
                cwd,
                source,
            })
        }
        "D" => {
            let mut exit_code: Option<i32> = None;
            let mut nonce = None;
            let mut generation = None;
            let mut cwd = None;

            for (idx, param) in parts.enumerate() {
                let trimmed = param.trim();
                if idx == 0 && !trimmed.contains('=') {
                    if !trimmed.is_empty() {
                        exit_code = trimmed.parse::<i32>().ok();
                    }
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    match k.trim() {
                        "exit" | "code" | "status" => {
                            exit_code = v.trim().parse::<i32>().ok();
                        }
                        "nonce" | "n" => nonce = non_empty_trimmed(v),
                        "g" | "gen" => generation = v.trim().parse::<u64>().ok(),
                        "cwd_b64" => {
                            if let Some(decoded) = decode_base64_utf8(v) {
                                cwd = normalize_and_validate_cwd(&decoded);
                            }
                        }
                        "cwd" | "Cwd" => {
                            let unescaped = unescape_osc_value(v);
                            cwd = normalize_and_validate_cwd(&unescaped);
                        }
                        _ => {}
                    }
                }
            }

            Some(OscKind::CommandFinished {
                exit_code,
                nonce,
                generation,
                cwd,
                source,
            })
        }
        "E" => {
            let raw_cmd = parts.next().unwrap_or("");
            let mut nonce = None;
            let mut generation = None;
            let mut quality = CommandTextQuality::Exact;

            for param in parts {
                let trimmed = param.trim();
                if let Some((k, v)) = trimmed.split_once('=') {
                    match k.trim() {
                        "nonce" | "n" => nonce = non_empty_trimmed(v),
                        "g" | "gen" => generation = v.trim().parse::<u64>().ok(),
                        "q" | "quality" => {
                            quality = match v.trim() {
                                "partial" | "argv_reconstructed" => {
                                    CommandTextQuality::ArgvReconstructed
                                }
                                "unavailable" => CommandTextQuality::Unavailable,
                                _ => CommandTextQuality::Exact,
                            };
                        }
                        _ => {}
                    }
                } else if nonce.is_none() && !trimmed.is_empty() {
                    // Compatibilidade com `OSC 633 ; E ; <cmd> ; <nonce>`
                    nonce = Some(trimmed.to_string());
                }
            }

            let decoded = unescape_osc_value(raw_cmd);
            let command = if decoded.trim().is_empty() {
                None
            } else {
                Some(decoded)
            };
            if command.is_none() && quality == CommandTextQuality::Exact {
                quality = CommandTextQuality::Unavailable;
            }

            Some(OscKind::CommandLine {
                command,
                quality,
                nonce,
                generation,
                source,
            })
        }
        "P" => {
            let mut cwd = None;
            let mut shell = None;
            let mut handshake_mt = false;
            let mut nonce = None;
            let mut generation = None;

            for param in parts {
                if let Some((k, v)) = param.split_once('=') {
                    match k.trim() {
                        "Cwd" | "cwd" => {
                            let unescaped = unescape_osc_value(v);
                            cwd = normalize_and_validate_cwd(&unescaped);
                        }
                        "cwd_b64" => {
                            if let Some(decoded) = decode_base64_utf8(v) {
                                cwd = normalize_and_validate_cwd(&decoded);
                            }
                        }
                        "shell" | "Shell" => shell = non_empty_trimmed(v),
                        "MT" => handshake_mt = v.trim() == "1",
                        "nonce" | "n" => nonce = non_empty_trimmed(v),
                        "g" | "gen" => generation = v.trim().parse::<u64>().ok(),
                        _ => {}
                    }
                }
            }

            Some(OscKind::Property {
                cwd,
                shell,
                handshake_mt,
                nonce,
                generation,
                source,
            })
        }
        _ => None,
    }
}

fn non_empty_trimmed(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD;

    #[test]
    fn scans_complete_and_split_osc_sequences() {
        let mut scanner = OscScanner::new();
        let cmd_b64 = STANDARD.encode("echo hello");
        let workdir_b64 = STANDARD.encode("/home/user/proj");
        let payload = format!(
            "antes\x1b]133;A;cl=m;aid=1;nonce=abc\x07\x1b]133;B;nonce=abc\x1b\\$ \x1b]133;C;nonce=abc;cmd_b64={cmd_b64};cwd_b64={workdir_b64}\x07hello\r\n\x1b]133;D;0;nonce=abc;cwd_b64={workdir_b64}\x07depois"
        );

        let out = scanner.scan_chunk(1, payload.as_bytes());
        assert_eq!(out.events.len(), 4);
        assert_eq!(out.concatenated_text(), b"antes$ hello\r\ndepois");
    }

    #[test]
    fn discards_overflow_osc_and_resumes_cleanly() {
        let mut scanner = OscScanner::with_max_payload_bytes(64);
        let mut huge = vec![0x1b, b']'];
        huge.extend_from_slice(b"133;C;cmd_b64=");
        huge.extend(std::iter::repeat_n(b'A', 200));
        huge.push(0x07);
        huge.extend_from_slice(b"visivel\x1b]133;A;nonce=n1\x07");

        let out = scanner.scan_chunk(1, &huge);
        assert_eq!(scanner.overflow_discards(), 1);
        assert_eq!(out.events.len(), 1);
        assert_eq!(out.concatenated_text(), b"visivel");
    }
}
