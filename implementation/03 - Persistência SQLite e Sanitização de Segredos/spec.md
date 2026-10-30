# Fase 03 — Persistência SQLite e Sanitização de Segredos (`spec.md`)

> Complementa [`./instructions.md`](./instructions.md). Consome apenas contratos da Fase 01
> (`mt-core::{MtError, ErrorCode, MtResult, paths::AppPaths}`, `tauri-specta`, `LogRedactor`, `src/ipc/core.ts`,
> `useIpcError`). Expõe pontos de extensão para as Fases 05, 06, 07, 10, 11, 12, 13, 15, 16 e 17.

---

## 1. Decisão de Driver: `rusqlite` (bundled) + `r2d2` vs. `tauri-plugin-sql`

| Critério | `rusqlite` bundled + writer thread + `r2d2` | `tauri-plugin-sql` (sqlx) |
|----------|---------------------------------------------|---------------------------|
| Garantia "sanitizar antes de gravar" (AGENTS.md §3, Diretriz §4) | Toda escrita passa por APIs Rust tipadas com `SanitizedText` | SQL é emitido do JavaScript; o WebView poderia gravar texto cru |
| Acesso a partir de crates Rust (`mt-shell`, `mt-pty` via 05/07) | Direto, sem passar pelo WebView | Plugin é pensado para o frontend; uso Rust é secundário |
| FTS5, JSON1, `STRICT` | Garantidos pelo `libsqlite3-sys` bundled (SQLite ≥ 3.45, `SQLITE_ENABLE_FTS5`) | Dependem do build do `libsqlite3-sys` do sqlx; menos controle |
| Serialização de escritas / batching de scrollback | Controle total (uma thread, transações agrupadas) | Pool sqlx genérico, sem política de batch |
| Superfície de ataque IPC | Apenas comandos de domínio | Expõe `execute/select` arbitrários ao WebView (exigiria capabilities restritivas) |
| Online Backup API, `update_hook`, funções SQL customizadas | Disponíveis (`backup`, `functions`) | Indisponíveis/indiretos |

**Decisão:** `rusqlite` com features `bundled`, `backup`, `functions`, `serde_json`. Leitores via `r2d2` +
`r2d2_sqlite` (as leituras já rodam em `spawn_blocking`, logo um pool síncrono simples basta; `deadpool-sqlite`
adicionaria um runtime de interação sem benefício). `tauri-plugin-sql` **não** é adicionado ao projeto.

Fixar versões compatíveis de `rusqlite`/`r2d2_sqlite` no lockfile, evitando duas versões de `libsqlite3-sys`.
Validar `sqlite_version()`, FTS5 e JSON na conexão real; `bundled` não substitui teste do binário distribuído.
Para builds novos, exigir SQLite ≥ 3.51.3 (correção do WAL-reset; backports só com validação explícita).

---

## 2. Arquitetura de Módulos (`src-tauri/crates/mt-db`)

```
mt-db/
├── migrations/0001_03_base.sql
├── src/
│   ├── lib.rs            # Db (fachada), re-exports
│   ├── config.rs         # DbConfig, PragmaSet
│   ├── connection.rs     # open_writer/open_reader, PRAGMAs, permissões, quick_check
│   ├── error.rs          # DbError, códigos db.*, From<DbError> for MtError
│   ├── writer.rs         # DbWriter + thread escritora
│   ├── reader.rs         # ReadPool (r2d2)
│   ├── batch.rs          # BatchRecord, BatchWriter, BatchPolicy
│   ├── migrations/{mod.rs, runner.rs, registry.rs}
│   ├── fts.rs            # FtsSpec, FtsTokenizer, build_match_query
│   ├── settings/{mod.rs, registry.rs, repo.rs}
│   ├── models.rs          # DTOs de settings/status/manutenção
│   ├── maintenance/{mod.rs, retention.rs, backup.rs, scheduler.rs}
│   ├── sanitizer/{mod.rs, rules.rs, stream.rs}
│   └── time.rs
├── tests/ … benches/ …
```

### 2.1. Fachada `Db`

```rust
/// Fachada única de persistência; barata de clonar (Arc interno).
#[derive(Clone)]
pub struct Db { inner: Arc<DbInner> }

struct DbInner {
    config: DbConfig,
    writer: DbWriter,
    readers: ReadPool,
    settings: SettingsRegistry,
    retention: RetentionRegistry,
    fts: FtsRegistry,
    batches: Mutex<Vec<Weak<dyn BatchFlush>>>,
    events: Arc<dyn DbEventSink>,
}

impl Db {
    /// Abre/cria o banco, valida integridade, faz backup pré-migration e aplica migrations.
    pub fn open(config: DbConfig, setup: DbSetup) -> Result<(Db, OpenReport), DbError>;

    /// Executa um fechamento de escrita na writer thread (uma transação IMMEDIATE).
    pub async fn write<T, F>(&self, f: F) -> Result<T, DbError>
    where T: Send + 'static,
          F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T, DbError> + Send + 'static;

    /// Executa leitura numa conexão do pool dentro de `spawn_blocking`.
    pub async fn read<T, F>(&self, f: F) -> Result<T, DbError>
    where T: Send + 'static,
          F: FnOnce(&rusqlite::Connection) -> Result<T, DbError> + Send + 'static;

    /// Cria um escritor em lote para um tipo de registro (Fases 05/07: scrollback, eventos).
    pub fn batch_writer<R: BatchRecord>(&self, policy: BatchPolicy) -> BatchWriter<R>;

    pub fn settings(&self) -> SettingsRepo;
    pub fn maintenance(&self) -> MaintenanceHandle;
    /// Após drenar produtores, fecha ingresso do banco, drena commits/leitores e encerra workers fora da UI.
    pub async fn shutdown(&self) -> Result<ShutdownReport, DbError>;
}

/// Contribuições das fases, coletadas em `src-tauri/src/lib.rs` antes de `Db::open`.
#[derive(Default)]
pub struct DbSetup {
    pub events: Option<Arc<dyn DbEventSink>>,
    pub code_migrations: Vec<CodeMigration>,
    pub settings: Vec<SettingDescriptor>,
    pub retention: Vec<RetentionRule>,
    pub fts: Vec<FtsSpec>,
}

pub struct OpenReport {
    pub applied: Vec<MigrationInfo>,
    pub backup_path: Option<PathBuf>,
    pub recovered: Option<DbRecovered>,
    pub created: bool,
}

/// Desacopla mt-db do Tauri: o crate binário implementa via AppHandle::emit.
pub trait DbEventSink: Send + Sync + 'static {
    fn settings_changed(&self, ev: &SettingsChanged);
    fn recovered(&self, ev: &DbRecovered);
    fn maintenance_finished(&self, ev: &DbMaintenanceFinished);
    fn writer_lag(&self, ev: &DbWriterLag);
}
```

### 2.2. Configuração e PRAGMAs

```rust
pub struct DbConfig {
    pub path: PathBuf,            // AppPaths.db_path ou override desta fase em debug/testes
    pub backup_dir: PathBuf,      // <data>/maxterminal/backups
    pub reader_pool_size: u32,    // padrão 4
    pub write_queue_capacity: usize, // padrão 4096 jobs
    pub write_queue_bytes: usize, // padrão 16 MiB; compartilhado por todos os batches
    pub pragmas: PragmaSet,
}

pub struct PragmaSet {
    pub busy_timeout_ms: u32,     // 5000
    pub cache_size_kib: i64,      // 16000  -> PRAGMA cache_size = -16000
    pub mmap_size: i64,           // 256 MiB
}
```

Sequência aplicada por `connection::open_writer` (ordem importa):

```sql
-- somente quando o arquivo é novo (antes de qualquer CREATE TABLE):
PRAGMA auto_vacuum = INCREMENTAL;
-- sempre:
PRAGMA journal_mode = WAL;          -- verificado: o retorno DEVE ser 'wal', senão DbError::Pragma
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA temp_store = MEMORY;
PRAGMA cache_size = -16000;
PRAGMA mmap_size = 268435456;
PRAGMA wal_autocheckpoint = 1000;
PRAGMA journal_size_limit = 67108864;  -- limite ao reciclar WAL, não limite rígido durante uso
```

Leitores (`open_reader`): `OpenFlags::SQLITE_OPEN_READ_ONLY | NO_MUTEX | URI`, mais `foreign_keys`,
`busy_timeout`, `cache_size`, `mmap_size`, `query_only = ON`. Permissões: diretório criado com `0700`
(`DirBuilderExt::mode`), arquivo pré-criado com `OpenOptionsExt::mode(0o600)` antes do `Connection::open`;
`-wal`/`-shm` devem nascer restritos pelo VFS Unix a partir do modo do banco e ser verificados em teste.
Não alterar `umask` em runtime: é global ao processo. Diretório privado precede qualquer criação;
recusar symlinks/caminhos inseguros, verificar proprietário e corrigir permissões existentes antes de abrir.
`MAXTERMINAL_DB_PATH` exige caminho absoluto em debug/testes; backups e lock ficam junto do banco isolado.
Adquirir lock exclusivo do aplicativo em `<data>/maxterminal.db.lock` antes de migrations/recuperação e mantê-lo
até fechar. Outra instância recebe `db.in_use`; WAL continua permitindo conexões leitoras para inspeção.

### 2.3. Writer thread

```rust
pub(crate) enum WriteJob {
    Tx { f: Box<dyn FnOnce(&Transaction<'_>) -> Result<Box<dyn Any + Send>, DbError> + Send>,
         reply: oneshot::Sender<Result<Box<dyn Any + Send>, DbError>> },
    Batch { flush: Box<dyn FnOnce(&Transaction<'_>) -> Result<usize, DbError> + Send>,
            reply: oneshot::Sender<Result<usize, DbError>> },
    Raw { f: Box<dyn FnOnce(&mut Connection) -> Result<(), DbError> + Send>,  // VACUUM e checkpoint
          reply: oneshot::Sender<Result<(), DbError>> },
    Shutdown { reply: oneshot::Sender<()> },
}

pub(crate) struct DbWriter {
    tx: crossbeam_channel::Sender<WriteJob>,
    handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}
```

