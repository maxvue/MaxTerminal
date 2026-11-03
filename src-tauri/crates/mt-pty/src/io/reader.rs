//! Estado compartilhado de saída (`SharedOutputState`) e thread dedicada de leitura (`ReaderWorker`).
//!
//! Implementa leitura não bloqueante via `poll(2)`, coalescing adaptativo (janela de `4 ms` ou `256 KiB`
//! com fast-path de eco após ociosidade), despacho único para `OutputTap`, armazenamento em `ReplayRing`
//! e envio sob controle estrito de créditos para `OutputSink`.

use std::collections::VecDeque;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bytes::{Bytes, BytesMut};
use mt_core::ids::PtyId;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use parking_lot::Mutex;
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::io::Errno;

use crate::config::PtyConfig;
use crate::error::{PtyError, PtyResult};
use crate::io::EventFdWake;
use crate::io::flow::{FlowControl, FlowStatus, MonotonicClock};
use crate::io::frame::{
    MAX_DATA_FRAME_BYTES, encode_data_frame, encode_end_frame, encode_gap_frame,
    encode_replay_frame,
};
use crate::io::replay::{ReplayRing, StoredFrame};
use crate::sink::OutputSink;
use crate::tap::TapRegistry;
use crate::types::{AttachInfo, AttachmentId, ExitInfo, OutputCursor};

/// Limite máximo de bytes para considerar uma leitura após ociosidade como eco interativo imediato.
const INTERACTIVE_ECHO_MAX_BYTES: usize = 4096;

#[derive(Debug, Clone)]
enum PendingFrame {
    Gap(Bytes),
    Replay(StoredFrame),
    Data(StoredFrame),
    End {
        final_seq: u64,
        total_bytes: u64,
        exit: ExitInfo,
    },
}

struct OutputInner {
    ring: ReplayRing,
    pending_outbound: VecDeque<PendingFrame>,
    active_attachment_id: Option<AttachmentId>,
    end_info: Option<ExitInfo>,
    end_queued_for: Option<AttachmentId>,
}

/// Estado compartilhado thread-safe entre `PtyInstance` e `ReaderWorker`.
///
/// Serializa transições de `attach`/`detach`, coordena o envio ordenado `Gap? -> Replay* -> Data* -> End?`
/// sob controle de crédito e expõe contadores atômicos (`last_seq`, `bytes_out`) sem contenção.
pub struct SharedOutputState {
    inner: Mutex<OutputInner>,
    send_lock: Mutex<()>,
    flow: Arc<FlowControl>,
    taps: Arc<TapRegistry>,
    wake: Arc<EventFdWake>,
    last_seq: AtomicU64,
    bytes_out: AtomicU64,
}

impl SharedOutputState {
    /// Cria um novo `SharedOutputState` com capacidade de replay e dependências explícitas.
    #[must_use]
    pub fn new(
        replay_capacity: usize,
        flow: Arc<FlowControl>,
        taps: Arc<TapRegistry>,
        wake: Arc<EventFdWake>,
    ) -> Self {
        Self {
            inner: Mutex::new(OutputInner {
                ring: ReplayRing::new(replay_capacity),
                pending_outbound: VecDeque::new(),
                active_attachment_id: None,
                end_info: None,
                end_queued_for: None,
            }),
            send_lock: Mutex::new(()),
            flow,
            taps,
            wake,
            last_seq: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
        }
    }

    /// Cria um `SharedOutputState` a partir de `PtyConfig` usando `MonotonicClock`.
    pub fn from_config(config: &PtyConfig, taps: Arc<TapRegistry>) -> PtyResult<Self> {
        let wake = Arc::new(EventFdWake::new()?);
        let flow = Arc::new(FlowControl::with_wake(
            config.high_watermark,
            config.low_watermark,
            config.ack_timeout,
            Arc::new(MonotonicClock),
            Some(Arc::clone(&wake)),
        ));
        Ok(Self::new(config.replay_capacity, flow, taps, wake))
    }

