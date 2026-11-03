//! Crate `mt-db`: persistência SQLite local em modo WAL, runner de migrations, FTS5,
//! escrita em lote (`BatchWriter`), configurações tipadas (`SettingsRepo`),
//! backup online, retenção e sanitização obrigatória de segredos.

pub mod batch;
pub mod config;
pub mod connection;
pub mod error;
pub mod fts;
pub mod maintenance;
pub mod migrations;
pub mod models;
pub mod reader;
pub mod repos;
pub mod sanitizer;
pub mod search;
pub mod settings;
pub mod time;
pub mod writer;

use std::sync::atomic::AtomicI64;
use std::sync::{Arc, Weak};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use rusqlite::{Connection, Transaction};

pub use batch::{BatchPolicy, BatchRecord, BatchSink, BatchStats, BatchStatsSnapshot, BatchWriter};
pub use config::{DbConfig, PragmaSet};
pub use connection::{
    InstanceLock, OpenedWriter, ensure_secure_dir, open_reader, open_writer, run_quick_check,
    set_file_mode_0600, shm_path_for, verify_sqlite_capabilities, wal_path_for,
};
pub use error::{DbError, DbResult, codes};
pub use fts::{
    COMMAND_BLOCKS_FTS_SPEC, CachedSearchPage, FtsRegistry, FtsSpec, FtsTokenizer, KeysetCursor,
    KeysetPage, SearchSnapshotCache, build_match_query, build_match_query_checked,
    format_safe_snippet,
};
pub use maintenance::{
    BackupKind, MaintenanceHandle, RestoreStage, RetentionRegistry, RetentionRule,
    apply_pending_restore_on_boot, apply_pending_restore_with_injection, create_backup_blocking,
    execute_recovery_action, list_backups, pending_restore_marker_path, quarantine_corrupted_files,
    schedule_restore, schedule_restore_with_dirs, spawn_maintenance_scheduler,
    validate_candidate_backup,
};
pub use migrations::{
    AppliedMigration, CodeMigration, CodeMigrationFn, MigrationEntry, MigrationInfo,
    MigrationOutcome, MigrationRegistry, MigrationRunner, MigrationSource,
    compute_migration_checksum, parse_migration_filename,
};
pub use models::{
    BackupEntryDto, DbEventSink, DbHealthStateDto, DbMaintenanceFinished, DbRecovered,
    DbRecoveryAction, DbStatusDto, DbWriterLag, NoopDbEventSink, RestoreScheduledDto,
    SettingChangeOrigin, SettingSnapshot, SettingsChanged,
};
pub use reader::ReadPool;
pub use repos::{
    COMMAND_BLOCKS_RETENTION_RULE, CONSTRAINT_LAYOUT_CONFLICT, CONSTRAINT_LAYOUT_INVALID,
    CONSTRAINT_PANE_LIMIT_REACHED, CONSTRAINT_PANE_NOT_FOUND, CONSTRAINT_PERSISTENCE_PENDING,
    CONSTRAINT_PROJECT_DUPLICATE_PATH, CONSTRAINT_PROJECT_NOT_FOUND,
    CONSTRAINT_PROJECT_REORDER_MISMATCH, CONSTRAINT_PROJECT_ROOT_DUPLICATE_PATH,
    CONSTRAINT_PROJECT_ROOT_NOT_FOUND, CONSTRAINT_SESSION_NOT_FOUND,
    CONSTRAINT_SESSION_QUERY_INVALID, CONSTRAINT_TAB_NOT_FOUND, CONSTRAINT_TAB_REORDER_MISMATCH,
    CONSTRAINT_TEMPLATE_CONFLICT, CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED,
    CONSTRAINT_TEMPLATE_INVALID, CONSTRAINT_TEMPLATE_NOT_FOUND, CONSTRAINT_TEMPLATE_SECRET_LITERAL,
    CONSTRAINT_WORKSPACE_OPERATION_INTERRUPTED, CONSTRAINT_WORKSPACE_REQUEST_CONFLICT, CatalogRows,
    CommandBlocksRepo, DefaultTemplateContributor, FinishCommandBlock, MAX_SCROLLBACK_PAGE_BYTES,
    NewCommandBlock, NewSession, NewTabTxInput, OperationStage, OsSettingsRepo, OsSettingsRow,
    PanesRepo, PrepareOperationOutcome, ProjectCommit, ProjectMutation, ProjectsRepo,
    SESSION_SCROLLBACK_FTS_SPEC, SESSIONS_ARCHIVED_RETENTION_RULE, ScrollbackRecord,
    ScrollbackRepo, SessionDto, SessionEnd, SessionsRepo, StoredAppearanceRow,
    StoredColorSchemeRow, StoredDismissed, StoredIdentityUpdate, StoredMissingUpdate,
    StoredNewProject, StoredPaneRow, StoredProject, StoredProjectThemeOverrideRow,
    StoredReconcileBatch, StoredRoot, StoredRootEntry, StoredTabRow, TabSessionsRepo, TabsRepo,
    TemplatesRepo, ThemesRepo, WorkspaceOperationRow, WorkspaceOperationsRepo, close_session_tx,
    insert_session_tx,
};
pub use settings::{SettingDescriptor, SettingLookup, SettingsRegistry, SettingsRepo};
pub use writer::{ByteBudget, BytePermit, DbWriter, WriteJob, WriterMetricsSnapshot};

