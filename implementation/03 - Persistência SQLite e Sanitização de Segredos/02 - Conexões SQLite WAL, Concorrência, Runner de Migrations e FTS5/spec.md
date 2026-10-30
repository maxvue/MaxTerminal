# Implementação 02 — Conexões SQLite WAL, Concorrência, Runner de Migrations e FTS5 (`spec.md`)

> Escopo da Fase 03: **I-02** (Abertura e PRAGMAs), **I-03** (Caminho, permissões e lock), **I-04** (Writer thread única), **I-05** (Pool de leitores `r2d2`), **I-06** (Runner de migrations), **I-07** (Schema base `schema_migrations` e `0001_03_base.sql`), **I-08** (Infraestrutura FTS5 e paginação/busca segura) e **I-15** (`DbError` e conversão `MtError`).
> Critérios de aceite cobertos: **CA-07**, **CA-08**, **CA-09**, **CA-10**, **CA-11**, **CA-12**, **CA-13**, **CA-14**, **CA-15**, **CA-16**, **CA-18**, **CA-23**, **CA-24**, **CA-25**, **CA-26**, **CA-44**, **CA-45**.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói o alicerce de banco de dados do crate `mt-db`:
1. **Catálogo completo de erros (`error.rs`)**: `DbError` e códigos `mt_db::error::codes::*` convertidos para `MtError` com detalhes estritamente filtrados por allowlist (nunca vazando SQL, caminhos brutos ou valores de parâmetros).
2. **Abertura segura, PRAGMAs, permissões Unix e lock de instância (`config.rs`, `connection.rs`)**: criação de diretório com modo `0700` e arquivo `.db`/`-wal`/`-shm` com modo `0600` (sem alterar `umask` global), recusa de symlinks, lock exclusivo `<data>/maxterminal.db.lock` (`flock` não bloqueante), validação de `sqlite_version() >= 3.51.3`, aplicação ordenada dos PRAGMAs (`auto_vacuum=INCREMENTAL` em banco novo, `journal_mode=WAL` obrigatório, `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`, `cell_size_check=ON`) e `PRAGMA quick_check`.
3. **Arquitetura de concorrência (`writer.rs`, `reader.rs`)**: uma thread dedicada `mt-db-writer` dona da conexão de escrita (`BEGIN IMMEDIATE`) com fila limitada em jobs (4096) e bytes (16 MiB, máx. 1 MiB/job), mais um pool `r2d2` de 4 conexões somente leitura (`query_only=ON`) acessadas via `tokio::task::spawn_blocking`.
4. **Runner de migrations versionadas (`migrations/`)**: tabela de bootstrap `schema_migrations`, migration embutida `0001_03_base.sql` (tabela `settings` `STRICT, WITHOUT ROWID`), suporte a `CodeMigration`, validação de nomenclatura e contiguidade, verificação de checksum SHA-256 normalizado (`\r\n` → `\n`), bloqueio de downgrade (`db.schema_too_new`), hook de backup pré-migration e execução transacional com `PRAGMA foreign_key_check` e `PRAGMA user_version`.
5. **Infraestrutura FTS5 e contrato de paginação/busca (`fts.rs`)**: `FtsSpec` (gerador de DDL `USING fts5` com `content` externo + triggers `_ai`, `_ad`, `_au`), `FtsTokenizer` (`Text` e `Code`), `build_match_query()` à prova de injeção de sintaxe FTS5, `FtsRegistry`, codificador de cursores opacos e cache RAM de snapshots de busca com TTL de 30 s.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos e Módulos em `src-tauri/crates/mt-db/`
- `migrations/0001_03_base.sql`
- `src/error.rs` (implementação completa)
- `src/config.rs`
- `src/connection.rs`
- `src/writer.rs`
- `src/reader.rs`
- `src/migrations/mod.rs`
- `src/migrations/registry.rs`
- `src/migrations/runner.rs`
- `src/fts.rs`
- `tests/migrations.rs`
- `tests/concurrency.rs`
- `tests/fts.rs`
- `tests/pagination.rs`

---

## 3. Erros e Mapeamento para `MtError` (`src/error.rs`)

### 3.1. Códigos de Erro (`mt_db::error::codes`)

