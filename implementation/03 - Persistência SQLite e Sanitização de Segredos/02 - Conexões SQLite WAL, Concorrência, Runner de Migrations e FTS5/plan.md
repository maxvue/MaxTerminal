# Implementação 02 — Conexões SQLite WAL, Concorrência, Runner de Migrations e FTS5 (`plan.md`)

> Guia de execução sequencial para a Implementação 02 da Fase 03.
> Referência técnica: [`./spec.md`](./spec.md).

---

## Passo 1 — Implementar `DbError`, Códigos `db.*` e Conversão para `MtError` (`src/error.rs`)

1. Atualizar `src-tauri/crates/mt-db/src/error.rs`:
   - Declarar o módulo `pub mod codes` com todas as 24 constantes `ErrorCode` (`db.open_failed` até `db.not_ready`).
   - Implementar todas as variantes de `DbError`, `From<rusqlite::Error> for DbError` e `From<DbError> for MtError`.
   - Implementar `fmt::Debug` seguro para `DbError` omitindo caminhos completos e textos de queries/parâmetros.
   - Adicionar testes unitários em `src/error.rs` verificando a serialização JSON de `MtError` gerado a partir de cada variante de `DbError` e garantindo ausência de vazamento de `source` ou SQL.

---

## Passo 2 — Implementar `DbConfig`, `PragmaSet`, Permissões Unix e `InstanceLock` (`config.rs`, `connection.rs`)

1. Criar `src-tauri/crates/mt-db/src/config.rs`:
   - Implementar `PragmaSet` (defaults: `busy_timeout_ms = 5000`, `cache_size_kib = 16000`, `mmap_size = 268435456`) e `DbConfig` (`reader_pool_size = 4`, `write_queue_capacity = 4096`, `write_queue_bytes = 16 * 1024 * 1024`).
   - Implementar `DbConfig::from_paths` com suporte ao override `MAXTERMINAL_DB_PATH` (exigindo caminho absoluto quando definido e rejeitando caminhos relativos/vazios).
2. Criar `src-tauri/crates/mt-db/src/connection.rs`:
   - Implementar validação contra symlinks e criação de diretórios com permissão `0700` e arquivos `.db`/`-wal`/`-shm` com `0600`.
   - Implementar `InstanceLock::acquire` usando lock de arquivo não bloqueante (`flock` no Unix) retornando `Err(DbError::InUse)` quando ocupado.
   - Implementar `open_writer`, `open_reader`, `run_quick_check` e `verify_sqlite_capabilities`.
   - Criar os testes unitários `pragmas_applied`, `permissions_0600`, `wal_is_mandatory` e `instance_lock_prevents_second_writer` no módulo `#[cfg(test)]` de `connection.rs`.

---

## Passo 3 — Implementar `DbWriter` e `ReadPool` (`writer.rs`, `reader.rs`)

1. Criar `src-tauri/crates/mt-db/src/writer.rs`:
   - Implementar `ByteBudget` e `BytePermit` para limitar o total de bytes em fila a 16 MiB (e máximo de 1 MiB por job individual).
   - Implementar `DbWriter::spawn` iniciando a thread `mt-db-writer` que processa `WriteJob::{Tx, Batch, Raw, Shutdown}` com `TransactionBehavior::Immediate` e proteção `catch_unwind`.
   - Implementar espera assíncrona de até 250 ms via `tokio::sync::Notify` quando a fila estiver cheia em `execute_tx`.
2. Criar `src-tauri/crates/mt-db/src/reader.rs`:
   - Implementar `ReadPool` usando `r2d2::Pool<SqliteConnectionManager>` com inicializador customizado que aplica os PRAGMAs de leitor (`query_only = ON`, `trusted_schema = OFF`, cache de 64 statements) e executa leituras dentro de `tokio::task::spawn_blocking`.

---

## Passo 4 — Implementar `MigrationRunner` e Migration `0001_03_base.sql` (`migrations/`)

