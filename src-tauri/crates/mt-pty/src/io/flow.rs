//! Controle de fluxo baseado em créditos (`high_watermark` / `low_watermark`) e lease de conexão.
//!
//! Coordena a contrapressão real entre o consumidor anexado (`OutputSink`) e a thread de leitura
//! do PTY (`ReaderWorker`), além de expirar conexões mudas após `ack_timeout` quando há bytes em trânsito.

use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::error::{PtyError, PtyResult};
use crate::io::EventFdWake;
use crate::sink::OutputSink;
use crate::types::{AttachmentId, OutputCursor};

/// Abstração de relógio monotônico para permitir testes determinísticos de expiração de ACK.
pub trait Clock: Send + Sync + 'static {
    /// Retorna o instante atual.
    fn now(&self) -> Instant;
}

/// Relógio monotônico real do sistema operacional (`Instant::now`).
#[derive(Debug, Default, Clone, Copy)]
pub struct MonotonicClock;

impl Clock for MonotonicClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Relógio determinístico controlável manualmente para testes unitários.
#[derive(Debug)]
pub struct MockClock {
    now: Mutex<Instant>,
}

impl MockClock {
    /// Cria um novo `MockClock` ancorado no instante atual.
    #[must_use]
    pub fn new() -> Self {
        Self {
            now: Mutex::new(Instant::now()),
        }
    }

    /// Avança o relógio simulado pelo intervalo `delta`.
    pub fn advance(&self, delta: Duration) {
        let mut guard = self.now.lock();
        *guard += delta;
    }
}

impl Default for MockClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for MockClock {
    fn now(&self) -> Instant {
        *self.now.lock()
    }
}

/// Estado operacional do controle de fluxo de saída de um PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowStatus {
    /// Fluxo normal com consumidor anexado e crédito disponível.
    Flowing,
    /// Leitura do master PTY pausada porque `unacked >= high_watermark`.
    Paused,
    /// Nenhum consumidor anexado; leitura prossegue apenas para `ReplayRing` e `OutputTap`.
    Detached,
}

/// Estado contábil de uma geração de conexão (`attach`) ativa.
#[derive(Clone)]
pub struct AttachmentState {
    /// Identificador único desta geração de conexão.
    pub id: AttachmentId,
    /// Destino binário dos frames desta conexão.
    pub sink: Arc<dyn OutputSink>,
    /// Total acumulado de bytes de payload (`Data` + `Replay`) enviados nesta geração.
    pub sent_bytes: u64,
    /// Total acumulado de bytes de payload confirmados via `pty_ack` nesta geração.
    pub consumed_bytes: u64,
    /// Instante do último progresso de ACK ou de início de bytes em trânsito.
    pub last_progress_at: Instant,
    /// Próximo cursor esperado na conexão.
    pub next_cursor: OutputCursor,
    /// Indica se o envio está pausado aguardando ACK baixar até `low_watermark`.
    pub paused: bool,
}

impl fmt::Debug for AttachmentState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AttachmentState")
            .field("id", &self.id)
            .field("sent_bytes", &self.sent_bytes)
            .field("consumed_bytes", &self.consumed_bytes)
            .field("last_progress_at", &self.last_progress_at)
            .field("next_cursor", &self.next_cursor)
            .field("paused", &self.paused)
            .finish_non_exhaustive()
    }
}

/// Coordenador de controle de fluxo e janela de crédito de saída de um PTY.
pub struct FlowControl {
    high_watermark: u64,
    low_watermark: u64,
    ack_timeout: Duration,
    clock: Arc<dyn Clock>,
    wake: Option<Arc<EventFdWake>>,
    state: Mutex<Option<AttachmentState>>,
    cv: Condvar,
}

impl FlowControl {
    /// Cria um novo `FlowControl` sem `EventFdWake` acoplado.
    #[must_use]
    pub fn new(
        high_watermark: usize,
        low_watermark: usize,
        ack_timeout: Duration,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self::with_wake(high_watermark, low_watermark, ack_timeout, clock, None)
    }