```rust
pub mod codes {
    use mt_core::ErrorCode;

    pub const OPEN_FAILED: ErrorCode = ErrorCode("db.open_failed");
    pub const PRAGMA_FAILED: ErrorCode = ErrorCode("db.pragma_failed");
    pub const BUSY: ErrorCode = ErrorCode("db.busy");
    pub const CONSTRAINT: ErrorCode = ErrorCode("db.constraint");
    pub const NOT_FOUND: ErrorCode = ErrorCode("db.not_found");
    pub const MIGRATION_FAILED: ErrorCode = ErrorCode("db.migration_failed");
    pub const MIGRATION_CHECKSUM_MISMATCH: ErrorCode = ErrorCode("db.migration_checksum_mismatch");
    pub const SCHEMA_TOO_NEW: ErrorCode = ErrorCode("db.schema_too_new");
    pub const CORRUPT: ErrorCode = ErrorCode("db.corrupt");
    pub const INTERNAL: ErrorCode = ErrorCode("db.internal");
    pub const SETTING_UNKNOWN: ErrorCode = ErrorCode("db.setting_unknown");
    pub const SETTING_INVALID: ErrorCode = ErrorCode("db.setting_invalid");
    pub const SETTING_TOO_LARGE: ErrorCode = ErrorCode("db.setting_too_large");
    pub const SETTING_SENSITIVE: ErrorCode = ErrorCode("db.setting_sensitive");
    pub const BACKUP_FAILED: ErrorCode = ErrorCode("db.backup_failed");
    pub const IN_USE: ErrorCode = ErrorCode("db.in_use");
    pub const RECOVERY_REQUIRED: ErrorCode = ErrorCode("db.recovery_required");
    pub const CONFLICT: ErrorCode = ErrorCode("db.conflict");
    pub const CURSOR_INVALID: ErrorCode = ErrorCode("db.cursor_invalid");
    pub const CURSOR_EXPIRED: ErrorCode = ErrorCode("db.cursor_expired");
    pub const QUERY_INVALID: ErrorCode = ErrorCode("db.query_invalid");
    pub const STORAGE_FULL: ErrorCode = ErrorCode("db.storage_full");
    pub const SHUTDOWN_TIMEOUT: ErrorCode = ErrorCode("db.shutdown_timeout");
    pub const NOT_READY: ErrorCode = ErrorCode("db.not_ready");
}
```

### 3.2. Enum `DbError` e Conversões Seguras

```rust
use std::path::PathBuf;
use mt_core::MtError;

#[derive(thiserror::Error)]
pub enum DbError {
    #[error("falha ao abrir o banco de dados")]
    Open {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("PRAGMA {name} não pôde ser aplicado (obtido: {got})")]
    Pragma { name: &'static str, got: String },
    #[error("banco ocupado; tente novamente")]
    Busy,
    #[error("violação de restrição: {0}")]
    Constraint(&'static str),
    #[error("registro não encontrado")]
    NotFound,
    #[error("falha na migration {version} ({name}): {message}")]
    MigrationFailed {
        version: u32,
        name: String,
        message: String,
    },
    #[error("a migration {version} foi alterada após ser aplicada")]
    MigrationChecksumMismatch { version: u32 },
    #[error("o banco foi criado por uma versão mais nova do MaxTerminal (schema {found} > {known})")]
    SchemaTooNew { found: u32, known: u32 },
    #[error("banco de dados corrompido")]
    Corrupt,
    #[error("recuperação do banco de dados necessária")]
    RecoveryRequired,
    #[error("banco de dados já está em uso por outra instância do MaxTerminal")]
    InUse,
    #[error("conflito de atualização concorrente")]
    Conflict,
    #[error("cursor de paginação inválido")]
    InvalidCursor,
    #[error("cursor de busca expirado; refaça a consulta")]
    CursorExpired,
    #[error("consulta de busca inválida")]
    InvalidQuery,
    #[error("espaço em disco insuficiente para gravar no banco de dados")]
    StorageFull,
    #[error("tempo esgotado ao encerrar o banco de dados ({pending_records} registros pendentes)")]
    ShutdownTimeout { pending_records: u64 },
    #[error("banco de dados ainda está inicializando")]
    NotReady,
    #[error("fila de escrita cheia")]
    Backpressure,
    #[error("escritor do banco encerrado")]
    WriterClosed,
    #[error("erro interno no escritor do banco")]
    WriterPanicked,
    #[error("configuração desconhecida: {0}")]
    SettingUnknown(String),
    #[error("valor inválido para {key}: {reason}")]
    SettingInvalid { key: String, reason: String },
    #[error("valor de {0} excede 64 KiB")]
    SettingTooLarge(String),
    #[error("{0} é sensível e não pode ser gravada no banco")]
    SettingSensitive(String),
    #[error("falha no backup: {0}")]
    Backup(String),
    #[error("erro SQLite interno")]
    Sqlite(#[source] rusqlite::Error),
    #[error("erro de entrada/saída no banco")]
    Io(#[from] std::io::Error),
}
```

