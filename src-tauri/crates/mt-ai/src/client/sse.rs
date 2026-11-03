//! Processador incremental de Server-Sent Events (SSE) com coalescência de deltas (`32 ms`),
//! limites de frame (`64 KiB`) e de resposta (`1 MiB`), e retenção de cauda para sanitização
//! de segredos divididos entre chunks (`CA-08`, `CA-36`).

use crate::client::dialect::{ChatResponse, GatewayDialect};
use crate::error::{ai_error, codes};
use mt_core::MtResult;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};
use tokio_util::sync::CancellationToken;

/// Tamanho máximo total acumulado de uma resposta SSE (`1 MiB`).
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// Tamanho máximo de um único frame/evento SSE (`64 KiB`).
pub const MAX_SSE_FRAME_BYTES: usize = 64 * 1024;
/// Tempo máximo de ociosidade entre bytes de um stream SSE (`15 s`).
pub const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(15);
/// Intervalo de coalescência para emissão de deltas ao frontend (`32 ms`).
pub const DELTA_COALESCE_INTERVAL: Duration = Duration::from_millis(32);
/// Quantidade de bytes retidos no final do buffer antes de emitir deltas para impedir
/// que segredos divididos entre dois chunks escapem antes da sanitização (`CA-36`).
pub const SANITIZER_HOLD_TAIL_BYTES: usize = 128;

/// Sanitizador incremental que retém até `SANITIZER_HOLD_TAIL_BYTES` da cauda ativa
/// para garantir que segredos divididos na fronteira de dois chunks SSE sejam mascarados
/// antes de qualquer emissão para `on_delta`.
#[derive(Default)]
pub struct SseSanitizerBuffer {
    raw_accumulated: String,
    emitted_sanitized_bytes: usize,
    pending_coalesced: String,
    last_emit: Option<Instant>,
}

impl SseSanitizerBuffer {
    /// Cria um novo buffer incremental de sanitização SSE.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adiciona um pedaço bruto de texto ao acumulador e emite deltas sanitizados
    /// respeitando a janela de retenção de cauda (`SANITIZER_HOLD_TAIL_BYTES`) e coalescência.
    pub fn push_delta(&mut self, chunk: &str, on_delta: &mut (dyn FnMut(&str) + Send)) {
        if chunk.is_empty() {
            return;
        }
        self.raw_accumulated.push_str(chunk);

        let safe_raw_end = if self.raw_accumulated.len() > SANITIZER_HOLD_TAIL_BYTES {
            let target = self.raw_accumulated.len() - SANITIZER_HOLD_TAIL_BYTES;
            floor_char_boundary(&self.raw_accumulated, target)
        } else {
            0
        };

        if safe_raw_end > 0 {
            let prefix_sanitized =
                mt_db::sanitizer::sanitize(&self.raw_accumulated[..safe_raw_end]);
            if prefix_sanitized.len() > self.emitted_sanitized_bytes {
                let start = floor_char_boundary(&prefix_sanitized, self.emitted_sanitized_bytes);
                let new_piece = &prefix_sanitized[start..];
                if !new_piece.is_empty() {
                    self.pending_coalesced.push_str(new_piece);
                    self.emitted_sanitized_bytes = prefix_sanitized.len();
                }
            }
        }

        let now = Instant::now();
        let should_flush = match self.last_emit {
            None => !self.pending_coalesced.is_empty(),
            Some(last) => {
                !self.pending_coalesced.is_empty()
                    && now.duration_since(last) >= DELTA_COALESCE_INTERVAL
            }
        };

        if should_flush {
            on_delta(&self.pending_coalesced);
            self.pending_coalesced.clear();
            self.last_emit = Some(now);
        }
    }

