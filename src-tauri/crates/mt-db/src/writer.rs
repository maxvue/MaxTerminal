//! Thread dedicada de escrita (`DbWriter`) com transações `BEGIN IMMEDIATE`,
//! controle de orçamento de bytes (`ByteBudget`) e sobrevivência a pânicos (`catch_unwind`).

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rusqlite::{Connection, Transaction, TransactionBehavior};
use tokio::sync::{Notify, mpsc, oneshot};

use crate::error::{DbError, DbResult};
use crate::models::{DbEventSink, DbWriterLag};
use crate::time::now_unix_ms;

/// Tamanho máximo permitido para um único job de escrita (1 MiB).
pub const MAX_SINGLE_WRITE_JOB_BYTES: usize = 1024 * 1024;

/// Intervalo mínimo entre emissões consecutivas de `DbWriterLag` (1 segundo).
const LAG_EVENT_MIN_INTERVAL_MS: i64 = 1_000;

/// Controlador compartilhado de orçamento de bytes em memória para filas de escrita e lote.
#[derive(Debug)]
pub struct ByteBudget {
    max_bytes: usize,
    used_bytes: AtomicUsize,
    notify: Notify,
}

impl ByteBudget {
    /// Cria um novo orçamento de bytes com o limite máximo informado.
    #[must_use]
    pub fn new(max_bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            max_bytes: max_bytes.max(MAX_SINGLE_WRITE_JOB_BYTES),
            used_bytes: AtomicUsize::new(0),
            notify: Notify::new(),
        })
    }

    /// Tenta adquirir imediatamente `bytes` do orçamento sem bloquear.
    pub fn try_acquire(self: &Arc<Self>, bytes: usize) -> DbResult<BytePermit> {
        if bytes > self.max_bytes {
            return Err(DbError::ConstraintViolation {
                constraint: String::from("tamanho do job excede o orçamento máximo de escrita"),
            });
        }

        loop {
            let current = self.used_bytes.load(Ordering::Acquire);
            let next = current.saturating_add(bytes);
            if next > self.max_bytes {
                return Err(DbError::Busy {
                    operation: "byte_budget",
                });
            }
            if self
                .used_bytes
                .compare_exchange_weak(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(BytePermit {
                    budget: Arc::clone(self),
                    bytes,
                });
            }
        }
    }

    /// Aguarda de forma assíncrona até `timeout` pela liberação de `bytes` no orçamento.
    pub async fn acquire_timeout(
        self: &Arc<Self>,
        bytes: usize,
        timeout: Duration,
        operation: &'static str,
    ) -> DbResult<BytePermit> {
        if bytes > self.max_bytes {
            return Err(DbError::ConstraintViolation {
                constraint: String::from("tamanho do job excede o orçamento máximo de escrita"),
            });
        }

        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.notify.notified();
            if let Ok(permit) = self.try_acquire(bytes) {
                return Ok(permit);
            }

            let now = Instant::now();
            if now >= deadline {
                return Err(DbError::Busy { operation });
            }

            if tokio::time::timeout(deadline.saturating_duration_since(now), notified)
                .await
                .is_err()
            {
                return self
                    .try_acquire(bytes)
                    .map_err(|_| DbError::Busy { operation });
            }
        }
    }

    /// Retorna a quantidade de bytes atualmente alocados no orçamento.
    #[must_use]
    pub fn used(&self) -> usize {
        self.used_bytes.load(Ordering::Acquire)
    }

    /// Retorna o limite total de bytes deste orçamento.
    #[must_use]
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }
}

/// Permissão RAII que devolve automaticamente os bytes ao `ByteBudget` quando descartada.
#[derive(Debug)]
pub struct BytePermit {
    budget: Arc<ByteBudget>,
    bytes: usize,
}

impl BytePermit {
    /// Quantidade de bytes reservados por este permit.
    #[must_use]
    pub const fn bytes(&self) -> usize {
        self.bytes
    }
}

impl Drop for BytePermit {
    fn drop(&mut self) {
        if self.bytes > 0 {
            self.budget
                .used_bytes
                .fetch_sub(self.bytes, Ordering::AcqRel);
            self.budget.notify.notify_waiters();
        }
    }
}

