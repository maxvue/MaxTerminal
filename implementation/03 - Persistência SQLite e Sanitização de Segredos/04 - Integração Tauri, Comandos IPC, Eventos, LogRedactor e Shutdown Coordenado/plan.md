# Implementação 04 — Integração Tauri, Comandos IPC, Eventos, `LogRedactor` e Shutdown Coordenado (`plan.md`)

> Guia de execução sequencial para a Implementação 04 da Fase 03.
> Referência técnica: [`./spec.md`](./spec.md).

---

## Passo 1 — Conectar `mt-db` ao Crate Binário e Implementar `SanitizerLogRedactor`

1. Editar `src-tauri/Cargo.toml`:
   - Adicionar `mt-db.workspace = true` e `tokio.workspace = true` em `[dependencies]`.
   - Garantir que `tauri-specta` tenha as features `["derive", "typescript"]` habilitadas no workspace.
2. Editar `src-tauri/src/logging.rs`:
   - Implementar `pub struct SanitizerLogRedactor;` implementando `LogRedactor` via `mt_db::sanitizer::sanitize(input)`.
   - Atualizar o formatador/visitante de campos de `logging.rs` para passar mensagens e campos dinâmicos permitidos por `redactor.redact()` antes da serialização JSON, mantendo arquivo com modo `0600` e cobrindo também stderr de debug.
   - Adicionar teste unitário em `logging.rs` validando **CA-32** (`Authorization: Bearer abc.def.ghi123456` → `Authorization: Bearer [REDACTED:bearer]`).

---

## Passo 2 — Implementar Eventos `db://*` e `TauriDbEventSink` (`src-tauri/src/events/`)

1. Criar `src-tauri/src/events/db.rs`:
   - Definir `SettingsChangedEvent` (`db://settings-changed`), `DbRecoveredEvent` (`db://recovered`), `DbMaintenanceFinishedEvent` (`db://maintenance-finished`) e `DbWriterLagEvent` (`db://writer-lag`) com `#[derive(Serialize, Deserialize, Clone, specta::Type, tauri_specta::Event)]`.
   - Implementar `TauriDbEventSink` implementando `mt_db::DbEventSink`.
2. Atualizar `src-tauri/src/events/mod.rs` expondo `pub mod db;`.

---

## Passo 3 — Implementar `DbService` e Inicialização Assíncrona (`src-tauri/src/state.rs`)

1. Editar `src-tauri/src/state.rs`:
   - Implementar `DbService` com estados `Opening`, `Ready(Db)`, `RecoveryRequired(DbRecoveryInfo)`, `Closing` e `Failed(String)`.
   - Implementar `require_db()`, `status()`, `recover(action)`, `register_producer_stop_hook()` e `shutdown()`.
   - Implementar `start_db_service(app_handle, paths, service)` que executa `Db::open` em `tokio::task::spawn_blocking`, publica o estado resultante e agenda a tarefa periódica de manutenção (`60 s` após o boot e depois a cada `db.maintenance.interval_hours`).

---

## Passo 4 — Implementar os 5 Comandos IPC e Atualizar `specta.rs` e ACL

1. Criar `src-tauri/src/commands/db.rs`:
   - Implementar `db_settings_get`, `db_settings_set`, `db_setting_snapshot`, `db_status` e `db_recover`.
   - Adicionar testes unitários/integração em `commands/db.rs` exercitando todos os 5 comandos e validando os códigos de erro `db.setting_invalid`, `db.setting_unknown`, `db.setting_too_large`, `db.setting_sensitive` e `db.recovery_required`.
2. Atualizar `src-tauri/src/commands/mod.rs` expondo `pub mod db;`.
3. Atualizar `src-tauri/src/specta.rs`:
   - Registrar `db_settings_get`, `db_settings_set`, `db_setting_snapshot`, `db_status` e `db_recover` em `collect_commands!`.
   - Registrar `SettingsChangedEvent`, `DbRecoveredEvent`, `DbMaintenanceFinishedEvent` e `DbWriterLagEvent` em `collect_events!` e chamar `builder.mount_events(app)` no setup.
4. Atualizar `src-tauri/build.rs`, `src-tauri/permissions/` e `src-tauri/capabilities/default.json`:
   - Incluir os 5 comandos `db_*` no `AppManifest` de `build.rs`, criar as permissões correspondentes e adicioná-las em `capabilities/default.json` (incluindo permissão `core:event:default` ou específica para escutar os eventos `db://*` na janela `main`).
5. Atualizar `src-tauri/src/lib.rs`:
   - Inicializar `init_logging(&paths.log_dir, Box::new(SanitizerLogRedactor))`.
   - No `.setup()`, criar e gerenciar `DbService` e disparar `start_db_service`.
   - No loop `.build(context)?.run(...)`, interceptar `RunEvent::ExitRequested` para executar `service.shutdown().await` antes de encerrar.

---

## Passo 5 — Criar Teste de Auditoria Fim a Fim (`tests/sanitizer_sinks.rs`) e Regenerar Bindings

1. Criar `src-tauri/crates/mt-db/tests/sanitizer_sinks.rs`:
   - Injetar segredos de todas as 13 famílias em `StreamSanitizer` → `BatchWriter` → tabela com FTS5 → `create_backup_blocking` → `wal_checkpoint` → `RetentionRegistry` → `prepare_external_payload` → `inspect_settings_json`.
   - Inspecionar os bytes em disco de `.db`, `-wal`, `-shm`, arquivo de backup e payloads gerados, comprovando zero vazamentos antes e depois de checkpoint, retenção e reabertura (CA-47).
2. Executar `npm run bindings` para regenerar `src/types/bindings.ts` e validar `cargo test --test bindings`.

---

## Passo 6 — Comandos de Verificação e Validação

Executar a partir de `src-tauri/`:

```bash
cargo build -p mt-db -p maxterminal
ldd target/debug/maxterminal | (! grep -i sqlite)
cargo clippy -p mt-db -p maxterminal --all-targets -- -D warnings
cargo fmt --all --check
cargo test -p mt-db
cargo test -p maxterminal
```

---

## Critérios de Aceite Cobertos nesta Implementação

- [x] **CA-01 / CA-02 / CA-03 / CA-04**: `cargo build`, `clippy -D warnings`, `cargo fmt --check` e `cargo test` passam em `mt-db` e `maxterminal`; `ldd` confirma ausência de `libsqlite3` dinâmica; `bindings.ts` regenerado sem drift.
- [x] **CA-06 / CA-21 / CA-23**: Inicialização cria `maxterminal.db` em `$XDG_DATA_HOME/maxterminal/`, nenhum comando async bloqueia o runtime e `ExitRequested` coordena `DbService::shutdown()`.
- [x] **CA-32**: Log contendo `Authorization: Bearer abc.def.ghi123456` é gravado no arquivo como `Authorization: Bearer [REDACTED:bearer]`.
- [x] **CA-34 / CA-35 / CA-36 / CA-37 / CA-38**: Comandos `db_settings_get`, `db_settings_set` e `db_setting_snapshot` respeitam defaults, validadores, escopo `ui.*`, limite de 64 KiB e emitem `db://settings-changed` pós-commit com `revision`.
- [x] **CA-47 / CA-48**: Segredos de fixture jamais aparecem em `.db`, `-wal`, FTS, backups, logs, erros IPC ou `ExternalPayload`.