    /// Cria um novo `FlowControl` com `EventFdWake` opcional para acordar o `poll(2)` do leitor.
    #[must_use]
    pub fn with_wake(
        high_watermark: usize,
        low_watermark: usize,
        ack_timeout: Duration,
        clock: Arc<dyn Clock>,
        wake: Option<Arc<EventFdWake>>,
    ) -> Self {
        let hw = u64::try_from(high_watermark).unwrap_or(u64::MAX).max(1);
        let lw = u64::try_from(low_watermark).unwrap_or(0).min(hw);
        Self {
            high_watermark: hw,
            low_watermark: lw,
            ack_timeout,
            clock,
            wake,
            state: Mutex::new(None),
            cv: Condvar::new(),
        }
    }

    /// Instala uma nova geração de conexão, substituindo eventual geração anterior e zerando o ledger.
    pub fn install_attachment(
        &self,
        id: AttachmentId,
        sink: Arc<dyn OutputSink>,
        initial_cursor: OutputCursor,
    ) {
        let now = self.clock.now();
        {
            let mut guard = self.state.lock();
            *guard = Some(AttachmentState {
                id,
                sink,
                sent_bytes: 0,
                consumed_bytes: 0,
                last_progress_at: now,
                next_cursor: initial_cursor,
                paused: false,
            });
        }
        self.cv.notify_all();
        if let Some(wake) = &self.wake {
            wake.wake();
        }
    }

    /// Remove a geração de conexão caso `attachment_id` corresponda à geração ativa.
    ///
    /// Se `attachment_id` pertencer a uma geração antiga já substituída, atua como no-op idempotente
    /// retornando `Ok(false)`.
    pub fn detach(&self, attachment_id: AttachmentId) -> PtyResult<bool> {
        let removed = {
            let mut guard = self.state.lock();
            if guard
                .as_ref()
                .is_some_and(|active| active.id == attachment_id)
            {
                *guard = None;
                true
            } else {
                false
            }
        };
        if removed {
            self.cv.notify_all();
            if let Some(wake) = &self.wake {
                wake.wake();
            }
        }
        Ok(removed)
    }

    /// Desconecta imediatamente qualquer geração atualmente ativa.
    pub fn force_detach_current(&self) {
        let removed = self.state.lock().take().is_some();
        if removed {
            self.cv.notify_all();
            if let Some(wake) = &self.wake {
                wake.wake();
            }
        }
    }

    /// Registra a confirmação cumulativa de `consumed_bytes` para a geração `attachment_id`.
    pub fn record_ack(
        &self,
        attachment_id: AttachmentId,
        consumed_bytes: u64,
    ) -> PtyResult<FlowStatus> {
        let mut unpaused = false;
        let status = {
            let mut guard = self.state.lock();
            let Some(active) = guard.as_mut() else {
                return Err(PtyError::AttachmentStale);
            };
            if active.id != attachment_id {
                return Err(PtyError::AttachmentStale);
            }
            if consumed_bytes > active.sent_bytes {
                return Err(PtyError::AckInvalid(format!(
                    "consumed_bytes ({consumed_bytes}) excede sent_bytes ({})",
                    active.sent_bytes
                )));
            }
            if consumed_bytes <= active.consumed_bytes {
                return Ok(if active.paused {
                    FlowStatus::Paused
                } else {
                    FlowStatus::Flowing
                });
            }

            active.consumed_bytes = consumed_bytes;
            active.last_progress_at = self.clock.now();
            let unacked = active.sent_bytes.saturating_sub(active.consumed_bytes);
            if active.paused && unacked <= self.low_watermark {
                active.paused = false;
                unpaused = true;
            }
            if active.paused {
                FlowStatus::Paused
            } else {
                FlowStatus::Flowing
            }
        };

        if unpaused {
            self.cv.notify_all();
            if let Some(wake) = &self.wake {
                wake.wake();
            }
        }
        Ok(status)
    }

    /// Retorna o crédito disponível em bytes para a geração `attachment_id`, ou `None` se inativa.
    #[must_use]
    pub fn available_credit(&self, attachment_id: AttachmentId) -> Option<usize> {
        let guard = self.state.lock();
        let active = guard.as_ref()?;
        if active.id != attachment_id {
            return None;
        }
        if active.paused {
            return Some(0);
        }
        let unacked = active.sent_bytes.saturating_sub(active.consumed_bytes);
        let rem = self.high_watermark.saturating_sub(unacked);
        Some(usize::try_from(rem).unwrap_or(usize::MAX))
    }

