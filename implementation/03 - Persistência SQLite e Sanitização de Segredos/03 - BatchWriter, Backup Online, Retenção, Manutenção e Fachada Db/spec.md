# Implementação 03 — `BatchWriter`, Backup Online, Retenção, Manutenção e Fachada `Db` (`spec.md`)

> Escopo da Fase 03: **I-09** (`BatchWriter<R>`), **I-12** (Retenção e manutenção), **I-13** (Backup Online e recuperação), **I-14** (`SettingsRegistry` e `SettingsRepo` no crate `mt-db`), **I-18** (Fachada `Db` e extensões) e **I-19** (Sanitização obrigatória de settings e registros em lote).
> Critérios de aceite cobertos: **CA-17**, **CA-19**, **CA-20**, **CA-21**, **CA-22**, **CA-33**, **CA-40**, **CA-41**, **CA-42**, **CA-43**, **CA-48**, **CA-49**, **CA-50**.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação completa o crate `src-tauri/crates/mt-db`, unificando todos os subsistemas sob a fachada pública **`Db`**:
1. **DTOs e Contratos de Eventos (`src/models.rs`)**: `SettingSnapshot`, `DbStatusDto`, `DbStateKind`, `DbRecoveryInfo`, `BackupEntryDto`, `DbRecoveryAction`, `SettingsChanged`, `DbRecovered`, `DbMaintenanceFinished`, `DbWriterLag` e trait `DbEventSink`.
2. **Escritor em Lote (`src/batch.rs`)**: `BatchRecord`, `BatchPolicy`, `BatchWriter<R>`, `BatchStats` e `BatchStatsSnapshot`. Agrupa inserts de alto volume (200 ms, 512 registros ou 1 MiB) em uma única transação `IMMEDIATE`, oferece `push()` não bloqueante (< 1 ms) com `DbError::Backpressure` e rate-limit de 1 evento `DbWriterLag` por segundo, além de `flush()` com barreira de confirmação até o `COMMIT`.
3. **Subsistema de Settings (`src/settings/`)**: `SettingDescriptor`, `SettingsRegistry`, `SettingLookup` (`Registered`, `FreeUi`, `Unknown`), `SettingsRepo` (`get`, `snapshot`, `set`, `set_internal`, `get_typed`) e as 4 configurações canônicas da Fase 03 (`db.retention.backup_daily_keep`, `db.retention.max_db_size_mb`, `db.maintenance.interval_hours`, `db.maintenance.last_run_at`), com inspeção recursiva de JSON contra chaves e valores sensíveis antes de qualquer gravação.
4. **Manutenção, Retenção e Backup Online (`src/maintenance/`)**:
   - `RetentionRule` e `RetentionRegistry`: expurgo em lotes (`LIMIT batch_size`) com intervalo entre transações para não monopolizar o writer.
   - `backup`: Online Backup API (`rusqlite::backup::Backup`) em `step(256)` com pausa de 10 ms e deadline de 60 s, escrita em arquivo temporário `0600` validado por `PRAGMA quick_check` antes de rename atômico, rotação de `backup_daily_keep` diários e 3 pré-migration, e procedimento explícito de quarentena/recuperação (`recover_database`).
   - `MaintenanceHandle` e `MaintenanceScheduler`: execução de retenção, `optimize` FTS5, `PRAGMA optimize`, `wal_checkpoint(PASSIVE)`, `incremental_vacuum(2000)`, alerta de tamanho máximo, backup diário, gravação de `db.maintenance.last_run_at` e emissão de `DbMaintenanceFinished`.
5. **Fachada `Db` (`src/lib.rs`)**: `Db::open(config, setup) -> Result<(Db, OpenReport), DbError>`, `Db::write`, `Db::read`, `Db::batch_writer`, `Db::settings`, `Db::maintenance` e `Db::shutdown`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos e Módulos em `src-tauri/crates/mt-db/`
- `src/models.rs`
- `src/batch.rs`
- `src/settings/mod.rs`
- `src/settings/registry.rs`
- `src/settings/repo.rs`
- `src/maintenance/mod.rs`
- `src/maintenance/retention.rs`
- `src/maintenance/backup.rs`
- `src/maintenance/scheduler.rs`
- `src/lib.rs` (implementação completa da fachada `Db`)
- `tests/batch.rs`
- `tests/settings.rs`
- `tests/retention.rs`
- `tests/backup.rs`
- `benches/batch.rs`

