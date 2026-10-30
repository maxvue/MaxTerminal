# Implementação 03 — `BatchWriter`, Backup Online, Retenção, Manutenção e Fachada `Db` (`plan.md`)

> Guia de execução sequencial para a Implementação 03 da Fase 03.
> Referência técnica: [`./spec.md`](./spec.md).

---

## Passo 1 — Implementar DTOs e `DbEventSink` (`src/models.rs`)

1. Criar `src-tauri/crates/mt-db/src/models.rs`:
   - Definir `SettingSnapshot`, `DbStateKind`, `BackupEntryDto`, `DbRecoveryInfo`, `DbStatusDto`, `DbRecoveryAction`, `SettingsChanged`, `DbRecovered`, `DbMaintenanceFinished`, `DbWriterLag`, a trait `DbEventSink` e `NoopEventSink`.
   - Garantir que todas as structs públicas de IPC derivem `Serialize`, `Deserialize` (quando aplicável) e `specta::Type` com `#[serde(rename_all = "camelCase")]`.

---

## Passo 2 — Implementar `BatchWriter<R>` (`src/batch.rs`)

1. Criar `src-tauri/crates/mt-db/src/batch.rs`:
   - Definir `BatchRecord`, `BatchPolicy`, `BatchStats`, `BatchStatsSnapshot` e a trait interna `BatchFlush`.
   - Implementar `BatchWriter::push` não bloqueante (`try_acquire` no `ByteBudget` + `try_send` no canal limitado), registrando `rejected` e emitindo `DbWriterLag` com rate-limit máximo de 1 por segundo via `AtomicU64`.
   - Implementar a thread agregadora `mt-db-batch-<WRITER_NAME>` com flush por tempo (`200 ms`), contagem (`512`) ou bytes (`1 MiB`), enviando `WriteJob::Batch` para o `DbWriter` e mantendo o lote em memória até confirmação do `COMMIT`.
   - Implementar `BatchWriter::flush()` com barreira `oneshot` e `flush_blocking` para o ciclo de shutdown.

---

## Passo 3 — Implementar `SettingsRegistry` e `SettingsRepo` (`src/settings/`)

1. Criar `src-tauri/crates/mt-db/src/settings/registry.rs`:
   - Implementar `SettingDescriptor`, `SettingLookup` e `SettingsRegistry::with_phase03_defaults()` registrando as 4 settings da Fase 03 (`db.retention.backup_daily_keep`, `db.retention.max_db_size_mb`, `db.maintenance.interval_hours`, `db.maintenance.last_run_at`).
2. Criar `src-tauri/crates/mt-db/src/settings/repo.rs` e `src/settings/mod.rs`:
   - Implementar `SettingsRepo::get`, `snapshot`, `set`, `set_internal` e `get_typed`.
   - Em `set`: validar formato da chave (`^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$`), rejeitar `Unknown` (`db.setting_unknown`), rejeitar `sensitive` (`db.setting_sensitive`), rejeitar `internal_only` (`db.setting_invalid`), inspecionar recursivamente o valor JSON com `inspect_settings_json` (retornando `db.setting_sensitive` se houver chave/valor sensível), validar tamanho serializado ≤ 65 536 bytes (`db.setting_too_large`), executar o validador do descritor e realizar UPSERT na tabela `settings`.
   - Emitir `DbEventSink::settings_changed` **somente após o COMMIT** e apenas quando o valor efetivamente mudou.

---

## Passo 4 — Implementar Retenção, Backup Online e Manutenção (`src/maintenance/`)

1. Criar `src-tauri/crates/mt-db/src/maintenance/retention.rs`:
   - Implementar `RetentionRule`, validação de identificadores estáticos e `RetentionRegistry::run_all` executando `DELETE` em lotes (`LIMIT batch_size`) em transações separadas no writer.
2. Criar `src-tauri/crates/mt-db/src/maintenance/backup.rs`:
   - Implementar `create_backup_blocking` usando `rusqlite::backup::Backup` (`step(256)`, pausa de 10 ms, deadline de 60 s), arquivo temporário `0600`, verificação `PRAGMA quick_check` no backup gerado, rename atômico e rotação (`backup_daily_keep` diários e 3 pré-migration).
   - Implementar `list_backups` e `execute_recovery_action` (movendo `.db`, `-wal` e `-shm` corrompidos para quarentena privada antes de restaurar backup validado ou permitir recriação explícita).
