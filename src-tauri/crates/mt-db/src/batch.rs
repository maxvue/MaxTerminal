//! Escritor em lote (`BatchWriter<R>`) para inserções de alto volume em transação única `BEGIN IMMEDIATE`.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rusqlite::Statement;
use tokio::sync::{mpsc, oneshot};

use crate::error::{DbError, DbResult};
use crate::time::now_unix_ms;
use crate::writer::{ByteBudget, BytePermit, DbWriter};

/// Contrato para tipos de registro persistidos em lote pelo `BatchWriter`.
pub trait BatchRecord: Send + Sync + 'static {
    /// Nome identificador do writer para métricas e telemetria.
    const WRITER_NAME: &'static str = "batch";
    /// Comando SQL `INSERT` parametrizado preparado com `prepare_cached`.
    const INSERT_SQL: &'static str;

    /// Faz o bind dos campos deste registro e executa o `Statement`.
    fn bind(&self, stmt: &mut Statement<'_>) -> rusqlite::Result<usize>;

    /// Estimativa de bytes ocupados em memória por este registro.
    fn approx_bytes(&self) -> usize;
}

/// Política de agregação e limites de fila de um `BatchWriter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchPolicy {
    /// Atraso máximo desde o primeiro item do lote antes de forçar flush (`200 ms`).
    pub max_delay: Duration,
    /// Quantidade máxima de registros acumulados por transação (`512`).
    pub max_records: usize,
    /// Quantidade máxima de bytes acumulados por transação (`1 MiB`).
    pub max_bytes: usize,
    /// Capacidade máxima da fila de entrada do `BatchWriter` (`8192`).
    pub queue_capacity: usize,
    /// Limite máximo de bytes na fila (`16 MiB`).
    pub queue_bytes: usize,
}

impl Default for BatchPolicy {
    fn default() -> Self {
        Self {
            max_delay: Duration::from_millis(200),
            max_records: 512,
            max_bytes: 1024 * 1024,
            queue_capacity: 8_192,
            queue_bytes: 16 * 1024 * 1024,
        }
    }
}

/// Contadores atômicos de operação de um `BatchWriter`.
#[derive(Debug, Default)]
pub struct BatchStats {
    /// Total de registros aceitos por `push()`.
    pub pushed: AtomicU64,
    /// Total de registros efetivamente commitados no SQLite.
    pub flushed_records: AtomicU64,
    /// Total de transações de lote executadas.
    pub flushes: AtomicU64,
    /// Total de chamadas `push()` rejeitadas por backpressure.
    pub rejected: AtomicU64,
    /// Timestamp UNIX em milissegundos do último flush concluído.
    pub last_flush_ms: AtomicI64,
    /// Quantidade de registros atualmente pendentes de commit.
    pub pending_records: AtomicI64,
}

/// Fotografia imutável das estatísticas de um `BatchWriter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchStatsSnapshot {
    /// Total de registros aceitos por `push()`.
    pub pushed: u64,
    /// Total de registros commitados.
    pub flushed_records: u64,
    /// Total de transações de flush realizadas.
    pub flushes: u64,
    /// Total de registros rejeitados por fila/orçamento cheio.
    pub rejected: u64,
    /// Timestamp UNIX em ms do último flush.
    pub last_flush_ms: i64,
    /// Registros pendentes na fila/lote atual.
    pub pending_records: u64,
}

/// Trait objeto-seguro usado pela fachada `Db` para drenar e inspecionar todos os `BatchWriter` ativos.
pub trait BatchSink: Send + Sync {
    /// Força o flush síncrono de todos os registros pendentes dentro de `timeout`.
    fn flush_blocking(&self, timeout: Duration) -> DbResult<u64>;
    /// Retorna a quantidade de registros pendentes neste `BatchWriter`.
    fn pending_records(&self) -> i64;
    /// Encerra a thread agregadora após comitar todos os registros pendentes.
    fn shutdown_blocking(&self, timeout: Duration) -> DbResult<u64>;
}

