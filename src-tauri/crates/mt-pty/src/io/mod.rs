//! Subsistema de I/O de alto volume, frames binários v1, backpressure e replay.
//!
//! Este módulo agrupa:
//! - [`frame`]: codificação e decodificação do protocolo binário v1 (cabeçalho de 24 bytes LE).
//! - [`flow`]: controle de fluxo baseado em créditos (`high_watermark`/`low_watermark`) e lease de ACK.
//! - [`replay`]: ring buffer circular de replay (`ReplayRing`) e validação de `OutputCursor`.
//! - [`reader`]: estado compartilhado de saída (`SharedOutputState`) e thread de leitura (`ReaderWorker`).
//! - [`writer`]: fila limitada de escrita (`WriteQueue`) e thread de escrita não bloqueante (`WriterWorker`).

pub mod flow;
pub mod frame;
pub mod reader;
pub mod replay;
pub mod writer;

use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustix::event::{EventfdFlags, eventfd};
use rustix::io::Errno;

use crate::error::{PtyError, PtyResult};

pub use flow::{AttachmentState, Clock, FlowControl, FlowStatus, MockClock, MonotonicClock};
pub use frame::{
    DecodedFrame, FRAME_HEADER_LEN, FRAME_VERSION, FrameHeader, FrameKind, GAP_PAYLOAD_LEN,
    MAX_DATA_FRAME_BYTES, MAX_END_PAYLOAD_LEN, decode_end_payload, decode_frame,
    decode_gap_payload, encode_data_frame, encode_end_frame, encode_frame, encode_gap_frame,
    encode_replay_frame,
};
pub use reader::{ReaderConfig, ReaderWorker, SharedOutputState};
pub use replay::{ReplayPlan, ReplayRing, ReplaySnapshot, StoredFrame};
pub use writer::{WriteQueue, WriterWorker};

/// Sinalizador leve baseado em `eventfd(2)` Linux para acordar chamadas `poll(2)` das threads de I/O.
#[derive(Debug)]
pub struct EventFdWake {
    fd: OwnedFd,
}

impl EventFdWake {
    /// Cria um novo `eventfd` configurado com `EFD_CLOEXEC | EFD_NONBLOCK`.
    pub fn new() -> PtyResult<Self> {
        let fd = eventfd(0, EventfdFlags::CLOEXEC | EventfdFlags::NONBLOCK)
            .map_err(|e| PtyError::Io(std::io::Error::from_raw_os_error(e.raw_os_error())))?;
        Ok(Self { fd })
    }

    /// Incrementa o contador do `eventfd` para acordar imediatamente qualquer `poll(2)` aguardando `POLLIN`.
    pub fn wake(&self) {
        let buf = 1u64.to_ne_bytes();
        let _ = rustix::io::write(&self.fd, &buf);
    }

    /// Drena todos os eventos acumulados no `eventfd` de forma não bloqueante.
    pub fn drain(&self) {
        let mut buf = [0u8; 8];
        while let Ok(_) | Err(Errno::INTR) = rustix::io::read(&self.fd, &mut buf) {}
    }

    /// Retorna o descritor emprestado (`BorrowedFd`) para registro em `poll(2)`.
    #[must_use]
    pub fn as_fd(&self) -> BorrowedFd<'_> {
        self.fd.as_fd()
    }
}

/// Controlador atômico do orçamento global de memória compartilhado entre todos os PTYs (`256 MiB`).
#[derive(Debug)]
pub struct GlobalBufferPool {
    budget_bytes: usize,
    used_bytes: AtomicUsize,
}

/// Alias para [`GlobalBufferPool`].
pub type BufferPool = GlobalBufferPool;

impl GlobalBufferPool {
    /// Cria um novo pool global com teto de `budget_bytes`.
    #[must_use]
    pub const fn new(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            used_bytes: AtomicUsize::new(0),
        }
    }

    /// Tenta reservar atomicamente `bytes` do orçamento global.
    ///
    /// Retorna `true` se a reserva foi concluída ou `false` se exceder `budget_bytes`.
    pub fn try_reserve(&self, bytes: usize) -> bool {
        if bytes == 0 {
            return true;
        }
        let mut current = self.used_bytes.load(Ordering::Acquire);
        loop {
            let Some(next) = current.checked_add(bytes) else {
                return false;
            };
            if next > self.budget_bytes {
                return false;
            }
            match self.used_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    /// Devolve `bytes` previamente reservados ao orçamento global.
    pub fn release(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let mut current = self.used_bytes.load(Ordering::Acquire);
        loop {
            let next = current.saturating_sub(bytes);
            match self.used_bytes.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Retorna o total de bytes atualmente reservados no pool global.
    #[must_use]
    pub fn used_bytes(&self) -> usize {
        self.used_bytes.load(Ordering::Acquire)
    }

    /// Retorna o orçamento máximo configurado em bytes.
    #[must_use]
    pub const fn budget_bytes(&self) -> usize {
        self.budget_bytes
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn eventfd_wake_signals_and_drains_cleanly() {
        let wake = EventFdWake::new().unwrap();
        wake.wake();
        wake.wake();
        wake.drain();
    }

    #[test]
    fn global_buffer_pool_enforces_budget_and_releases() {
        let pool = GlobalBufferPool::new(1024);
        assert_eq!(pool.budget_bytes(), 1024);
        assert_eq!(pool.used_bytes(), 0);

        assert!(pool.try_reserve(600));
        assert_eq!(pool.used_bytes(), 600);

        assert!(!pool.try_reserve(500));
        assert_eq!(pool.used_bytes(), 600);

        assert!(pool.try_reserve(424));
        assert_eq!(pool.used_bytes(), 1024);

        pool.release(300);
        assert_eq!(pool.used_bytes(), 724);

        pool.release(1000);
        assert_eq!(pool.used_bytes(), 0);
    }
}