**Regras de segurança de `DbError`:**
- Implementar `fmt::Debug` manual para `DbError` que **não** imprime caminhos completos de arquivo nem mensagens brutas do `rusqlite::Error` (que poderiam conter fragmentos de SQL/valores), exibindo apenas o código de erro estático do SQLite (`ErrorCode` do libsqlite3).
- Implementar `From<rusqlite::Error> for DbError` mapeando:
  - `ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked` → `DbError::Busy`
  - `ErrorCode::ConstraintViolation` → `DbError::Constraint("constraint_violation")`
  - `ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase` → `DbError::Corrupt`
  - `ErrorCode::DiskFull` → `DbError::StorageFull`
  - `rusqlite::Error::QueryReturnedNoRows` → `DbError::NotFound`
  - Demais → `DbError::Sqlite(err)`
- Implementar `From<DbError> for MtError` preenchendo `details` estritamente com a allowlist da spec §3 (`{ name, got }`, `{ constraint }`, `{ version, name }`, `{ version }`, `{ found, known }`, `{ key, reason? }`, `{ pendingRecords }`).

---

## 4. Configuração, Abertura, Permissões e PRAGMAs (`config.rs`, `connection.rs`)

### 4.1. `src/config.rs`

```rust
use std::path::PathBuf;
use mt_core::paths::{AppPaths, EnvSource};
use crate::error::DbError;

#[derive(Debug, Clone)]
pub struct PragmaSet {
    pub busy_timeout_ms: u32,
    pub cache_size_kib: i64,
    pub mmap_size: i64,
}

impl Default for PragmaSet {
    fn default() -> Self {
        Self {
            busy_timeout_ms: 5_000,
            cache_size_kib: 16_000,
            mmap_size: 268_435_456, // 256 MiB
        }
    }
}

#[derive(Debug, Clone)]
pub struct DbConfig {
    pub path: PathBuf,
    pub backup_dir: PathBuf,
    pub lock_path: PathBuf,
    pub quarantine_dir: PathBuf,
    pub reader_pool_size: u32,
    pub write_queue_capacity: usize,
    pub write_queue_bytes: usize,
    pub pragmas: PragmaSet,
}

impl DbConfig {
    /// Resolve `DbConfig` a partir de `AppPaths` e `EnvSource`.
    /// Em debug/testes (`cfg!(debug_assertions)` ou variável de teste), aceita override `MAXTERMINAL_DB_PATH`
    /// desde que seja um caminho absoluto; se relativo ou vazio, retorna erro.
    /// Quando `MAXTERMINAL_DB_PATH` é usado, `backup_dir`, `lock_path` e `quarantine_dir` ficam no mesmo diretório pai.
    pub fn from_paths(paths: &AppPaths, env: &impl EnvSource) -> Result<Self, DbError>;

    /// Construtor para testes com diretório temporário isolado.
    #[must_use]
    pub fn for_path(db_path: PathBuf) -> Self;
}
```

### 4.2. `src/connection.rs`

```rust
use std::{fs::File, path::Path};
use rusqlite::Connection;
use crate::{config::{DbConfig, PragmaSet}, error::DbError};

/// Guarda o descritor de arquivo do lock exclusivo (`<data>/maxterminal.db.lock`) adquirido com `flock(LOCK_EX | LOCK_NB)`.
pub struct InstanceLock {
    file: File,
}

impl InstanceLock {
    /// Adquire lock exclusivo não bloqueante. Se outra instância já detiver o lock, retorna `Err(DbError::InUse)`.
    pub fn acquire(lock_path: &Path) -> Result<Self, DbError>;
}

/// Resultado da abertura da conexão de escrita.
pub struct OpenedWriter {
    pub conn: Connection,
    pub is_new_file: bool,
}

/// Garante diretórios (`0700`), recusa symlinks em `db_path`, `-wal`, `-shm` e `lock_path`,
/// pré-cria o arquivo `.db` com `0600` (se não existir), aplica todos os PRAGMAs de escrita e valida `journal_mode == "wal"`.
pub fn open_writer(cfg: &DbConfig) -> Result<OpenedWriter, DbError>;

/// Abre uma conexão somente leitura (`SQLITE_OPEN_READ_ONLY | SQLITE_OPEN_NO_MUTEX | SQLITE_OPEN_URI`)
/// e aplica PRAGMAs de leitor (`query_only = ON`, `foreign_keys = ON`, `busy_timeout`, `cache_size`, `mmap_size`, `trusted_schema = OFF`).
pub fn open_reader(path: &Path, pragmas: &PragmaSet) -> Result<Connection, DbError>;

/// Executa `PRAGMA quick_check` e valida que todas as linhas retornam `"ok"`. Qualquer divergência retorna `Err(DbError::Corrupt)`.
pub fn run_quick_check(conn: &Connection) -> Result<(), DbError>;

/// Verifica em runtime se `sqlite_compileoption_used('ENABLE_FTS5') == 1` e `sqlite_version_number() >= 3051003` (SQLite 3.51.3; piso do WAL-reset, D-06).
pub fn verify_sqlite_capabilities(conn: &Connection) -> Result<(), DbError>;
```