enum BatchMsg<R> {
    Item {
        record: R,
        bytes: usize,
        permit: BytePermit,
        enqueued_at_ms: i64,
    },
    Barrier {
        reply: oneshot::Sender<DbResult<u64>>,
    },
    Shutdown {
        reply: oneshot::Sender<DbResult<u64>>,
    },
}

struct BatchWorkerControl<R: BatchRecord> {
    tx: mpsc::Sender<BatchMsg<R>>,
    stats: Arc<BatchStats>,
    budget: Arc<ByteBudget>,
    writer: Arc<DbWriter>,
    oldest_enqueued_ms: Arc<AtomicI64>,
    latched_error: Arc<Mutex<Option<String>>>,
    join_handle: Mutex<Option<JoinHandle<()>>>,
}

impl<R: BatchRecord> BatchSink for BatchWorkerControl<R> {
    fn flush_blocking(&self, timeout: Duration) -> DbResult<u64> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let deadline = Instant::now() + timeout;
        let mut msg_opt = Some(BatchMsg::Barrier { reply: reply_tx });

        loop {
            let msg = msg_opt.take().ok_or(DbError::WriterClosed)?;
            match self.tx.try_send(msg) {
                Ok(()) => break,
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(DbError::WriterClosed),
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        return Err(DbError::Busy {
                            operation: R::WRITER_NAME,
                        });
                    }
                    msg_opt = Some(returned);
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }

        reply_rx
            .blocking_recv()
            .map_err(|_| DbError::WriterClosed)?
    }

    fn pending_records(&self) -> i64 {
        self.stats.pending_records.load(Ordering::Acquire).max(0)
    }

    fn shutdown_blocking(&self, timeout: Duration) -> DbResult<u64> {
        let (reply_tx, reply_rx) = oneshot::channel();
        let deadline = Instant::now() + timeout;
        let mut msg_opt = Some(BatchMsg::Shutdown { reply: reply_tx });

        loop {
            let msg = msg_opt.take().ok_or(DbError::WriterClosed)?;
            match self.tx.try_send(msg) {
                Ok(()) | Err(mpsc::error::TrySendError::Closed(_)) => break,
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        return Err(DbError::Busy {
                            operation: R::WRITER_NAME,
                        });
                    }
                    msg_opt = Some(returned);
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }

        let committed = match reply_rx.blocking_recv() {
            Ok(res) => res?,
            Err(_) => self.stats.flushed_records.load(Ordering::Acquire),
        };

        if let Some(handle) = self.join_handle.lock().take() {
            let _ = handle.join();
        }
        Ok(committed)
    }
}

/// Escritor em lote tipado que agrupa múltiplos registros `R` em uma única transação `BEGIN IMMEDIATE`.
pub struct BatchWriter<R: BatchRecord> {
    control: Arc<BatchWorkerControl<R>>,
}

impl<R: BatchRecord> Clone for BatchWriter<R> {
    fn clone(&self) -> Self {
        Self {
            control: Arc::clone(&self.control),
        }
    }
}

impl<R: BatchRecord> std::fmt::Debug for BatchWriter<R> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BatchWriter")
            .field("writer_name", &R::WRITER_NAME)
            .field("stats", &self.stats())
            .finish()
    }
}