    /// Reserva `bytes` de crédito para envio na geração `attachment_id` e retorna o `OutputSink`.
    ///
    /// Retorna `Err(())` se a geração não estiver ativa, estiver pausada ou se `bytes` exceder o
    /// crédito restante até `high_watermark` (marcando `paused = true` quando já há bytes em trânsito).
    #[allow(clippy::result_unit_err)]
    pub fn reserve_credit(
        &self,
        attachment_id: AttachmentId,
        bytes: usize,
    ) -> Result<Arc<dyn OutputSink>, ()> {
        let mut guard = self.state.lock();
        let active = guard.as_mut().ok_or(())?;
        if active.id != attachment_id || active.paused {
            return Err(());
        }

        let bytes_u64 = u64::try_from(bytes).map_err(|_| ())?;
        let unacked_before = active.sent_bytes.saturating_sub(active.consumed_bytes);
        let unacked_after = unacked_before.saturating_add(bytes_u64);

        if unacked_after > self.high_watermark && unacked_before > 0 {
            active.paused = true;
            return Err(());
        }

        if unacked_before == 0 && bytes_u64 > 0 {
            active.last_progress_at = self.clock.now();
        }

        active.sent_bytes = active.sent_bytes.saturating_add(bytes_u64);
        if unacked_after >= self.high_watermark {
            active.paused = true;
        }

        Ok(Arc::clone(&active.sink))
    }

