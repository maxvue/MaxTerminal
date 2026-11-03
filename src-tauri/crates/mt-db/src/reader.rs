//! Pool de conexões somente leitura (`ReadPool`) apoiado em `r2d2` e `r2d2_sqlite`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use r2d2::{CustomizeConnection, Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::{Connection, OpenFlags};
use tokio::sync::Notify;

use crate::config::{DbConfig, PragmaSet};
use crate::connection::apply_reader_pragmas;
use crate::error::{DbError, DbResult};

#[derive(Debug, Clone, Copy)]
struct ReaderCustomizer {
    pragmas: PragmaSet,
}

impl CustomizeConnection<Connection, rusqlite::Error> for ReaderCustomizer {
    fn on_acquire(&self, conn: &mut Connection) -> Result<(), rusqlite::Error> {
        apply_reader_pragmas(conn, &self.pragmas)
    }
}

/// Pool concorrente de conexões SQLite somente leitura (`query_only = ON`).
#[derive(Clone, Debug)]
pub struct ReadPool {
    pool: Pool<SqliteConnectionManager>,
    active_readers: Arc<AtomicUsize>,
    idle_notify: Arc<Notify>,
}

struct ActiveReaderGuard<'a> {
    counter: &'a AtomicUsize,
    notify: &'a Notify,
}

impl<'a> ActiveReaderGuard<'a> {
    fn acquire(counter: &'a AtomicUsize, notify: &'a Notify) -> Self {
        counter.fetch_add(1, Ordering::AcqRel);
        Self { counter, notify }
    }
}

impl Drop for ActiveReaderGuard<'_> {
    fn drop(&mut self) {
        if self.counter.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.notify.notify_waiters();
        }
    }
}

impl ReadPool {
    /// Cria um novo pool de conexões somente leitura para o banco configurado.
    pub fn new(cfg: &DbConfig) -> DbResult<Self> {
        let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_URI;

        let manager = SqliteConnectionManager::file(&cfg.db_path).with_flags(flags);

        let pool = Pool::builder()
            .max_size(cfg.reader_pool_size.max(1))
            .connection_timeout(Duration::from_millis(u64::from(
                cfg.pragmas.busy_timeout_ms.max(500),
            )))
            .connection_customizer(Box::new(ReaderCustomizer {
                pragmas: cfg.pragmas,
            }))
            .build(manager)
            .map_err(|err| DbError::OpenFailed {
                path: cfg.db_path.clone(),
                reason: format!("Falha ao inicializar ReadPool r2d2: {err}"),
            })?;

        Ok(Self {
            pool,
            active_readers: Arc::new(AtomicUsize::new(0)),
            idle_notify: Arc::new(Notify::new()),
        })
    }

    /// Obtém uma conexão emprestada do pool.
    pub fn get(&self) -> DbResult<PooledConnection<SqliteConnectionManager>> {
        self.pool.get().map_err(|_| DbError::Busy {
            operation: "read_pool_get",
        })
    }

    /// Executa uma função síncrona sobre uma conexão do `ReadPool`.
    pub fn with_conn<T, F>(&self, f: F) -> DbResult<T>
    where
        F: FnOnce(&Connection) -> DbResult<T>,
    {
        let _guard = ActiveReaderGuard::acquire(&self.active_readers, &self.idle_notify);
        let conn = self.get()?;
        f(&conn)
    }

    /// Executa uma consulta somente leitura em `tokio::task::spawn_blocking`.
    pub async fn read<T, F>(&self, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> DbResult<T> + Send + 'static,
    {
        let pool = self.clone();
        tokio::task::spawn_blocking(move || pool.with_conn(f))
            .await
            .map_err(|_| DbError::Busy {
                operation: "read_spawn_blocking_join",
            })?
    }

    /// Retorna o número de leitores ativos no momento.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active_readers.load(Ordering::Acquire)
    }

    /// Aguarda até que todas as leituras em andamento terminem ou `timeout` expire.
    pub async fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            let notified = self.idle_notify.notified();
            if self.active_readers.load(Ordering::Acquire) == 0 {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return self.active_readers.load(Ordering::Acquire) == 0;
            }
            if tokio::time::timeout(deadline.saturating_duration_since(now), notified)
                .await
                .is_err()
            {
                return self.active_readers.load(Ordering::Acquire) == 0;
            }
        }
    }
}