impl<R: BatchRecord> BatchWriter<R> {
    /// Cria e inicia a thread agregadora `mt-db-batch-<WRITER_NAME>` acoplada ao `DbWriter`.
    pub fn spawn(writer: Arc<DbWriter>, policy: BatchPolicy) -> DbResult<Self> {
        let capacity = policy.queue_capacity.max(1);
        let (tx, rx) = mpsc::channel::<BatchMsg<R>>(capacity);
        let stats = Arc::new(BatchStats::default());
        let budget = Arc::clone(writer.budget());
        let oldest_enqueued_ms = Arc::new(AtomicI64::new(0));
        let latched_error = Arc::new(Mutex::new(None));

        let stats_for_thread = Arc::clone(&stats);
        let writer_for_thread = Arc::clone(&writer);
        let oldest_for_thread = Arc::clone(&oldest_enqueued_ms);
        let latched_for_thread = Arc::clone(&latched_error);

        let join_handle = thread::Builder::new()
            .name(format!("mt-db-batch-{}", R::WRITER_NAME))
            .spawn(move || {
                run_batch_aggregator::<R>(
                    rx,
                    policy,
                    writer_for_thread,
                    stats_for_thread,
                    oldest_for_thread,
                    latched_for_thread,
                );
            })
            .map_err(DbError::Io)?;

        Ok(Self {
            control: Arc::new(BatchWorkerControl {
                tx,
                stats,
                budget,
                writer,
                oldest_enqueued_ms,
                latched_error,
                join_handle: Mutex::new(Some(join_handle)),
            }),
        })
    }

    /// Enfileira um registro sem bloquear a thread chamadora (`< 1 ms`).
    ///
    /// Caso a fila ou o orçamento de bytes esteja esgotado, incrementa `rejected`,
    /// emite no máximo 1 evento `DbWriterLag` por segundo e retorna `Err(DbError::Busy)`.
    pub fn push(&self, record: R) -> DbResult<()> {
        if let Some(reason) = self.control.latched_error.lock().clone() {
            return Err(DbError::ConstraintViolation { constraint: reason });
        }

        let approx = record.approx_bytes().max(1);
        let now = now_unix_ms();

        let Ok(permit) = self.control.budget.try_acquire(approx) else {
            self.on_push_rejected(now);
            return Err(DbError::Busy {
                operation: R::WRITER_NAME,
            });
        };

        let msg = BatchMsg::Item {
            record,
            bytes: approx,
            permit,
            enqueued_at_ms: now,
        };

        match self.control.tx.try_send(msg) {
            Ok(()) => {
                self.control.stats.pushed.fetch_add(1, Ordering::AcqRel);
                let prev_pending = self
                    .control
                    .stats
                    .pending_records
                    .fetch_add(1, Ordering::AcqRel);
                if prev_pending == 0 {
                    self.control
                        .oldest_enqueued_ms
                        .store(now, Ordering::Release);
                }
                Ok(())
            }
            Err(mpsc::error::TrySendError::Full(_)) => {
                self.on_push_rejected(now);
                Err(DbError::Busy {
                    operation: R::WRITER_NAME,
                })
            }
            Err(mpsc::error::TrySendError::Closed(_)) => Err(DbError::WriterClosed),
        }
    }

    fn on_push_rejected(&self, now_ms: i64) {
        self.control.stats.rejected.fetch_add(1, Ordering::AcqRel);
        let pending = self
            .control
            .stats
            .pending_records
            .load(Ordering::Acquire)
            .max(0);
        let oldest = self.control.oldest_enqueued_ms.load(Ordering::Acquire);
        let age = if oldest > 0 {
            now_ms.saturating_sub(oldest)
        } else {
            0
        };
        self.control.writer.record_lag_event(pending, age);
    }

