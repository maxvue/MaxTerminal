//! Fila limitada de escrita (`WriteQueue`) e thread trabalhadora de escrita (`WriterWorker`).
//!
//! Garante reserva atômica de mensagens (`<= 1024`), orçamento por PTY (`<= 4 MiB`), tamanho máximo
//! por pedido (`<= 1 MiB`) e cota no [`GlobalBufferPool`], escrevendo no descritor mestre do PTY em
//! fatias de `4 KiB` com `poll(POLLOUT)` e cancelamento imediato por `eventfd`.

use std::collections::VecDeque;
use std::os::fd::{AsFd, OwnedFd};
use std::sync::Arc;

use bytes::Bytes;
use nix::poll::{PollFd, PollFlags, PollTimeout, poll};
use parking_lot::{Condvar, Mutex};
use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
use rustix::io::Errno;

use crate::config::PtyConfig;
use crate::error::{PtyError, PtyResult};
use crate::io::{EventFdWake, GlobalBufferPool};

struct WriteQueueState {
    messages: VecDeque<Bytes>,
    queued_messages: usize,
    queued_bytes: usize,
    closed: bool,
}

/// Fila concorrente de escrita limitada por quantidade de pedidos, bytes por PTY e orçamento global.
pub struct WriteQueue {
    max_messages: usize,
    max_queue_bytes: usize,
    max_write_bytes: usize,
    global_pool: Arc<GlobalBufferPool>,
    wake: Arc<EventFdWake>,
    state: Mutex<WriteQueueState>,
    cv: Condvar,
}

impl WriteQueue {
    /// Cria uma nova `WriteQueue` alocando seu próprio `EventFdWake`.
    pub fn new(
        max_messages: usize,
        max_queue_bytes: usize,
        max_write_bytes: usize,
        global_pool: Arc<GlobalBufferPool>,
    ) -> PtyResult<Self> {
        let wake = Arc::new(EventFdWake::new()?);
        Ok(Self::with_wake(
            max_messages,
            max_queue_bytes,
            max_write_bytes,
            global_pool,
            wake,
        ))
    }

    /// Cria uma nova `WriteQueue` usando um `EventFdWake` fornecido.
    #[must_use]
    pub fn with_wake(
        max_messages: usize,
        max_queue_bytes: usize,
        max_write_bytes: usize,
        global_pool: Arc<GlobalBufferPool>,
        wake: Arc<EventFdWake>,
    ) -> Self {
        Self {
            max_messages: max_messages.max(1),
            max_queue_bytes: max_queue_bytes.max(1),
            max_write_bytes: max_write_bytes.max(1),
            global_pool,
            wake,
            state: Mutex::new(WriteQueueState {
                messages: VecDeque::new(),
                queued_messages: 0,
                queued_bytes: 0,
                closed: false,
            }),
            cv: Condvar::new(),
        }
    }

    /// Cria uma `WriteQueue` configurada a partir de `PtyConfig`.
    pub fn from_config(config: &PtyConfig, global_pool: Arc<GlobalBufferPool>) -> PtyResult<Self> {
        Self::new(
            config.write_queue_capacity,
            config.write_queue_bytes,
            config.max_write_bytes,
            global_pool,
        )
    }

    /// Enfileira atomicamente um pedido de escrita no PTY.
    ///
    /// - Retorna `Err(PtyError::InputInvalid)` se `data.len() > max_write_bytes`.
    /// - Retorna `Err(PtyError::AlreadyExited)` se a fila já estiver fechada (`Exiting` / `Exited`).
    /// - Retorna `Err(PtyError::WriteQueueFull)` se exceder `max_messages`, `max_queue_bytes` ou `GlobalBufferPool`.
    pub fn enqueue(&self, data: Bytes) -> PtyResult<()> {
        let len = data.len();
        if len > self.max_write_bytes {
            return Err(PtyError::InputInvalid(format!(
                "Pedido de escrita ({len} bytes) excede o limite de {} bytes",
                self.max_write_bytes
            )));
        }

        let mut guard = self.state.lock();
        if guard.closed {
            return Err(PtyError::AlreadyExited);
        }
        if len == 0 {
            return Ok(());
        }

        if guard.queued_messages >= self.max_messages
            || guard.queued_bytes.saturating_add(len) > self.max_queue_bytes
        {
            return Err(PtyError::WriteQueueFull);
        }

        if !self.global_pool.try_reserve(len) {
            return Err(PtyError::WriteQueueFull);
        }

        guard.queued_messages = guard.queued_messages.saturating_add(1);
        guard.queued_bytes = guard.queued_bytes.saturating_add(len);
        guard.messages.push_back(data);
        drop(guard);

        self.cv.notify_one();
        Ok(())
    }