- Thread nomeada `mt-db-writer`; loop `recv()` → `conn.transaction_with_behavior(Immediate)` → executa → `commit`.
- Jobs `Tx` consecutivos **não** são fundidos (semântica de erro por chamada); jobs `Batch` já chegam agregados.
- Em builds com unwind, panic pode ser capturado e resultar em rollback/`WriterPanicked`. A Fase 01 usa
  `panic=abort` em release: não prometer sobrevivência da thread nesse perfil; a reabertura recupera o WAL.
- Fila cheia em `Db::write` → espera por capacidade **assíncrona** com orçamento de 250 ms e `try_send`;
  nunca `crossbeam::send_timeout` na thread async. Reservar bytes antes de enfileirar; recusar job >1 MiB.
- Cada resposta de sucesso e evento sai **após COMMIT**. Cancelar o futuro de quem espera não desfaz um job
  já aceito: operação pode ter sido aplicada; ids e revisão permitem reconciliar/repetir com idempotência.
- `Db::read/write` são pontos de extensão **públicos Rust**, consumidos por repositórios confiáveis das fases
  posteriores, inclusive a 05. `DbWriter`/fila permanecem privados; não há SQL genérico no IPC. Closures
  recebem Connection/Transaction apenas durante o job e não os retêm. Essa API privilegiada pode executar
  SQL: a barreira `SanitizedText` é obrigatória nos registros/repositórios de conteúdo, não uma prova de que
  qualquer closure arbitrária seja segura. Revisar cada bind de texto e testar os sinks das integrações.

### 2.4. Pool de leitores

```rust
pub(crate) struct ReadPool { pool: r2d2::Pool<SqliteConnectionManager> }
impl ReadPool {
    pub fn new(cfg: &DbConfig) -> Result<Self, DbError>; // with_init aplica PRAGMAs de leitor
    pub fn get(&self) -> Result<PooledConnection<SqliteConnectionManager>, DbError>; // timeout 2 s -> db.busy
}
```

Em WAL, leitores enxergam o snapshot commitado no início da leitura; leituras longas retêm WAL e impedem
checkpoint completo. Tratar `BUSY` mesmo em WAL. Statements usam `prepare_cached` (cache de 64 por conexão).
Uma página ocupa uma transação curta; nenhum cursor mantém conexão entre chamadas IPC. `spawn_blocking`
não é cancelado só porque o futuro foi descartado: usar deadline/progress handler e liberar o pool ao terminar.

---

## 3. Erros

```rust
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("falha ao abrir o banco de dados em {path}: {source}")]
    Open { path: PathBuf, #[source] source: rusqlite::Error },
    #[error("PRAGMA {name} não pôde ser aplicado (obtido: {got})")]
    Pragma { name: &'static str, got: String },
    #[error("banco ocupado; tente novamente")]
    Busy,
    #[error("violação de restrição: {0}")]
    Constraint(String),
    #[error("registro não encontrado")]
    NotFound,
    #[error("falha na migration {version} ({name}): {message}")]
    MigrationFailed { version: u32, name: String, message: String },
    #[error("a migration {version} foi alterada após ser aplicada")]
    MigrationChecksumMismatch { version: u32 },
    #[error("o banco foi criado por uma versão mais nova do MaxTerminal (schema {found} > {known})")]
    SchemaTooNew { found: u32, known: u32 },
    #[error("banco de dados corrompido")]
    Corrupt,
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
    #[error(transparent)]
    Sqlite(#[from] rusqlite::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

Códigos (constantes `ErrorCode` com prefixo de domínio, padrão da Fase 01, em `mt_db::error::codes`):

| Variante | `code` | `details` |
|----------|--------|-----------|
| `Open`, `Io` | `db.open_failed` | — (caminho bruto não serializado) |
| `Pragma` | `db.pragma_failed` | `{ name, got }` |
| `Busy`, `Backpressure` | `db.busy` | — |
| `Constraint` | `db.constraint` | `{ constraint: código_estático }` |
| `NotFound` | `db.not_found` | — |
| `MigrationFailed` | `db.migration_failed` | `{ version, name }` |
| `MigrationChecksumMismatch` | `db.migration_checksum_mismatch` | `{ version }` |
| `SchemaTooNew` | `db.schema_too_new` | `{ found, known }` |
| `Corrupt` | `db.corrupt` | — |
| `WriterClosed`, `WriterPanicked`, `Sqlite` (demais) | `db.internal` | — (causa só em diagnóstico sanitizado) |
| `SettingUnknown` / `SettingInvalid` / `SettingTooLarge` / `SettingSensitive` | `db.setting_unknown` / `db.setting_invalid` / `db.setting_too_large` / `db.setting_sensitive` | `{ key, reason? }` |
| `Backup` | `db.backup_failed` | — |

Mapeamento de `rusqlite::Error::SqliteFailure`: `DatabaseBusy`/`DatabaseLocked` → `Busy`;
`ConstraintViolation` → `Constraint`; `DatabaseCorrupt`/`NotADatabase` → `Corrupt`. `impl From<DbError> for MtError`
vive em `error.rs`. Mensagens enviadas ao frontend nunca contêm SQL nem valores de parâmetros.
Acrescentar variantes/códigos `InUse/db.in_use`, `RecoveryRequired/db.recovery_required`,
`Conflict/db.conflict`, `InvalidCursor/db.cursor_invalid`, `CursorExpired/db.cursor_expired`,
`InvalidQuery/db.query_invalid`, `StorageFull/db.storage_full`, `ShutdownTimeout/db.shutdown_timeout`
e `NotReady/db.not_ready`. Campo `details` usa allowlist de ids, versão, contadores e nomes estáticos.
Os derives `Debug`/`Display` ilustrativos acima **não** autorizam logar `source`, caminhos ou strings arbitrárias:
implementar representação segura dos erros, inclusive para panic hook. Não registrar SQL/params nem query FTS.

---

## 4. Migrations

### 4.1. Convenções
- Arquivo: `migrations/NNNN_FF_desc.sql` — `NNNN` = versão global sequencial (atribuída no merge da fase),
  `FF` = número da fase dona, `desc` em `snake_case` inglês. Nesta fase: `0001_03_base.sql`;
  próxima versão disponível é atribuída globalmente à migration da fase dona.
- Uma migration é imutável após mergeada; correções exigem nova migration.
- Tabelas indexadas têm `rowid INTEGER PRIMARY KEY` **explícito** (estável após VACUUM), além do id `TEXT`
  UUID v7 `UNIQUE NOT NULL`. `content_rowid` usa esse inteiro; ids públicos nunca dependem de rowid.
  Datas são `INTEGER` Unix ms; tabelas ordinárias novas são `STRICT` (FTS5 não aceita `STRICT`).
- Toda FK declara `ON DELETE` explicitamente.

### 4.2. Tipos

```rust
pub struct MigrationInfo { pub version: u32, pub phase: u8, pub name: String, pub checksum: String }

pub enum MigrationKind {
    Sql(&'static str),
    /// Para migrations que precisam de lógica (ex.: gerar DDL FTS via FtsSpec, backfill).
    Code(fn(&rusqlite::Transaction<'_>) -> Result<(), DbError>),
}

pub struct CodeMigration {
    pub version: u32,
    pub phase: u8,
    pub name: &'static str,
    /// Checksum declarado manualmente (string estável), pois código não tem conteúdo hasheável.
    pub checksum_seed: &'static str,
    pub kind: MigrationKind,
}

pub(crate) struct MigrationRunner<'a> { conn: &'a mut Connection, set: Vec<Migration> }
impl MigrationRunner<'_> {
    pub fn pending(&self) -> Result<Vec<MigrationInfo>, DbError>;
    pub fn run(&mut self) -> Result<Vec<MigrationInfo>, DbError>;
}
```

### 4.3. Algoritmo do runner
1. `CREATE TABLE IF NOT EXISTS schema_migrations (...)` (bootstrap, fora das migrations versionadas).
2. Carrega `include_dir!("$CARGO_MANIFEST_DIR/migrations")` + `DbSetup.code_migrations`; valida nomes, duplicatas
   e contiguidade (falha de programação → panic em debug, `MigrationFailed` em release).
3. `known = max(version)`; `found = PRAGMA user_version`. Se `found > known` → `SchemaTooNew`.
4. Para cada migration aplicada, compara `checksum` (SHA-256 hex do SQL normalizado `\r\n`→`\n`, ou de
   `checksum_seed`) → divergência = `MigrationChecksumMismatch`.
5. Se há pendentes e o banco não é novo → `backup::create(BackupKind::PreMigration { target })`.
6. Validar também que `user_version` coincide com o máximo aplicado, sem lacunas nem versões desconhecidas.
   Para cada pendente: `BEGIN IMMEDIATE` → executa → `foreign_key_check` e invariantes FTS →
   `INSERT INTO schema_migrations` → `PRAGMA user_version = N` → `COMMIT`. Erro → rollback da migration atual.
   Migrations anteriores já commitadas permanecem; reabertura continua a partir delas.
7. Após aplicar: `PRAGMA optimize`. Migrations nunca interpolam dados externos ou gravam texto cru para
   sanitizar depois; backfills usam o pipeline antes do bind. Falha de backup impede migration.

### 4.4. DDL — bootstrap e `0001_03_base.sql`

```sql
-- Bootstrap (runner.rs)
CREATE TABLE IF NOT EXISTS schema_migrations (
    version     INTEGER PRIMARY KEY,
    phase       INTEGER NOT NULL CHECK (phase BETWEEN 1 AND 99),
    name        TEXT    NOT NULL,
    checksum    TEXT    NOT NULL CHECK (length(checksum) = 64),
    applied_at  INTEGER NOT NULL,           -- Unix ms
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0)
) STRICT;
```

```sql
-- migrations/0001_03_base.sql
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

`settings` não tem FTS. A Fase 03 cria somente o schema de suporte acima.

### 4.5. Ownership e adoção futura do histórico

| Fase dona | Migration/tabelas | Contrato de adoção |
|---|---|---|
| 03 | `0001_03_base.sql`: settings; bootstrap schema_migrations | WAL, runner, writer/readers, sanitizer, FTS/retention registry; nenhuma tabela de sessão/comando/scrollback |
| 05 | `NNNN_05_shell_sessions_command_blocks.sql`: sessions, command_blocks, command_blocks_fts | Cria sessões mínimas por pane, blocos e índices; repositórios/SessionProvider usam Db::read/write |
| 07 | Migration própria com extensão de sessions e scrollback/FTS | Consome sessions da 05 sem recriar; acrescenta arquivamento/workspace, retenção e UI |