    /// Anexa um novo `OutputSink` a partir de `cursor` (`None` inicia do começo retido).
    ///
    /// Substitui qualquer geração anterior, enfileira `Gap? -> Replay* -> End?` e inicia o envio
    /// imediato até o limite de crédito disponível.
    pub fn attach(
        &self,
        sink: Arc<dyn OutputSink>,
        cursor: Option<OutputCursor>,
    ) -> PtyResult<AttachInfo> {
        let info = {
            let mut guard = self.inner.lock();
            let plan = guard.ring.prepare_attach(cursor)?;
            let attachment_id = AttachmentId::new();

            self.flow
                .install_attachment(attachment_id, sink, plan.replay_from);

            guard.pending_outbound.clear();
            if let Some((first_seq, first_offset, dropped)) = plan.gap {
                guard
                    .pending_outbound
                    .push_back(PendingFrame::Gap(encode_gap_frame(
                        first_seq,
                        first_offset,
                        dropped,
                    )));
            }
            for stored in plan.frames {
                guard
                    .pending_outbound
                    .push_back(PendingFrame::Replay(stored));
            }
            if let Some(exit) = guard.end_info.clone() {
                let final_seq = guard.ring.last_seq();
                let total_bytes = guard.ring.total_bytes();
                guard.pending_outbound.push_back(PendingFrame::End {
                    final_seq,
                    total_bytes,
                    exit,
                });
                guard.end_queued_for = Some(attachment_id);
            } else {
                guard.end_queued_for = None;
            }
            guard.active_attachment_id = Some(attachment_id);

            AttachInfo {
                attachment_id,
                replay_from: plan.replay_from,
                live_from: plan.live_from,
                truncated: plan.truncated,
            }
        };

        self.drain_outbound();
        self.wake.wake();
        Ok(info)
    }

    /// Desanexa a geração `attachment_id` se ainda estiver ativa (no-op idempotente se antiga).
    pub fn detach(&self, attachment_id: AttachmentId) -> PtyResult<()> {
        let removed = self.flow.detach(attachment_id)?;
        if removed {
            let mut guard = self.inner.lock();
            if guard.active_attachment_id == Some(attachment_id) {
                guard.active_attachment_id = None;
                guard.pending_outbound.clear();
                guard.end_queued_for = None;
            }
        }
        self.wake.wake();
        Ok(())
    }

    /// Força o desanexo imediato da geração atual (usado durante `kill`/`shutdown` ou falha de canal).
    pub fn force_detach(&self) {
        self.flow.force_detach_current();
        {
            let mut guard = self.inner.lock();
            guard.active_attachment_id = None;
            guard.pending_outbound.clear();
            guard.end_queued_for = None;
        }
        self.wake.wake();
    }

    /// Registra confirmação cumulativa de leitura (`consumed_bytes`) na geração `attachment_id`.
    pub fn ack(&self, attachment_id: AttachmentId, consumed_bytes: u64) -> PtyResult<()> {
        let status = self.flow.record_ack(attachment_id, consumed_bytes)?;
        if status == FlowStatus::Flowing {
            self.drain_outbound();
            self.wake.wake();
        }
        Ok(())
    }

    /// Registra o término definitivo do PTY (`ExitInfo`) e enfileira o frame `End` para a conexão ativa.
    pub fn record_end(&self, exit: ExitInfo) {
        {
            let mut guard = self.inner.lock();
            guard.end_info = Some(exit.clone());
            if let Some(active_id) = guard.active_attachment_id
                && guard.end_queued_for != Some(active_id)
            {
                let final_seq = guard.ring.last_seq();
                let total_bytes = guard.ring.total_bytes();
                guard.pending_outbound.push_back(PendingFrame::End {
                    final_seq,
                    total_bytes,
                    exit,
                });
                guard.end_queued_for = Some(active_id);
            }
        }
        self.drain_outbound();
        self.wake.wake();
    }

    /// Verifica se o lease de ACK da conexão atual expirou e limpa a fila pendente caso positivo.
    pub fn check_ack_timeout(&self) -> bool {
        let expired = self.flow.check_ack_timeout();
        if expired {
            let mut guard = self.inner.lock();
            if self.flow.current_attachment().is_none() {
                guard.active_attachment_id = None;
                guard.pending_outbound.clear();
                guard.end_queued_for = None;
            }
        }
        expired
    }

    /// Registra um novo frame de dados ao vivo no `ReplayRing`, notifica `OutputTap` uma única vez
    /// e enfileira/envia para o consumidor anexado.
    pub fn publish_live_data(&self, pty_id: PtyId, payload: Bytes) {
        if payload.is_empty() {
            return;
        }
        let stored = {
            let mut guard = self.inner.lock();
            let stored = guard.ring.push_frame(payload);
            self.last_seq.store(stored.seq, Ordering::Release);
            self.bytes_out
                .store(guard.ring.total_bytes(), Ordering::Release);
            if guard.active_attachment_id.is_some() {
                guard
                    .pending_outbound
                    .push_back(PendingFrame::Data(stored.clone()));
            }
            stored
        };

        self.taps
            .dispatch_output(pty_id, stored.seq, &stored.payload);
        self.drain_outbound();
    }