    /// Alias para [`Self::enqueue`].
    pub fn push(&self, data: Bytes) -> PtyResult<()> {
        self.enqueue(data)
    }

    /// Fecha a fila, descarta pedidos pendentes devolvendo o orçamento ao `GlobalBufferPool`
    /// e acorda imediatamente o `WriterWorker` (inclusive se estiver bloqueado em `poll(POLLOUT)`).
    pub fn close(&self) {
        let mut released_bytes = 0usize;
        {
            let mut guard = self.state.lock();
            if guard.closed {
                return;
            }
            guard.closed = true;
            while let Some(msg) = guard.messages.pop_front() {
                let len = msg.len();
                released_bytes = released_bytes.saturating_add(len);
                guard.queued_bytes = guard.queued_bytes.saturating_sub(len);
                guard.queued_messages = guard.queued_messages.saturating_sub(1);
            }
        }
        if released_bytes > 0 {
            self.global_pool.release(released_bytes);
        }
        self.cv.notify_all();
        self.wake.wake();
    }

    /// Verifica se a fila de escrita foi fechada.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.state.lock().closed
    }

    /// Retorna a quantidade de mensagens na fila (incluindo a mensagem atualmente em escrita).
    #[must_use]
    pub fn queued_messages(&self) -> usize {
        self.state.lock().queued_messages
    }

    /// Retorna o total de bytes pendentes na fila (incluindo bytes restantes da mensagem em escrita).
    #[must_use]
    pub fn queued_bytes(&self) -> usize {
        self.state.lock().queued_bytes
    }

    /// Retorna referência para o `EventFdWake` de cancelamento/controle.
    #[must_use]
    pub const fn wake(&self) -> &Arc<EventFdWake> {
        &self.wake
    }

    fn pop_blocking(&self) -> Option<Bytes> {
        let mut guard = self.state.lock();
        loop {
            if guard.closed {
                return None;
            }
            if let Some(msg) = guard.messages.pop_front() {
                return Some(msg);
            }
            self.cv.wait(&mut guard);
        }
    }

    fn release_written_bytes(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        {
            let mut guard = self.state.lock();
            guard.queued_bytes = guard.queued_bytes.saturating_sub(bytes);
        }
        self.global_pool.release(bytes);
    }

    fn finish_message(&self, remaining_unwritten_bytes: usize) {
        {
            let mut guard = self.state.lock();
            if remaining_unwritten_bytes > 0 {
                guard.queued_bytes = guard.queued_bytes.saturating_sub(remaining_unwritten_bytes);
            }
            guard.queued_messages = guard.queued_messages.saturating_sub(1);
        }
        if remaining_unwritten_bytes > 0 {
            self.global_pool.release(remaining_unwritten_bytes);
        }
    }
}

impl Drop for WriteQueue {
    fn drop(&mut self) {
        self.close();
    }
}

struct InFlightWriteGuard<'a> {
    queue: &'a WriteQueue,
    remaining_bytes: usize,
}

impl<'a> InFlightWriteGuard<'a> {
    const fn new(queue: &'a WriteQueue, total_bytes: usize) -> Self {
        Self {
            queue,
            remaining_bytes: total_bytes,
        }
    }

    fn record_written(&mut self, written: usize) {
        let actual = written.min(self.remaining_bytes);
        self.remaining_bytes -= actual;
        self.queue.release_written_bytes(actual);
    }
}