**Regras estritas de `connection.rs`:**
1. **Permissões Unix sem alterar `umask`:**
   - Criar `parent_dir`, `backup_dir` e `quarantine_dir` com `DirBuilder::new().recursive(true).mode(0o700)`. Se o diretório já existir, verificar que não é symlink, pertence ao `uid` atual e ajustar `Permissions::from_mode(0o700)` no próprio diretório do app.
   - Se `cfg.path` não existir, criar arquivo vazio via `OpenOptions::new().write(true).create_new(true).mode(0o600)` antes de chamar `Connection::open`. Se já existir, validar que é arquivo regular (não symlink) e garantir modo `0o600` nele e, se existirem, em `<db>-wal` e `<db>-shm`.
2. **Sequência exata de PRAGMAs na conexão de escrita:**
   - Se `is_new_file` (ou `page_count == 0`): `PRAGMA auto_vacuum = INCREMENTAL;` antes de criar qualquer tabela.
   - `PRAGMA journal_mode = WAL;` — ler string retornada e exigir `got.eq_ignore_ascii_case("wal")`, caso contrário retornar `Err(DbError::Pragma { name: "journal_mode", got })`.
   - `PRAGMA synchronous = NORMAL;`
   - `PRAGMA foreign_keys = ON;`
   - `PRAGMA busy_timeout = <busy_timeout_ms>;`
   - `PRAGMA temp_store = MEMORY;`
   - `PRAGMA cache_size = -<cache_size_kib>;`
   - `PRAGMA mmap_size = <mmap_size>;`
   - `PRAGMA wal_autocheckpoint = 1000;`
   - `PRAGMA journal_size_limit = 67108864;`
   - `PRAGMA cell_size_check = ON;`
   - `PRAGMA trusted_schema = ON;` (necessário no writer para triggers FTS5 sobre schema local validado por checksum).
   - Configurar `conn.set_prepared_statement_cache_capacity(64)`.

---

## 5. Concorrência: `DbWriter` e `ReadPool` (`writer.rs`, `reader.rs`)

### 5.1. `src/writer.rs`