---

## 3. Modelos, Eventos e `DbEventSink` (`src/models.rs`)

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingSnapshot {
    pub value: Option<serde_json::Value>,
    pub revision: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DbStateKind {
    Opening,
    Ready,
    RecoveryRequired,
    Closing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupEntryDto {
    pub id: String,
    pub kind: String,
    pub created_at: i64,
    pub size_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbRecoveryInfo {
    pub reason_code: String,
    pub available_backups: Vec<BackupEntryDto>,
    pub last_recovered: Option<DbRecovered>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbStatusDto {
    pub state: DbStateKind,
    pub schema_version: u32,
    pub recovery: Option<DbRecoveryInfo>,
    pub pending_records: i64,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DbRecoveryAction {
    Restore {
        #[serde(rename = "backupId")]
        backup_id: String,
    },
    CreateEmpty,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChanged {
    pub key: String,
    pub value: serde_json::Value,
    pub revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbRecovered {
    pub backup_id: Option<String>,
    pub recovered_at: i64,
    pub created_empty: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbMaintenanceFinished {
    pub duration_ms: i64,
    pub deleted_rows: i64,
    pub db_size_bytes: i64,
    pub backup_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct DbWriterLag {
    pub writer: String,
    pub queued: i64,
    pub rejected: i64,
}

/// Sink desacoplado do Tauri para emissão de eventos pós-commit.
pub trait DbEventSink: Send + Sync + 'static {
    fn settings_changed(&self, ev: &SettingsChanged);
    fn recovered(&self, ev: &DbRecovered);
    fn maintenance_finished(&self, ev: &DbMaintenanceFinished);
    fn writer_lag(&self, ev: &DbWriterLag);
}

/// Implementação no-op padrão para testes que não inspecionam eventos.
#[derive(Default)]
pub struct NoopEventSink;
impl DbEventSink for NoopEventSink {
    fn settings_changed(&self, _: &SettingsChanged) {}
    fn recovered(&self, _: &DbRecovered) {}
    fn maintenance_finished(&self, _: &DbMaintenanceFinished) {}
    fn writer_lag(&self, _: &DbWriterLag) {}
}
```

> **Nota sobre tipos inteiros para Specta:** Conforme a Fase 01 (`spec.md` §4.5 item 7), usar `i64`/`u32` de forma consistente com a configuração do `specta-typescript` (`BigIntExportBehavior` ou `i64`/`u64` suportado pelo gerador em `src-tauri/src/specta.rs`) para que `bindings.ts` exporte `number` de forma determinística.

---

## 4. Escritor em Lote (`src/batch.rs`)

```rust
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use crossbeam_channel::Sender;
use rusqlite::Statement;
use tokio::sync::oneshot;
use crate::{
    error::DbError,
    models::{DbEventSink, DbWriterLag},
    writer::{ByteBudget, BytePermit, DbWriter},
};

pub trait BatchRecord: Send + 'static {
    /// Nome identificador do writer para telemetria/lag (ex.: `"scrollback"`).
    const WRITER_NAME: &'static str = "batch";
    /// SQL `INSERT` parametrizado; preparado com `prepare_cached` uma vez por flush.
    const INSERT_SQL: &'static str;
    /// Faz o bind e executa o statement para este registro.
    fn bind(&self, stmt: &mut Statement<'_>) -> rusqlite::Result<usize>;
    /// Estimativa de bytes ocupados em memória para controle de orçamento e flush.
    fn approx_bytes(&self) -> usize;
}

#[derive(Debug, Clone, Copy)]
pub struct BatchPolicy {
    pub max_delay: Duration,
    pub max_records: usize,
    pub max_bytes: usize,
    pub queue_capacity: usize,
    pub queue_bytes: usize,
}

impl Default for BatchPolicy {
    fn default() -> Self {
        Self {
            max_delay: Duration::from_millis(200),
            max_records: 512,
            max_bytes: 1024 * 1024,        // 1 MiB por lote
            queue_capacity: 8_192,
            queue_bytes: 16 * 1024 * 1024, // 16 MiB compartilhados
        }
    }
}

#[derive(Default)]
pub struct BatchStats {
    pub pushed: AtomicU64,
    pub flushed_records: AtomicU64,
    pub flushes: AtomicU64,
    pub rejected: AtomicU64,
    pub last_flush_ms: AtomicU64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchStatsSnapshot {
    pub pushed: u64,
    pub flushed_records: u64,
    pub flushes: u64,
    pub rejected: u64,
    pub last_flush_ms: u64,
    pub pending_records: u64,
}

pub(crate) trait BatchFlush: Send + Sync {
    fn flush_blocking(&self, timeout: Duration) -> Result<u64, DbError>;
    fn pending_records(&self) -> u64;
}

enum BatchMsg<R> {
    Item { record: R, permit: BytePermit },
    Barrier { reply: oneshot::Sender<Result<(), DbError>> },
    Shutdown { reply: oneshot::Sender<Result<u64, DbError>> },
}

#[derive(Clone)]
pub struct BatchWriter<R: BatchRecord> {
    tx: Sender<BatchMsg<R>>,
    stats: Arc<BatchStats>,
    budget: Arc<ByteBudget>,
    events: Arc<dyn DbEventSink>,
    last_lag_emit_ms: Arc<AtomicU64>,
    latched_error: Arc<Mutex<Option<DbError>>>,
}

impl<R: BatchRecord> BatchWriter<R> {
    /// Enfileira um registro sem bloquear a thread chamadora (< 1 ms).
    /// Se a fila de registros ou o orçamento global de bytes estiver cheio, incrementa `rejected`,
    /// emite no máximo 1 `DbWriterLag` por segundo e retorna `Err(DbError::Backpressure)`.
    pub fn push(&self, record: R) -> Result<(), DbError>;

    /// Insere uma barreira ordenada na fila e aguarda até que todos os registros anteriores sejam commitados no SQLite.
    pub async fn flush(&self) -> Result<(), DbError>;

    #[must_use]
    pub fn stats(&self) -> BatchStatsSnapshot;
}
```

**Regras operacionais de `BatchWriter`:**
1. A thread agregadora `mt-db-batch-<WRITER_NAME>` acumula itens até atingir `policy.max_records` (512), `policy.max_bytes` (1 MiB), `policy.max_delay` (200 ms desde o primeiro item do lote atual) ou até receber `BatchMsg::Barrier` / `BatchMsg::Shutdown`.
2. Quando um lote é fechado, a agregadora envia um `WriteJob::Batch` para `DbWriter` e aguarda a confirmação do `COMMIT` antes de responder às barreiras pendentes e liberar os `BytePermit` dos itens daquele lote.
3. Em caso de `DbError::Busy` transitório, a agregadora tenta novamente o mesmo lote (até o deadline de 2 s); em caso de falha permanente (ex.: `StorageFull` ou `Constraint`), armazena o erro em `latched_error` para que chamadas subsequentes a `push()` e `flush()` reportem a falha imediatamente sem descartar silenciosamente (CA-49).

---

## 5. Subsistema de Settings (`src/settings/`)

### 5.1. `src/settings/registry.rs`

```rust
use std::collections::HashMap;
use serde_json::Value;

#[derive(Clone)]
pub struct SettingDescriptor {
    pub key: &'static str,
    pub default: fn() -> Value,
    pub validate: fn(&Value) -> Result<(), String>,
    pub sensitive: bool,
    pub internal_only: bool,
    pub description: &'static str,
}

pub enum SettingLookup<'a> {
    Registered(&'a SettingDescriptor),
    FreeUi,
    Unknown,
}

#[derive(Clone, Default)]
pub struct SettingsRegistry {
    map: HashMap<&'static str, SettingDescriptor>,
}

impl SettingsRegistry {
    #[must_use]
    pub fn with_phase03_defaults() -> Self;

    /// Registra um descritor. Entra em panic na inicialização se a chave for duplicada ou inválida.
    pub fn register(&mut self, descriptor: SettingDescriptor);

    #[must_use]
    pub fn resolve(&self, key: &str) -> SettingLookup<'_>;
}
```

**As 4 Settings registradas por `SettingsRegistry::with_phase03_defaults()`:**
1. `db.retention.backup_daily_keep`:
   - `default`: `json!(7)`
   - `validate`: inteiro entre `1` e `60` (erro pt-BR: `"O número de backups diários deve ser um inteiro entre 1 e 60."`)
   - `sensitive: false`, `internal_only: false`
2. `db.retention.max_db_size_mb`:
   - `default`: `json!(2048)`
   - `validate`: inteiro entre `64` e `65536` (erro pt-BR: `"O limite de tamanho do banco deve estar entre 64 MiB e 65536 MiB."`)
   - `sensitive: false`, `internal_only: false`
3. `db.maintenance.interval_hours`:
   - `default`: `json!(24)`
   - `validate`: inteiro entre `1` e `168` (erro pt-BR: `"O intervalo de manutenção deve estar entre 1 e 168 horas."`)
   - `sensitive: false`, `internal_only: false`
4. `db.maintenance.last_run_at`:
   - `default`: `Value::Null`
   - `validate`: `Value::Null` ou inteiro `>= 0`
   - `sensitive: false`, `internal_only: true` (chamadas públicas via `SettingsRepo::set` / IPC `db_settings_set` retornam `Err(DbError::SettingInvalid { key, reason: "Esta configuração é gerenciada internamente pelo sistema.".into() })`; apenas `SettingsRepo::set_internal` pode alterá-la).

### 5.2. `src/settings/repo.rs`

```rust
use serde::de::DeserializeOwned;
use serde_json::Value;
use crate::{Db, error::DbError, models::SettingSnapshot};

#[derive(Clone)]
pub struct SettingsRepo {
    db: Db,
}

impl SettingsRepo {
    pub(crate) fn new(db: Db) -> Self;

    /// Retorna o valor persistido, ou o `default()` se registrada, ou `Ok(None)` se `FreeUi`/`Unknown` sem linha no banco.
    pub async fn get(&self, key: &str) -> Result<Option<Value>, DbError>;

    /// Retorna `{ value, revision }` (quando não há linha persistida no banco, `revision = 0` e `value` é o default ou `None`).
    pub async fn snapshot(&self, key: &str) -> Result<SettingSnapshot, DbError>;

    /// Valida chave, verifica permissão (rejeita `internal_only` e `sensitive`), inspeciona recursivamente
    /// o JSON contra conteúdo sensível (`inspect_settings_json`), valida limite de 64 KiB e executa UPSERT.
    /// Se o novo valor for idêntico ao valor já persistido na tabela, não escreve, não incrementa `revision`
    /// e não emite evento `SettingsChanged`.
    pub async fn set(&self, key: &str, value: Value) -> Result<SettingSnapshot, DbError>;

    /// Usado exclusivamente por rotinas internas do `mt-db` (ex.: atualizar `db.maintenance.last_run_at`).
    pub(crate) async fn set_internal(&self, key: &str, value: Value) -> Result<SettingSnapshot, DbError>;

    pub async fn get_typed<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, DbError>;
}
```

---

## 6. Manutenção, Retenção, Backup Online e Recuperação (`src/maintenance/`)

### 6.1. `src/maintenance/retention.rs`

```rust
#[derive(Debug, Clone)]
pub struct RetentionRule {
    pub name: &'static str,
    pub table: &'static str,
    pub timestamp_column: &'static str,
    pub setting_key: &'static str,
    pub extra_where: Option<&'static str>,
    pub batch_size: usize,
}

impl RetentionRule {
    /// Valida que `table` e `timestamp_column` casam com `^[a-z][a-z0-9_]*$` e `batch_size > 0`.
    pub fn validate(&self) -> Result<(), crate::error::DbError>;
}

#[derive(Debug, Clone, Default)]
pub struct RetentionRegistry {
    rules: Vec<RetentionRule>,
}
```

- Na execução de cada `RetentionRule`, obtém os dias de retenção via `SettingsRepo::get_typed::<i64>(rule.setting_key)` (se `<= 0` ou ausente, pula a regra), calcula `cutoff_ms = now_ms() - days * 86_400_000` e executa transações separadas no writer:
  ```sql
  DELETE FROM {table}
  WHERE rowid IN (
      SELECT rowid FROM {table}
      WHERE {timestamp_column} < ?1 [AND ({extra_where})]
      LIMIT ?2
  )
  ```
  em loop até que `rows_affected == 0`, cedendo a fila de escrita entre lotes (CA-40, CA-50).

### 6.2. `src/maintenance/backup.rs`

```rust
use std::path::{Path, PathBuf};
use crate::{
    config::DbConfig,
    error::DbError,
    models::{BackupEntryDto, DbRecovered, DbRecoveryAction},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackupKind {
    Daily,
    PreMigration { target: u32 },
}

/// Cria um backup consistente usando `rusqlite::backup::Backup` (`step(256)`, pausa de 10 ms, deadline de 60 s).
/// Grava primeiramente em arquivo temporário `.tmp` com permissão `0600`, executa `PRAGMA quick_check` no destino,
/// faz `sync_all()`, renomeia atomicamente para `maxterminal-<daily|premigration-vN>-<AAAAMMDD-HHMMSS>-<ms>.db`
/// e aplica rotação mantendo no máximo `daily_keep` diários e `3` pré-migration.
pub fn create_backup_blocking(
    source_db_path: &Path,
    backup_dir: &Path,
    kind: BackupKind,
    daily_keep: usize,
) -> Result<(PathBuf, BackupEntryDto), DbError>;

/// Lista backups válidos disponíveis em `backup_dir` ordenados do mais recente para o mais antigo.
pub fn list_backups(backup_dir: &Path) -> Result<Vec<(PathBuf, BackupEntryDto)>, DbError>;

/// Executa recuperação explícita sob `InstanceLock`:
/// 1. Move `.db`, `-wal` e `-shm` atuais para subpasta privada em `quarantine_dir/corrupt-<ts>/` (preservando originais!).
/// 2. Se `DbRecoveryAction::Restore { backup_id }`: localiza o backup pelo ID na allowlist de `backup_dir`,
///    valida `quick_check`, `foreign_key_check` e `user_version <= known`, e copia com modo `0600` para `cfg.path`.
/// 3. Se `DbRecoveryAction::CreateEmpty`: apenas preserva os originais em quarentena para que `Db::open` crie um banco limpo.
pub fn execute_recovery_action(
    cfg: &DbConfig,
    action: &DbRecoveryAction,
    known_max_version: u32,
) -> Result<DbRecovered, DbError>;
```

### 6.3. `src/maintenance/scheduler.rs` e `src/maintenance/mod.rs`

```rust
use std::path::PathBuf;
use crate::{Db, error::DbError, maintenance::backup::BackupKind, models::DbMaintenanceFinished};

#[derive(Clone)]
pub struct MaintenanceHandle {
    db: Db,
}

impl MaintenanceHandle {
    /// Executa a rotina completa de manutenção em passos desacoplados no writer:
    /// 1. Executa todas as `RetentionRule` registradas em lotes.
    /// 2. Executa `optimize` em todas as tabelas FTS5 registradas no `FtsRegistry`.
    /// 3. Executa `PRAGMA optimize;`, `PRAGMA wal_checkpoint(PASSIVE);` e `PRAGMA incremental_vacuum(2000);`.
    /// 4. Verifica tamanho total (`.db` + `-wal`) contra `db.retention.max_db_size_mb` (emite `tracing::warn!` se exceder).
    /// 5. Cria backup diário se o último backup diário tiver mais de 20 horas (ou não existir) e rotaciona backups.
    /// 6. Grava `db.maintenance.last_run_at = now_ms()` via `set_internal` e emite `DbMaintenanceFinished`.
    pub async fn run_now(&self) -> Result<DbMaintenanceFinished, DbError>;

    /// Cria um backup imediato (`Daily` ou `PreMigration`) em `spawn_blocking`.
    pub async fn backup(&self, kind: BackupKind) -> Result<PathBuf, DbError>;
}
```

---

## 7. Fachada Pública `Db` (`src/lib.rs`)

```rust
pub mod batch;
pub mod config;
pub mod connection;
pub mod error;
pub mod fts;
pub mod maintenance;
pub mod migrations;
pub mod models;
pub mod reader;
pub mod sanitizer;
pub mod settings;
pub mod time;

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, Weak},
};
use rusqlite::{Connection, Transaction};

pub use batch::{BatchPolicy, BatchRecord, BatchStats, BatchStatsSnapshot, BatchWriter};
pub use config::{DbConfig, PragmaSet};
pub use error::{DbError, codes as error_codes};
pub use fts::{FtsRegistry, FtsSpec, FtsTokenizer, KeysetCursor, SearchSnapshotCache, build_match_query};
pub use maintenance::{BackupKind, MaintenanceHandle, RetentionRegistry, RetentionRule};
pub use migrations::{CodeMigration, MigrationInfo, MigrationKind};
pub use models::{
    BackupEntryDto, DbEventSink, DbMaintenanceFinished, DbRecovered, DbRecoveryAction,
    DbRecoveryInfo, DbStateKind, DbStatusDto, DbWriterLag, NoopEventSink, SettingSnapshot,
    SettingsChanged,
};
pub use sanitizer::{
    ExternalPayload, Finding, SanitizeError, SanitizeOptions, SanitizeOutcome, SanitizedText,
    SecretKind, StreamSanitizer, inspect_settings_json, prepare_external_payload, sanitize,
    sanitize_terminal, sanitize_with,
};
pub use settings::{SettingDescriptor, SettingLookup, SettingsRegistry, SettingsRepo};

#[derive(Default)]
pub struct DbSetup {
    pub events: Option<Arc<dyn DbEventSink>>,
    pub code_migrations: Vec<CodeMigration>,
    pub settings: Vec<SettingDescriptor>,
    pub retention: Vec<RetentionRule>,
    pub fts: Vec<FtsSpec>,
}

#[derive(Debug, Clone)]
pub struct OpenReport {
    pub applied: Vec<MigrationInfo>,
    pub backup_path: Option<PathBuf>,
    pub recovered: Option<DbRecovered>,
    pub created: bool,
    pub schema_version: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShutdownReport {
    pub committed_records: u64,
    pub checkpoint_complete: bool,
}

#[derive(Clone)]
pub struct Db {
    inner: Arc<DbInner>,
}

impl Db {
    /// Abre ou cria o banco SQLite em modo WAL:
    /// 1. Adquire `InstanceLock` exclusivo (`<data>/maxterminal.db.lock`).
    /// 2. Abre a conexão de escrita aplicando PRAGMAs e permissões `0600`/`0700`.
    /// 3. Executa `verify_sqlite_capabilities` e `run_quick_check` (se corrompido -> retorna `Err(DbError::RecoveryRequired)` sem apagar nada).
    /// 4. Verifica migrations pendentes; se o banco não era novo e há pendentes, cria backup `PreMigration`.
    /// 5. Aplica migrations pendentes e inicia `DbWriter` + `ReadPool`.
    pub fn open(config: DbConfig, setup: DbSetup) -> Result<(Self, OpenReport), DbError>;

    /// Executa transação `IMMEDIATE` na thread escritora.
    pub async fn write<T, F>(&self, f: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T, DbError> + Send + 'static;

    /// Executa consulta somente leitura no `ReadPool` dentro de `spawn_blocking`.
    pub async fn read<T, F>(&self, f: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, DbError> + Send + 'static;

    /// Cria e registra um `BatchWriter<R>` supervisionado para flush automático no shutdown.
    pub fn batch_writer<R: BatchRecord>(&self, policy: BatchPolicy) -> BatchWriter<R>;

    #[must_use]
    pub fn settings(&self) -> SettingsRepo;

    #[must_use]
    pub fn maintenance(&self) -> MaintenanceHandle;

    #[must_use]
    pub fn schema_version(&self) -> u32;

    #[must_use]
    pub fn pending_batch_records(&self) -> u64;

    /// Confirma todos os `BatchWriter` registrados, aguarda leitores ativos drenarem,
    /// executa `PRAGMA wal_checkpoint(TRUNCATE)` e encerra a thread escritora dentro do orçamento de 5 s.
    pub async fn shutdown(&self) -> Result<ShutdownReport, DbError>;
}
```

---

## 8. Testes e Benchmarks Obrigatórios da Implementação 03

1. **`tests/batch.rs` (CA-19, CA-20, CA-21, CA-49):**
   - `coalesces`: 10 000 registros enviados sob carga controlada resultam em ≤ 25 transações (`stats.flushes <= 25`) e todas as 10 000 linhas persistidas.
   - `order_preserved`: ordem sequencial preservada dentro do mesmo `BatchWriter`.
   - `backpressure_non_blocking`: fila cheia retorna `Err(DbError::Backpressure)` em < 1 ms e emite no máximo 1 evento `DbWriterLag` por segundo.
   - `explicit_flush_awaits_commit` e `flush_on_shutdown`: `flush()` e `Db::shutdown()` confirmam todos os registros aceitos e deixam o WAL truncado/vazio quando não há leitores externos.
2. **`tests/settings.rs` (CA-33, CA-34, CA-35, CA-36, CA-37, CA-38, CA-48):**
   - Retorno de defaults (`db.retention.backup_daily_keep` → `7`), validação de limites (`0` → `db.setting_invalid`), rejeição de chave não registrada fora de `ui.*` (`db.setting_unknown`), aceitação de `ui.*` livre com JSON limpo, rejeição de valor > 64 KiB (`db.setting_too_large`), rejeição de descritor `sensitive` e de JSON aninhado contendo chaves/valores sensíveis (`password`, `APP_KEY`, `Authorization`, cookies, tokens) com `db.setting_sensitive` sem ecoar o segredo no erro (CA-48), emissão de 1 evento pós-commit por mudança real e zero eventos quando o valor é igual ao atual.
3. **`tests/retention.rs` (CA-40, CA-41, CA-50):**
   - `applies_rules`: remove apenas linhas mais antigas que o limite configurado respeitando `extra_where` e `batch_size`, sincroniza FTS5 nas deleções em lote, grava `db.maintenance.last_run_at` e emite `DbMaintenanceFinished`.
4. **`tests/backup.rs` e `tests/migrations.rs` (CA-17, CA-42, CA-43):**
   - `premigration_backup_created` (CA-17): banco existente na v1 ao receber migration v2 cria automaticamente `backups/maxterminal-premigration-v2-<ts>.db` antes de aplicar a v2.
   - Backup online válido mesmo com dados no WAL, rotação mantendo `backup_daily_keep` diários e 3 pré-migration (CA-42).
   - Corrupção simulada no arquivo `.db` faz `Db::open` retornar `Err(DbError::RecoveryRequired)` sem apagar os arquivos originais; `execute_recovery_action` move `.db/-wal/-shm` para quarentena e restaura o backup validado (CA-43).
5. **`benches/batch.rs` (CA-22):**
   - Benchmark `criterion` inserindo 100 000 registros de 200 bytes via `BatchWriter` e confirmando commit total em **< 1,5 s** em release.