A 03 não entrega `commands`, `scrollback_chunks`, `SessionsRepo`, `CommandsRepo` ou `ScrollbackRepo`.
O DDL canônico de sessões/blocos está na spec da 05 §7.1. Não duplicá-lo nesta fase; versões globais são
atribuídas na integração. FKs só apontam para tabelas da própria migration ou de fases anteriores já aplicadas.

- A 05 mantém `tab_id/pane_id/project_id` como identidades lógicas sem FK para tabelas futuras; repositórios
  dela validam vínculos. A 07 pode acrescentar relacionamentos apenas após tabelas-alvo existirem, com
  backfill/validação e preservação dos ids e rowids FTS; não cria outra sessão independente.
- `command_blocks.id` usa `CommandBlockId`; não introduzir a tabela concorrente `commands` na 03/07.
  Semântica de status, `sessions.next_seq`, fechamento e recuperação de execuções pertence à 05.
- A 07 é dona da sequência e retenção de scrollback. Não impor `sessions.last_seq` ou coluna inexistente
  pela infraestrutura. Consistência entre contador, registros e índices deve usar uma transação `Db::write`.
- Repositórios futuros recebem texto com `SanitizedText`, calculam tamanho/versão no backend, truncam só
  depois da sanitização e registram lacunas sem bytes crus. Cópias sanitizadas de caminhos servem à exibição,
  não à restauração de credenciais/processos. Não persistir ambiente completo.
- Repetição idêntica de operação é idempotente; conflito não deve usar `INSERT OR REPLACE`, que pode gerar
  cascatas/reindexação incorreta. Eventos de sucesso só após commit. Cada fase define DTOs e erros do seu
  repositório; a 03 fornece `DbError`, transações e callbacks de infraestrutura.

### 4.6. Contrato de shutdown da infraestrutura

```rust
pub struct ShutdownReport { pub committed_records: u64, pub checkpoint_complete: bool }
```

`Db::shutdown` confirma apenas registros aceitos. A integração 05/07 drena seus produtores/sanitizadores
e fecha o ciclo de vida de domínio antes de encerrar Db; fixtures da 03 simulam esses hooks sem depender
dos crates posteriores. Não há recuperação de sessões ou `SessionProvider` implementado nesta fase.

---

## 5. Batching (`batch.rs`)

```rust
/// Registro gravável em lote, implementado por repositórios Rust confiáveis com conteúdo sanitizado.
pub trait BatchRecord: Send + 'static {
    /// INSERT parametrizado; preparado uma vez por flush (`prepare_cached`).
    const INSERT_SQL: &'static str;
    fn bind(&self, stmt: &mut rusqlite::Statement<'_>) -> rusqlite::Result<usize>;
    /// Estimativa de bytes para a política de flush.
    fn approx_bytes(&self) -> usize;
}

#[derive(Clone, Copy)]
pub struct BatchPolicy {
    pub max_delay: Duration,   // 200 ms
    pub max_records: usize,    // 512
    pub max_bytes: usize,      // 1 MiB
    pub queue_capacity: usize, // 8192 registros
    pub queue_bytes: usize,    // orçamento global compartilhado de 16 MiB
}

pub struct BatchWriter<R: BatchRecord> {
    tx: crossbeam_channel::Sender<R>,
    stats: Arc<BatchStats>,
}

impl<R: BatchRecord> BatchWriter<R> {
    /// Não bloqueante. Fila cheia -> Err(DbError::Backpressure) + DbWriterLag (rate limit 1/s).
    pub fn push(&self, record: R) -> Result<(), DbError>;
    /// Força flush e aguarda commit (usado ao arquivar sessão — Fase 07 — e no shutdown).
    pub async fn flush(&self) -> Result<(), DbError>;
    pub fn stats(&self) -> BatchStatsSnapshot;
}

#[derive(Default)]
pub struct BatchStats { pub pushed: AtomicU64, pub flushed_records: AtomicU64,
                        pub flushes: AtomicU64, pub rejected: AtomicU64, pub last_flush_ms: AtomicU64 }

pub(crate) trait BatchFlush: Send + Sync { fn flush_blocking(&self) -> Result<(), DbError>; }
```

Funcionamento: uma thread agregadora leve por `BatchWriter` (`mt-db-batch-<tabela>`) faz `recv_timeout`,
acumula até atingir qualquer limite e envia **um** `WriteJob::Batch` com o `Vec<R>`; a writer thread executa
todos os `bind` + `execute` em uma única transação. Ordem de inserção preservada por `BatchWriter`.
A segurança de conteúdo é responsabilidade do repositório confiável da fase dona: texto livre usa `SanitizedText`.
`BatchWriter::push` bem-sucedido confirma **aceitação**, não durabilidade. A agregadora mantém o lote até receber
o resultado de COMMIT; `flush` insere barreira ordenada e confirma todos os registros aceitos antes
dela. Erro de SQL/disco cheio fica latched e chega a `flush`/status; não descartar lote silenciosamente nem
repetir indefinidamente. Retry apenas para Busy com deadline e registros idempotentes; falha permanente
interrompe captura persistente, preserva erro seguro e sinaliza histórico incompleto.

Cada stream tem um produtor ordenado e um `StreamSanitizer` dedicado **antes** da fila. Intercalar bytes de
streams distintos é proibido. Backpressure nunca deve bloquear leitor PTY: produtor mantém no máximo um
lote sanitizado de retry ou descarta com contador e marcador de lacuna reservado. Se bytes crus forem perdidos
antes do sanitizador, chamar `on_gap()` e suprimir a unidade lógica corrente até delimitador seguro; retomar
sem esse reset poderia publicar a continuação de um token. Não usar arquivo temporário/spool cru.
Limites de bytes abrangem filas, lotes em voo e agregadoras, evitando multiplicar 16 MiB por painel.

Shutdown coordenado por `DbService`: fechar ingressos de produtores → drenar decoder/parser e `finish()` → enviar saída sanitizada final
→ barreiras/commits → parar manutenção → aguardar leitores ativos → checkpoint → fechar conexões e joins
em worker. Prazo padrão de 5 s é orçamento, não promessa de flush: timeout retorna erro com contagem de
pendências, mantém WAL e nunca relata persistência concluída. Sair abruptamente perde apenas dados ainda
não commitados, sujeito também à durabilidade NORMAL em queda de energia.
Na 03 os produtores são fixtures; fases consumidoras registram no `DbService` hooks assíncronos de parada
que aguardam dreno/finalização. `Db::shutdown` sozinho só conhece registros já aceitos: é chamado depois
desses hooks e não tenta acessar sanitizadores pertencentes a crates posteriores.

---

## 6. Infraestrutura FTS5 (`fts.rs`)

```rust
pub enum FtsTokenizer {
    /// Linguagem natural, saída de terminal: `unicode61 remove_diacritics 2`.
    Text,
    /// Comandos/caminhos: `unicode61 remove_diacritics 2 tokenchars '-_./:@'`.
    Code,
}

pub struct FtsSpec {
    pub content_table: &'static str,     // ex.: "command_blocks" (migration da Fase 05)
    pub fts_table: &'static str,         // ex.: "command_blocks_fts"
    pub columns: &'static [&'static str],
    pub tokenizer: FtsTokenizer,
    pub prefix: &'static [u8],           // padrão [2, 3]
}

impl FtsSpec {
    /// DDL completo: CREATE VIRTUAL TABLE + 3 triggers. Usado em CodeMigration ou para
    /// validar migrations SQL escritas à mão (`assert_eq!(normalize(sql), normalize(spec.ddl()))`).
    pub fn ddl(&self) -> String;
    pub fn rebuild_sql(&self) -> String;   // INSERT INTO x(x) VALUES('rebuild')
    pub fn optimize_sql(&self) -> String;  // INSERT INTO x(x) VALUES('optimize')
}

/// Converte texto do usuário em expressão MATCH segura: cada termo vira "termo" (aspas duplicadas
/// escapadas), operadores/colunas são neutralizados e o último termo recebe `*` (busca por prefixo).
/// Retorna None para entrada vazia/só pontuação.
pub fn build_match_query(input: &str) -> Option<String>;

pub struct FtsRegistry { specs: Vec<FtsSpec> }  // usado pela manutenção (optimize) e integrity-check
```

Template gerado por `ddl()` (exemplo com `t(a, b)`):

```sql
CREATE VIRTUAL TABLE t_fts USING fts5(
    a, b,
    content = 't', content_rowid = 'rowid',
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);
CREATE TRIGGER t_fts_ai AFTER INSERT ON t BEGIN
    INSERT INTO t_fts(rowid, a, b) VALUES (new.rowid, new.a, new.b);
END;
CREATE TRIGGER t_fts_ad AFTER DELETE ON t BEGIN
    INSERT INTO t_fts(t_fts, rowid, a, b) VALUES ('delete', old.rowid, old.a, old.b);
END;
CREATE TRIGGER t_fts_au AFTER UPDATE OF a, b ON t BEGIN
    INSERT INTO t_fts(t_fts, rowid, a, b) VALUES ('delete', old.rowid, old.a, old.b);
    INSERT INTO t_fts(rowid, a, b) VALUES (new.rowid, new.a, new.b);
END;
```

Identificadores em `FtsSpec` são validados contra `^[a-z][a-z0-9_]*$`; nunca interpolar entrada do usuário.
Executar `INSERT INTO t_fts(t_fts, rank) VALUES('integrity-check', 1)` para comparar com conteúdo externo;
o comando sem `rank=1` verifica apenas a consistência interna. Criar triggers não indexa linhas antigas:
em migration com dados existentes executar `rebuild` na mesma transação, somente sobre conteúdo sanitizado.

### 6.1. Registro de índices das fases donas