```rust
use std::{
    any::Any,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize},
    },
    thread::JoinHandle,
};
use crossbeam_channel::Sender;
use rusqlite::{Connection, Transaction};
use tokio::sync::{Notify, oneshot};
use crate::error::DbError;

pub const MAX_SINGLE_WRITE_JOB_BYTES: usize = 1024 * 1024; // 1 MiB

pub(crate) enum WriteJob {
    Tx {
        approx_bytes: usize,
        f: Box<dyn FnOnce(&Transaction<'_>) -> Result<Box<dyn Any + Send>, DbError> + Send>,
        reply: oneshot::Sender<Result<Box<dyn Any + Send>, DbError>>,
    },
    Batch {
        approx_bytes: usize,
        flush: Box<dyn FnOnce(&Transaction<'_>) -> Result<usize, DbError> + Send>,
        reply: oneshot::Sender<Result<usize, DbError>>,
    },
    Raw {
        f: Box<dyn FnOnce(&mut Connection) -> Result<(), DbError> + Send>,
        reply: oneshot::Sender<Result<(), DbError>>,
    },
    Shutdown {
        reply: oneshot::Sender<()>,
    },
}

/// Controlador de orçamento de bytes compartilhado entre `DbWriter` e todas as instâncias de `BatchWriter`.
pub(crate) struct ByteBudget {
    max_bytes: usize,
    used_bytes: AtomicUsize,
    notify: Notify,
}

impl ByteBudget {
    pub fn new(max_bytes: usize) -> Arc<Self>;
    pub fn try_acquire(&self, bytes: usize) -> Result<BytePermit, DbError>;
    pub async fn acquire_timeout(&self, bytes: usize, timeout: std::time::Duration) -> Result<BytePermit, DbError>;
    pub fn used(&self) -> usize;
}

pub(crate) struct BytePermit { /* devolve bytes no Drop e chama notify.notify_waiters() */ }

pub(crate) struct DbWriter {
    tx: Sender<WriteJob>,
    budget: Arc<ByteBudget>,
    closed: Arc<AtomicBool>,
    slot_freed: Arc<Notify>,
    handle: Mutex<Option<JoinHandle<()>>>,
}

impl DbWriter {
    pub fn spawn(conn: Connection, queue_capacity: usize, max_queue_bytes: usize) -> Self;

    /// Executa `f` dentro de `conn.transaction_with_behavior(TransactionBehavior::Immediate)`.
    /// Se a fila estiver cheia, aguarda de forma assíncrona por até 250 ms usando `Notify` (nunca bloqueia thread Tokio).
    pub async fn execute_tx<T, F>(&self, approx_bytes: usize, f: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T, DbError> + Send + 'static;

    /// Executa operação direta na `Connection` (usada para `VACUUM`, `incremental_vacuum` e `wal_checkpoint`).
    pub async fn execute_raw<F>(&self, f: F) -> Result<(), DbError>
    where
        F: FnOnce(&mut Connection) -> Result<(), DbError> + Send + 'static;

    /// Enfileira um lote pré-agregado pelo `BatchWriter`.
    pub(crate) fn send_batch_blocking(
        &self,
        approx_bytes: usize,
        flush: Box<dyn FnOnce(&Transaction<'_>) -> Result<usize, DbError> + Send>,
        reply: oneshot::Sender<Result<usize, DbError>>,
    ) -> Result<(), DbError>;

    pub(crate) fn budget(&self) -> &Arc<ByteBudget>;

    /// Drena a fila de escrita e encerra a thread `mt-db-writer`.
    pub async fn shutdown(&self) -> Result<(), DbError>;
}
```

- Na thread `mt-db-writer`, cada job `Tx` e `Batch` é envolvido em `std::panic::catch_unwind(AssertUnwindSafe(...))` sobre um bloco que abre `conn.transaction_with_behavior(TransactionBehavior::Immediate)`: se a closure entrar em panic (em perfis `unwind`), a `Transaction` sofre rollback automático no `Drop`, a thread sobrevive e responde `Err(DbError::WriterPanicked)` ao chamador (CA-18 / `writer_survives_panic`).

### 5.2. `src/reader.rs`

```rust
use r2d2::{Pool, PooledConnection};
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use crate::{config::DbConfig, error::DbError};

#[derive(Clone)]
pub(crate) struct ReadPool {
    pool: Pool<SqliteConnectionManager>,
    active_readers: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    idle_notify: std::sync::Arc<tokio::sync::Notify>,
}

impl ReadPool {
    pub fn new(cfg: &DbConfig) -> Result<Self, DbError>;
    pub fn get(&self) -> Result<PooledConnection<SqliteConnectionManager>, DbError>;

    /// Executa leitura dentro de `tokio::task::spawn_blocking` com progress handler (deadline de 5 s por consulta).
    pub async fn read<T, F>(&self, f: F) -> Result<T, DbError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, DbError> + Send + 'static;

    /// Aguarda todos os leitores ativos terminarem durante o `Db::shutdown`.
    pub async fn wait_idle(&self, timeout: std::time::Duration) -> bool;
}
```

---

## 6. Runner de Migrations (`src/migrations/`) e Schema Base

### 6.1. `migrations/0001_03_base.sql`

```sql
-- Fase 03: configurações chave/valor globais do MaxTerminal.
CREATE TABLE settings (
    key        TEXT    NOT NULL PRIMARY KEY
               CHECK (length(key) BETWEEN 3 AND 128
                      AND key NOT GLOB '*[^a-z0-9_.]*'
                      AND key GLOB '[a-z]*.[a-z]*'),
    value      TEXT    NOT NULL CHECK (json_valid(value) AND length(CAST(value AS BLOB)) <= 65536),
    updated_at INTEGER NOT NULL,
    revision   INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0)
) STRICT, WITHOUT ROWID;
```