3. Criar `src-tauri/crates/mt-db/src/maintenance/scheduler.rs` e `mod.rs`:
   - Implementar `MaintenanceHandle::run_now()` e `MaintenanceHandle::backup()`.

---

## Passo 5 — Implementar a Fachada `Db` (`src/lib.rs`)

1. Atualizar `src-tauri/crates/mt-db/src/lib.rs`:
   - Implementar `Db::open(config, setup)` integrando `InstanceLock`, `open_writer`, `run_quick_check` (retornando `DbError::RecoveryRequired` em caso de corrupção), hook de backup `PreMigration` no `MigrationRunner::run`, inicialização do `DbWriter` e `ReadPool`.
   - Implementar `Db::write`, `Db::read`, `Db::batch_writer`, `Db::settings`, `Db::maintenance` e `Db::shutdown` (drenando todos os `BatchWriter` ativos, aguardando leitores do `ReadPool`, executando `PRAGMA wal_checkpoint(TRUNCATE)` e encerrando a thread escritora).

---

## Passo 6 — Criar Testes de Integração e Benchmark de Batch

1. Adicionar o teste `premigration_backup_created` em `src-tauri/crates/mt-db/tests/migrations.rs`.
2. Criar `src-tauri/crates/mt-db/tests/batch.rs` (`coalesces`, `order_preserved`, `backpressure_non_blocking`, `explicit_flush_awaits_commit`, `flush_on_shutdown`, `storage_error_latched`).
3. Criar `src-tauri/crates/mt-db/tests/settings.rs` cobrindo defaults, `ui.*` livre, `unknown`, `invalid`, `too_large`, `sensitive`, inspeção de JSON aninhado com segredos (CA-48) e emissão de eventos pós-commit.
4. Criar `src-tauri/crates/mt-db/tests/retention.rs` cobrindo expurgo em lotes, `extra_where`, sincronização com FTS5, gravação de `last_run_at` e evento `DbMaintenanceFinished`.
5. Criar `src-tauri/crates/mt-db/tests/backup.rs` cobrindo backup online com WAL ativo, rotação de diários/pré-migration, detecção de corrupção (`RecoveryRequired`) sem apagar originais e restauração explícita.
6. Implementar `src-tauri/crates/mt-db/benches/batch.rs` medindo a inserção de 100 000 registros de 200 bytes.

---

## Passo 7 — Comandos de Verificação e Validação

Executar a partir de `src-tauri/`:

```bash
cargo check -p mt-db --all-targets
cargo clippy -p mt-db --all-targets -- -D warnings
cargo fmt --all --check
cargo test -p mt-db
cargo bench -p mt-db --bench batch -- --quick
```

---

## Critérios de Aceite Cobertos nesta Implementação

- [x] **CA-17**: Existindo migrations pendentes em banco não vazio, `backups/maxterminal-premigration-v<N>-<ts>.db` é criado antes da aplicação.
- [x] **CA-19 / CA-20 / CA-21 / CA-22**: `BatchWriter` coalescendo 10 000 registros em ≤ 25 transações, `push()` não bloqueante (< 1 ms) com `Backpressure` e rate-limit de 1/s em `DbWriterLag`, `shutdown()` confirmando lotes e truncando WAL, e benchmark de 100 000 registros em < 1,5 s.
- [x] **CA-33 / CA-48 / CA-49**: Barreira `SanitizedText` nos repositórios/batches, inspeção recursiva de JSON em `settings` rejeitando chaves/valores sensíveis sem ecoar o segredo, e falhas de commit/disco cheio tratadas sem gerar eventos falsos de sucesso.
- [x] **CA-40 / CA-41 / CA-42 / CA-43 / CA-50**: `RetentionRule` removendo registros antigos em lotes sincronizados com FTS, manutenção gravando `db.maintenance.last_run_at`, rotação de backups (`backup_daily_keep` diários + 3 pré-migration) e recuperação explícita preservando `.db/-wal/-shm` em quarentena.
