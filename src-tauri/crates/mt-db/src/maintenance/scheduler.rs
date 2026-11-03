//! Agendador e executor de manutenção periódica (`MaintenanceHandle` e `spawn_maintenance_scheduler`).

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::json;
use tokio::sync::Notify;

use crate::Db;
use crate::connection::wal_path_for;
use crate::error::{DbError, DbResult};
use crate::maintenance::backup::{BackupKind, create_backup_blocking, list_backups};
use crate::models::{BackupEntryDto, DbMaintenanceFinished};
use crate::time::now_unix_ms;

/// Intervalo mínimo (20 horas) antes de gerar um novo backup diário automático.
const DAILY_BACKUP_MIN_AGE_MS: i64 = 20 * 3_600 * 1_000;

/// Handle de controle e execução sob demanda do subsistema de manutenção do `Db`.
#[derive(Clone, Debug)]
pub struct MaintenanceHandle {
    db: Db,
    stop_flag: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
}

impl MaintenanceHandle {
    /// Cria um novo `MaintenanceHandle` associado à fachada `Db`.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self {
            db,
            stop_flag: Arc::new(AtomicBool::new(false)),
            stop_notify: Arc::new(Notify::new()),
        }
    }

    /// Sinaliza interrupção para a tarefa periódica em background, se ativa.
    pub fn stop(&self) {
        self.stop_flag.store(true, Ordering::Release);
        self.stop_notify.notify_waiters();
    }

    /// Cria um backup online imediato (`Daily`, `Manual`, `PreMigration`, etc.) em `spawn_blocking`.
    pub async fn create_backup(&self, kind: BackupKind) -> DbResult<BackupEntryDto> {
        self.db.require_ready()?;
        let settings = self.db.settings()?;
        let daily_keep_i64 = settings
            .get_typed::<i64>("db.retention.backup_daily_keep")?
            .unwrap_or(7)
            .clamp(1, 60);
        let daily_keep = usize::try_from(daily_keep_i64).unwrap_or(7);

        let db_path = self.db.config().db_path.clone();
        let backup_dir = self.db.config().backup_dir.clone();

        tokio::task::spawn_blocking(move || {
            create_backup_blocking(&db_path, &backup_dir, kind, daily_keep)
        })
        .await
        .map_err(|_| DbError::BackupFailed {
            reason: String::from("A tarefa de backup online foi interrompida."),
        })?
    }

    /// Executa um ciclo completo de manutenção:
    /// 1. Aplica todas as `RetentionRule` registradas em lotes limitados.
    /// 2. Executa `optimize` nas tabelas FTS5 registradas e invalida o `SearchSnapshotCache` se houve deleções.
    /// 3. Executa `PRAGMA optimize;`, `PRAGMA wal_checkpoint(PASSIVE);` e `PRAGMA incremental_vacuum(2000);`.
    /// 4. Verifica o tamanho em disco (`.db` + `-wal`) contra `db.retention.max_db_size_mb`.
    /// 5. Cria backup diário automático caso o último backup diário tenha mais de 20 h.
    /// 6. Atualiza `db.maintenance.last_run_at` e emite `DbMaintenanceFinished`.
    #[allow(clippy::too_many_lines)]
    pub async fn run_now(&self) -> DbResult<DbMaintenanceFinished> {
        self.db.require_ready()?;
        let started = Instant::now();
        let settings = self.db.settings()?;

        let mut rows_deleted: i64 = 0;
        let rules = self.db.retention_registry().rules().to_vec();
        let now_ms = now_unix_ms();

        for rule in rules {
            let days_opt = match settings.get_typed::<i64>(rule.setting_key) {
                Ok(Some(days)) => Some(days),
                Ok(None) | Err(DbError::SettingUnknownKey { .. }) => rule.default_days,
                Err(err) => return Err(err),
            };

            let Some(days) = days_opt.filter(|&d| d > 0) else {
                continue;
            };

            let cutoff_ms = now_ms.saturating_sub(days.saturating_mul(86_400_000));
            let sql = rule.delete_batch_sql();
            let batch_limit = i64::try_from(rule.batch_size.max(1)).unwrap_or(500);
            let table_name = rule.table;

            loop {
                let sql_clone = sql.clone();
                let deleted_in_batch = self
                    .db
                    .write(move |tx| {
                        let table_exists: i64 = tx
                            .query_row(
                                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1);",
                                [table_name],
                                |r| r.get(0),
                            )
                            .map_err(|e| DbError::from_rusqlite(e, "retention_table_check"))?;
                        if table_exists == 0 {
                            return Ok(0usize);
                        }
                        tx.execute(&sql_clone, rusqlite::params![cutoff_ms, batch_limit])
                            .map_err(|e| DbError::from_rusqlite(e, "retention_delete_batch"))
                    })
                    .await?;

                let batch_i64 = i64::try_from(deleted_in_batch).unwrap_or(0);
                rows_deleted = rows_deleted.saturating_add(batch_i64);

                if deleted_in_batch == 0 || batch_i64 < batch_limit {
                    break;
                }
                // Cede brevemente a fila de escrita entre lotes para não bloquear operações da UI
                tokio::task::yield_now().await;
            }
        }

        let fts_reg = self.db.fts_registry().clone();
        let (wal_pages_checkpointed, vacuum_ran) = self
            .db
            .execute_writer_conn("maintenance_pragmas", move |conn| {
                fts_reg.optimize_all(conn)?;
                conn.execute_batch("PRAGMA optimize;")
                    .map_err(|e| DbError::from_rusqlite(e, "maintenance_pragma_optimize"))?;

                let (_busy, _log, checkpointed): (i64, i64, i64) = conn
                    .query_row("PRAGMA wal_checkpoint(PASSIVE);", [], |r| {
                        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                    })
                    .unwrap_or((0, 0, 0));

                let freelist_count: i64 = conn
                    .query_row("PRAGMA freelist_count;", [], |r| r.get(0))
                    .unwrap_or(0);
                let did_vacuum = if freelist_count > 0 {
                    conn.execute_batch("PRAGMA incremental_vacuum(2000);")
                        .map_err(|e| DbError::from_rusqlite(e, "maintenance_incremental_vacuum"))?;
                    true
                } else {
                    false
                };

                Ok((checkpointed.max(0), did_vacuum))
            })
            .await?;

        // Verifica quota máxima do banco em disco
        let max_mb = settings
            .get_typed::<i64>("db.retention.max_db_size_mb")?
            .unwrap_or(2048)
            .max(64);
        let db_bytes = file_len(&self.db.config().db_path)
            + file_len(&wal_path_for(&self.db.config().db_path));
        let max_bytes = u64::try_from(max_mb).unwrap_or(2048) * 1024 * 1024;
        if db_bytes > max_bytes {
            tracing::warn!(
                target: "mt_db",
                db_bytes,
                max_bytes,
                "O tamanho do banco de dados SQLite excedeu o limite configurado em db.retention.max_db_size_mb"
            );
        }

        // Cria backup diário se o último tiver mais de 20 horas
        let existing_backups = list_backups(&self.db.config().backup_dir).unwrap_or_default();
        let latest_daily_at = existing_backups
            .iter()
            .find(|b| b.kind == "daily")
            .map_or(0, |b| b.created_at);
        if now_ms.saturating_sub(latest_daily_at) >= DAILY_BACKUP_MIN_AGE_MS {
            let _ = self.create_backup(BackupKind::Daily).await;
        }

        let completed_at = now_unix_ms();
        let _ = settings
            .set_internal("db.maintenance.last_run_at", json!(completed_at))
            .await?;

        let duration_ms = i64::try_from(started.elapsed().as_millis()).unwrap_or(0);
        let event = DbMaintenanceFinished {
            rows_deleted,
            wal_pages_checkpointed,
            vacuum_ran,
            duration_ms,
        };
        self.db.event_sink().on_maintenance_finished(event.clone());
        Ok(event)
    }
}

/// Inicia a task assíncrona do agendador periódico de manutenção (1ª execução após `initial_delay`).
#[must_use]
pub fn spawn_maintenance_scheduler(db: Db, initial_delay: Duration) -> MaintenanceHandle {
    let handle = MaintenanceHandle::new(db.clone());
    let runner_handle = handle.clone();

    if let Ok(rt) = tokio::runtime::Handle::try_current() {
        rt.spawn(async move {
            let mut next_wait = initial_delay;
            loop {
                let notified = runner_handle.stop_notify.notified();
                let _ = tokio::time::timeout(next_wait, notified).await;
                if runner_handle.stop_flag.load(Ordering::Acquire) {
                    break;
                }

                let _ = runner_handle.run_now().await;

                let hours = db
                    .settings()
                    .ok()
                    .and_then(|s| {
                        s.get_typed::<i64>("db.maintenance.interval_hours")
                            .ok()
                            .flatten()
                    })
                    .unwrap_or(24)
                    .clamp(1, 168);
                next_wait = Duration::from_secs(u64::try_from(hours).unwrap_or(24) * 3600);
            }
        });
    }

    handle
}

fn file_len(path: &std::path::Path) -> u64 {
    fs::metadata(path).map_or(0, |m| m.len())
}
