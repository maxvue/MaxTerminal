//! Testes de integração do `BatchWriter` (coalescência, ordem, backpressure e flush no shutdown).

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mt_db::{
    BatchPolicy, BatchRecord, Db, DbConfig, DbError, DbEventSink, DbOpenOptions, DbWriterLag,
};
use rusqlite::Statement;
use tempfile::TempDir;

#[derive(Debug, Clone)]
struct SampleRecord {
    seq: u32,
    payload: String,
}

impl BatchRecord for SampleRecord {
    const WRITER_NAME: &'static str = "sample_batch";
    const INSERT_SQL: &'static str = "INSERT INTO batch_records (seq, payload) VALUES (?1, ?2);";

    fn bind(&self, stmt: &mut Statement<'_>) -> rusqlite::Result<usize> {
        stmt.execute(rusqlite::params![self.seq, self.payload])
    }

    fn approx_bytes(&self) -> usize {
        self.payload.len() + 16
    }
}

#[derive(Default)]
struct LagCollector {
    events: Mutex<Vec<DbWriterLag>>,
}

impl DbEventSink for LagCollector {
    fn on_writer_lag(&self, event: DbWriterLag) {
        self.events.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn coalesces() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    db.write(|tx| {
        tx.execute_batch(
            "CREATE TABLE batch_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                seq INTEGER NOT NULL,
                payload TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "create_batch_records"))?;
        Ok(())
    })
    .await
    .unwrap();

    let batch_writer = db
        .batch_writer::<SampleRecord>(BatchPolicy {
            max_delay: Duration::from_millis(50),
            max_records: 500,
            max_bytes: 512 * 1024,
            queue_capacity: 16_384,
            queue_bytes: 16 * 1024 * 1024,
        })
        .unwrap();

    for seq in 0..10_000_u32 {
        batch_writer
            .push(SampleRecord {
                seq,
                payload: format!("row-{seq}"),
            })
            .unwrap();
    }

    batch_writer.flush().await.unwrap();

    let snap = batch_writer.stats();
    assert_eq!(snap.flushed_records, 10_000);
    assert_eq!(snap.rejected, 0);
    assert!(
        snap.flushes <= 25,
        "esperava <= 25 transações para 10.000 itens, mas realizou {}",
        snap.flushes
    );

    let count: i64 = db
        .read_blocking(|conn| {
            Ok(conn
                .query_row("SELECT COUNT(*) FROM batch_records;", [], |r| r.get(0))
                .unwrap())
        })
        .unwrap();
    assert_eq!(count, 10_000);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn order_preserved_and_explicit_flush_awaits_commit() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    db.write(|tx| {
        tx.execute_batch(
            "CREATE TABLE batch_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                seq INTEGER NOT NULL,
                payload TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "create_batch_records"))?;
        Ok(())
    })
    .await
    .unwrap();

    let batch_writer = db
        .batch_writer::<SampleRecord>(BatchPolicy {
            max_delay: Duration::from_millis(200),
            max_records: 200,
            max_bytes: 256 * 1024,
            queue_capacity: 1024,
            queue_bytes: 16 * 1024 * 1024,
        })
        .unwrap();

    for seq in 0..350_u32 {
        batch_writer
            .push(SampleRecord {
                seq,
                payload: format!("ordered-{seq}"),
            })
            .unwrap();
    }

    batch_writer.flush().await.unwrap();

    let seqs: Vec<u32> = db
        .read_blocking(|conn| {
            let mut stmt = conn
                .prepare("SELECT seq FROM batch_records ORDER BY id ASC;")
                .unwrap();
            let list = stmt
                .query_map([], |r| r.get(0))
                .unwrap()
                .collect::<Result<Vec<u32>, _>>()
                .unwrap();
            Ok(list)
        })
        .unwrap();

    let expected: Vec<u32> = (0..350).collect();
    assert_eq!(seqs, expected);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn backpressure_non_blocking() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let lag_collector = Arc::new(LagCollector::default());

    let options = DbOpenOptions {
        event_sink: lag_collector.clone(),
        ..Default::default()
    };
    let db = Db::open_with(cfg, options).unwrap();

    db.write(|tx| {
        tx.execute_batch(
            "CREATE TABLE batch_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                seq INTEGER NOT NULL,
                payload TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "create_batch_records"))?;
        Ok(())
    })
    .await
    .unwrap();

    // Segura a thread do DbWriter para que a fila curta do BatchWriter fique cheia.
    let db_blocker = db.clone();
    let block_handle = tokio::spawn(async move {
        db_blocker
            .write(|_tx| {
                std::thread::sleep(Duration::from_millis(160));
                Ok(())
            })
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(20)).await;

    let batch_writer = db
        .batch_writer::<SampleRecord>(BatchPolicy {
            max_delay: Duration::from_millis(500),
            max_records: 2,
            max_bytes: 64 * 1024,
            queue_capacity: 2,
            queue_bytes: 64 * 1024,
        })
        .unwrap();

    let start = Instant::now();
    for seq in 0..100_u32 {
        let _ = batch_writer.push(SampleRecord {
            seq,
            payload: "x".repeat(128),
        });
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(25),
        "push nao-bloqueante demorou demais: {elapsed:?}"
    );

    let snap = batch_writer.stats();
    assert!(snap.rejected > 0);

    // Rate-limit: no máximo 1 evento DbWriterLag dentro da janela de 1s.
    {
        let lag_events = lag_collector.events.lock().unwrap();
        assert_eq!(lag_events.len(), 1);
    }

    block_handle.await.unwrap();
    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn flush_on_shutdown() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg.clone()).unwrap();

    db.write(|tx| {
        tx.execute_batch(
            "CREATE TABLE batch_records (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                seq INTEGER NOT NULL,
                payload TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "create_batch_records"))?;
        Ok(())
    })
    .await
    .unwrap();

    let batch_writer = db
        .batch_writer::<SampleRecord>(BatchPolicy {
            max_delay: Duration::from_secs(30),
            max_records: 1000,
            max_bytes: 512 * 1024,
            queue_capacity: 1024,
            queue_bytes: 16 * 1024 * 1024,
        })
        .unwrap();

    for seq in 0..50_u32 {
        batch_writer
            .push(SampleRecord {
                seq,
                payload: format!("shutdown-{seq}"),
            })
            .unwrap();
    }

    // Sem chamar flush(), o db.shutdown() deve drenar todos os 50 itens antes do checkpoint!
    db.shutdown().await.unwrap();

    let reopened = Db::open(cfg).unwrap();
    let count: i64 = reopened
        .read_blocking(|conn| {
            Ok(conn
                .query_row("SELECT COUNT(*) FROM batch_records;", [], |r| r.get(0))
                .unwrap())
        })
        .unwrap();
    assert_eq!(count, 50);
    reopened.shutdown().await.unwrap();
}