    /// Finaliza o stream no marcador `[DONE]`, sanitizando o conteúdo integral e emitindo
    /// qualquer cauda pendente antes de retornar o texto completo sanitizado.
    pub fn finish(mut self, on_delta: &mut (dyn FnMut(&str) + Send)) -> String {
        let full_sanitized = mt_db::sanitizer::sanitize(&self.raw_accumulated).into_owned();
        if full_sanitized.len() > self.emitted_sanitized_bytes {
            let start = floor_char_boundary(&full_sanitized, self.emitted_sanitized_bytes);
            self.pending_coalesced.push_str(&full_sanitized[start..]);
        }
        if !self.pending_coalesced.is_empty() {
            on_delta(&self.pending_coalesced);
            self.pending_coalesced.clear();
        }
        full_sanitized
    }
}

fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Consome um stream SSE a partir de um leitor assíncrono bufferizado, validando
/// limites de tamanho, idle timeout, cancelamento e presença obrigatória do marcador `[DONE]`.
pub async fn consume_sse_stream<R: AsyncBufRead + Unpin>(
    mut reader: R,
    dialect: &dyn GatewayDialect,
    cancel: &CancellationToken,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> MtResult<ChatResponse> {
    let mut san_buf = SseSanitizerBuffer::new();
    let mut current_event_data = String::new();
    let mut total_bytes = 0usize;
    let mut saw_done = false;
    let mut line = String::new();

    loop {
        line.clear();
        let read_fut = tokio::time::timeout(STREAM_IDLE_TIMEOUT, reader.read_line(&mut line));
        let n = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                return Err(ai_error(codes::AI_CANCELLED, "Operação cancelada pelo usuário."));
            }
            res = read_fut => {
                match res {
                    Ok(Ok(bytes_read)) => bytes_read,
                    Ok(Err(io_err)) => {
                        return Err(ai_error(
                            codes::AI_BAD_RESPONSE,
                            format!("Erro de leitura durante streaming SSE: {io_err}"),
                        ));
                    }
                    Err(_) => {
                        return Err(ai_error(
                            codes::AI_TIMEOUT,
                            "Tempo limite de inatividade excedido durante o streaming SSE.",
                        ));
                    }
                }
            }
        };

        if n == 0 {
            break;
        }

        total_bytes = total_bytes.saturating_add(n);
        if total_bytes > MAX_RESPONSE_BYTES {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "A resposta em streaming excedeu o limite máximo de 1 MiB.",
            ));
        }

        let trimmed_line = line.trim_end_matches(['\r', '\n']);
        if trimmed_line.is_empty() {
            if !current_event_data.is_empty() {
                let data_payload = current_event_data.trim_end_matches('\n');
                if dialect.is_sse_done(data_payload) {
                    saw_done = true;
                    break;
                }
                if let Some(delta) = dialect.parse_sse_delta(data_payload)? {
                    san_buf.push_delta(&delta, on_delta);
                }
                current_event_data.clear();
            }
            continue;
        }

        if trimmed_line.starts_with(':') {
            continue;
        }

        if let Some(rest) = trimmed_line.strip_prefix("data:") {
            let data_part = rest.strip_prefix(' ').unwrap_or(rest);
            if dialect.is_sse_done(data_part) {
                saw_done = true;
                break;
            }
            if current_event_data.len() + data_part.len() + 1 > MAX_SSE_FRAME_BYTES {
                return Err(ai_error(
                    codes::AI_BAD_RESPONSE,
                    "Frame SSE excedeu o limite máximo de 64 KiB.",
                ));
            }
            if !current_event_data.is_empty() {
                current_event_data.push('\n');
            }
            current_event_data.push_str(data_part);
        }
    }

    if !saw_done && !current_event_data.is_empty() {
        let data_payload = current_event_data.trim_end_matches('\n');
        if dialect.is_sse_done(data_payload) {
            saw_done = true;
        }
    }

    if !saw_done {
        return Err(ai_error(
            codes::AI_BAD_RESPONSE,
            "Conexão SSE encerrada prematuramente sem marcador de conclusão.",
        ));
    }

    let content = san_buf.finish(on_delta);
    Ok(ChatResponse {
        content,
        model: None,
        usage: None,
    })
}