/// Opções de extensão fornecidas ao abrir a fachada `Db`.
#[derive(Clone)]
pub struct DbOpenOptions {
    /// Sink de eventos do banco (`NoopDbEventSink` por padrão).
    pub event_sink: Arc<dyn DbEventSink>,
    /// Catálogo de migrations (usa `MigrationRegistry::builtin()` quando `None`).
    pub migrations: Option<MigrationRegistry>,
    /// Catálogo de configurações (usa `SettingsRegistry::with_phase03_defaults()` por padrão).
    pub settings: SettingsRegistry,
    /// Regras de retenção periódica registradas.
    pub retention: RetentionRegistry,
    /// Especificações FTS5 registradas para manutenção (`optimize` / `integrity-check`).
    pub fts: FtsRegistry,
}

impl Default for DbOpenOptions {
    fn default() -> Self {
        let mut retention = RetentionRegistry::new();
        let _ = retention.register(COMMAND_BLOCKS_RETENTION_RULE);
        let _ = retention.register(SESSIONS_ARCHIVED_RETENTION_RULE);
        let mut fts = FtsRegistry::new();
        let _ = fts.register(COMMAND_BLOCKS_FTS_SPEC);
        let _ = fts.register(SESSION_SCROLLBACK_FTS_SPEC);
        let _ = fts.register(crate::search::projection::SEARCH_METADATA_FTS_SPEC);
        Self {
            event_sink: Arc::new(NoopDbEventSink),
            migrations: None,
            settings: SettingsRegistry::with_phase03_defaults(),
            retention,
            fts,
        }
    }
}

impl std::fmt::Debug for DbOpenOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbOpenOptions")
            .field("has_custom_migrations", &self.migrations.is_some())
            .field("retention_rules", &self.retention.rules().len())
            .field("fts_specs", &self.fts.specs().len())
            .finish_non_exhaustive()
    }
}

/// Estado interno quando o banco está íntegro e operacional.
#[derive(Debug)]
pub struct ReadyDbRuntime {
    writer: Arc<DbWriter>,
    reader: ReadPool,
    schema_version: i64,
    sqlite_version: String,
    applied_migrations: Vec<AppliedMigration>,
    batch_sinks: Mutex<Vec<Weak<dyn BatchSink>>>,
}

/// Estado operacional interno da fachada `Db`.
#[derive(Debug)]
pub enum DbOperationalState {
    /// Banco saudável, migrado e pronto para leituras e escritas.
    Ready(Arc<ReadyDbRuntime>),
    /// Banco bloqueado em modo de recuperação aguardando chamada explícita a `Db::recover`.
    RecoveryRequired {
        /// Código estável do erro que motivou o modo de recuperação.
        code: String,
        /// Mensagem descritiva em `pt-BR`.
        reason: String,
    },
}

struct DbInner {
    config: DbConfig,
    instance_lock: Mutex<Option<InstanceLock>>,
    event_sink: Arc<dyn DbEventSink>,
    migration_registry: MigrationRegistry,
    settings_registry: Arc<SettingsRegistry>,
    retention_registry: Arc<RetentionRegistry>,
    fts_registry: Arc<FtsRegistry>,
    state: RwLock<DbOperationalState>,
    maintenance_scheduler: Mutex<Option<MaintenanceHandle>>,
    total_lag_events: AtomicI64,
}

/// Fachada principal de acesso ao banco de dados SQLite do `MaxTerminal`.
#[derive(Clone)]
pub struct Db {
    inner: Arc<DbInner>,
}