    /// Verifica se a conexão ativa expirou por falta de ACK (`ack_timeout`) havendo bytes em trânsito.
    ///
    /// Se `sent_bytes == consumed_bytes` (terminal ocioso), o timeout nunca expira.
    pub fn check_ack_timeout(&self) -> bool {
        let now = self.clock.now();
        let expired = {
            let mut guard = self.state.lock();
            if let Some(active) = guard.as_ref() {
                let has_unacked = active.sent_bytes > active.consumed_bytes;
                let elapsed = now.saturating_duration_since(active.last_progress_at);
                if has_unacked && elapsed >= self.ack_timeout {
                    *guard = None;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if expired {
            self.cv.notify_all();
            if let Some(wake) = &self.wake {
                wake.wake();
            }
        }
        expired
    }

    /// Retorna o identificador e o `OutputSink` da geração atualmente anexada, se houver.
    #[must_use]
    pub fn current_attachment(&self) -> Option<(AttachmentId, Arc<dyn OutputSink>)> {
        let guard = self.state.lock();
        guard
            .as_ref()
            .map(|active| (active.id, Arc::clone(&active.sink)))
    }

    /// Retorna uma cópia do `AttachmentState` atual para inspeção/diagnóstico.
    #[must_use]
    pub fn attachment_snapshot(&self) -> Option<AttachmentState> {
        self.state.lock().clone()
    }

    /// Retorna o `FlowStatus` atual (`Flowing`, `Paused` ou `Detached`).
    #[must_use]
    pub fn status(&self) -> FlowStatus {
        let guard = self.state.lock();
        match guard.as_ref() {
            None => FlowStatus::Detached,
            Some(active) if active.paused => FlowStatus::Paused,
            Some(_) => FlowStatus::Flowing,
        }
    }

    /// Retorna a quantidade de bytes enviados e ainda não confirmados na geração ativa (`0` se `Detached`).
    #[must_use]
    pub fn unacked_bytes(&self) -> u64 {
        let guard = self.state.lock();
        guard
            .as_ref()
            .map_or(0, |a| a.sent_bytes.saturating_sub(a.consumed_bytes))
    }

    /// Marca `high_watermark` configurada em bytes.
    #[must_use]
    pub const fn high_watermark(&self) -> u64 {
        self.high_watermark
    }

    /// Marca `low_watermark` configurada em bytes.
    #[must_use]
    pub const fn low_watermark(&self) -> u64 {
        self.low_watermark
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::sink::NullSink;
    use crate::types::U64Decimal;

    fn zero_cursor() -> OutputCursor {
        OutputCursor {
            next_seq: U64Decimal(1),
            byte_offset: U64Decimal(0),
        }
    }

    #[test]
    fn transitions_flowing_to_paused_and_resumes_at_low_watermark() {
        let clock = Arc::new(MockClock::new());
        let flow = FlowControl::new(1024, 256, Duration::from_secs(5), clock);
        assert_eq!(flow.status(), FlowStatus::Detached);

        let id = AttachmentId::new();
        flow.install_attachment(id, Arc::new(NullSink), zero_cursor());
        assert_eq!(flow.status(), FlowStatus::Flowing);
        assert_eq!(flow.available_credit(id), Some(1024));

        // Reserva 600 bytes -> continua Flowing
        assert!(flow.reserve_credit(id, 600).is_ok());
        assert_eq!(flow.status(), FlowStatus::Flowing);
        assert_eq!(flow.unacked_bytes(), 600);
        assert_eq!(flow.available_credit(id), Some(424));

        // Reserva mais 424 bytes -> atinge high_watermark (1024) e passa para Paused
        assert!(flow.reserve_credit(id, 424).is_ok());
        assert_eq!(flow.status(), FlowStatus::Paused);
        assert_eq!(flow.unacked_bytes(), 1024);
        assert_eq!(flow.available_credit(id), Some(0));

        // Nova tentativa de reserva enquanto Paused falha
        assert!(flow.reserve_credit(id, 1).is_err());

        // ACK parcial (consumed = 500 -> unacked = 524 > 256): permanece Paused
        let st = flow.record_ack(id, 500).unwrap();
        assert_eq!(st, FlowStatus::Paused);

        // ACK repetido ou menor é no-op idempotente
        assert_eq!(flow.record_ack(id, 500).unwrap(), FlowStatus::Paused);
        assert_eq!(flow.record_ack(id, 400).unwrap(), FlowStatus::Paused);

        // ACK que reduz unacked para <= 256 (consumed = 768 -> unacked = 256): retoma Flowing
        let st = flow.record_ack(id, 768).unwrap();
        assert_eq!(st, FlowStatus::Flowing);
        assert_eq!(flow.unacked_bytes(), 256);
        assert_eq!(flow.available_credit(id), Some(768));
    }

    #[test]
    fn idle_attachment_never_expires_but_unacked_expires_after_timeout() {
        let clock = Arc::new(MockClock::new());
        let flow = FlowControl::new(1024, 256, Duration::from_secs(5), clock.clone());
        let id = AttachmentId::new();
        flow.install_attachment(id, Arc::new(NullSink), zero_cursor());

        // Ocioso (unacked == 0) por 60 segundos: não expira
        clock.advance(Duration::from_secs(60));
        assert!(!flow.check_ack_timeout());
        assert_eq!(flow.status(), FlowStatus::Flowing);

        // Envia 100 bytes
        assert!(flow.reserve_credit(id, 100).is_ok());
        clock.advance(Duration::from_secs(4));
        assert!(!flow.check_ack_timeout());

        // Progresso parcial de ACK reseta o timer
        assert_eq!(flow.record_ack(id, 50).unwrap(), FlowStatus::Flowing);
        clock.advance(Duration::from_secs(4));
        assert!(!flow.check_ack_timeout());

        // ACK repetido não reseta o timer; mais 1s completa 5s sem progresso real
        assert_eq!(flow.record_ack(id, 50).unwrap(), FlowStatus::Flowing);
        clock.advance(Duration::from_secs(1));
        assert!(flow.check_ack_timeout());
        assert_eq!(flow.status(), FlowStatus::Detached);

        // ACK atrasado na geração expirada retorna AttachmentStale
        let err = flow.record_ack(id, 100).unwrap_err();
        assert_eq!(err.code(), "pty.attachment_stale");
    }

    #[test]
    fn rejects_stale_attachment_and_excess_ack_and_ignores_stale_detach() {
        let clock = Arc::new(MockClock::new());
        let flow = FlowControl::new(1024, 256, Duration::from_secs(5), clock);

        let old_id = AttachmentId::new();
        let new_id = AttachmentId::new();
        flow.install_attachment(old_id, Arc::new(NullSink), zero_cursor());
        assert!(flow.reserve_credit(old_id, 200).is_ok());

        // Novo attach substitui old_id
        flow.install_attachment(new_id, Arc::new(NullSink), zero_cursor());
        assert_eq!(flow.unacked_bytes(), 0);

        // Detach antigo não remove a conexão nova
        assert!(!flow.detach(old_id).unwrap());
        assert_eq!(flow.status(), FlowStatus::Flowing);

        // ACK antigo retorna AttachmentStale
        assert_eq!(
            flow.record_ack(old_id, 100).unwrap_err().code(),
            "pty.attachment_stale"
        );

        // ACK acima de sent_bytes retorna AckInvalid
        assert!(flow.reserve_credit(new_id, 50).is_ok());
        assert_eq!(
            flow.record_ack(new_id, 51).unwrap_err().code(),
            "pty.ack_invalid"
        );
    }
}
