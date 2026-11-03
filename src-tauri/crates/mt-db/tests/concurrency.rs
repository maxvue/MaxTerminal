//! Testes de integração de concorrência entre `DbWriter` e `ReadPool` (`mt-db`).

#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::time::Duration;

use mt_db::{Db, DbConfig, DbError};
use tempfile::TempDir;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn writer_and_readers() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Arc::new(Db::open(cfg).unwrap());

    db.write(|tx| {
        tx.execute_batch(
            "CREATE TABLE concurrent_items (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                val TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "create_concurrent_items"))?;
        Ok(())
    })
    .await
    .unwrap();

    // Verifica que conexões do ReadPool possuem query_only = ON.
    let write_on_reader = db.read_blocking(|conn| {
        conn.execute(
            "INSERT INTO concurrent_items (val) VALUES ('forbidden');",
            [],
        )
        .map_err(|e| DbError::from_rusqlite(e, "forbidden_insert"))
    });
    assert!(write_on_reader.is_err());

    let db_writer = Arc::clone(&db);
    let writer_handle = tokio::spawn(async move {
        for idx in 0..200_u32 {
            db_writer
                .write(move |tx| {
                    tx.execute(
                        "INSERT INTO concurrent_items (val) VALUES (?1);",
                        [format!("item-{idx}")],
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "insert_item"))?;
                    Ok(())
                })
                .await
                .unwrap();
        }
    });

    let mut reader_handles = Vec::with_capacity(8);
    for _ in 0..8 {
        let db_reader = Arc::clone(&db);
        reader_handles.push(tokio::spawn(async move {
            for _ in 0..100 {
                let count: i64 = db_reader
                    .read_blocking(|conn| {
                        conn.query_row("SELECT COUNT(*) FROM concurrent_items;", [], |r| r.get(0))
                            .map_err(|e| DbError::from_rusqlite(e, "count_items"))
                    })
                    .unwrap();
                assert!((0..=200).contains(&count));
                tokio::task::yield_now().await;
            }
        }));
    }

    writer_handle.await.unwrap();
    for handle in reader_handles {
        handle.await.unwrap();
    }

    let final_count: i64 = db
        .read_blocking(|conn| {
            conn.query_row("SELECT COUNT(*) FROM concurrent_items;", [], |r| r.get(0))
                .map_err(|e| DbError::from_rusqlite(e, "final_count"))
        })
        .unwrap();
    assert_eq!(final_count, 200);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn writer_survives_panic() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    db.write(|tx| {
        tx.execute_batch(
            "CREATE TABLE panic_test (id INTEGER PRIMARY KEY, name TEXT NOT NULL) STRICT;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "create_panic_test"))?;
        Ok(())
    })
    .await
    .unwrap();

    let panic_res: Result<(), DbError> = db
        .write(|tx| {
            tx.execute(
                "INSERT INTO panic_test (id, name) VALUES (1, 'should_rollback');",
                [],
            )
            .map_err(|e| DbError::from_rusqlite(e, "insert_panic"))?;
            panic!("falha intencional dentro do job do writer");
        })
        .await;
    assert!(panic_res.is_err());

    // O writer deve continuar vivo e a linha da closure que entrou em pânico deve ter sofrido rollback.
    db.write(|tx| {
        tx.execute(
            "INSERT INTO panic_test (id, name) VALUES (2, 'survived');",
            [],
        )
        .map_err(|e| DbError::from_rusqlite(e, "insert_survived"))?;
        Ok(())
    })
    .await
    .unwrap();

    let rows: Vec<(i64, String)> = db
        .read_blocking(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, name FROM panic_test ORDER BY id ASC;")
                .unwrap();
            let mapped = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            Ok(mapped)
        })
        .unwrap();

    assert_eq!(rows, vec![(2, "survived".to_owned())]);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn write_timeout_returns_busy() {
    let tmp = TempDir::new().unwrap();
    let mut cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    cfg.write_queue_capacity = 1;
    let db = Db::open(cfg).unwrap();

    let db_slow1 = db.clone();
    let slow_task1 = tokio::spawn(async move {
        db_slow1
            .write(|_tx| {
                std::thread::sleep(Duration::from_millis(380));
                Ok(())
            })
            .await
            .unwrap();
    });

    tokio::time::sleep(Duration::from_millis(20)).await;

    // Enche o único slot da fila mpsc (capacidade 1)
    let db_slow2 = db.clone();
    let slow_task2 = tokio::spawn(async move {
        let _ = db_slow2
            .write(|_tx| {
                std::thread::sleep(Duration::from_millis(50));
                Ok(())
            })
            .await;
    });

    tokio::time::sleep(Duration::from_millis(20)).await;

    let res = db.write(|_tx| Ok(())).await;
    assert!(matches!(res, Err(DbError::Busy { .. })));

    slow_task1.await.unwrap();
    slow_task2.await.unwrap();
    db.shutdown().await.unwrap();
}