impl std::fmt::Debug for Db {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Db")
            .field("db_path", &self.inner.config.db_path)
            .field("is_ready", &self.is_ready())
            .finish()
    }
}

impl Db {
    /// Abre (ou cria) o banco SQLite aplicando `InstanceLock`, `PRAGMA`s WAL, `quick_check` e migrations.
    ///
    /// - Se outra instância já detiver o lock exclusivo, retorna `Err(DbError::LockedByOtherInstance)`.
    /// - Se uma migration falhar, tiver checksum divergente ou pertencer a versão futura da aplicação,
    ///   retorna o erro tipado correspondente.
    /// - Se o arquivo SQLite estiver estruturalmente corrompido (`DbError::Corrupted`), preserva os arquivos
    ///   intactos no disco e inicializa a fachada em `DbOperationalState::RecoveryRequired`.
    pub fn open(config: DbConfig) -> DbResult<Self> {
        Self::open_with(config, DbOpenOptions::default())
    }

    /// Abre a fachada `Db` com opções customizadas de eventos, migrations, settings, retenção e FTS5.
    #[allow(clippy::needless_pass_by_value)]
    pub fn open_with(config: DbConfig, options: DbOpenOptions) -> DbResult<Self> {
        let instance_lock = InstanceLock::acquire(&config.lock_path)?;
        let migration_registry = match options.migrations {
            Some(reg) => reg,
            None => MigrationRegistry::builtin()?,
        };
        let settings_registry = Arc::new(options.settings);
        let retention_registry = Arc::new(options.retention);
        let fts_registry = Arc::new(options.fts);

        let initial_state = match open_healthy_runtime(
            &config,
            &migration_registry,
            Arc::clone(&options.event_sink),
        ) {
            Ok(runtime) => DbOperationalState::Ready(Arc::new(runtime)),
            Err(DbError::Corrupted { detail, .. }) => DbOperationalState::RecoveryRequired {
                code: codes::CORRUPTED.0.to_string(),
                reason: format!(
                    "O banco de dados local apresenta inconsistências estruturais ({detail})."
                ),
            },
            Err(other_err) => return Err(other_err),
        };

        let db = Self {
            inner: Arc::new(DbInner {
                config: config.clone(),
                instance_lock: Mutex::new(Some(instance_lock)),
                event_sink: options.event_sink,
                migration_registry,
                settings_registry,
                retention_registry,
                fts_registry,
                state: RwLock::new(initial_state),
                maintenance_scheduler: Mutex::new(None),
                total_lag_events: AtomicI64::new(0),
            }),
        };

        if config.enable_background_maintenance && db.is_ready() {
            let sched = spawn_maintenance_scheduler(db.clone(), Duration::from_secs(60));
            *db.inner.maintenance_scheduler.lock() = Some(sched);
        }

        Ok(db)
    }

    /// Variante de `open` que retorna `Err(DbError::RecoveryRequired)` imediatamente se o banco entrar em recuperação.
    pub fn open_strict(config: DbConfig) -> DbResult<Self> {
        let db = Self::open(config)?;
        db.require_ready()?;
        Ok(db)
    }