### 6.2. Tipos e Contrato do Runner (`src/migrations/mod.rs`, `registry.rs`, `runner.rs`)

```rust
use rusqlite::{Connection, Transaction};
use serde::Serialize;
use crate::error::DbError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct MigrationInfo {
    pub version: u32,
    pub phase: u8,
    pub name: String,
    pub checksum: String,
}

pub enum MigrationKind {
    Sql(&'static str),
    Code(fn(&Transaction<'_>) -> Result<(), DbError>),
}

pub struct CodeMigration {
    pub version: u32,
    pub phase: u8,
    pub name: &'static str,
    pub checksum_seed: &'static str,
    pub kind: MigrationKind,
}

/// Calcula o SHA-256 hex (64 chars minúsculos) normalizando `\r\n` para `\n`.
#[must_use]
pub fn compute_migration_checksum(raw: &str) -> String;

/// Valida o nome de um arquivo SQL contra `^(\d{4})_(\d{2})_([a-z0-9_]+)\.sql$`.
pub fn parse_migration_filename(filename: &str) -> Result<(u32, u8, String), DbError>;

pub(crate) struct MigrationRunner<'a> {
    conn: &'a mut Connection,
    migrations: Vec<ResolvedMigration>,
}

impl<'a> MigrationRunner<'a> {
    /// Carrega migrations embutidas via `include_dir!("$CARGO_MANIFEST_DIR/migrations")` + `code_migrations`,
    /// valida nomenclatura, ausência de duplicatas e sequência contígua `1..=N`.
    pub fn new(conn: &'a mut Connection, code_migrations: Vec<CodeMigration>) -> Result<Self, DbError>;

    /// Construtor para testes que permite fornecer lista arbitrária de migrations SQL/Code.
    pub fn with_custom_migrations(
        conn: &'a mut Connection,
        migrations: Vec<CodeMigration>,
    ) -> Result<Self, DbError>;

    /// Garante a tabela `schema_migrations`, valida `user_version` contra `known`, verifica checksums das
    /// migrations já aplicadas e retorna a lista de migrations pendentes.
    pub fn pending(&mut self) -> Result<Vec<MigrationInfo>, DbError>;

    /// Aplica todas as migrations pendentes em transações `IMMEDIATE` individuais.
    /// Se `pre_migration_hook` for fornecido e o banco já contiver dados (`applied_count > 0` ou tabelas existentes),
    /// invoca o hook com a versão-alvo antes da primeira migration pendente.
    pub fn run<H>(&mut self, pre_migration_hook: H) -> Result<Vec<MigrationInfo>, DbError>
    where
        H: FnOnce(&Connection, u32) -> Result<(), DbError>;
}
```

**Tabela canônica de migrations (D-01):** o runner exige versões contíguas `1..=N`; a numeração `NNNN` é a versão
global sequencial e **não** o número da fase. Placeholders literais `NNNN_` são proibidos em qualquer documento.
Todos os arquivos ficam em `src-tauri/crates/mt-db/migrations/`:

| Versão | Arquivo | Fase dona |
|---|---|---|
| 1 | `0001_03_base.sql` | 03 (única migration entregue por esta fase) |
| 2 | `0002_05_shell_sessions_command_blocks.sql` | 05 |
| 3 | `0003_06_projects.sql` | 06 |
| 4 | `0004_07_workspace_templates_tabs_sessions.sql` | 07 |
| 5 | `0005_09_os_settings.sql` | 09 |
| 6 | `0006_10_themes.sql` | 10 |
| 7 | `0007_12_ai.sql` | 12 |
| 8 | `0008_13_agents_skills_memory.sql` | 13 |
| 9 | `0009_15_command_groups.sql` | 15 |
| 10 | `0010_16_github.sql` | 16 |
| 11 | `0011_17_search.sql` | 17 |

O teste `naming_contract` valida nome, duplicata e lacuna contra a lista de arquivos presente em `migrations/`
no momento da execução (fases posteriores acrescentam seus arquivos na ordem acima).

**Regras críticas do `MigrationRunner` (spec §4.3):**
1. Tabela de bootstrap criada fora das migrations versionadas:
   ```sql
   CREATE TABLE IF NOT EXISTS schema_migrations (
       version     INTEGER PRIMARY KEY,
       phase       INTEGER NOT NULL CHECK (phase BETWEEN 1 AND 99),
       name        TEXT    NOT NULL,
       checksum    TEXT    NOT NULL CHECK (length(checksum) = 64),
       applied_at  INTEGER NOT NULL,
       duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0)
   ) STRICT;
   ```