1. Criar `src-tauri/crates/mt-db/migrations/0001_03_base.sql` com a DDL exata da tabela `settings` (`STRICT, WITHOUT ROWID`).
2. Criar `src-tauri/crates/mt-db/src/migrations/mod.rs`, `registry.rs` e `runner.rs`:
   - Embutir o diretório `migrations/` com `include_dir!("$CARGO_MANIFEST_DIR/migrations")`.
   - Implementar `parse_migration_filename`, `compute_migration_checksum` (normalizando `\r\n` → `\n` antes do SHA-256) e validação de sequência contígua `1..=N`.
   - Implementar `MigrationRunner::pending` e `MigrationRunner::run` com criação de `schema_migrations`, checagem de `SchemaTooNew`, checagem de `MigrationChecksumMismatch`, hook de backup pré-migration, transação `IMMEDIATE` por migration, `PRAGMA foreign_key_check`, atualização de `PRAGMA user_version` e `PRAGMA optimize`.

---

## Passo 5 — Implementar Infraestrutura FTS5 e Cursores de Paginação (`src/fts.rs`)

1. Criar `src-tauri/crates/mt-db/src/fts.rs`:
   - Implementar `FtsTokenizer`, `FtsSpec` (`validate`, `ddl`, `rebuild_sql`, `optimize_sql`, `integrity_check_sql`) e `FtsRegistry`.
   - Implementar `build_match_query(input: &str) -> Option<String>` com neutralização de operadores FTS5, escape de aspas e sufixo `*` no último termo.
   - Implementar `KeysetCursor` e `SearchSnapshotCache` (TTL 30 s, máx. 8 snapshots / 8 MiB, invalidação em mutações).

---

## Passo 6 — Criar Suítes de Testes de Integração (`tests/`)

1. Criar `src-tauri/crates/mt-db/tests/migrations.rs` cobrindo todos os cenários de migrations (`fresh_db_applies_base`, `idempotent_reopen`, `checksum_mismatch`, `failed_migration_rolls_back`, `schema_too_new`, `naming_contract`, `code_migration_runs`, `foreign_key_check_after_migrate`).
2. Criar `src-tauri/crates/mt-db/tests/concurrency.rs` cobrindo `writer_and_readers` (50 000 inserts + 8 leitores por 5 s com 0 `SQLITE_BUSY`), `writer_survives_panic` e `write_timeout_returns_busy`.
3. Criar `src-tauri/crates/mt-db/tests/fts.rs` cobrindo `fts5_available`, `external_content_roundtrip`, `diacritics_removed`, `prefix_search`, `ddl_matches_handwritten` e o proptest `build_match_query_never_syntax_error` (10 000 casos).
4. Criar `src-tauri/crates/mt-db/tests/pagination.rs` cobrindo paginação keyset de 3 páginas com desempate estável, mutações concorrentes entre páginas, `db.cursor_invalid` e `db.cursor_expired`.

---

## Passo 7 — Comandos de Verificação e Validação

Executar a partir de `src-tauri/`:

```bash
cargo check -p mt-db --all-targets
cargo clippy -p mt-db --all-targets -- -D warnings
cargo fmt --all --check
cargo test -p mt-db --lib
cargo test -p mt-db --test migrations --test concurrency --test fts --test pagination
```

---

## Critérios de Aceite Cobertos nesta Implementação

- [x] **CA-07 / CA-08**: `PRAGMA journal_mode` retorna `wal`, `PRAGMA auto_vacuum` retorna `2`, writer aplica `foreign_keys=1`, `busy_timeout=5000`, `synchronous=1` e reader aplica `query_only=1`.
- [x] **CA-09**: Diretório do banco criado com permissão `0700` e arquivos `.db`, `-wal` e `-shm` com `0600`.
- [x] **CA-10**: `SELECT sqlite_compileoption_used('ENABLE_FTS5')` retorna `1`.
- [x] **CA-11 a CA-16**: Banco novo aplica `1|3|base` (`user_version = 1`), reabertura idempotente, divergência de checksum falha com `db.migration_checksum_mismatch`, SQL inválido faz rollback com `db.migration_failed`, `user_version` futuro falha com `db.schema_too_new` e contrato de nomes `migrations::naming_contract` validado.
- [x] **CA-18 / CA-23**: 50 000 escritas concorrentes com 8 leitores contínuos por 5 s sem `SQLITE_BUSY`; leituras e escritas nunca bloqueiam threads do runtime Tokio.
- [x] **CA-24 a CA-26**: Índices FTS5 external-content sincronizados por triggers e verificados por `integrity-check`, busca insensível a acentos (`remove_diacritics 2`), prefixo no último termo e `build_match_query` validado com 10 000 casos no proptest.
- [x] **CA-44 / CA-45**: Extensões de FTS e paginação keyset/snapshot testadas com fixtures isoladas sem antecipar tabelas das Fases 05 e 07.