    /// Retorna `true` se o banco estiver no estado operacional `Ready`.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(*self.inner.state.read(), DbOperationalState::Ready(_))
    }

    /// Retorna `true` se o banco estiver bloqueado em `RecoveryRequired`.
    #[must_use]
    pub fn is_recovery_required(&self) -> bool {
        matches!(
            *self.inner.state.read(),
            DbOperationalState::RecoveryRequired { .. }
        )
    }

    /// Garante que o banco está `Ready`, retornando `Err(DbError::RecoveryRequired)` caso contrário.
    pub fn require_ready(&self) -> DbResult<()> {
        self.ready_runtime().map(|_| ())
    }

    fn ready_runtime(&self) -> DbResult<Arc<ReadyDbRuntime>> {
        match &*self.inner.state.read() {
            DbOperationalState::Ready(rt) => Ok(Arc::clone(rt)),
            DbOperationalState::RecoveryRequired { reason, .. } => Err(DbError::RecoveryRequired {
                reason: reason.clone(),
            }),
        }
    }

    /// Retorna a configuração ativa do banco.
    #[must_use]
    pub fn config(&self) -> &DbConfig {
        &self.inner.config
    }

    /// Retorna o sink de eventos configurado.
    #[must_use]
    pub fn event_sink(&self) -> &Arc<dyn DbEventSink> {
        &self.inner.event_sink
    }

    /// Retorna o registro de especificações FTS5.
    #[must_use]
    pub fn fts_registry(&self) -> &Arc<FtsRegistry> {
        &self.inner.fts_registry
    }

    /// Retorna o registro de regras de retenção.
    #[must_use]
    pub fn retention_registry(&self) -> &Arc<RetentionRegistry> {
        &self.inner.retention_registry
    }

    /// Retorna a versão atual do schema (`0` se estiver em `RecoveryRequired`).
    #[must_use]
    pub fn schema_version(&self) -> i64 {
        match &*self.inner.state.read() {
            DbOperationalState::Ready(rt) => rt.schema_version,
            DbOperationalState::RecoveryRequired { .. } => 0,
        }
    }

    /// Retorna a lista de migrations que foram aplicadas na abertura atual.
    #[must_use]
    pub fn applied_migrations(&self) -> Vec<AppliedMigration> {
        match &*self.inner.state.read() {
            DbOperationalState::Ready(rt) => rt.applied_migrations.clone(),
            DbOperationalState::RecoveryRequired { .. } => Vec::new(),
        }
    }

    /// Retorna o repositório tipado de configurações (`SettingsRepo`).
    pub fn settings(&self) -> DbResult<SettingsRepo> {
        let rt = self.ready_runtime()?;
        Ok(SettingsRepo::new(
            Arc::clone(&self.inner.settings_registry),
            rt.reader.clone(),
            Arc::clone(&rt.writer),
            Arc::clone(&self.inner.event_sink),
        ))
    }

    /// Retorna o repositório tipado de sessões (`SessionsRepo`).
    #[must_use]
    pub fn sessions(&self) -> SessionsRepo {
        SessionsRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de blocos de comando (`CommandBlocksRepo`).
    #[must_use]
    pub fn command_blocks(&self) -> CommandBlocksRepo {
        CommandBlocksRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de projetos e pastas de projetos (`ProjectsRepo`).
    #[must_use]
    pub fn projects(&self) -> ProjectsRepo {
        ProjectsRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de templates de aba (`TemplatesRepo`).
    #[must_use]
    pub fn templates(&self) -> TemplatesRepo {
        TemplatesRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de abas (`TabsRepo`).
    #[must_use]
    pub fn tabs(&self) -> TabsRepo {
        TabsRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de painéis (`PanesRepo`).
    #[must_use]
    pub fn panes(&self) -> PanesRepo {
        PanesRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de sessões de aba (`TabSessionsRepo`).
    #[must_use]
    pub fn tab_sessions(&self) -> TabSessionsRepo {
        TabSessionsRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de scrollback (`ScrollbackRepo`).
    #[must_use]
    pub fn scrollback(&self) -> ScrollbackRepo {
        ScrollbackRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de operações do workspace (`WorkspaceOperationsRepo`).
    #[must_use]
    pub fn workspace_operations(&self) -> WorkspaceOperationsRepo {
        WorkspaceOperationsRepo::new(self.clone())
    }

    /// Retorna o repositório tipado das configurações de integração com o SO (`OsSettingsRepo`).
    #[must_use]
    pub fn os_settings(&self) -> OsSettingsRepo {
        OsSettingsRepo::new(self.clone())
    }

    /// Retorna o repositório tipado de esquemas de cores e configurações de aparência (`ThemesRepo`).
    #[must_use]
    pub fn themes(&self) -> ThemesRepo {
        ThemesRepo::new(self.clone())
    }

    /// Retorna o `MaintenanceHandle` associado a este banco.
    #[must_use]
    pub fn maintenance(&self) -> MaintenanceHandle {
        MaintenanceHandle::new(self.clone())
    }

    /// Cria e registra um `BatchWriter<R>` supervisionado para flush automático no shutdown.
    pub fn batch_writer<R: BatchRecord>(&self, policy: BatchPolicy) -> DbResult<BatchWriter<R>> {
        let rt = self.ready_runtime()?;
        let bw = BatchWriter::<R>::spawn(Arc::clone(&rt.writer), policy)?;
        rt.batch_sinks.lock().push(Arc::downgrade(&bw.as_sink()));
        Ok(bw)
    }

    /// Executa uma transação `BEGIN IMMEDIATE` na thread escritora.
    pub async fn write<T, F>(&self, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> DbResult<T> + Send + 'static,
    {
        self.write_sized("db_write", 1024, f).await
    }

    /// Executa uma transação `BEGIN IMMEDIATE` informando a estimativa de bytes para o `ByteBudget`.
    pub async fn write_sized<T, F>(
        &self,
        operation: &'static str,
        approx_bytes: usize,
        f: F,
    ) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> DbResult<T> + Send + 'static,
    {
        let rt = self.ready_runtime()?;
        rt.writer.execute_tx(operation, approx_bytes, f).await
    }

    /// Executa uma transação `BEGIN IMMEDIATE` de forma síncrona na thread escritora.
    pub fn write_blocking<T, F>(
        &self,
        operation: &'static str,
        approx_bytes: usize,
        f: F,
    ) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> DbResult<T> + Send + 'static,
    {
        let rt = self.ready_runtime()?;
        let permit = rt.writer.budget().try_acquire(approx_bytes.max(1))?;
        rt.writer
            .execute_tx_with_permit_blocking(operation, permit, Duration::from_secs(2), f)
    }

    /// Executa uma operação direta sobre `&mut Connection` na thread escritora.
    pub async fn execute_writer_conn<T, F>(&self, operation: &'static str, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> DbResult<T> + Send + 'static,
    {
        let rt = self.ready_runtime()?;
        rt.writer.execute_conn(operation, f).await
    }

    /// Executa uma consulta somente leitura no `ReadPool` dentro de `tokio::task::spawn_blocking`.
    pub async fn read<T, F>(&self, f: F) -> DbResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> DbResult<T> + Send + 'static,
    {
        let rt = self.ready_runtime()?;
        rt.reader.read(f).await
    }

    /// Executa uma consulta somente leitura de forma síncrona usando uma conexão do `ReadPool`.
    pub fn read_blocking<T, F>(&self, f: F) -> DbResult<T>
    where
        F: FnOnce(&Connection) -> DbResult<T>,
    {
        let rt = self.ready_runtime()?;
        rt.reader.with_conn(f)
    }

    /// Retorna o status operacional completo (`DbStatusDto`) consumido pelo comando `db_status`.
    pub async fn status(&self) -> DbStatusDto {
        let state_snapshot = {
            let state_guard = self.inner.state.read();
            match &*state_guard {
                DbOperationalState::Ready(rt) => Ok(Arc::clone(rt)),
                DbOperationalState::RecoveryRequired { code, reason } => {
                    Err((code.clone(), reason.clone()))
                }
            }
        };

        match state_snapshot {
            Ok(rt) => {
                let batch_pending = sum_batch_pending(&rt);
                let metrics = rt.writer.metrics(batch_pending);
                DbStatusDto {
                    health: DbHealthStateDto::Ready {
                        schema_version: rt.schema_version,
                        sqlite_version: rt.sqlite_version.clone(),
                    },
                    pending_bytes: metrics.pending_bytes,
                    pending_records: metrics.pending_records,
                    writer_lag_events: metrics.writer_lag_events,
                }
            }
            Err((code, reason)) => {
                let backup_dir = self.inner.config.backup_dir.clone();
                let available_backups = tokio::task::spawn_blocking(move || {
                    list_backups(&backup_dir).unwrap_or_default()
                })
                .await
                .unwrap_or_default();

                DbStatusDto {
                    health: DbHealthStateDto::RecoveryRequired {
                        code,
                        reason,
                        db_path: self.inner.config.db_path.to_string_lossy().into_owned(),
                        available_backups,
                    },
                    pending_bytes: 0,
                    pending_records: 0,
                    writer_lag_events: self
                        .inner
                        .total_lag_events
                        .load(std::sync::atomic::Ordering::Relaxed),
                }
            }
        }
    }

    /// Executa uma ação explícita de recuperação (`RetryOpen`, `RestoreFromBackup` ou `ResetDatabase`).
    pub async fn recover(&self, action: DbRecoveryAction) -> DbResult<DbStatusDto> {
        // Se havia runtime ativo (ex.: recuperação acionada manualmente), encerra agendador e writer antes de mover arquivos
        let prev_sched = self.inner.maintenance_scheduler.lock().take();
        if let Some(sched) = prev_sched {
            sched.stop();
        }

        let prev_ready = {
            let state_guard = self.inner.state.read();
            match &*state_guard {
                DbOperationalState::Ready(rt) => Some(Arc::clone(rt)),
                DbOperationalState::RecoveryRequired { .. } => None,
            }
        };

        if let Some(rt) = prev_ready {
            shutdown_runtime(&rt, Duration::from_secs(3)).await?;
        }

        let cfg = self.inner.config.clone();
        let mig_reg = self.inner.migration_registry.clone();
        let sink = Arc::clone(&self.inner.event_sink);
        let max_ver = mig_reg.max_version();

        let (recovered_ev, new_runtime) = tokio::task::spawn_blocking(move || {
            let ev = execute_recovery_action(&cfg, &action, max_ver)?;
            let rt = open_healthy_runtime(&cfg, &mig_reg, sink)?;
            Ok::<_, DbError>((ev, rt))
        })
        .await
        .map_err(|_| DbError::BackupFailed {
            reason: String::from("A tarefa de recuperação foi interrompida."),
        })??;

        {
            let mut guard = self.inner.state.write();
            *guard = DbOperationalState::Ready(Arc::new(new_runtime));
        }

        if self.inner.config.enable_background_maintenance {
            let sched = spawn_maintenance_scheduler(self.clone(), Duration::from_secs(60));
            *self.inner.maintenance_scheduler.lock() = Some(sched);
        }

        self.inner.event_sink.on_recovered(recovered_ev);
        Ok(self.status().await)
    }

    /// Drena todos os `BatchWriter` registrados, aguarda leitores ativos terminarem,
    /// executa `PRAGMA wal_checkpoint(TRUNCATE)` e encerra a thread escritora.
    pub async fn shutdown(&self) -> DbResult<()> {
        let prev_sched = self.inner.maintenance_scheduler.lock().take();
        if let Some(sched) = prev_sched {
            sched.stop();
        }

        let rt_opt = {
            let state_guard = self.inner.state.read();
            match &*state_guard {
                DbOperationalState::Ready(rt) => Some(Arc::clone(rt)),
                DbOperationalState::RecoveryRequired { .. } => None,
            }
        };

        if let Some(rt) = rt_opt {
            shutdown_runtime(&rt, Duration::from_secs(5)).await?;
        }
        let _ = self.inner.instance_lock.lock().take();
        Ok(())
    }
}

fn open_healthy_runtime(
    config: &DbConfig,
    migration_registry: &MigrationRegistry,
    event_sink: Arc<dyn DbEventSink>,
) -> DbResult<ReadyDbRuntime> {
    let _ = apply_pending_restore_on_boot(config)?;
    let mut opened = open_writer(config)?;
    let sqlite_version = verify_sqlite_capabilities(&opened.conn, &config.db_path)?;
    run_quick_check(&opened.conn, &config.db_path)?;

    let runner = MigrationRunner::new(migration_registry.clone());
    let outcome = runner.run(&mut opened.conn, &config.db_path, &config.backup_dir, 7)?;

    let writer = Arc::new(DbWriter::spawn(
        opened.conn,
        config.write_queue_capacity,
        config.write_queue_bytes,
        event_sink,
    )?);
    let reader = ReadPool::new(config)?;

    Ok(ReadyDbRuntime {
        writer,
        reader,
        schema_version: outcome.schema_version,
        sqlite_version,
        applied_migrations: outcome.applied,
        batch_sinks: Mutex::new(Vec::new()),
    })
}

fn sum_batch_pending(rt: &ReadyDbRuntime) -> i64 {
    let mut sinks = rt.batch_sinks.lock();
    let mut total = 0i64;
    sinks.retain(|weak| {
        if let Some(sink) = weak.upgrade() {
            total = total.saturating_add(sink.pending_records());
            true
        } else {
            false
        }
    });
    total
}

async fn shutdown_runtime(rt: &Arc<ReadyDbRuntime>, timeout: Duration) -> DbResult<()> {
    let active_sinks: Vec<Arc<dyn BatchSink>> = {
        let mut guard = rt.batch_sinks.lock();
        let list = guard.iter().filter_map(Weak::upgrade).collect();
        guard.clear();
        list
    };

    if !active_sinks.is_empty() {
        let per_sink_timeout = timeout;
        tokio::task::spawn_blocking(move || -> DbResult<()> {
            for sink in active_sinks {
                sink.shutdown_blocking(per_sink_timeout)?;
            }
            Ok(())
        })
        .await
        .map_err(|_| DbError::WriterClosed)??;
    }

    let _ = rt.reader.wait_idle(Duration::from_secs(1)).await;

    let _ = rt
        .writer
        .execute_conn("shutdown_checkpoint", |conn| {
            let _ = conn.execute_batch("PRAGMA optimize; PRAGMA wal_checkpoint(TRUNCATE);");
            Ok(())
        })
        .await;

    rt.writer.shutdown(timeout).await
}
