//! Testes de integração de retenção periódica e sincronização FTS5 (`mt-db`).

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use mt_db::{
    Db, DbConfig, DbError, DbEventSink, DbMaintenanceFinished, DbOpenOptions, FtsSpec,
    FtsTokenizer, RetentionRule, time::now_unix_ms,
};
use tempfile::TempDir;

const LOGS_FTS_SPEC: FtsSpec = FtsSpec {
    content_table: "logs_test",
    fts_table: "logs_test_fts",
    columns: &["message"],
    tokenizer: FtsTokenizer::Text,
    prefix: &[2, 3],
};

#[derive(Default)]
struct MaintenanceEventCollector {
    finished: Mutex<Vec<DbMaintenanceFinished>>,
}

impl DbEventSink for MaintenanceEventCollector {
    fn on_maintenance_finished(&self, event: DbMaintenanceFinished) {
        self.finished.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn applies_rules() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let collector = Arc::new(MaintenanceEventCollector::default());

    let mut options = DbOpenOptions {
        event_sink: collector.clone(),
        ..Default::default()
    };
    options.fts.register(LOGS_FTS_SPEC).unwrap();
    options
        .retention
        .register(RetentionRule {
            name: "logs_test_retention",
            table: "logs_test",
            timestamp_column: "created_at",
            setting_key: "db.retention.backup_daily_keep",
            extra_where: Some("pinned = 0"),
            batch_size: 2,
            default_days: Some(7),
        })
        .unwrap();

    let db = Db::open_with(cfg, options).unwrap();

    let now = now_unix_ms();
    let old_ts = now - 30 * 86_400_000;
    let recent_ts = now - 1_000;

    db.write(move |tx| {
        tx.execute_batch(
            "CREATE TABLE logs_test (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 0,
                message TEXT NOT NULL
            ) STRICT;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "create_logs_test"))?;
        tx.execute_batch(&LOGS_FTS_SPEC.ddl())
            .map_err(|e| DbError::from_rusqlite(e, "create_logs_fts"))?;

        // 5 antigos não fixados (devem ser removidos em lotes de 2)
        for idx in 0..5 {
            tx.execute(
                "INSERT INTO logs_test (created_at, pinned, message) VALUES (?1, 0, ?2);",
                rusqlite::params![old_ts - idx, format!("antigo_removivel_{idx}")],
            )
            .map_err(|e| DbError::from_rusqlite(e, "insert_old_removable"))?;
        }

        // 2 antigos fixados (pinned = 1 -> devem permanecer!)
        for idx in 0..2 {
            tx.execute(
                "INSERT INTO logs_test (created_at, pinned, message) VALUES (?1, 1, ?2);",
                rusqlite::params![old_ts - idx, format!("antigo_fixado_{idx}")],
            )
            .map_err(|e| DbError::from_rusqlite(e, "insert_old_pinned"))?;
        }

        // 3 recentes (devem permanecer!)
        for idx in 0..3 {
            tx.execute(
                "INSERT INTO logs_test (created_at, pinned, message) VALUES (?1, 0, ?2);",
                rusqlite::params![recent_ts + idx, format!("recente_{idx}")],
            )
            .map_err(|e| DbError::from_rusqlite(e, "insert_recent"))?;
        }

        Ok(())
    })
    .await
    .unwrap();

    let event = db.maintenance().run_now().await.unwrap();
    assert_eq!(event.rows_deleted, 5);

    let (remaining_total, remaining_fts): (i64, i64) = db
        .read_blocking(|conn| {
            let total: i64 = conn
                .query_row("SELECT COUNT(*) FROM logs_test;", [], |r| r.get(0))
                .unwrap();
            let fts: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM logs_test_fts WHERE logs_test_fts MATCH '\"antigo_removivel\"*';",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            Ok((total, fts))
        })
        .unwrap();

    assert_eq!(remaining_total, 5); // 2 fixados + 3 recentes
    assert_eq!(remaining_fts, 0);

    let last_run: i64 = db
        .settings()
        .unwrap()
        .get_typed("db.maintenance.last_run_at")
        .unwrap()
        .unwrap();
    assert!(last_run > 0);

    assert_eq!(collector.finished.lock().unwrap().len(), 1);

    db.shutdown().await.unwrap();
}