impl Drop for InFlightWriteGuard<'_> {
    fn drop(&mut self) {
        self.queue.finish_message(self.remaining_bytes);
    }
}

/// Worker dedicado à escrita não bloqueante no descritor mestre do PTY (`master_fd`).
pub struct WriterWorker {
    master_fd: OwnedFd,
    queue: Arc<WriteQueue>,
    write_chunk_bytes: usize,
}

impl WriterWorker {
    /// Cria um novo `WriterWorker` com tamanho de bloco de escrita `write_chunk_bytes` (padrão `4 KiB`).
    #[must_use]
    pub fn new(master_fd: OwnedFd, queue: Arc<WriteQueue>, write_chunk_bytes: usize) -> Self {
        Self {
            master_fd,
            queue,
            write_chunk_bytes: write_chunk_bytes.max(512),
        }
    }

    /// Executa o loop de escrita até que `WriteQueue::close` seja acionado ou o descritor feche.
    pub fn run(self) -> PtyResult<()> {
        let flags = fcntl_getfl(&self.master_fd)
            .map_err(|e| PtyError::Io(std::io::Error::from_raw_os_error(e.raw_os_error())))?;
        fcntl_setfl(&self.master_fd, flags | OFlags::NONBLOCK)
            .map_err(|e| PtyError::Io(std::io::Error::from_raw_os_error(e.raw_os_error())))?;

        while let Some(msg) = self.queue.pop_blocking() {
            let mut in_flight = InFlightWriteGuard::new(&self.queue, msg.len());
            let mut offset = 0usize;

            while offset < msg.len() {
                if self.queue.is_closed() {
                    return Ok(());
                }

                let end = (offset + self.write_chunk_bytes).min(msg.len());
                let slice = &msg[offset..end];

                match rustix::io::write(&self.master_fd, slice) {
                    Ok(0) => {
                        self.queue.close();
                        return Err(PtyError::WriteFailed(
                            "write(2) retornou 0 bytes no descritor do PTY".into(),
                        ));
                    }
                    Ok(n) => {
                        offset += n;
                        in_flight.record_written(n);
                    }
                    Err(Errno::INTR) => {}
                    Err(Errno::AGAIN) => {
                        let mut fds = [
                            PollFd::new(
                                self.master_fd.as_fd(),
                                PollFlags::POLLOUT | PollFlags::POLLERR | PollFlags::POLLHUP,
                            ),
                            PollFd::new(self.queue.wake().as_fd(), PollFlags::POLLIN),
                        ];
                        match poll(&mut fds, PollTimeout::from(100u16)) {
                            Ok(_) | Err(nix::errno::Errno::EINTR) => {}
                            Err(e) => {
                                self.queue.close();
                                return Err(PtyError::WriteFailed(format!(
                                    "Falha em poll(POLLOUT): {e}"
                                )));
                            }
                        }
                        if fds[1]
                            .revents()
                            .is_some_and(|r| r.contains(PollFlags::POLLIN))
                        {
                            self.queue.wake().drain();
                            if self.queue.is_closed() {
                                return Ok(());
                            }
                        }
                        if fds[0]
                            .revents()
                            .is_some_and(|r| r.contains(PollFlags::POLLERR | PollFlags::POLLHUP))
                            && self.queue.is_closed()
                        {
                            return Ok(());
                        }
                    }
                    Err(err) => {
                        self.queue.close();
                        return Err(PtyError::WriteFailed(format!(
                            "Erro de escrita no master PTY: {err}"
                        )));
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rustix::net::{AddressFamily, SocketFlags, SocketType, socketpair};
    use std::time::{Duration, Instant};

    #[test]
    fn enforces_message_size_queue_capacity_and_global_pool_limits() {
        let pool = Arc::new(GlobalBufferPool::new(1000));
        let queue = WriteQueue::new(2, 800, 500, Arc::clone(&pool)).unwrap();

        // Acima de max_write_bytes (500) -> InputInvalid
        let oversized = Bytes::from(vec![b'x'; 501]);
        assert_eq!(
            queue.enqueue(oversized).unwrap_err().code(),
            "pty.input_invalid"
        );
        assert_eq!(pool.used_bytes(), 0);

        // Primeiro pedido: 400 bytes -> Ok
        queue.enqueue(Bytes::from(vec![b'a'; 400])).unwrap();
        assert_eq!(queue.queued_messages(), 1);
        assert_eq!(queue.queued_bytes(), 400);
        assert_eq!(pool.used_bytes(), 400);

        // Segundo pedido que excede max_queue_bytes (400 + 450 > 800) -> WriteQueueFull
        assert_eq!(
            queue
                .enqueue(Bytes::from(vec![b'b'; 450]))
                .unwrap_err()
                .code(),
            "pty.write_queue_full"
        );

        // Segundo pedido de 300 bytes -> Ok (total 700 bytes, 2 mensagens)
        queue.enqueue(Bytes::from(vec![b'b'; 300])).unwrap();
        assert_eq!(queue.queued_messages(), 2);
        assert_eq!(queue.queued_bytes(), 700);
        assert_eq!(pool.used_bytes(), 700);

        // Terceiro pedido excede max_messages (2) -> WriteQueueFull
        assert_eq!(
            queue.enqueue(Bytes::from_static(b"c")).unwrap_err().code(),
            "pty.write_queue_full"
        );

        // Fechar libera imediatamente o orçamento no GlobalBufferPool
        queue.close();
        assert_eq!(queue.queued_messages(), 0);
        assert_eq!(queue.queued_bytes(), 0);
        assert_eq!(pool.used_bytes(), 0);

        // Enqueue após close retorna AlreadyExited
        assert_eq!(
            queue
                .enqueue(Bytes::from_static(b"after"))
                .unwrap_err()
                .code(),
            "pty.already_exited"
        );
    }

    #[test]
    fn chunks_writes_and_cancels_blocked_pollout_immediately_on_close() {
        let (tx_fd, rx_fd) = socketpair(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::CLOEXEC,
            None,
        )
        .unwrap();

        let pool = Arc::new(GlobalBufferPool::new(16 * 1024 * 1024));
        let queue = Arc::new(
            WriteQueue::new(1024, 4 * 1024 * 1024, 1024 * 1024, Arc::clone(&pool)).unwrap(),
        );
        let worker = WriterWorker::new(tx_fd, Arc::clone(&queue), 4096);

        let worker_handle = std::thread::spawn(move || worker.run());

        // Envia 32 KiB e lê do outro lado para validar escrita fatiada em 4 KiB
        let payload: Vec<u8> = (0..(32 * 1024))
            .map(|i| u8::try_from(i % 253).unwrap())
            .collect();
        queue.enqueue(Bytes::from(payload.clone())).unwrap();

        let mut received = vec![0u8; payload.len()];
        let mut read_offset = 0;
        while read_offset < received.len() {
            let n = rustix::io::read(&rx_fd, &mut received[read_offset..]).unwrap();
            assert!(n > 0);
            read_offset += n;
        }
        assert_eq!(received, payload);

        // Agora satura o buffer do kernel sem ler de rx_fd para forçar bloqueio em POLLOUT
        let big_block = Bytes::from(vec![b'Z'; 512 * 1024]);
        for _ in 0..6 {
            let _ = queue.enqueue(big_block.clone());
        }
        std::thread::sleep(Duration::from_millis(20));

        // Cancela a fila: o worker bloqueado em poll(POLLOUT) deve acordar e sair em < 100 ms
        let cancel_start = Instant::now();
        queue.close();
        let res = worker_handle.join().unwrap();
        assert!(res.is_ok());
        assert!(cancel_start.elapsed() < Duration::from_millis(100));
        assert_eq!(pool.used_bytes(), 0);
        assert_eq!(queue.queued_bytes(), 0);
        assert_eq!(queue.queued_messages(), 0);
    }
}