    /// Drena os frames pendentes na fila da conexão ativa até esvaziar ou atingir o teto de crédito.
    pub fn drain_outbound(&self) {
        let _send_guard = self.send_lock.lock();
        loop {
            let step = {
                let mut guard = self.inner.lock();
                let Some(active_id) = guard.active_attachment_id else {
                    guard.pending_outbound.clear();
                    break;
                };
                let Some(front) = guard.pending_outbound.front() else {
                    break;
                };

                match front {
                    PendingFrame::Gap(raw) => {
                        let Some((curr_id, sink)) = self.flow.current_attachment() else {
                            guard.active_attachment_id = None;
                            guard.pending_outbound.clear();
                            break;
                        };
                        if curr_id != active_id {
                            guard.active_attachment_id = None;
                            guard.pending_outbound.clear();
                            break;
                        }
                        let raw_clone = raw.clone();
                        guard.pending_outbound.pop_front();
                        (active_id, sink, Ok(raw_clone))
                    }
                    PendingFrame::End {
                        final_seq,
                        total_bytes,
                        exit,
                    } => {
                        let Some((curr_id, sink)) = self.flow.current_attachment() else {
                            guard.active_attachment_id = None;
                            guard.pending_outbound.clear();
                            break;
                        };
                        if curr_id != active_id {
                            guard.active_attachment_id = None;
                            guard.pending_outbound.clear();
                            break;
                        }
                        let encoded = encode_end_frame(*final_seq, *total_bytes, exit);
                        guard.pending_outbound.pop_front();
                        (active_id, sink, encoded)
                    }
                    PendingFrame::Replay(stored) => {
                        let Ok(sink) = self.flow.reserve_credit(active_id, stored.payload.len())
                        else {
                            break;
                        };
                        let encoded =
                            encode_replay_frame(stored.seq, stored.byte_offset, &stored.payload);
                        guard.pending_outbound.pop_front();
                        (active_id, sink, encoded)
                    }
                    PendingFrame::Data(stored) => {
                        let Ok(sink) = self.flow.reserve_credit(active_id, stored.payload.len())
                        else {
                            break;
                        };
                        let encoded =
                            encode_data_frame(stored.seq, stored.byte_offset, &stored.payload);
                        guard.pending_outbound.pop_front();
                        (active_id, sink, encoded)
                    }
                }
            };

            let (active_id, sink, encoded_res) = step;
            if let Ok(frame_bytes) = encoded_res
                && sink.send(frame_bytes).is_err()
            {
                let _ = self.detach(active_id);
                break;
            }
        }
    }

    /// Indica se há frames pendentes na fila de saída da conexão ativa.
    #[must_use]
    pub fn has_pending_outbound(&self) -> bool {
        let guard = self.inner.lock();
        guard.active_attachment_id.is_some() && !guard.pending_outbound.is_empty()
    }

    /// Retorna o crédito disponível para a conexão ativa, ou `None` se `Detached`.
    #[must_use]
    pub fn available_credit(&self) -> Option<usize> {
        let (id, _) = self.flow.current_attachment()?;
        self.flow.available_credit(id)
    }

    /// Retorna o último número de sequência `Data` produzido (`0` se nenhum).
    #[must_use]
    pub fn last_seq(&self) -> u64 {
        self.last_seq.load(Ordering::Acquire)
    }

    /// Retorna o próximo número de sequência `Data` esperado (`>= 1`).
    #[must_use]
    pub fn next_seq(&self) -> u64 {
        self.last_seq.load(Ordering::Acquire).saturating_add(1)
    }

    /// Retorna o total acumulado de bytes brutos produzidos pelo PTY.
    #[must_use]
    pub fn bytes_out(&self) -> u64 {
        self.bytes_out.load(Ordering::Acquire)
    }

    /// Retorna a quantidade de bytes retidos atualmente no `ReplayRing`.
    #[must_use]
    pub fn retained_replay_bytes(&self) -> usize {
        self.inner.lock().ring.retained_bytes()
    }

    /// Indica se há um consumidor (`OutputSink`) anexado no momento.
    #[must_use]
    pub fn is_attached(&self) -> bool {
        self.flow.status() != FlowStatus::Detached
    }

    /// Retorna referência para o coordenador `FlowControl`.
    #[must_use]
    pub const fn flow(&self) -> &Arc<FlowControl> {
        &self.flow
    }

    /// Retorna referência para o `EventFdWake` associado ao leitor.
    #[must_use]
    pub const fn wake(&self) -> &Arc<EventFdWake> {
        &self.wake
    }

    /// Retorna referência para o `TapRegistry` compartilhado.
    #[must_use]
    pub const fn taps(&self) -> &Arc<TapRegistry> {
        &self.taps
    }
}

/// Parâmetros de configuração do `ReaderWorker`.
#[derive(Debug, Clone, Copy)]
pub struct ReaderConfig {
    /// Tamanho do buffer de syscall `read(2)` (padrão: `64 KiB`).
    pub read_buffer_size: usize,
    /// Janela temporal de coalescing (padrão: `4 ms`).
    pub coalesce_window: Duration,
    /// Tamanho máximo de payload de um frame (padrão: `256 KiB`).
    pub max_frame_bytes: usize,
}