A 03 testa o template com `t(a,b)` em banco-fixture, sem instalar essa tabela em produção. A 05 cria
`command_blocks_fts(command,cwd,output_text)` sobre `command_blocks` na própria migration e registra o
`FtsSpec` equivalente em `DbSetup.fts` (inclusive prefixos `2 3 4` definidos na 05). Registro é catálogo para manutenção/validação, **não** executa DDL
automaticamente; evita dupla criação. A 07 faz o mesmo com o índice de scrollback que vier a criar.
`rowid` deve ser estável; updates de estado sem mudança textual não reindexam. Deletes/cascatas da fase
dona precisam manter triggers sincronizados. Nunca inserir texto bruto diretamente no FTS, indexar o
terminal visual ou guardar cópia crua para rebuild. Backfills de regras novas sanitizam antes do bind;
sanitização retroativa não desfaz exposição prévia em WAL/backups.

### 6.2. Contrato de adoção para busca e paginação (05/07/17)

Repositórios e IPC de histórico pertencem às fases donas, não são exports da 03. A infraestrutura fornece
`Db::read` e `build_match_query`; fixtures exercitam as recomendações seguintes sem depender de tabelas futuras.
Usar parâmetros SQL; consulta com até 256 caracteres e 16 termos; limite padrão 50, máximo 200.
`build_match_query` extrai termos com letras/números, coloca cada um em aspas com escape e aplica prefixo
apenas no último (`"npm" AND "ru"*`). Query vazia/pontuação retorna página vazia, não MATCH vazio.
Operadores escritos pelo usuário são texto literal; nunca permitir SQL/FTS avançado por concatenação.

| Operação | Ordenação e cursor | Filtro obrigatório/opcional |
|---|---|---|
| Sessões futuras | timestamp/id com keyset e watermark não reutilizável definido pela fase dona | filtros definidos na 05/07 |
| Blocos da 05 | `seq` por sessão, cursor/high-water conforme contrato de leitura da 05 | sessionId obrigatório |
| Scrollback | `seq ASC`, cursor lastSeq e máximo seq inicial | sessionId obrigatório; tamanho adicional máximo 1 MiB/página |
| Busca | snapshot materializado e limitado, `score ASC, domain ASC, sourceRowid ASC` | domínio sessions/commands/scrollback ou all; projectId/sessionId e intervalo opcionais |

Listagens não mantêm snapshot SQL entre páginas: registros apagados são omitidos, campos podem refletir
updates recentes, novas inserções acima do watermark ficam para nova consulta; desempate impede duplicação.
Scrollback reduz quantidade para caber no orçamento e informa a sequência efetiva de continuação.

Busca: numa transação de leitura curta, coletar até 1001 candidatos por domínio com `bm25`, ids e snippet
sanitizado, mesclar e guardar no máximo 1000 hits em cache **RAM**. Scores de índices diferentes não têm
normalização semântica; ordenação cruzada é determinística, não promessa de relevância global perfeita.
Snapshot tem TTL 30 s, limite global 8 MiB/8 snapshots, token aleatório vinculado à janela e filtros,
`truncated=true` se exceder 1000, `nextCursor=null` ao final. `search` seguinte com cursor não recalcula rank.
Evicção/reinício retorna `db.cursor_expired`. Qualquer mutação de histórico invalida snapshots após commit
para não servir conteúdo removido; frontend reinicia a consulta. Isso evita paginação por rank mutável.

Cursor de listagem é opaco, versionado, limitado a 2 KiB e vinculado à operação/filtros; validar UUIDs,
inteiros, watermark e direção. Cursor inválido → `db.cursor_invalid`. Não contém query crua nem texto.
Resultados: `{ items, nextCursor, truncated }`; nunca carregar todo o scrollback em uma resposta.
Snippets são texto puro com marcadores `« »`, renderizados por interpolação Vue, nunca `v-html`.
Não logar nem persistir termos de busca: podem conter segredos digitados pelo usuário.

---

## 7. Settings

### 7.1. Registro (Rust)

```rust
pub struct SettingDescriptor {
    pub key: &'static str,                       // "<domínio>.<grupo>.<nome>"
    pub default: fn() -> serde_json::Value,
    pub validate: fn(&serde_json::Value) -> Result<(), String>, // mensagem pt-BR
    pub sensitive: bool,                         // true => nunca persiste em `settings`
    pub description: &'static str,              // pt-BR (documentação/telemetria de debug)
}

pub struct SettingsRegistry { map: HashMap<&'static str, SettingDescriptor> }
impl SettingsRegistry {
    pub fn register(&mut self, d: SettingDescriptor); // chave duplicada => panic na inicialização
    pub fn resolve(&self, key: &str) -> SettingLookup<'_>;
}
pub enum SettingLookup<'a> { Registered(&'a SettingDescriptor), FreeUi, Unknown }

#[derive(Clone)]
pub struct SettingsRepo { db: Db }
impl SettingsRepo {
    pub async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, DbError>; // valor ou default
    pub async fn snapshot(&self, key: &str) -> Result<SettingSnapshot, DbError>;
    pub async fn set(&self, key: &str, value: serde_json::Value) -> Result<SettingSnapshot, DbError>;
    pub async fn get_typed<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>, DbError>;
}
```

Regras de `set`: chave casa `^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`; `Unknown` → `SettingUnknown`;
`FreeUi` (prefixo `ui.`) aceita JSON não sensível; `Registered` → `validate` → erro `SettingInvalid`;
`sensitive` → `SettingSensitive`; JSON serializado > 64 KiB → `SettingTooLarge`. Persistência:
`INSERT INTO settings(key, value, updated_at, revision) VALUES (?1, ?2, ?3, 1) ON CONFLICT(key) DO UPDATE SET value =
excluded.value, updated_at = excluded.updated_at, revision = settings.revision + 1`. Valor igual ao atual
não escreve, não incrementa revisão e não emite evento. Revisão/default sem linha = 0.
`get` de chave `Unknown` → `Ok(None)`.

Antes do bind: percorrer JSON decodificado (máximo 16 níveis, 1024 nós), detectar nomes sensíveis em **chaves**
e valores, inclusive `ui.*`, com inspeção de strings pelo sanitizador. Se algum valor exigir redação, rejeitar
com `db.setting_sensitive`, sem ecoar conteúdo. Não sanitizar JSON serializado via regex (escapes ocultam
segredos e substituições podem quebrar sintaxe), nem alterar silenciosamente credencial/configuração.
Não permitir nomes sensíveis arbitrários no registro dinâmico. `SettingDescriptor.sensitive` bloqueia antes
da validação e também os defaults; valores livres não devem ser usados para credenciais desconhecidas.
`set_internal` usa permissão Rust separada para `last_run_at`, mantendo os mesmos limites; não expor bypass IPC.

### 7.2. Settings registradas pela Fase 03

| Chave | Tipo | Default | Validação |
|-------|------|---------|-----------|
| `db.retention.backup_daily_keep` | inteiro | `7` | 1–60 |
| `db.retention.max_db_size_mb` | inteiro | `2048` | 64–65536 |
| `db.maintenance.interval_hours` | inteiro | `24` | 1–168 |
| `db.maintenance.last_run_at` | inteiro (Unix ms) ou `null` | `null` | escrita apenas interna (IPC recusa: `db.setting_invalid`) |

Fases futuras registram as suas (07: retenção de sessões/scrollback; 12: harness padrão). A 03 não presume
nome de tabela, coluna ou setting de retenção ainda não criado.

---

## 8. Sanitizador (`sanitizer/`)

### 8.1. API

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SecretKind {
    UrlCredentials, AppKey, Bearer, Cookie, GithubToken, AwsAccessKeyId, AwsSecretKey,
    OpenAiKey, AnthropicKey, Jwt, EnvAssignment, CliFlag, PemPrivateKey,
}

/// Sanitiza com o catálogo padrão. Cow::Borrowed quando nada é encontrado (zero alocação).
pub fn sanitize(input: &str) -> Cow<'_, str>;

/// Variante com relatório (para testes, logs de auditoria e Fase 11).
pub fn sanitize_with(input: &str, opts: &SanitizeOptions) -> SanitizeOutcome<'_>;

pub struct SanitizeOptions { pub kinds: Option<&'static [SecretKind]> }
pub struct SanitizeOutcome<'a> { pub text: Cow<'a, str>, pub findings: Vec<Finding> }
pub struct Finding { pub kind: SecretKind, pub range: Range<usize> } // range no texto ORIGINAL