    /// Insere uma barreira na fila e aguarda até que todos os registros anteriores estejam commitados no SQLite.
    pub async fn flush(&self) -> DbResult<()> {
        if let Some(reason) = self.control.latched_error.lock().clone() {
            return Err(DbError::ConstraintViolation { constraint: reason });
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        self.control
            .tx
            .send(BatchMsg::Barrier { reply: reply_tx })
            .await
            .map_err(|_| DbError::WriterClosed)?;

        let _ = reply_rx.await.map_err(|_| DbError::WriterClosed)??;
        Ok(())
    }

    /// Retorna uma fotografia instantânea das estatísticas deste `BatchWriter`.
    #[must_use]
    pub fn stats(&self) -> BatchStatsSnapshot {
        let pending_raw = self
            .control
            .stats
            .pending_records
            .load(Ordering::Acquire)
            .max(0);
        let pending_records = u64::try_from(pending_raw).unwrap_or(0);

        BatchStatsSnapshot {
            pushed: self.control.stats.pushed.load(Ordering::Acquire),
            flushed_records: self.control.stats.flushed_records.load(Ordering::Acquire),
            flushes: self.control.stats.flushes.load(Ordering::Acquire),
            rejected: self.control.stats.rejected.load(Ordering::Acquire),
            last_flush_ms: self.control.stats.last_flush_ms.load(Ordering::Acquire),
            pending_records,
        }
    }

    /// Retorna um `Arc<dyn BatchSink>` para supervisão no `Db`.
    #[must_use]
    pub fn as_sink(&self) -> Arc<dyn BatchSink> {
        Arc::clone(&self.control) as Arc<dyn BatchSink>
    }
}

#[allow(clippy::too_many_lines, clippy::needless_pass_by_value)]
fn run_batch_aggregator<R: BatchRecord>(
    mut rx: mpsc::Receiver<BatchMsg<R>>,
    policy: BatchPolicy,
    writer: Arc<DbWriter>,
    stats: Arc<BatchStats>,
    oldest_enqueued_ms: Arc<AtomicI64>,
    latched_error: Arc<Mutex<Option<String>>>,
) {
    let max_records = policy.max_records.max(1);
    let max_bytes = policy.max_bytes.max(1024);
    let max_delay = policy.max_delay;

    let mut buffer: Vec<R> = Vec::with_capacity(max_records);
    let mut permits: Vec<BytePermit> = Vec::with_capacity(max_records);
    let mut barriers: Vec<oneshot::Sender<DbResult<u64>>> = Vec::new();
    let mut buffered_bytes: usize = 0;
    let mut batch_started: Option<Instant> = None;

    loop {
        if buffer.is_empty() {
            match rx.blocking_recv() {
                Some(BatchMsg::Item {
                    record,
                    bytes,
                    permit,
                    enqueued_at_ms,
                }) => {
                    oldest_enqueued_ms.store(enqueued_at_ms, Ordering::Release);
                    batch_started = Some(Instant::now());
                    buffered_bytes = bytes;
                    buffer.push(record);
                    permits.push(permit);
                }
                Some(BatchMsg::Barrier { reply }) => {
                    let res = read_latched_or_ok(&latched_error, &stats);
                    let _ = reply.send(res);
                    continue;
                }
                Some(BatchMsg::Shutdown { reply }) => {
                    let res = read_latched_or_ok(&latched_error, &stats);
                    let _ = reply.send(res);
                    break;
                }
                None => break,
            }
        }

        // Drena mensagens imediatamente disponíveis até atingir max_records ou max_bytes
        let mut shutdown_reply: Option<oneshot::Sender<DbResult<u64>>> = None;
        let mut force_flush = buffer.len() >= max_records || buffered_bytes >= max_bytes;

        while !force_flush {
            match rx.try_recv() {
                Ok(BatchMsg::Item {
                    record,
                    bytes,
                    permit,
                    enqueued_at_ms: _,
                }) => {
                    buffered_bytes = buffered_bytes.saturating_add(bytes);
                    buffer.push(record);
                    permits.push(permit);
                    if buffer.len() >= max_records || buffered_bytes >= max_bytes {
                        force_flush = true;
                    }
                }
                Ok(BatchMsg::Barrier { reply }) => {
                    barriers.push(reply);
                    force_flush = true;
                }
                Ok(BatchMsg::Shutdown { reply }) => {
                    shutdown_reply = Some(reply);
                    force_flush = true;
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    if let Some(started) = batch_started {
                        let elapsed = started.elapsed();
                        if elapsed >= max_delay {
                            force_flush = true;
                        } else {
                            let sleep_step = (max_delay.checked_sub(elapsed).unwrap_or_default())
                                .min(Duration::from_millis(5));
                            thread::sleep(sleep_step);
                        }
                    } else {
                        force_flush = true;
                    }
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    force_flush = true;
                }
            }
        }

        if !buffer.is_empty() {
            let records_to_flush = std::mem::replace(&mut buffer, Vec::with_capacity(max_records));
            let permits_to_drop = std::mem::replace(&mut permits, Vec::with_capacity(max_records));
            let count = records_to_flush.len();
            buffered_bytes = 0;
            batch_started = None;

            let flush_res = flush_records_to_writer::<R>(
                &writer,
                records_to_flush,
                permits_to_drop,
                &stats,
                &latched_error,
            );

            let count_i64 = i64::try_from(count).unwrap_or(0);
            let remaining =
                stats.pending_records.fetch_sub(count_i64, Ordering::AcqRel) - count_i64;
            if remaining <= 0 {
                oldest_enqueued_ms.store(0, Ordering::Release);
            }

            for barrier in barriers.drain(..) {
                let reply_val = match &flush_res {
                    Ok(total) => Ok(*total),
                    Err(err) => Err(clone_batch_error(err)),
                };
                let _ = barrier.send(reply_val);
            }

            if let Some(stop_reply) = shutdown_reply {
                let _ = stop_reply.send(flush_res);
                break;
            }
        } else if let Some(stop_reply) = shutdown_reply {
            let res = read_latched_or_ok(&latched_error, &stats);
            let _ = stop_reply.send(res);
            break;
        }
    }
}

fn flush_records_to_writer<R: BatchRecord>(
    writer: &Arc<DbWriter>,
    records: Vec<R>,
    permits: Vec<BytePermit>,
    stats: &Arc<BatchStats>,
    latched_error: &Arc<Mutex<Option<String>>>,
) -> DbResult<u64> {
    let record_count = u64::try_from(records.len()).unwrap_or(0);
    let records_arc = Arc::new(records);
    let mut attempt = 0u32;

    loop {
        attempt += 1;
        let records_ref = Arc::clone(&records_arc);
        let dummy_permit = writer
            .budget()
            .try_acquire(0)
            .unwrap_or_else(|_| unreachable!());

        let tx_res = writer.execute_tx_with_permit_blocking(
            R::WRITER_NAME,
            dummy_permit,
            Duration::from_secs(2),
            move |tx| {
                let mut stmt = tx
                    .prepare_cached(R::INSERT_SQL)
                    .map_err(|e| DbError::from_rusqlite(e, R::WRITER_NAME))?;
                for item in records_ref.iter() {
                    item.bind(&mut stmt)
                        .map_err(|e| DbError::from_rusqlite(e, R::WRITER_NAME))?;
                }
                Ok(())
            },
        );

        match tx_res {
            Ok(()) => {
                drop(permits);
                stats.flushes.fetch_add(1, Ordering::AcqRel);
                let total = stats
                    .flushed_records
                    .fetch_add(record_count, Ordering::AcqRel)
                    + record_count;
                stats.last_flush_ms.store(now_unix_ms(), Ordering::Release);
                return Ok(total);
            }
            Err(DbError::Busy { .. }) if attempt < 10 => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(err) => {
                drop(permits);
                *latched_error.lock() = Some(err.to_string());
                return Err(err);
            }
        }
    }
}

fn read_latched_or_ok(
    latched_error: &Arc<Mutex<Option<String>>>,
    stats: &Arc<BatchStats>,
) -> DbResult<u64> {
    if let Some(reason) = latched_error.lock().clone() {
        Err(DbError::ConstraintViolation { constraint: reason })
    } else {
        Ok(stats.flushed_records.load(Ordering::Acquire))
    }
}

fn clone_batch_error(err: &DbError) -> DbError {
    match err {
        DbError::Busy { operation } => DbError::Busy { operation },
        DbError::WriterClosed => DbError::WriterClosed,
        other => DbError::ConstraintViolation {
            constraint: other.to_string(),
        },
    }
}