2. Se `PRAGMA user_version` (`found`) > `max(version)` (`known`) → `Err(DbError::SchemaTooNew { found, known })` (CA-15).
3. Se `found` divergir de `COALESCE(MAX(version), 0)` em `schema_migrations` ou se houver lacunas nas versões aplicadas → `Err(DbError::Corrupt)`.
4. Para cada linha já presente em `schema_migrations`, comparar `checksum` gravado com o checksum calculado; se diferente → `Err(DbError::MigrationChecksumMismatch { version })` (CA-13).
5. Para cada pendente `m`:
   - `let tx = self.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;`
   - Executar `Sql(sql)` via `tx.execute_batch(sql)` ou `Code(f)` via `f(&tx)`. Qualquer erro faz rollback da transação atual e retorna `Err(DbError::MigrationFailed { version: m.version, name: m.name, message })` (CA-14).
   - Executar `PRAGMA foreign_key_check` dentro da transação; se retornar qualquer linha, abortar com `DbError::MigrationFailed` (CA-11).
   - Inserir registro em `schema_migrations` e atualizar `PRAGMA user_version = <m.version>`.
   - `tx.commit()?`.
6. Ao final de todas as migrations pendentes, executar `PRAGMA optimize;`.

---

## 7. Infraestrutura FTS5 e Contrato de Busca/Paginação (`src/fts.rs`)

