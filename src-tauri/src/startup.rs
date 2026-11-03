//! Inicialização controlada do banco antes de restaurar sessões e iniciar serviços.

use std::sync::Arc;

use mt_db::{
    COMMAND_BLOCKS_FTS_SPEC, COMMAND_BLOCKS_RETENTION_RULE, Db, DbConfig, DbEventSink,
    DbOpenOptions, DbResult, FtsRegistry, MigrationRegistry, RetentionRegistry,
    SESSION_SCROLLBACK_FTS_SPEC, SESSIONS_ARCHIVED_RETENTION_RULE, SettingsRegistry,
};

/// Registra o erro tipado e solicita saída normal, sem devolver `Err` ao setup do Tauri.
pub(crate) fn initialize_database(
    config: DbResult<DbConfig>,
    sink: Arc<dyn DbEventSink>,
    request_exit: impl FnOnce(i32),
) -> Option<Db> {
    let result = config.and_then(|config| {
        let migrations = MigrationRegistry::builtin()?;
        let settings = SettingsRegistry::with_phase03_defaults();
        let mut retention = RetentionRegistry::new();
        retention.register(COMMAND_BLOCKS_RETENTION_RULE)?;
        retention.register(SESSIONS_ARCHIVED_RETENTION_RULE)?;
        let mut fts = FtsRegistry::new();
        fts.register(COMMAND_BLOCKS_FTS_SPEC)?;
        fts.register(SESSION_SCROLLBACK_FTS_SPEC)?;
        fts.register(mt_db::search::projection::SEARCH_METADATA_FTS_SPEC)?;
        Db::open_with(
            config,
            DbOpenOptions {
                event_sink: sink,
                migrations: Some(migrations),
                settings,
                retention,
                fts,
            },
        )
    });
    match result {
        Ok(db) => Some(db),
        Err(err) => {
            tracing::error!(
                target: "maxterminal",
                event = "startup_failed",
                code = err.code().0,
                stage = "startup.db",
                error = %err,
                "Falha na etapa de inicialização"
            );
            request_exit(1);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::sync::{Arc, Mutex};

    use mt_db::{Db, DbConfig, DbError, NoopDbEventSink};
    use tracing_subscriber::layer::SubscriberExt;

    use super::*;
    use crate::logging::{RedactedJsonFormatter, SanitizerLogRedactor};

    #[derive(Clone, Default)]
    struct Buffer(Arc<Mutex<Vec<u8>>>);

    impl Write for Buffer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().write(bytes)
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Buffer {
        type Writer = Self;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    #[test]
    fn occupied_database_requests_exit_without_panicking_or_losing_data() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/startup-tests")
            .join(uuid::Uuid::now_v7().to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = DbConfig::for_path(dir.join("maxterminal.db"));
        let owner = Db::open(cfg.clone()).unwrap();
        owner
            .write_blocking("startup_test", 128, |tx| {
                tx.execute_batch("CREATE TABLE startup_sentinel(value TEXT); INSERT INTO startup_sentinel VALUES ('preservado');")
                    .map_err(|err| DbError::from_rusqlite(err, "startup_test"))
            })
            .unwrap();

        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(RedactedJsonFormatter::new(Arc::new(SanitizerLogRedactor)))
                .with_writer(buffer.clone()),
        );
        let mut exit_code = None;
        let result = tracing::subscriber::with_default(subscriber, || {
            initialize_database(Ok(cfg.clone()), Arc::new(NoopDbEventSink), |code| {
                exit_code = Some(code);
            })
        });
        assert!(result.is_none());
        assert_eq!(exit_code, Some(1));
        let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("db.locked_by_other_instance"));
        assert!(log.contains("startup.db"));
        assert!(!log.contains("backend_panic"));
        let preserved: String = owner
            .read_blocking(|conn| {
                conn.query_row("SELECT value FROM startup_sentinel", [], |row| row.get(0))
                    .map_err(|err| DbError::from_rusqlite(err, "startup_test"))
            })
            .unwrap();
        assert_eq!(preserved, "preservado");
        tauri::async_runtime::block_on(owner.shutdown()).unwrap();
        drop(owner);

        let reopened = initialize_database(Ok(cfg), Arc::new(NoopDbEventSink), |_| {
            panic!("Não deve encerrar com o banco disponível");
        })
        .unwrap();
        assert_eq!(
            reopened.schema_version(),
            MigrationRegistry::builtin().unwrap().max_version()
        );
        tauri::async_runtime::block_on(reopened.shutdown()).unwrap();
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn invalid_configuration_is_logged_with_sanitized_details() {
        let buffer = Buffer::default();
        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .event_format(RedactedJsonFormatter::new(Arc::new(SanitizerLogRedactor)))
                .with_writer(buffer.clone()),
        );
        tracing::subscriber::with_default(subscriber, || {
            let result = initialize_database(
                Err(DbError::OpenFailed {
                    path: "/invalid".into(),
                    reason: "Falha de configuração: Bearer STARTUP_SECRET_SENTINEL_123456".into(),
                }),
                Arc::new(NoopDbEventSink),
                |code| assert_eq!(code, 1),
            );
            assert!(result.is_none());
        });
        let log = String::from_utf8(buffer.0.lock().unwrap().clone()).unwrap();
        assert!(log.contains("db.open_failed"));
        assert!(!log.contains("STARTUP_SECRET_SENTINEL"));
    }
}