impl Default for ReaderConfig {
    fn default() -> Self {
        Self {
            read_buffer_size: 64 * 1024,
            coalesce_window: Duration::from_millis(4),
            max_frame_bytes: MAX_DATA_FRAME_BYTES,
        }
    }
}

/// Worker dedicado à leitura do descritor mestre do PTY (`master_fd`).
pub struct ReaderWorker {
    pty_id: PtyId,
    master_fd: OwnedFd,
    state: Arc<SharedOutputState>,
    read_buffer_size: usize,
    coalesce_window: Duration,
    max_frame_bytes: usize,
    stop_flag: Arc<AtomicBool>,
}

impl ReaderWorker {
    /// Cria um novo `ReaderWorker` com parâmetros explícitos.
    #[must_use]
    pub fn new(
        pty_id: PtyId,
        master_fd: OwnedFd,
        state: Arc<SharedOutputState>,
        read_buffer_size: usize,
        coalesce_window: Duration,
        max_frame_bytes: usize,
        stop_flag: Arc<AtomicBool>,
    ) -> Self {
        let clamped_max_frame = max_frame_bytes.clamp(1024, MAX_DATA_FRAME_BYTES);
        let clamped_read_buf = read_buffer_size.clamp(1024, clamped_max_frame);
        Self {
            pty_id,
            master_fd,
            state,
            read_buffer_size: clamped_read_buf,
            coalesce_window,
            max_frame_bytes: clamped_max_frame,
            stop_flag,
        }
    }

    /// Cria um novo `ReaderWorker` extraindo os parâmetros de `PtyConfig`.
    #[must_use]
    pub fn from_config(
        pty_id: PtyId,
        master_fd: OwnedFd,
        state: Arc<SharedOutputState>,
        config: &PtyConfig,
        stop_flag: Arc<AtomicBool>,
    ) -> Self {
        let effective_max_frame = if config.replay_capacity > 0 {
            config.max_frame_bytes.min(config.replay_capacity)
        } else {
            config.max_frame_bytes
        };
        Self::new(
            pty_id,
            master_fd,
            state,
            config.read_buffer_size,
            config.coalesce_window,
            effective_max_frame,
            stop_flag,
        )
    }

    fn flush_coalesced(
        &self,
        coalesce_buf: &mut BytesMut,
        coalesce_started_at: &mut Option<Instant>,
        last_flush_at: &mut Option<Instant>,
    ) {
        if coalesce_buf.is_empty() {
            *coalesce_started_at = None;
            return;
        }
        let payload = coalesce_buf.split().freeze();
        *coalesce_started_at = None;
        *last_flush_at = Some(Instant::now());
        self.state.publish_live_data(self.pty_id, payload);
    }

    fn compute_read_budget(&self, current_buffered: usize) -> usize {
        if self.state.has_pending_outbound() {
            return 0;
        }
        let frame_space = self.max_frame_bytes.saturating_sub(current_buffered);
        if frame_space == 0 {
            return 0;
        }
        match self.state.flow().status() {
            FlowStatus::Paused => 0,
            FlowStatus::Detached => frame_space,
            FlowStatus::Flowing => {
                let credit = self.state.available_credit().unwrap_or(0);
                let remaining_credit = credit.saturating_sub(current_buffered);
                frame_space.min(remaining_credit)
            }
        }
    }

