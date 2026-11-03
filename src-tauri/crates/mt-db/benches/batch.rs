//! Benchmark `criterion` para throughput de inserção em lote via `BatchWriter` (`mt-db`).

#![allow(clippy::unwrap_used, missing_docs)]

use std::time::Duration;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use mt_db::{BatchPolicy, BatchRecord, Db, DbConfig, DbError};
use rusqlite::Statement;
use tempfile::TempDir;
use tokio::runtime::Runtime;

#[derive(Clone)]
struct BenchRow {
    seq: u32,
    line: String,
}

impl BatchRecord for BenchRow {
    const WRITER_NAME: &'static str = "bench_batch";
    const INSERT_SQL: &'static str = "INSERT INTO bench_items (seq, line) VALUES (?1, ?2);";

    fn bind(&self, stmt: &mut Statement<'_>) -> rusqlite::Result<usize> {
        stmt.execute(rusqlite::params![self.seq, self.line])
    }

    fn approx_bytes(&self) -> usize {
        self.line.len() + 16
    }
}

fn bench_batch_writer(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));

    let (db, batch_writer) = rt.block_on(async {
        let db = Db::open(cfg).unwrap();
        db.write(|tx| {
            tx.execute_batch(
                "CREATE TABLE bench_items (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    seq INTEGER NOT NULL,
                    line TEXT NOT NULL
                ) STRICT;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "create_bench_items"))?;
            Ok(())
        })
        .await
        .unwrap();

        let bw = db
            .batch_writer::<BenchRow>(BatchPolicy {
                max_delay: Duration::from_millis(25),
                max_records: 500,
                max_bytes: 256 * 1024,
                queue_capacity: 16_384,
                queue_bytes: 16 * 1024 * 1024,
            })
            .unwrap();
        (db, bw)
    });

    let mut group = c.benchmark_group("mt_db_batch_writer");
    group.throughput(Throughput::Elements(1_000));
    group.bench_function("insert_1000_rows_coalesced", |b| {
        b.iter(|| {
            rt.block_on(async {
                for seq in 0..1_000_u32 {
                    batch_writer
                        .push(BenchRow {
                            seq,
                            line: "linha de log sanitizada do terminal para benchmark".to_owned(),
                        })
                        .unwrap();
                }
                batch_writer.flush().await.unwrap();
            });
        });
    });
    group.finish();

    rt.block_on(async {
        db.shutdown().await.unwrap();
    });
}

criterion_group!(benches, bench_batch_writer);
criterion_main!(benches);