/// Fotografia das métricas atuais da fila de escrita.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterMetricsSnapshot {
    /// Bytes atualmente alocados no `ByteBudget`.
    pub pending_bytes: u32,
    /// Registros pendentes (jobs na fila + itens no `BatchWriter`).
    pub pending_records: i64,
    /// Total acumulado de eventos de contenção/lag do writer.
    pub writer_lag_events: i64,
}

type BoxedAnyResult = DbResult<Box<dyn Any + Send>>;
type TxClosure = Box<dyn FnOnce(&Transaction<'_>) -> BoxedAnyResult + Send>;
type ConnClosure = Box<dyn FnOnce(&mut Connection) -> BoxedAnyResult + Send>;

/// Unidade de trabalho enviada para a thread exclusiva `mt-db-writer`.
pub enum WriteJob {
    /// Executa closure dentro de uma transação `BEGIN IMMEDIATE`.
    Tx {
        /// Operação semântica para diagnóstico.
        operation: &'static str,
        /// Permissão de orçamento de bytes mantida até o término da transação.
        permit: BytePermit,
        /// Função executada sobre a transação.
        work: TxClosure,
        /// Canal de resposta.
        reply: oneshot::Sender<BoxedAnyResult>,
    },
    /// Executa closure diretamente na `Connection` (usado para checkpoints, `VACUUM` e `optimize`).
    Conn {
        /// Operação semântica para diagnóstico.
        operation: &'static str,
        /// Permissão de orçamento de bytes.
        permit: BytePermit,
        /// Função executada sobre a conexão mutável.
        work: ConnClosure,
        /// Canal de resposta.
        reply: oneshot::Sender<BoxedAnyResult>,
    },
    /// Solicita o encerramento gracioso da thread após drenar jobs anteriores.
    Shutdown {
        /// Confirmação de término.
        reply: oneshot::Sender<()>,
    },
}

/// Escritor single-threaded dono da conexão de escrita SQLite.
pub struct DbWriter {
    tx: mpsc::Sender<WriteJob>,
    budget: Arc<ByteBudget>,
    queued_jobs: Arc<AtomicI64>,
    lag_events: Arc<AtomicI64>,
    last_lag_emit_ms: Arc<AtomicI64>,
    closed: Arc<AtomicBool>,
    event_sink: Arc<dyn DbEventSink>,
    join_handle: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for DbWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbWriter")
            .field("used_bytes", &self.budget.used())
            .field("queued_jobs", &self.queued_jobs.load(Ordering::Relaxed))
            .field("closed", &self.closed.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

impl DbWriter {
    /// Inicia a thread dedicada `mt-db-writer` assumindo a propriedade da `Connection`.
    pub fn spawn(
        mut conn: Connection,
        queue_capacity: usize,
        max_queue_bytes: usize,
        event_sink: Arc<dyn DbEventSink>,
    ) -> DbResult<Self> {
        let capacity = queue_capacity.max(1);
        let (tx, mut rx) = mpsc::channel::<WriteJob>(capacity);
        let budget = ByteBudget::new(max_queue_bytes);
        let queued_jobs = Arc::new(AtomicI64::new(0));
        let queued_for_thread = Arc::clone(&queued_jobs);
        let closed = Arc::new(AtomicBool::new(false));
        let closed_for_thread = Arc::clone(&closed);

        let join_handle = thread::Builder::new()
            .name(String::from("mt-db-writer"))
            .spawn(move || {
                while let Some(job) = rx.blocking_recv() {
                    match job {
                        WriteJob::Tx {
                            operation,
                            permit,
                            work,
                            reply,
                        } => {
                            let _permit = permit;
                            queued_for_thread.fetch_sub(1, Ordering::AcqRel);
                            let outcome = run_tx_catching_panic(&mut conn, operation, work);
                            let _ = reply.send(outcome);
                        }
                        WriteJob::Conn {
                            operation: _,
                            permit,
                            work,
                            reply,
                        } => {
                            let _permit = permit;
                            queued_for_thread.fetch_sub(1, Ordering::AcqRel);
                            let outcome = run_conn_catching_panic(&mut conn, work);
                            let _ = reply.send(outcome);
                        }
                        WriteJob::Shutdown { reply } => {
                            closed_for_thread.store(true, Ordering::Release);
                            let _ = reply.send(());
                            break;
                        }
                    }
                }
                closed_for_thread.store(true, Ordering::Release);
            })
            .map_err(DbError::Io)?;

        Ok(Self {
            tx,
            budget,
            queued_jobs,
            lag_events: Arc::new(AtomicI64::new(0)),
            last_lag_emit_ms: Arc::new(AtomicI64::new(0)),
            closed,
            event_sink,
            join_handle: Mutex::new(Some(join_handle)),
        })
    }

    /// Executa `f` dentro de uma transação `BEGIN IMMEDIATE` na thread escritora.
    pub async fn execute_tx<T, F>(
        &self,
        operation: &'static str,
        approx_bytes: usize,
        f: F,
    ) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> DbResult<T> + Send + 'static,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(DbError::WriterClosed);
        }
        if approx_bytes > MAX_SINGLE_WRITE_JOB_BYTES {
            return Err(DbError::ConstraintViolation {
                constraint: String::from("job de escrita individual excede o limite de 1 MiB"),
            });
        }

        let wait_started = Instant::now();
        let permit = match self
            .budget
            .acquire_timeout(approx_bytes, Duration::from_millis(250), operation)
            .await
        {
            Ok(p) => p,
            Err(err) => {
                self.record_lag_event(
                    self.queued_jobs.load(Ordering::Relaxed),
                    i64::try_from(wait_started.elapsed().as_millis()).unwrap_or(250),
                );
                return Err(err);
            }
        };

        let (reply_tx, reply_rx) = oneshot::channel();
        let job = WriteJob::Tx {
            operation,
            permit,
            work: Box::new(move |tx| f(tx).map(|val| Box::new(val) as Box<dyn Any + Send>)),
            reply: reply_tx,
        };

        let send_res = tokio::time::timeout(Duration::from_millis(250), self.tx.send(job)).await;
        match send_res {
            Ok(Ok(())) => {
                self.queued_jobs.fetch_add(1, Ordering::AcqRel);
            }
            Ok(Err(_)) => return Err(DbError::WriterClosed),
            Err(_) => {
                self.record_lag_event(
                    self.queued_jobs.load(Ordering::Relaxed),
                    i64::try_from(wait_started.elapsed().as_millis()).unwrap_or(250),
                );
                return Err(DbError::Busy { operation });
            }
        }

        let boxed = reply_rx.await.map_err(|_| DbError::WriterClosed)??;
        boxed
            .downcast::<T>()
            .map(|b| *b)
            .map_err(|_| DbError::Busy {
                operation: "downcast_error",
            })
    }

    /// Envia uma transação a partir de um contexto síncrono/bloqueante (ex.: `BatchWriter`) usando um `BytePermit` já adquirido.
    pub fn execute_tx_with_permit_blocking<T, F>(
        &self,
        operation: &'static str,
        permit: BytePermit,
        timeout: Duration,
        f: F,
    ) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> DbResult<T> + Send + 'static,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(DbError::WriterClosed);
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        let mut job_opt = Some(WriteJob::Tx {
            operation,
            permit,
            work: Box::new(move |tx| f(tx).map(|val| Box::new(val) as Box<dyn Any + Send>)),
            reply: reply_tx,
        });

        let deadline = Instant::now() + timeout;
        loop {
            let job = job_opt.take().ok_or(DbError::WriterClosed)?;
            match self.tx.try_send(job) {
                Ok(()) => {
                    self.queued_jobs.fetch_add(1, Ordering::AcqRel);
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(DbError::WriterClosed),
                Err(mpsc::error::TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        self.record_lag_event(
                            self.queued_jobs.load(Ordering::Relaxed),
                            i64::try_from(timeout.as_millis()).unwrap_or(250),
                        );
                        return Err(DbError::Busy { operation });
                    }
                    job_opt = Some(returned);
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }

        let recv_res = if tokio::runtime::Handle::try_current().is_ok() {
            thread::scope(|s| {
                s.spawn(|| reply_rx.blocking_recv())
                    .join()
                    .map_err(|_| DbError::WriterClosed)?
                    .map_err(|_| DbError::WriterClosed)
            })
        } else {
            reply_rx.blocking_recv().map_err(|_| DbError::WriterClosed)
        };
        let boxed = recv_res??;
        boxed
            .downcast::<T>()
            .map(|b| *b)
            .map_err(|_| DbError::Busy {
                operation: "downcast_error",
            })
    }

    /// Executa uma operação direta sobre `&mut Connection` na thread escritora.
    pub async fn execute_conn<T, F>(&self, operation: &'static str, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> DbResult<T> + Send + 'static,
    {
        if self.closed.load(Ordering::Acquire) {
            return Err(DbError::WriterClosed);
        }

        let permit = self
            .budget
            .acquire_timeout(0, Duration::from_secs(2), operation)
            .await?;

        let (reply_tx, reply_rx) = oneshot::channel();
        let job = WriteJob::Conn {
            operation,
            permit,
            work: Box::new(move |conn| f(conn).map(|val| Box::new(val) as Box<dyn Any + Send>)),
            reply: reply_tx,
        };

        self.tx.send(job).await.map_err(|_| DbError::WriterClosed)?;
        self.queued_jobs.fetch_add(1, Ordering::AcqRel);

        let boxed = reply_rx.await.map_err(|_| DbError::WriterClosed)??;
        boxed
            .downcast::<T>()
            .map(|b| *b)
            .map_err(|_| DbError::Busy {
                operation: "downcast_error",
            })
    }

    /// Registra ocorrência de saturação/lag e emite `DbWriterLag` limitado a 1 evento por segundo.
    pub fn record_lag_event(&self, extra_pending_records: i64, oldest_age_ms: i64) {
        self.lag_events.fetch_add(1, Ordering::AcqRel);
        let now = now_unix_ms();
        let prev = self.last_lag_emit_ms.load(Ordering::Acquire);
        if now.saturating_sub(prev) >= LAG_EVENT_MIN_INTERVAL_MS
            && self
                .last_lag_emit_ms
                .compare_exchange(prev, now, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            #[allow(clippy::cast_possible_truncation)]
            let pending_bytes = self.budget.used().min(u32::MAX as usize) as u32;
            let pending_records = self
                .queued_jobs
                .load(Ordering::Relaxed)
                .saturating_add(extra_pending_records);

            self.event_sink.on_writer_lag(DbWriterLag {
                pending_bytes,
                pending_records,
                oldest_age_ms: oldest_age_ms.max(0),
            });
        }
    }

    /// Retorna referência ao `ByteBudget` compartilhado.
    #[must_use]
    pub fn budget(&self) -> &Arc<ByteBudget> {
        &self.budget
    }

    /// Captura um snapshot instantâneo das métricas do writer.
    #[must_use]
    pub fn metrics(&self, extra_pending_records: i64) -> WriterMetricsSnapshot {
        #[allow(clippy::cast_possible_truncation)]
        let pending_bytes = self.budget.used().min(u32::MAX as usize) as u32;
        let pending_records = self
            .queued_jobs
            .load(Ordering::Relaxed)
            .max(0)
            .saturating_add(extra_pending_records.max(0));

        WriterMetricsSnapshot {
            pending_bytes,
            pending_records,
            writer_lag_events: self.lag_events.load(Ordering::Relaxed),
        }
    }

    /// Drena a fila de escrita e aguarda o encerramento da thread `mt-db-writer`.
    pub async fn shutdown(&self, timeout: Duration) -> DbResult<()> {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let (reply_tx, reply_rx) = oneshot::channel();
            if tokio::time::timeout(
                timeout,
                self.tx.send(WriteJob::Shutdown { reply: reply_tx }),
            )
            .await
            .is_ok()
            {
                let _ = tokio::time::timeout(timeout, reply_rx).await;
            }
        }

        let join_handle = self.join_handle.lock().take();
        if let Some(handle) = join_handle {
            let _ = tokio::task::spawn_blocking(move || {
                let _ = handle.join();
            })
            .await;
        }
        Ok(())
    }
}

fn run_tx_catching_panic(
    conn: &mut Connection,
    operation: &'static str,
    work: TxClosure,
) -> BoxedAnyResult {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|e| DbError::from_rusqlite(e, operation))?;
        let val = work(&tx)?;
        tx.commit()
            .map_err(|e| DbError::from_rusqlite(e, operation))?;
        Ok(val)
    }));

    match result {
        Ok(res) => res,
        Err(_panic_payload) => Err(DbError::ConstraintViolation {
            constraint: format!("pânico capturado na transação '{operation}'; rollback aplicado"),
        }),
    }
}

fn run_conn_catching_panic(conn: &mut Connection, work: ConnClosure) -> BoxedAnyResult {
    let result = catch_unwind(AssertUnwindSafe(|| work(conn)));
    match result {
        Ok(res) => res,
        Err(_panic_payload) => Err(DbError::ConstraintViolation {
            constraint: String::from("pânico capturado na operação de conexão do writer"),
        }),
    }
}