    /// Executa o loop de leitura do PTY até EOF/`EIO` no master FD ou acionamento de `stop_flag`.
    #[allow(clippy::too_many_lines)]
    pub fn run(self) -> PtyResult<()> {
        let flags = fcntl_getfl(&self.master_fd)
            .map_err(|e| PtyError::Io(std::io::Error::from_raw_os_error(e.raw_os_error())))?;
        fcntl_setfl(&self.master_fd, flags | OFlags::NONBLOCK)
            .map_err(|e| PtyError::Io(std::io::Error::from_raw_os_error(e.raw_os_error())))?;

        let mut read_buf = vec![0u8; self.read_buffer_size];
        let mut coalesce_buf = BytesMut::with_capacity(self.max_frame_bytes);
        let mut coalesce_started_at: Option<Instant> = None;
        let mut last_flush_at: Option<Instant> = None;

        loop {
            self.state.drain_outbound();
            self.state.check_ack_timeout();

            let stopping = self.stop_flag.load(Ordering::Acquire);
            if stopping && self.state.flow().status() == FlowStatus::Paused {
                // Durante encerramento, não permite que consumidor pausado bloqueie a drenagem final.
                self.state.force_detach();
            }

            if !coalesce_buf.is_empty() {
                let window_elapsed = coalesce_started_at
                    .is_some_and(|started| started.elapsed() >= self.coalesce_window);
                if coalesce_buf.len() >= self.max_frame_bytes || window_elapsed {
                    self.flush_coalesced(
                        &mut coalesce_buf,
                        &mut coalesce_started_at,
                        &mut last_flush_at,
                    );
                }
            }

            let mut budget = self.compute_read_budget(coalesce_buf.len());
            if budget == 0 && !coalesce_buf.is_empty() && !self.state.has_pending_outbound() {
                // Se o buffer já preencheu todo o crédito restante da conexão ativa, faz flush imediato
                // para atualizar sent_bytes e transicionar formalmente para Paused.
                self.flush_coalesced(
                    &mut coalesce_buf,
                    &mut coalesce_started_at,
                    &mut last_flush_at,
                );
                budget = self.compute_read_budget(coalesce_buf.len());
            }

            if budget > 0 {
                let was_empty = coalesce_buf.is_empty();
                let was_idle = last_flush_at.is_none_or(|t| t.elapsed() >= self.coalesce_window);
                let mut reached_eof = false;

                while budget > 0 {
                    let to_read = read_buf.len().min(budget);
                    match rustix::io::read(&self.master_fd, &mut read_buf[..to_read]) {
                        Ok(0) => {
                            reached_eof = true;
                            break;
                        }
                        Ok(n) => {
                            if coalesce_buf.is_empty() {
                                coalesce_started_at = Some(Instant::now());
                            }
                            coalesce_buf.extend_from_slice(&read_buf[..n]);
                            budget = budget.saturating_sub(n);

                            let window_expired = coalesce_started_at
                                .is_some_and(|t| t.elapsed() >= self.coalesce_window);
                            if coalesce_buf.len() >= self.max_frame_bytes
                                || budget == 0
                                || window_expired
                            {
                                self.flush_coalesced(
                                    &mut coalesce_buf,
                                    &mut coalesce_started_at,
                                    &mut last_flush_at,
                                );
                                budget = self.compute_read_budget(coalesce_buf.len());
                            }
                        }
                        Err(Errno::INTR) => {}
                        Err(Errno::AGAIN) => {
                            if was_empty
                                && was_idle
                                && !coalesce_buf.is_empty()
                                && coalesce_buf.len() <= INTERACTIVE_ECHO_MAX_BYTES
                            {
                                self.flush_coalesced(
                                    &mut coalesce_buf,
                                    &mut coalesce_started_at,
                                    &mut last_flush_at,
                                );
                            }
                            break;
                        }
                        Err(Errno::IO) => {
                            // No Linux, EIO no master PTY indica fechamento de todos os FDs slave (EOF).
                            reached_eof = true;
                            break;
                        }
                        Err(err) => {
                            self.flush_coalesced(
                                &mut coalesce_buf,
                                &mut coalesce_started_at,
                                &mut last_flush_at,
                            );
                            return Err(PtyError::Io(std::io::Error::from_raw_os_error(
                                err.raw_os_error(),
                            )));
                        }
                    }
                }

                if reached_eof || stopping {
                    self.flush_coalesced(
                        &mut coalesce_buf,
                        &mut coalesce_started_at,
                        &mut last_flush_at,
                    );
                    self.state.drain_outbound();
                    return Ok(());
                }
            } else if stopping {
                self.flush_coalesced(
                    &mut coalesce_buf,
                    &mut coalesce_started_at,
                    &mut last_flush_at,
                );
                self.state.drain_outbound();
                return Ok(());
            }

            let timeout_ms = if let Some(started) = coalesce_started_at {
                let rem = self.coalesce_window.saturating_sub(started.elapsed());
                u16::try_from(rem.as_millis().max(1)).unwrap_or(50)
            } else {
                50u16
            };

            let can_poll_master = self.compute_read_budget(coalesce_buf.len()) > 0;
            if can_poll_master {
                let mut fds = [
                    PollFd::new(
                        self.master_fd.as_fd(),
                        PollFlags::POLLIN | PollFlags::POLLHUP | PollFlags::POLLERR,
                    ),
                    PollFd::new(self.state.wake().as_fd(), PollFlags::POLLIN),
                ];
                match poll(&mut fds, PollTimeout::from(timeout_ms)) {
                    Ok(_) | Err(nix::errno::Errno::EINTR) => {}
                    Err(e) => {
                        return Err(PtyError::Io(std::io::Error::from_raw_os_error(e as i32)));
                    }
                }
                if fds[1]
                    .revents()
                    .is_some_and(|r| r.contains(PollFlags::POLLIN))
                {
                    self.state.wake().drain();
                }
            } else {
                let mut fds = [PollFd::new(self.state.wake().as_fd(), PollFlags::POLLIN)];
                match poll(&mut fds, PollTimeout::from(timeout_ms)) {
                    Ok(_) | Err(nix::errno::Errno::EINTR) => {}
                    Err(e) => {
                        return Err(PtyError::Io(std::io::Error::from_raw_os_error(e as i32)));
                    }
                }
                if fds[0]
                    .revents()
                    .is_some_and(|r| r.contains(PollFlags::POLLIN))
                {
                    self.state.wake().drain();
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::io::frame::{FrameKind, decode_end_payload, decode_frame, decode_gap_payload};
    use crate::sink::VecSink;
    use crate::tap::OutputTap;
    use crate::types::{ExitReason, U64Decimal};
    use nix::pty::openpty;
    use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};
    use sha2::{Digest, Sha256};

    struct RecordingTap {
        chunks: Mutex<Vec<(u64, Vec<u8>)>>,
    }

    impl RecordingTap {
        fn new() -> Self {
            Self {
                chunks: Mutex::new(Vec::new()),
            }
        }
    }

    impl OutputTap for RecordingTap {
        fn on_output(&self, _pty_id: PtyId, seq: u64, bytes: &[u8]) {
            self.chunks.lock().push((seq, bytes.to_vec()));
        }
    }

    #[test]
    fn preserves_1mib_arbitrary_bytes_and_split_utf8_ansi_over_real_pty() {
        let pty = openpty(None, None).unwrap();
        let mut term = tcgetattr(&pty.slave).unwrap();
        cfmakeraw(&mut term);
        tcsetattr(&pty.slave, SetArg::TCSANOW, &term).unwrap();

        let pty_id = PtyId::new();
        let taps = Arc::new(TapRegistry::new());
        let wake = Arc::new(EventFdWake::new().unwrap());
        let flow = Arc::new(FlowControl::with_wake(
            256 * 1024,
            64 * 1024,
            Duration::from_secs(5),
            Arc::new(MonotonicClock),
            Some(Arc::clone(&wake)),
        ));
        let state = Arc::new(SharedOutputState::new(1024 * 1024, flow, taps, wake));
        let sink = Arc::new(VecSink::new());
        let attach_info = state.attach(sink.clone(), None).unwrap();

        let stop_flag = Arc::new(AtomicBool::new(false));
        let reader = ReaderWorker::new(
            pty_id,
            pty.master,
            Arc::clone(&state),
            64 * 1024,
            Duration::from_millis(4),
            256 * 1024,
            Arc::clone(&stop_flag),
        );

        let reader_handle = std::thread::spawn(move || reader.run().unwrap());

        // Constrói 1 MiB contendo todos os bytes 0..=255 + UTF-8 multibyte + ANSI CSI/OSC
        let mut payload = Vec::with_capacity(1024 * 1024);
        let pattern = "ação 🚀 日本語 \x1b[38;2;255;128;0mMAX\x1b[0m".as_bytes();
        while payload.len() < 1024 * 1024 {
            let b = u8::try_from(payload.len() % 256).unwrap();
            payload.push(b);
            if payload.len() + pattern.len() <= 1024 * 1024 {
                payload.extend_from_slice(pattern);
            }
        }
        let expected_hash = Sha256::digest(&payload);

        // Escreve no slave em tamanhos primos (ex.: 4093 bytes) para forçar corte de UTF-8 e ANSI entre reads
        let slave_fd = pty.slave;
        let writer_handle = std::thread::spawn(move || {
            let mut offset = 0;
            while offset < payload.len() {
                let end = (offset + 4093).min(payload.len());
                match rustix::io::write(&slave_fd, &payload[offset..end]) {
                    Ok(n) => offset += n,
                    Err(Errno::INTR | Errno::AGAIN) => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("erro ao escrever no slave: {e}"),
                }
            }
            drop(slave_fd);
        });

        let mut received = Vec::with_capacity(1024 * 1024);
        let mut consumed = 0u64;
        let mut expected_seq = 1u64;

        while received.len() < 1024 * 1024 {
            let frames = sink.take_frames();
            if frames.is_empty() {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            for raw in frames {
                let decoded = decode_frame(raw).unwrap();
                assert_eq!(decoded.kind, FrameKind::Data);
                assert_eq!(decoded.seq, expected_seq);
                assert_eq!(decoded.byte_offset, consumed);
                expected_seq += 1;
                consumed += u64::try_from(decoded.payload.len()).unwrap();
                received.extend_from_slice(&decoded.payload);
            }
            state.ack(attach_info.attachment_id, consumed).unwrap();
        }

        writer_handle.join().unwrap();
        reader_handle.join().unwrap();

        assert_eq!(received.len(), 1024 * 1024);
        assert_eq!(Sha256::digest(&received), expected_hash);
    }

    #[test]
    fn enforces_backpressure_pause_and_resumes_without_loss_or_duplication() {
        let pty = openpty(None, None).unwrap();
        let mut term = tcgetattr(&pty.slave).unwrap();
        cfmakeraw(&mut term);
        tcsetattr(&pty.slave, SetArg::TCSANOW, &term).unwrap();

        let pty_id = PtyId::new();
        let taps = Arc::new(TapRegistry::new());
        let wake = Arc::new(EventFdWake::new().unwrap());
        let high_watermark = 32 * 1024;
        let low_watermark = 8 * 1024;
        let flow = Arc::new(FlowControl::with_wake(
            high_watermark,
            low_watermark,
            Duration::from_secs(5),
            Arc::new(MonotonicClock),
            Some(Arc::clone(&wake)),
        ));
        let state = Arc::new(SharedOutputState::new(512 * 1024, flow, taps, wake));
        let sink = Arc::new(VecSink::new());
        let attach_info = state.attach(sink.clone(), None).unwrap();

        let stop_flag = Arc::new(AtomicBool::new(false));
        let reader = ReaderWorker::new(
            pty_id,
            pty.master,
            Arc::clone(&state),
            16 * 1024,
            Duration::from_millis(4),
            64 * 1024,
            Arc::clone(&stop_flag),
        );
        let reader_handle = std::thread::spawn(move || reader.run().unwrap());

        let total_size = 128 * 1024;
        let data: Vec<u8> = (0..total_size)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect();
        let expected_copy = data.clone();
        let slave_fd = pty.slave;
        let writer_handle = std::thread::spawn(move || {
            let mut offset = 0;
            while offset < data.len() {
                let end = (offset + 4096).min(data.len());
                match rustix::io::write(&slave_fd, &data[offset..end]) {
                    Ok(n) => offset += n,
                    Err(Errno::INTR | Errno::AGAIN) => {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                    Err(e) => panic!("write error: {e}"),
                }
            }
            drop(slave_fd);
        });

        // Aguarda até o FlowControl pausar por atingir high_watermark
        let deadline = Instant::now() + Duration::from_secs(2);
        while state.flow().status() != FlowStatus::Paused && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(state.flow().status(), FlowStatus::Paused);
        assert!(state.flow().unacked_bytes() <= u64::try_from(high_watermark).unwrap());

        // Agora consome e envia ACKs até receber todos os 128 KiB
        let mut received = Vec::with_capacity(total_size);
        let mut consumed = 0u64;
        let mut next_seq = 1u64;
        while received.len() < total_size {
            let frames = sink.take_frames();
            if frames.is_empty() {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            }
            for raw in frames {
                let decoded = decode_frame(raw).unwrap();
                assert_eq!(decoded.kind, FrameKind::Data);
                assert_eq!(decoded.seq, next_seq);
                next_seq += 1;
                consumed += u64::try_from(decoded.payload.len()).unwrap();
                received.extend_from_slice(&decoded.payload);
            }
            state.ack(attach_info.attachment_id, consumed).unwrap();
        }

        writer_handle.join().unwrap();
        reader_handle.join().unwrap();
        assert_eq!(received, expected_copy);
    }

    #[test]
    fn detached_output_populates_replay_with_gap_and_calls_tap_exactly_once() {
        let pty_id = PtyId::new();
        let taps = Arc::new(TapRegistry::new());
        let tap = Arc::new(RecordingTap::new());
        let _tap_handle = taps.register(tap.clone());

        let wake = Arc::new(EventFdWake::new().unwrap());
        let flow = Arc::new(FlowControl::with_wake(
            1024 * 1024,
            256 * 1024,
            Duration::from_secs(5),
            Arc::new(MonotonicClock),
            Some(Arc::clone(&wake)),
        ));
        // Ring de 12 bytes
        let state = SharedOutputState::new(12, flow, taps, wake);

        state.publish_live_data(pty_id, Bytes::from_static(b"1111")); // seq 1, 0..4
        state.publish_live_data(pty_id, Bytes::from_static(b"2222")); // seq 2, 4..8
        state.publish_live_data(pty_id, Bytes::from_static(b"3333")); // seq 3, 8..12
        state.publish_live_data(pty_id, Bytes::from_static(b"4444")); // seq 4, 12..16 (expulsa seq 1)

        assert_eq!(tap.chunks.lock().len(), 4);

        let sink = Arc::new(VecSink::new());
        let info = state.attach(sink.clone(), None).unwrap();
        assert!(info.truncated);
        assert_eq!(info.replay_from.next_seq.0, 2);
        assert_eq!(info.replay_from.byte_offset.0, 4);
        assert_eq!(info.live_from.next_seq.0, 5);
        assert_eq!(info.live_from.byte_offset.0, 16);

        // Tap NÃO deve ter sido rechamado durante o attach/replay
        assert_eq!(tap.chunks.lock().len(), 4);

        // Dado ao vivo após attach
        state.publish_live_data(pty_id, Bytes::from_static(b"5555")); // seq 5, 16..20
        assert_eq!(tap.chunks.lock().len(), 5);

        let exit = ExitInfo {
            code: Some(0),
            signal: None,
            signal_name: None,
            reason: ExitReason::Exited,
            at_ms: U64Decimal(1000),
        };
        state.record_end(exit.clone());

        let frames = sink.take_frames();
        assert_eq!(frames.len(), 6); // Gap + 3 Replay (2, 3, 4) + 1 Data (5) + 1 End

        let f_gap = decode_frame(frames[0].clone()).unwrap();
        assert_eq!(f_gap.kind, FrameKind::Gap);
        assert_eq!(f_gap.seq, 2);
        assert_eq!(f_gap.byte_offset, 4);
        assert_eq!(decode_gap_payload(&f_gap).unwrap(), 4);

        let f_r2 = decode_frame(frames[1].clone()).unwrap();
        assert_eq!(f_r2.kind, FrameKind::Replay);
        assert_eq!(f_r2.seq, 2);
        assert_eq!(&f_r2.payload[..], b"2222");

        let f_r3 = decode_frame(frames[2].clone()).unwrap();
        assert_eq!(f_r3.kind, FrameKind::Replay);
        assert_eq!(f_r3.seq, 3);
        assert_eq!(&f_r3.payload[..], b"3333");

        let f_r4 = decode_frame(frames[3].clone()).unwrap();
        assert_eq!(f_r4.kind, FrameKind::Replay);
        assert_eq!(f_r4.seq, 4);
        assert_eq!(&f_r4.payload[..], b"4444");

        let frame_data_5 = decode_frame(frames[4].clone()).unwrap();
        assert_eq!(frame_data_5.kind, FrameKind::Data);
        assert_eq!(frame_data_5.seq, 5);
        assert_eq!(&frame_data_5.payload[..], b"5555");

        let f_end = decode_frame(frames[5].clone()).unwrap();
        assert_eq!(f_end.kind, FrameKind::End);
        assert_eq!(f_end.seq, 5);
        assert_eq!(f_end.byte_offset, 20);
        assert_eq!(decode_end_payload(&f_end).unwrap(), exit);
    }

    #[test]
    fn fast_path_echo_flushes_under_10ms_and_burst_coalesces_up_to_256kib() {
        let pty = openpty(None, None).unwrap();
        let mut term = tcgetattr(&pty.slave).unwrap();
        cfmakeraw(&mut term);
        tcsetattr(&pty.slave, SetArg::TCSANOW, &term).unwrap();

        let pty_id = PtyId::new();
        let taps = Arc::new(TapRegistry::new());
        let wake = Arc::new(EventFdWake::new().unwrap());
        let flow = Arc::new(FlowControl::with_wake(
            1024 * 1024,
            256 * 1024,
            Duration::from_secs(5),
            Arc::new(MonotonicClock),
            Some(Arc::clone(&wake)),
        ));
        let state = Arc::new(SharedOutputState::new(1024 * 1024, flow, taps, wake));
        let sink = Arc::new(VecSink::new());
        let _info = state.attach(sink.clone(), None).unwrap();

        let stop_flag = Arc::new(AtomicBool::new(false));
        let reader = ReaderWorker::new(
            pty_id,
            pty.master,
            Arc::clone(&state),
            64 * 1024,
            Duration::from_millis(4),
            256 * 1024,
            Arc::clone(&stop_flag),
        );
        let reader_handle = std::thread::spawn(move || reader.run().unwrap());

        // Eco interativo após ociosidade deve chegar em < 10 ms (CA-14)
        std::thread::sleep(Duration::from_millis(10));
        let echo_start = Instant::now();
        rustix::io::write(&pty.slave, b"a").unwrap();
        while sink.is_empty() && echo_start.elapsed() < Duration::from_millis(50) {
            std::thread::yield_now();
        }
        assert!(echo_start.elapsed() < Duration::from_millis(10));
        let echo_frames = sink.take_frames();
        assert_eq!(echo_frames.len(), 1);
        let decoded_echo = decode_frame(echo_frames[0].clone()).unwrap();
        assert_eq!(&decoded_echo.payload[..], b"a");

        stop_flag.store(true, Ordering::Release);
        state.wake().wake();
        drop(pty.slave);
        reader_handle.join().unwrap();
    }
}