```rust
use rusqlite::Connection;
use crate::error::DbError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FtsTokenizer {
    /// `unicode61 remove_diacritics 2`
    Text,
    /// `unicode61 remove_diacritics 2 tokenchars '-_./:@'`
    Code,
}

#[derive(Debug, Clone)]
pub struct FtsSpec {
    pub content_table: &'static str,
    pub fts_table: &'static str,
    pub columns: &'static [&'static str],
    pub tokenizer: FtsTokenizer,
    pub prefix: &'static [u8],
}

impl FtsSpec {
    /// Valida que `content_table`, `fts_table` e todos os itens de `columns` casam com `^[a-z][a-z0-9_]*$`.
    pub fn validate(&self) -> Result<(), DbError>;

    /// Gera o DDL completo: `CREATE VIRTUAL TABLE ... USING fts5(...)` + triggers `_ai`, `_ad` e `_au`.
    #[must_use]
    pub fn ddl(&self) -> String;

    /// `INSERT INTO {fts}({fts}) VALUES('rebuild');`
    #[must_use]
    pub fn rebuild_sql(&self) -> String;

    /// `INSERT INTO {fts}({fts}) VALUES('optimize');`
    #[must_use]
    pub fn optimize_sql(&self) -> String;

    /// `INSERT INTO {fts}({fts}, rank) VALUES('integrity-check', 1);`
    #[must_use]
    pub fn integrity_check_sql(&self) -> String;
}

#[derive(Debug, Clone, Default)]
pub struct FtsRegistry {
    specs: Vec<FtsSpec>,
}

impl FtsRegistry {
    pub fn new(specs: Vec<FtsSpec>) -> Result<Self, DbError>;
    pub fn specs(&self) -> &[FtsSpec];
    pub fn optimize_all(&self, conn: &Connection) -> Result<(), DbError>;
    pub fn integrity_check_all(&self, conn: &Connection) -> Result<(), DbError>;
}

/// Converte entrada do usuário em expressão `MATCH` FTS5 segura:
/// - Limita a 256 caracteres e no máximo 16 termos.
/// - Extrai sequências de caracteres alfanuméricos Unicode (ou `tokenchars` permitidos quando dentro de palavra),
///   descartando operadores FTS5 (`AND`, `OR`, `NOT`, `NEAR`), curingas soltos (`*`, `^`), filtros de coluna (`col:`) e pontuação isolada.
/// - Envolve cada termo entre aspas duplas (escapando `"` interno como `""`) e concatena com ` AND `.
/// - Ao último termo adiciona o sufixo de prefixo `*` fora das aspas: `"npm" AND "ru"*`.
/// - Retorna `None` se a entrada for vazia ou contiver apenas pontuação/espaços.
#[must_use]
pub fn build_match_query(input: &str) -> Option<String>;
```

### 7.1. Utilitários de Paginação Keyset e Snapshots de Busca (`src/fts.rs`)

Para suportar o contrato da spec §6.2 e os testes de `tests/pagination.rs` (CA-45):
- **`KeysetCursor`**: codificador/decodificador de cursor opaco versionado (`v1:<op>:<filter_hash>:<watermark>:<sort_key>:<tie_breaker>`), limitado a 2 KiB, que retorna `Err(DbError::InvalidCursor)` se corrompido, adulterado ou vinculado a outra operação/filtro.
- **`SearchSnapshotCache`**: cache em memória RAM limitado a **8 snapshots** e **8 MiB** totais, com TTL de **30 s**, invalidado automaticamente em qualquer mutação de histórico (`invalidate_all()`), retornando `Err(DbError::CursorExpired)` quando um cursor referencia um snapshot expirado ou evictado.
- Formatação de snippets segura: delimitadores `«` e `»` apenas sobre texto já sanitizado, nunca retornando tokens `[REDACTED:*]` parciais nem HTML.

---

## 8. Testes Obrigatórios da Implementação 02

1. **Unit tests em `src/connection.rs` (CA-07, CA-08, CA-09):**
   - `pragmas_applied`: verifica no writer `journal_mode == "wal"`, `auto_vacuum == 2`, `foreign_keys == 1`, `busy_timeout == 5000`, `synchronous == 1`; e no reader `query_only == 1`.
   - `permissions_0600`: verifica modo `0700` no diretório e `0600` em `.db`, `-wal` e `-shm`.
   - `instance_lock_prevents_second_writer`: segunda tentativa de `InstanceLock::acquire` no mesmo caminho retorna `Err(DbError::InUse)`.
2. **`tests/migrations.rs` (CA-11 a CA-16):**
   - `fresh_db_applies_base`: aplica migration `1|3|base`, verifica `PRAGMA user_version == 1`, existência exclusiva de `schema_migrations` e `settings` (sem tabelas de fases futuras) e `foreign_key_check` vazio.
   - `idempotent_reopen`: reabrir banco não reaplica migrations.
   - `checksum_mismatch`: adulterar checksum/SQL de migration já aplicada falha com `db.migration_checksum_mismatch`.
   - `failed_migration_rolls_back`: migration com SQL inválido faz rollback completo e retorna `db.migration_failed` com `details.version`.
   - `schema_too_new`: banco com `PRAGMA user_version = 99` falha ao abrir com `db.schema_too_new`.
   - `naming_contract`: varre `migrations/` e falha se houver nome inválido, versão duplicada ou lacuna.
   - `code_migration_runs` e `foreign_key_check_after_migrate`.
3. **`tests/concurrency.rs` (CA-18, CA-23):**
   - `writer_and_readers`: 1 escritor inserindo 50 000 linhas enquanto 8 tarefas Tokio leem continuamente via `ReadPool` por 5 s → zero erros `SQLITE_BUSY`/`SQLITE_LOCKED`.
   - `writer_survives_panic`: closure que faz panic em `execute_tx` sofre rollback e o writer continua processando transações subsequentes.
   - `write_timeout_returns_busy`: fila saturada retorna `DbError::Busy` após o orçamento de espera sem travar o runtime async.
4. **`tests/fts.rs` (CA-10, CA-24, CA-25, CA-26):**
   - `fts5_available`: confirma `ENABLE_FTS5 == 1`.
   - `external_content_roundtrip`: tabela fixture `t(rowid INTEGER PRIMARY KEY, a TEXT, b TEXT) STRICT` + `FtsSpec::ddl()` mantém índice sincronizado em insert, update, delete, rollback e cascade, validado por `integrity-check` com `rank = 1`.
   - `diacritics_removed` e `prefix_search`: busca por `acao` encontra `ação` e `AÇÃO`; busca `npm ru` encontra `npm run dev`.
   - `ddl_matches_handwritten` e `proptest` `build_match_query_never_syntax_error` com 10 000 entradas arbitrárias (`"`, `NEAR(`, `a OR b`, `*`, `-x`, `col:val`) executadas contra uma tabela FTS5 real sem nunca gerar erro de sintaxe SQLite.
5. **`tests/pagination.rs` (CA-44, CA-45):**
   - Paginação keyset sobre tabela fixture com desempate estável (`timestamp DESC, id DESC`), 3 páginas sem repetição nem omissão mesmo com inserções acima do watermark e deleções entre páginas.
   - Rejeição de cursor inválido (`db.cursor_invalid`) e expiração/invalidação de snapshot de busca (`db.cursor_expired`).