/// Texto processado pelo catálogo vigente; não prova ausência de todo segredo possível.
#[derive(Clone, PartialEq, Eq)]
pub struct SanitizedText(String);
impl SanitizedText {
    pub fn new(raw: &str) -> Self;               // mensagem completa: aplica sanitize(), inclusive EOF
    pub fn as_str(&self) -> &str;
    pub fn into_inner(self) -> String;
}
impl rusqlite::ToSql for SanitizedText { /* ... */ }
// Sem From<String>/Deserialize: impossível construir sem passar pelo sanitizador.
```

`SanitizedText` tem `Debug` opaco (versão/contagem de bytes, sem conteúdo). `StreamSanitizer` é o único
construtor interno para unidades fragmentadas; chamar `new()` separadamente em cada chunk é proibido.
`SanitizeOptions.kinds` serve para testes/diagnóstico local; persistência/exportação sempre aplicam todo o
catálogo obrigatório, sem disable seletivo. Findings expõem apenas família e offsets, nunca valores.
`sanitize()`/`SanitizedText::new()` usam o mesmo motor completo: se houver controles ANSI, encaminham à
normalização terminal antes dos detectores (sem recursão entre APIs públicas). Assim comandos, títulos,
cwd e erros têm a mesma proteção que scrollback; `Cow::Borrowed` permanece apenas para texto limpo sem
normalização necessária. Entrada avulsa é mensagem completa com EOF, nunca fragmento de stream.

Máscara: `[REDACTED:<kind>]` (kind em `snake_case`). Prefixos contextuais (nome da variável, esquema da URL,
nome do header, usuário da URL) são **preservados** para manter o scrollback legível.

### 8.2. Catálogo de regras (famílias e candidatos)

| # | Kind | Regex (crate `regex`, sem backreferences) | Substituição |
|---|------|-------------------------------------------|--------------|
| 1 | `PemPrivateKey` | `(?s)-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----.*?-----END [A-Z0-9 ]*PRIVATE KEY-----` | `[REDACTED:pem_private_key]` |
| 2 | `Jwt` | `\beyJ[A-Za-z0-9_-]{8,}\.eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}` | `[REDACTED:jwt]` |
| 3 | `GithubToken` | `\b(?:gh[pousr]_[A-Za-z0-9]{36,255}\|github_pat_[A-Za-z0-9_]{22,255})\b` | `[REDACTED:github_token]` |
| 4 | `AnthropicKey` | `\bsk-ant-[A-Za-z0-9_-]{20,}` | `[REDACTED:anthropic_key]` |
| 5 | `OpenAiKey` | `\bsk-(?:proj-\|svcacct-\|admin-)?[A-Za-z0-9_-]{20,}` | `[REDACTED:openai_key]` |
| 6 | `AwsAccessKeyId` | `\b(?:AKIA\|ASIA\|AGPA\|AIDA\|AROA\|ANPA\|ANVA\|AIPA)[A-Z0-9]{16}\b` | `[REDACTED:aws_access_key_id]` |
| 7 | `AwsSecretKey` | `(?i)(aws_secret_access_key\s*[=:]\s*["']?)[A-Za-z0-9/+=]{40}` | `${1}[REDACTED:aws_secret_key]` |
| 8 | `AppKey` | `(?i)(\bAPP_KEY\s*[=:]\s*["']?)base64:[A-Za-z0-9+/]{16,}={0,2}`; parser cobre também APP_KEY sem base64 | `${1}base64:[REDACTED:app_key]` ou valor inteiro mascarado |
| 9 | `Bearer` | `(?i)(\b(?:authorization\s*[:=]\s*)?bearer\s+)[A-Za-z0-9\-._~+/]{8,}=*` | `${1}[REDACTED:bearer]` |
| 10 | `Cookie` | `(?im)(^\s*(?:set-)?cookie\s*:\s*\|--cookie[= ]["']?\|\s-b\s+["'])([^\r\n"']+)` | `${1}[REDACTED:cookie]` |
| 11 | `UrlCredentials` | `(?i)\b([a-z][a-z0-9+.\-]{1,20}://[^\s:/@]+):([^\s@/]{1,256})@` | `${1}:[REDACTED:url_credentials]@` |
| 12 | `CliFlag` | `(?i)(--(?:password\|passwd\|token\|secret\|api-key\|access-token)(?:=\|\s+))("[^"]*"\|'[^']*'\|[^\s"'$][^\s]*)` | `${1}[REDACTED:cli_flag]` |
| 13 | `EnvAssignment` | `(?i)\b((?:export\s+)?[A-Z0-9_]*(?:PASSWORD\|PASSWD\|SECRET\|TOKEN\|API_KEY\|APIKEY\|PRIVATE_KEY\|CREDENTIALS?)[A-Z0-9_]*\s*[=:]\s*)("[^"\r\n]+"\|'[^'\r\n]+'\|[^\s"'$][^\s]*)` | `${1}[REDACTED:env_assignment]` |

Regras de exclusão (aplicadas no callback de substituição, não na regex):
- Valor que já é `[REDACTED:*]` → mantém (idempotência).
- `EnvAssignment`/`CliFlag`: vazios e referências **inteiras** `$VAR`/`${VAR}` podem ser preservados;
  expressões compostas, substituições `$(...)` e literais anexados não são isentos. `PWD`/`OLDPWD` não são
  nomes sensíveis por si só; seus valores ainda passam pelas regras de token/URL.
- `Bearer`: o valor `[REDACTED` não casa por conter `[`.
- `Cookie` redige o valor inteiro do header (nomes de cookie podem conter identificadores de sessão).

### 8.3. Algoritmo
1. `RULES`/`PREFILTER` em `LazyLock`, mais parser contextual de headers, atribuições, flags, URLs, aspas e PEM.
   A tabela de regex é catálogo de candidatos, **não** implementação suficiente para streaming ou JSON.
   Separar delimitador de segredo: headers/atribuições explicitamente sensíveis mascaram até valores curtos,
   PEM inacabado e aspas não fechadas; limites mínimos das regex só valem para tokens soltos.
2. Encontrar intervalos no texto original/visão normalizada; resolver sobreposições cobrindo a união dos
   valores sensíveis. Contexto externo tem precedência sobre match interno: `Bearer <JWT>` deve mascarar
   o valor inteiro com kind bearer, não publicar suffix nem aplicar máscaras conflitantes.
3. Aplicar substituições em um único passe, preservando máscaras existentes e offsets originais em findings.
   Retornar `Cow::Borrowed` somente quando não houver match **nem estado contextual pendente suspeito**.
4. Sanitização de mensagem completa chama a mesma finalização conservadora de EOF do stream; truncar ou
   formar um trecho para erro somente **depois** dessa passagem. Parser de campos JSON inspeciona chaves e
   valores decodificados, sem perder contexto de senha/header entre campos.

### 8.4. `StreamSanitizer` (fragmentos de PTY / scrollback)

```rust
pub struct StreamSanitizer {
    carry: String,          // unidade lógica ainda não publicável
    state: StreamState,     // Text, QuotedValue, Pem, Suppressed, Finished
    decoder: Utf8Decoder,   // retém bytes UTF-8 incompletos entre chamadas
    terminal: TerminalNormalizer, // retém CSI/OSC/DCS fragmentados
    max_carry: usize,       // 16 KiB, mais estados/delimitadores de tamanho fixo
}
impl StreamSanitizer {
    pub fn new() -> Self;
    /// Consome bytes na ordem; retorna somente unidades sanitizadas com fronteira comprovada.
    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<SanitizedText>, SanitizeError>;
    /// Perda de bytes: elimina cauda ambígua e suprime até fronteira segura.
    pub fn on_gap(&mut self);
    /// EOF real: conclui decoder/parser, mascara estados suspeitos; idempotente.
    pub fn finish(&mut self) -> SanitizedText;
}
```

- Receber bytes originais, não `from_utf8_lossy` por chunk; converter UTF-8 apenas depois de juntar fronteiras.
  UTF-8 inválido em candidato sensível causa supressão da unidade, não quebra artificial de um token.
- Newline encerra texto comum, mas **não** estado de PEM, valor entre aspas, continuação de shell ou header
  dobrado. `\r`/backspace não validam fronteira. Contexto e prefixos de regras atravessam qualquer chunk.
- Nunca emitir prefixo porque buffer encheu ou passou um timeout. Aos 16 KiB, descartar a unidade pendente
  inteira, emitir `[REDACTED:oversize]` uma vez e consumir sem armazenar até fronteira segura. Se a fronteira
  não puder ser reconhecida (ex.: contexto perdido), manter supressão até EOF. Espaço não é prova de segurança.
- PEM: reconhecer BEGIN mesmo fragmentado, emitir uma máscara e descartar até END; matcher incremental
  retém só sufixo limitado do delimitador. END fragmentado e ausência de END não liberam conteúdo.
- `finish()` em EOF, cancelamento, fechamento ou erro emite texto limpo pendente ou máscara para prefixos
  de credencial/PEM/header/aspas/escape truncados. Segunda chamada retorna vazio; `push` depois de finish
  retorna `sanitizer.finished` sem incluir input. `Drop` descarta a cauda e registra só métrica, nunca texto.
- Fronteira de comando OSC não é automaticamente EOF: a 05 atribui chunks de forma conservadora sem resetar
  parser, ou finaliza e recria stream apenas quando sabe que não há continuação. Armazenamento suporta
  commandId nulo para unidades que atravessam comandos; não reutilizar cauda em outra sessão.
- Propriedade: para cada fixture coberta, sanitização de todas as partições do stream não revela nenhum
  segmento secreto conhecido e preserva contexto útil; testar partições aleatórias multi-chunk e todos os
  cortes por byte, inclusive finais incompletos. Não basta testar ausência do token completo se metade vaza.

### 8.5. Sequências ANSI
Scrollback persistido é **texto normalizado**, não replay binário fiel da TUI. Decoder incremental remove
CSI/SGR e payloads OSC/DCS (inclusive títulos/clipboard/URLs ocultas) antes de detectar; controles inseridos
no meio de token não criam separadores que impeçam match. Retém estado ESC entre chunks e descarta controle
incompleto no EOF. CR/backspace são tratados em buffer de linha limitado; ambiguidades descartam a unidade.
`sanitize_terminal(&str) -> SanitizedText` aplica esse pipeline a uma mensagem completa. Renderização ao vivo
do terminal nas fases 02/04 pode consumir bytes crus só em RAM local; esses bytes não vão a logs, exports,
índices ou armazenamento. Não afirmar suporte a toda forma possível de ofuscação terminal.

### 8.6. Redator de logs
`src-tauri/src/logging.rs` implementa o `LogRedactor` da Fase 01 com `SanitizerLogRedactor` delegando a
`mt_db::sanitizer::sanitize` (assinatura conforme definida pela Fase 01). `mt-db` não depende do crate binário;
o sanitizador não tem dependência de banco (módulo puro, testável isoladamente).
Instalar antes da primeira mensagem, migrations ou erro de boot. A fronteira é o **evento completo** de
logging, não cada `Write::write`: decodificar/sanitizar campos antes da serialização JSON e envolver o writer
com buffer por evento como defesa adicional. Writer parcial nunca sai cru; overflow/erro/drop emitem só
marcador fixo. Aplicar também ao stderr de debug e panic hook. Não logar fragmentos de PTY como eventos
independentes: usar o stream associado. `app_log_frontend` sanitiza antes de truncar, inclusive `context`.

### 8.7. Payload externo e limites da proteção

`sanitizer::ExternalPayload` é newtype privado de JSON sanitizado; construtor
`prepare_external_payload(value: serde_json::Value) -> Result<ExternalPayload, SanitizeError>` percorre
chaves/valores decodificados, mascara campos sensíveis por contexto e aplica `sanitize_terminal` aos textos.
Limites: 16 níveis, 1024 nós, 256 KiB após sanitização; excedentes retornam erro estático. Transporte das
Fases 11/12/13 aceita apenas esse tipo. Formar mensagem completa de comando+erro+metadados, sanitizar na
última fronteira **antes** de serializar/enviar/cachear/logar; dados vindos do histórico também são revalidados
com catálogo atual. Não concatenar texto cru depois. Chunk parcial só é elegível após `finish`/emissão segura.
As fases futuras implementam transporte; nesta fase usar sink em memória que capture o payload para testes.

O catálogo é heurístico: senhas sem contexto, formatos novos, codificações/criptografia, ofuscação e dados
pessoais arbitrários podem escapar. Não prometer detecção universal nem criptografia por redação. Evitar
coleta desnecessária de ambiente/headers, permitir futuramente fornecer valores secretos conhecidos apenas
em memória e versionar regras/fixtures. Não há persistência temporária de texto cru "para sanitizar depois";
DB, WAL, FTS, backups, logs e envio externo recebem somente saídas do pipeline. Isso não garante proteção
contra swap, dump de processo ou leitura da memória do terminal, fora deste contrato.

---

## 9. Manutenção, Retenção e Backup

```rust
pub struct RetentionRule {
    pub name: &'static str,             // identificador de regra definido pela fase dona
    pub table: &'static str,
    pub timestamp_column: &'static str, // Unix ms
    pub setting_key: &'static str,      // dias de retenção (registrada pela mesma fase)
    pub extra_where: Option<&'static str>, // ex.: "archived_at IS NOT NULL" (SQL estático)
    pub batch_size: usize,              // DELETE em lotes (padrão 5000) para não segurar o writer
}

pub struct RetentionRegistry { rules: Vec<RetentionRule> }

pub struct MaintenanceHandle { db: Db }
impl MaintenanceHandle {
    pub async fn run_now(&self) -> Result<DbMaintenanceFinished, DbError>;
    pub async fn backup(&self, kind: BackupKind) -> Result<PathBuf, DbError>;
}

pub enum BackupKind { Daily, PreMigration { target: u32 } }

/// Agendado por src-tauri/src/lib.rs com tokio: primeira execução 60 s após o boot, depois a cada
/// `db.maintenance.interval_hours`.
pub(crate) struct MaintenanceScheduler;
```

Rotina `run_now` (cada passo é um job separado no writer para não monopolizar a fila):
1. Para cada `RetentionRule`: `DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} WHERE
   {timestamp_column} < ?1 [AND {extra_where}] LIMIT {batch_size})` em loop até 0 linhas.
2. `optimize` de cada `FtsSpec` registrada.
3. `PRAGMA optimize;` → `PRAGMA wal_checkpoint(PASSIVE);` → `PRAGMA incremental_vacuum(2000);` em lotes
   até o freelist esvaziar ou 5 s. TRUNCATE somente em janela ociosa/shutdown; inspecionar retorno
   `(busy, log, checkpointed)`, reagendar quando leitores impedirem conclusão. Nunca apagar WAL manualmente.
4. Tamanho do arquivo > `db.retention.max_db_size_mb` → `tracing::warn!` (alertas visuais ficam para fases com UI).
5. Backup diário se o último tiver > 20 h; rotação.
6. Grava `db.maintenance.last_run_at` e emite `db://maintenance-finished`.

O loop genérico de `RetentionRule` atende tabelas independentes; a fase dona pode fornecer um procedimento
especializado para limitar cascatas. O scheduler alterna lotes com outros jobs, respeita cancelamento e
não executa a mesma regra em paralelo. A 03 não registra regras sobre tabelas futuras.

Backup: `rusqlite::backup::Backup::new(&reader_conn, &mut dest)` em worker bloqueante dedicado, criado e
consumido na mesma thread (`Backup` não é Send). Laço `step(256)` com pausas de 10 ms, deadline de 60 s e
tratamento explícito de `Busy/Locked`; `run_to_completion` não oferece esse deadline sozinho. Snapshot
consistente ao completar, sujeito a reinícios sob escrita concorrente; não monopolizar writer. Destino
`backups/maxterminal-<daily|premigration-v<N>>-<AAAAMMDD-HHMMSS>.db` com `0600`; após cópia, `PRAGMA
quick_check` no arquivo de destino — falha remove o arquivo e retorna `DbError::Backup`. Rotação: mantém
`backup_daily_keep` diários e 3 pré-migration mais recentes.

Escrever backup em nome temporário privado, fechar/validar/sincronizar e renomear atomicamente; somente então
rotacionar. No startup, backup pré-migration usa conexão inicial antes de liberar writer/pool. Não copiar só
`.db` em uso: pode omitir commits ainda no WAL. Retornar ids de backup ao IPC, não caminhos escolhidos pelo usuário.

Recuperação no `Db::open`: falha de integridade comprovada (`quick_check` ≠ ok ou CORRUPT/NOTADB) →
`db.recovery_required`, preservando originais. Busy, permissão ou disco cheio **não** significam corrupção.
`DbService` fica disponível com diagnóstico estático/ids de backups e consultas de dados bloqueadas.
`db_recover` recebe ação explícita restaurar backup listado ou criar novo. Com lock exclusivo, fechar todas
as conexões, preservar conjunto `.db/-wal/-shm` em diretório privado de quarentena, validar backup por
quick_check/FKs/checksums/schema suportado e só então instalar cópia e reabrir. Falha mantém os originais.
Sem backup válido, nunca iniciar vazio silenciosamente. `db://recovered` informa perda potencial até a data
do backup. A UI mínima de recuperação usa o estado de erro do shell da 01; não depende de notificações da 09.

### 9.1. Requisitos para políticas de histórico da Fase 07

- A 07 define nomes de settings/colunas, elegibilidade e limites; recomenda-se remover scrollback antigo
  somente de sessões arquivadas, marcando histórico incompleto conforme o DTO dessa fase.
- Antes de apagar uma sessão elegível, remover chunks em lotes ≤5000 e blocos em lotes ≤500,
  cedendo writer entre lotes; a sessão arquivada não aceita novos produtores.
  Só ao final apagar o pai, evitando uma cascata única com milhões de linhas. Triggers sincronizam FTS.
- Sessões ativas/fechadas não arquivadas não são removidas automaticamente. Com quota atingida, tentar
  purgar arquivadas mais antigas; se insuficiente, pausar captura persistente e sinalizar lacuna/pressão,
  mantendo terminal vivo. Metadados de finalização têm reserva na fila. 2 GiB é limite operacional
  aproximado (medir banco + WAL + espaço de backup), não garantia rígida diante de leitores/disco cheio.
- Backup retém cópias até sua rotação própria; apagar linha/FTS não promete apagamento físico imediato em
  páginas livres, WAL, snapshots ou SSD. `incremental_vacuum` recupera espaço, não é sanitização retroativa.

---

## 10. Comandos Tauri IPC e Eventos

### 10.1. Comandos (`src-tauri/src/commands/db.rs`)

```rust
#[tauri::command]
#[specta::specta]
pub async fn db_settings_get(service: State<'_, DbService>, key: String)
    -> Result<Option<serde_json::Value>, MtError>;

#[tauri::command]
#[specta::specta]
pub async fn db_settings_set(service: State<'_, DbService>, key: String, value: serde_json::Value)
    -> Result<SettingSnapshot, MtError>;
```

| Comando | Args | Retorno | Erros |
|---------|------|---------|-------|
| `db_settings_get` | `key: string` | `JsonValue \| null` (persistido, senão default registrado, senão `null`) | `db.busy`, `db.internal` |
| `db_settings_set` | `key: string`, `value: JsonValue` | `SettingSnapshot` | `db.setting_unknown`, `db.setting_invalid`, `db.setting_too_large`, `db.setting_sensitive`, `db.busy`, `db.internal` |
| `db_setting_snapshot` | `key: string` | `SettingSnapshot { value, revision }` | mesmos erros de get; usado por stores para evitar corrida |
| `db_status` | — | `DbStatusDto { state, schemaVersion, recovery, pendingRecords, lastErrorCode }` | `db.internal` |
| `db_recover` | `action: Restore { backupId } \| CreateEmpty` | `DbStatusDto` | `db.recovery_required`, `db.backup_failed`, `db.in_use` |

Chaves internas são recusadas por `db_settings_set`. Todos os comandos usam `State<DbService>`, obtêm a
fachada `Db` pronta de forma async ou retornam `db.not_ready`.
`db_status`/`db_recover` funcionam sem banco aberto; recover só aceita ações no estado recovery_required e
ids de backups previamente listados, nunca path/SQL. Os demais retornam `db.recovery_required` nesse estado.
Não expor SQL genérico ao WebView. Comandos/eventos de blocos e histórico são definidos pela 05/07;
arquivamento/reinício é orquestração da 07 sobre sessões da 05, sem recriar schema. Não reservar na 03
nomes de IPC de domínio que possam conflitar com essas fases.

Contratos serializados com `#[serde(rename_all = "camelCase")]`, `Serialize/Deserialize/specta::Type` onde
aplicável; identificadores são newtypes da 01. Definir em `mt-db/src/models.rs` e gerar os tipos TS:

```rust
pub struct SettingSnapshot { pub value: Option<serde_json::Value>, pub revision: i64 }
```

DTOs de domínio futuros são gerados pelas fases donas e não devem expor rowid, texto bruto ou findings
originais. Seus limites/cursores seguem §6.2 e os contratos específicos 05/07, sem duplicação manual em TS.
`DbStatusDto.state`: opening/ready/recovery_required/closing/failed; recovery contém somente ids/datas de
backups e último `DbRecovered` seguro, disponível mesmo que o evento de boot tenha sido perdido.

### 10.2. Eventos globais (baixo volume → `emit`)

```rust
#[derive(Serialize, Clone, specta::Type, tauri_specta::Event)]
pub struct SettingsChanged { pub key: String, pub value: serde_json::Value, pub revision: i64 } // db://settings-changed

#[derive(Serialize, Clone, specta::Type, tauri_specta::Event)]
pub struct DbRecovered { pub backup_id: Option<String>, pub recovered_at: i64, pub created_empty: bool } // db://recovered

#[derive(Serialize, Clone, specta::Type, tauri_specta::Event)]
pub struct DbMaintenanceFinished {                                                // db://maintenance-finished
    pub duration_ms: u64, pub deleted_rows: u64, pub db_size_bytes: u64, pub backup_id: Option<String>,
}

#[derive(Serialize, Clone, specta::Type, tauri_specta::Event)]
pub struct DbWriterLag { pub writer: String, pub queued: u64, pub rejected: u64 } // db://writer-lag

```

Nomes de evento fixados via `#[tauri_specta(event_name = "db://...")]` (ou mecanismo equivalente adotado na
Fase 01). Usar camelCase nos payloads. `DbRecovered` fica retido em `DbService`; frontend instala listeners
e então consulta `db_status`, sem presumir evento de prontidão inexistente na 01. Eventos de domínio ficam
nas fases donas e só confirmam persistência após commit; evitar um evento global por chunk. Status cobre
falhas/lag; eventos podem ser perdidos e não são storage. Nenhum Channel é necessário nesta fase.

### 10.3. Integração no crate binário

```rust
// src-tauri/src/lib.rs (trecho)
.setup(|app| {
    let paths = app.state::<AppState>().paths.clone();
    let service = DbService::opening();
    app.manage(service.clone());
    // Helper desta fase: spawn_blocking para Db::open, depois publica ready/recovery_required/failed.
    start_db_service(app.handle().clone(), paths, service);
    Ok(())
})
// ExitRequested: impedir saída uma vez, aguardar shutdown async e então autorizar saída definitiva.
```

`start_db_service` resolve config, registra settings da 03 e contribuições de FTS/retenção/migrations antes de
`Db::open`. Registrar o redator antes disso. `DbService` usa estado sincronizado + notificação async, sem
polling da UI; bootstrap frontend instala listeners, consulta status e usa o shell de loading/erro da 01.
`Db::open` e checks/backup nunca rodam na thread de UI. 50 ms é alvo medido para banco pequeno sem pendências,
não SLA para quick_check de banco grande. Encerramento não espera flush em callback síncrono `RunEvent::Exit`.

---

## 11. Arquitetura Frontend

### 11.1. Contratos TypeScript (`src/types/settings.ts`)

```ts
import type { JsonValue } from './bindings';

/** Mapa chave → tipo. Fases futuras estendem via declaration merging. */
export interface SettingsSchema {
    'db.retention.backup_daily_keep': number;
    'db.retention.max_db_size_mb': number;
    'db.maintenance.interval_hours': number;
    'db.maintenance.last_run_at': number | null;
}

export type UiSettingKey = `ui.${string}`;
export type SettingKey = keyof SettingsSchema | UiSettingKey;
export type SettingValue<K extends SettingKey> = K extends keyof SettingsSchema ? SettingsSchema[K] : JsonValue;

// Exemplo de extensão (Fase 12):
// declare module '@/types/settings' { interface SettingsSchema { 'ai.default.harness': string } }
```

### 11.2. IPC (`src/ipc/db.ts`)

```ts
import { commands, events } from '@/types/bindings';
import { call } from './core';
import type { SettingKey, SettingValue } from '@/types/settings';

export async function getSetting<K extends SettingKey>(key: K): Promise<SettingValue<K> | null> {
    return (await call('db_settings_get', commands.dbSettingsGet(key))) as SettingValue<K> | null;
}

export async function setSetting<K extends SettingKey>(key: K, value: SettingValue<K>): Promise<void> {
    await call('db_settings_set', commands.dbSettingsSet(key, value));
}

export const onSettingsChanged = events.settingsChanged.listen;
export const onDbRecovered = events.dbRecovered.listen;
export const onDbMaintenanceFinished = events.dbMaintenanceFinished.listen;
```

`call(command, request)` é o helper real da Fase 01 e lança `MtIpcError`. O wrapper simples `setSetting`
acima serve a consumidores sem cache; a store usa `getSettingSnapshot`/`setSettingSnapshot`, que preservam
`SettingSnapshot` retornado por `db_setting_snapshot`/`db_settings_set`. `SettingsSchema` só é utilitário de
chaves: não duplicar DTOs Rust; valores ainda requerem validação backend. Adicionar wrappers tipados
`getDbStatus` e `recoverDb` em `ipc/db.ts`, usando sempre `call` e os tipos gerados, sem invoke/SQL direto
nas stores. Wrappers de histórico serão definidos pela 05/07/17 junto de seus IPCs.

### 11.3. Store `useSettingsStore` (`src/stores/settings.ts`)

```ts
interface SettingsState {
    values: Partial<Record<SettingKey, JsonValue | null>>;
    loading: Partial<Record<SettingKey, boolean>>;
    pending: Partial<Record<SettingKey, JsonValue | null>>;  // escrita otimista em andamento
    revisions: Partial<Record<SettingKey, number>>;
    requestGenerations: Partial<Record<SettingKey, number>>;
    listening: boolean;
}
```

| Tipo | Nome | Contrato |
|------|------|----------|
| getter | `get(key)` | `<K>(key: K) => SettingValue<K> \| undefined` — `pending` tem prioridade sobre `values` |
| getter | `isLoaded(key)` | `boolean` |
| action | `ensure(key)` | Carrega via `getSettingSnapshot`; deduplica chamadas e só aplica revisão ≥ revisão conhecida |
| action | `set(key, value)` | Otimista; serializa escritas por chave e mantém geração local; resposta confirma revisão; falha só desfaz pending da mesma geração, preservando edição posterior |
| action | `startListening()` | Idempotente; evento aplica revisão nova em values, sem apagar pending de edição local mais recente |
| action | `$reset()` | Limpa cache e cancela listener |

Store **não** usa cache persistente do `@maxvue/max-pinia` (a fonte da verdade é SQLite); registrar bootstrap
da 03 via `registerBootstrapStep` e instalar listener antes de carregar snapshots. `$reset/$dispose` removem
listeners/timers e invalidam gerações; se listen assíncrono resolver após dispose, executar unlisten na hora.
Evento perdido exige reload ao foco/reconexão. Janela adicional do mesmo processo sincroniza por evento;
outra instância escritora é bloqueada pelo lock do banco. Nunca usar localStorage/IndexedDB para histórico.

### 11.4. Composable `useSetting` (`src/composables/useSetting.ts`)

```ts
export function useSetting<K extends SettingKey>(
    key: K,
    fallback: SettingValue<K>,
    options?: { debounceMs?: number }   // padrão 300
): {
    value: WritableComputedRef<SettingValue<K>>;
    ready: Readonly<Ref<boolean>>;
    error: Readonly<Ref<MtIpcError | null>>;
    flush: () => Promise<void>;
};
```

Leitura: usar fallback apenas se não carregado/undefined, preservando `null` válido; escrita atualiza `pending` e agenda `store.set` com
debounce (`useDebounceFn` do `@maxvue/max-use`, se disponível, senão implementação local); erro populado e
traduzido por `useIpcError` (Fase 01). `onScopeDispose` cancela timer e solicita flush com erro tratado;
Vue não aguarda callback async de dispose. Fechamento controlado da aplicação chama/aguarda flush antes de
shutdown; não prometer persistência de debounce se o processo abortar.

### 11.5. UI mínima e recomendações para stores futuras

A 03 entrega `useSettingsStore`/`useSetting` e estado de prontidão/recuperação. Stores de blocos/sessões e
busca são ownership 05/07/17. Ao adotarem a infraestrutura, devem ignorar respostas antigas por geração,
deduplicar páginas em voo, reiniciar cursores expirados e manter caches só em RAM com limite de bytes.
Não criar `useHistoryStore` ou bindings de sessões nesta fase. A UI futura consome o IPC da fase dona,
não o SQL genérico Rust nem tipos inventados em paralelo.

Sem tela de busca global (17) ou navegador completo de sessões (07). Entregar apenas integração de loading,
erro e recuperação no shell da 01: "Restaurar backup" com id/data ou "Criar banco vazio", mostrando perda
potencial antes da ação explícita; sem importar componentes/stores de fases futuras. Textos pt-BR,
tokens semânticos e acessibilidade de foco/estado de carregamento seguem a 01. Testes exercitam wrappers e
stores independentemente dessa UI. Evento de recuperação não é reenviado ao logger com caminhos/conteúdo.

---

## 12. Fluxos de Dados

### 12.1. Inicialização

```mermaid
sequenceDiagram
    participant T as Tauri setup
    participant D as Db::open
    participant C as connection
    participant M as MigrationRunner
    participant B as backup
    T->>D: DbConfig + DbSetup
    D->>C: open_writer (PRAGMAs, 0600)
    C-->>D: quick_check
    alt corrompido
        D-->>T: recovery_required (preserva originais)
        T->>B: após ação explícita: restaurar backup validado
    end
    D->>M: pending()
    alt pendentes e banco não vazio
        M->>B: backup PreMigration
    end
    M->>M: BEGIN IMMEDIATE / exec / registra / COMMIT (por migration)
    D->>D: inicia writer thread + ReadPool
    D-->>T: (Db, OpenReport)
```

### 12.2. Escrita em lote (consumida pelas Fases 05/07)

```
PTY reader (Fase 02) ──bytes ordenados──> Fase 05/07: StreamSanitizer.push() ──SanitizedText──>
   repositório da fase dona → BatchWriter.push() [try_send, não bloqueia]
        │ fila cheia → Err(Backpressure) + db://writer-lag (≤1/s)
        ▼
   thread agregadora (200 ms | 512 | 1 MiB) ──WriteJob::Batch(Vec<R>)──> writer thread
        ▼
   BEGIN IMMEDIATE → INSERT×N (prepare_cached) → triggers FTS → COMMIT
```

### 12.3. Settings

```
useSetting(value = x) → pending[key] = x → (debounce 300 ms) → setSetting → db_settings_set
  → registro/validação/inspeção JSON → Db::write(UPSERT + revision) → COMMIT → DbEventSink.settings_changed
  → emit db://settings-changed → stores aplicam value/revision; pending só removido pela confirmação da geração
erro → desfaz somente a edição correspondente; preserva edição nova; error.value = MtIpcError
```

### 12.4. Histórico e envio externo

```
05: SessionProvider → sessions; bloco sanitizado → commit command_blocks + FTS → evento shell
07: bytes → decoder/parser/StreamSanitizer → registro de scrollback → commit + FTS → evento de domínio
EOF → finish → BatchWriter.flush/barreira → fechamento da 05 → arquivamento da 07
UI → wrapper → request validado → ReadPool → FTS/keyset → Page sanitizada → cache RAM limitado
Inspecionar/IA futura → agrega comando/erro/metadados → prepare_external_payload → transporte
```

Finalização do comando não encerra necessariamente o stream; EOF do exemplo é do stream/sessão. Snapshot
de erro é derivado de texto já emitido com segurança ou espera finalização; nunca ler cauda crua para resumir.

---

## 13. Performance

| Métrica | Alvo | Como medir |
|---------|------|-----------|
| `Db::open` (banco existente, sem pendências) | < 50 ms | `tracing` span `db.open` |
| `db_settings_get` (p99) | < 2 ms | teste com 1 000 chamadas |
| Inserção em lote | 100 000 × 200 B < 1,5 s | `benches/batch.rs` |
| Latência push → commit | ≤ 250 ms (p99) sob carga nominal | `BatchStats.last_flush_ms` |
| `sanitize` texto limpo | ≥ 100 MiB/s | `benches/sanitizer.rs` |
| `sanitize` com segredos (1/4 KiB) | ≥ 30 MiB/s | idem |
| Manutenção diária | sem bloquear writer > 200 ms contínuos | DELETE em lotes + jobs separados |

Detalhes: `prepare_cached` em todas as queries; transações `IMMEDIATE` (evita upgrade de lock); `mmap_size`
para leitura; checkpoint a cada 1000 páginas e `journal_size_limit` ajudam a reciclar WAL, mas leitores longos
podem fazê-lo crescer. Medir banco/WAL/filas separadamente. Alvos de benchmark exigem hardware, perfil,
corpus, tamanho de banco e p50/p95/p99 registrados; são critérios de medição, não resultados desta documentação.

---

## 14. Segurança

- Nenhum SQL arbitrário chega do WebView; comandos aceitam apenas chaves/valores validados. Sem
  `tauri-plugin-sql` nas capabilities.
- Todo SQL dinâmico (FTS DDL, retenção) usa apenas identificadores `&'static str` validados por regex; valores
  sempre via parâmetros `?N`.
- `SanitizedText` como barreira de pipeline (não prova de detecção perfeita); `Debug` de erros omite conteúdo.
- Arquivos `0600`/diretório `0700`; backups com as mesmas permissões.
- Settings `sensitive` nunca persistem; valores de settings não são logados (apenas a chave).
- Logs do app passam pelo `SanitizerLogRedactor`.
- `cell_size_check=ON`; não habilitar carregamento de extensões nem ATTACH arbitrário. FTS5 em triggers pode
  ser recusado com `trusted_schema=OFF` conforme versão/build. Testar a combinação real; baseline admite
  `trusted_schema=ON` no writer apenas para schema local validado contra migrations/checksums e inventário
  de tabelas/triggers; leitores usam OFF quando compatível. Arquivos importados/backup adulterado não são
  executados como schema confiável. Não recomendar OFF incondicional que quebre toda escrita indexada.

---

## 15. Estratégia de Testes

| Arquivo | Casos |
|---------|-------|
| `src/connection.rs` (unit) | `pragmas_applied` (writer e reader), `permissions_0600`, `wal_is_mandatory` (falha simulada) |
| `tests/migrations.rs` | `fresh_db_applies_base`, `idempotent_reopen`, `checksum_mismatch`, `failed_migration_rolls_back`, `schema_too_new`, `naming_contract`, `premigration_backup_created`, `code_migration_runs`, `foreign_key_check_after_migrate` |
| `tests/concurrency.rs` | `writer_and_readers` (50 000 inserts + 8 leitores, 0 BUSY), `writer_survives_panic`, `write_timeout_returns_busy` |
| `tests/batch.rs` | `coalesces` (≤ 25 transações para 10 000 pushes), `order_preserved`, `backpressure_non_blocking`, `flush_on_shutdown`, `explicit_flush_awaits_commit` |
| `tests/fts.rs` | `fts5_available`, `external_content_roundtrip`, `diacritics_removed`, `prefix_search`, `ddl_matches_handwritten`, proptest `build_match_query_never_syntax_error` |
| `tests/settings.rs` | default via registro, `ui.*` livre, `unknown`, `invalid`, `too_large`, `sensitive`, evento único por mudança, sem evento para valor igual |
| `tests/retention.rs` | regra remove somente antigos, `extra_where` respeitado, DELETE em lotes, `last_run_at` gravado |
| `tests/backup.rs` | backup válido com WAL pendente, rotação, prazo Busy, corrupção → recovery_required, restauração explícita, sem backup não reseta |
| `tests/sanitizer_table.rs` | carrega `fixtures/sanitizer_cases.toml` (`input`, `expected`, `kinds`) com `rstest`; ≥ 3 positivos + ≥ 2 negativos por `SecretKind`; `Cow::Borrowed` em negativos |
| `tests/sanitizer_props.rs` | idempotência, sem panic, `StreamSanitizer` com todos os pontos de corte, PEM em 50 chunks, ANSI entremeado |
| `src-tauri/src/commands/db.rs` (`tauri::test::mock_builder`) | `get`/`set` roundtrip, serialização `MtError { code: 'db.setting_invalid' }` |
| `tests/ipc/db.spec.ts` (Vitest + `mockIPC`) | wrappers chamam `db_settings_get/set` com args corretos; erro vira throw tipado |
| `tests/stores/settings.spec.ts` | `ensure` deduplica, `set` otimista + rollback, evento atualiza `values` |
| `tests/composables/useSetting.spec.ts` | debounce (fake timers) gera 1 IPC, `flush` em dispose, erro exposto |
| `tests/pagination.rs` (fixtures) | keyset, empate de timestamp, insert/delete entre páginas e FTS com limite; DTOs/cursores reais são testados nas fases donas |
| `tests/sanitizer_stream.rs` | cortes por byte UTF-8/ANSI/PEM, linha enorme, segredo curto contextual, EOF parcial, finish idempotente, gap, sem vazamento de prefixos |
| `tests/sanitizer_sinks.rs` | ausência de segredos-fixture em DB/WAL/FTS/sombra/backup/log/IPC/sink externo; JSON aninhado/escapado e erros/panic redigidos |
| `tests/stores/settings.spec.ts` (concorrência) | eventos/respostas fora de ordem, revisão monotônica, erro de edição antiga preserva nova, dispose durante listen |

Exemplos obrigatórios da tabela de casos (trecho de `sanitizer_cases.toml`):

```toml
[[case]]
input = "git clone https://joao:s3nh4F0rte@github.com/org/repo.git"
expected = "git clone https://joao:[REDACTED:url_credentials]@github.com/org/repo.git"
[[case]]
input = "APP_KEY=base64:2fl+Ktvkfl+Fuz4Qp/A75G2RTiWVA/ZoKZvp6fiiM10="
expected = "APP_KEY=base64:[REDACTED:app_key]"
[[case]]
input = "curl -H 'Authorization: Bearer abcdef123456'"
expected = "curl -H 'Authorization: Bearer [REDACTED:bearer]'"
[[case]]
input = "DB_PASSWORD=\"minha senha\""
expected = "DB_PASSWORD=[REDACTED:env_assignment]"
[[case]]   # negativos
input = "cd $OLDPWD && export GITHUB_TOKEN=$TOKEN && git log 3f2a9c1e0b7d4a6f8e9c0b1a2d3e4f5a6b7c8d9e"
expected = "cd $OLDPWD && export GITHUB_TOKEN=$TOKEN && git log 3f2a9c1e0b7d4a6f8e9c0b1a2d3e4f5a6b7c8d9e"
```

Execução: `cargo test -p mt-db`, `cargo test -p maxterminal db_`, `cargo bench -p mt-db`, `npm run test`.

---

## 16. Pontos de Extensão para Fases Posteriores

| Ponto | Consumidor | Uso |
|-------|-----------|-----|
| Arquivos `migrations/NNNN_FF_*.sql` / `CodeMigration` | 05, 06, 07, 10, 13, 15, 16 | 05 cria sessions/command_blocks/FTS; 07 estende sessions e cria scrollback; cada fase possui seu DDL |
| `Db::read/write` + `DbSetup` | 05, 07 e demais | Repositórios de domínio confiáveis, transações e contribuições de inicialização |
| `FtsSpec` + `build_match_query` | 05, 07, 17 | Registry/manutenção e construção segura; tabelas/queries/IPC concretos nas fases donas |
| `BatchWriter::push/flush` + `StreamSanitizer` | 05, 07 | Batching e sanitização antes de bind/disco; sem dependência de PTY na 03 |
| `RetentionRule` | 07 e demais | Regras e settings concretas definidas pela fase dona das tabelas |
| `SettingDescriptor` + `SettingsSchema` (TS) | 06, 07, 09, 10, 12, 13 | Configurações tipadas |
| `sanitize` / `StreamSanitizer` / `SanitizedText` / `ExternalPayload` | 05, 07, 11, 12, 13 | Persistência e fronteira de envio externo (Inspecionar, MaxAiManager), sem desabilitar regras obrigatórias |
| `DbEventSink` | 09 | Notificação nativa de `db://recovered` |

Dependência de implementação **somente 01**. Referências a 02/05/06/07/11/12/13/17 descrevem consumidores
futuros, não imports ou pré-requisitos. O registro de migrations agrega extensões no binário quando cada
fase existir; testes da 03 usam fixtures locais e sink de eventos em memória. A 03 é dona somente do schema
de suporte, infraestrutura, sanitizer e mecanismos de retenção. A 05 é dona de sessions/command_blocks/FTS
e seus repositórios; a 07 estende sessões e cria scrollback/retenção de domínio e UX/orquestração.

## 17. Referências técnicas e verificações na implementação

- SQLite FTS5, conteúdo externo, triggers, integrity-check e snippets: <https://www.sqlite.org/fts5.html>.
- SQLite WAL, checkpoint ocupado e correção WAL-reset: <https://www.sqlite.org/wal.html>.
- `rusqlite::backup::Backup`, `step`, `run_to_completion` e restrição de thread:
  <https://docs.rs/rusqlite/latest/rusqlite/backup/struct.Backup.html>.

Consulta Context7 tentada nesta revisão, indisponível por quota; referências oficiais acima consultadas como
fallback. Confirmar no lockfile as APIs/flags realmente disponíveis antes de implementar; snippets de tipos
omitem imports/derives repetitivos, não são código compilado nem prova de benchmarks executados.
