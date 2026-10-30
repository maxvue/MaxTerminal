# Implementação 04 — Integração Tauri, Comandos IPC, Eventos, `LogRedactor` e Shutdown Coordenado (`spec.md`)

> Escopo da Fase 03: **I-11** (`SanitizerLogRedactor` plugado em `src-tauri/src/logging.rs`), **I-14** (Comandos IPC `db_settings_get`, `db_settings_set`, `db_setting_snapshot`, `db_status`, `db_recover` e eventos `db://*`), **I-17** (Testes IPC e teste fim a fim de sinks `sanitizer_sinks.rs`) e **I-18/I-19** (`DbService`, inicialização em background e shutdown coordenado no `ExitRequested`).
> Critérios de aceite cobertos: **CA-01**, **CA-02**, **CA-03**, **CA-04**, **CA-06**, **CA-21**, **CA-23**, **CA-32**, **CA-34**, **CA-35**, **CA-36**, **CA-37**, **CA-38**, **CA-43**, **CA-47**, **CA-48**.

---

## 1. Contexto, Objetivo e Escopo

Com o crate `mt-db` completo, esta implementação integra a persistência SQLite e o sanitizador ao crate binário Tauri (`src-tauri` / `maxterminal`):
1. **Redator de Logs (`src-tauri/src/logging.rs`)**: implementa `SanitizerLogRedactor` usando `mt_db::sanitizer::sanitize` e instala-o no `init_logging` **antes** da primeira mensagem de log, migration ou abertura do banco, garantindo que qualquer linha de log (ex.: `tracing::info!("Authorization: Bearer abc.def.ghi123456")`), stderr de debug ou panic hook seja sanitizada por evento completo antes de serializar JSON (CA-32).
2. **Serviço Gerenciado `DbService` (`src-tauri/src/state.rs`)**: gerencia o ciclo de vida assíncrono do banco (`Opening`, `Ready(Db)`, `RecoveryRequired(DbRecoveryInfo)`, `Closing`, `Failed`), hooks de parada de produtores de fases futuras, agendador `MaintenanceScheduler` (primeira execução 60 s após o boot e depois a cada `db.maintenance.interval_hours`) e retenção do último `DbRecovered`.
3. **Eventos Globais Tipados (`src-tauri/src/events/db.rs`)**: registra no `tauri-specta` os 4 eventos da Fase 03 (`db://settings-changed`, `db://recovered`, `db://maintenance-finished`, `db://writer-lag`) e implementa `TauriDbEventSink` (`impl DbEventSink`).
4. **Comandos IPC (`src-tauri/src/commands/db.rs`)**: implementa `db_settings_get`, `db_settings_set`, `db_setting_snapshot`, `db_status` e `db_recover`, registrando-os em `src-tauri/src/specta.rs`, `src-tauri/build.rs`, `src-tauri/permissions/` e `src-tauri/capabilities/default.json`.
5. **Inicialização não bloqueante e Shutdown Coordenado (`src-tauri/src/lib.rs`)**: abre o banco em `tokio::task::spawn_blocking` durante o `.setup()` do Tauri e intercepta `tauri::RunEvent::ExitRequested` para drenar produtores, executar `Db::shutdown()` com orçamento de 5 s e só então liberar o encerramento do processo.
6. **Auditoria de Sinks (`src-tauri/crates/mt-db/tests/sanitizer_sinks.rs`)**: verifica que segredos de fixture nunca aparecem em `.db`, `-wal`, tabelas FTS/sombra, arquivos de backup, arquivos de log, respostas/erros IPC nem payloads externos (CA-47).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Novos Arquivos
- `src-tauri/src/commands/db.rs`
- `src-tauri/src/events/db.rs`
- `src-tauri/crates/mt-db/tests/sanitizer_sinks.rs`

### 2.2. Arquivos a Modificar
- `src-tauri/Cargo.toml` — adicionar `mt-db.workspace = true` e `tokio.workspace = true` em `[dependencies]`, e habilitar a feature `"derive"` em `tauri-specta` para suporte a `#[derive(tauri_specta::Event)]`.
- `src-tauri/src/logging.rs` — adicionar `SanitizerLogRedactor` e aplicar sanitização nos campos de eventos de log.
- `src-tauri/src/state.rs` — adicionar `DbService` e métodos de coordenação de estado/recuperação/shutdown.
- `src-tauri/src/commands/mod.rs` — adicionar `pub mod db;`.
- `src-tauri/src/events/mod.rs` — adicionar `pub mod db;`.
- `src-tauri/src/specta.rs` — registrar os 5 comandos `db_*` em `collect_commands!` e os 4 eventos `db://*` em `collect_events!`.
- `src-tauri/src/lib.rs` — plugar `SanitizerLogRedactor`, inicializar `DbService` no `.setup()` e tratar `RunEvent::ExitRequested`.
- `src-tauri/build.rs`, `src-tauri/permissions/` e `src-tauri/capabilities/default.json` — declarar permissões ACL para os 5 novos comandos e listening de eventos.
- `src-tauri/tests/bindings.rs` — validar exportação sem drift dos novos comandos, DTOs e eventos.

---

## 3. Redator de Logs (`src-tauri/src/logging.rs`)

```rust
use std::borrow::Cow;
use mt_db::sanitizer::sanitize;

/// Redator de produção que aplica o catálogo completo do sanitizador da Fase 03 a cada campo textual do evento.
#[derive(Debug, Default, Clone, Copy)]
pub struct SanitizerLogRedactor;

impl LogRedactor for SanitizerLogRedactor {
    fn redact<'a>(&self, input: &'a str) -> Cow<'a, str> {
        sanitize(input)
    }
}
```

**Regras de integração no logger (`logging.rs`):**
1. O visitante de campos (`tracing::field::Visit`) coleta `message` e demais campos estruturados do evento, passa cada valor textual por `redactor.redact(val)` **antes** de serializar o objeto JSON de linha de log e **antes** de escrever no buffer por evento. Ao instalar `SanitizerLogRedactor`, substituir a pré-filtragem por catálogo de `message`/`event` da Fase 01 pela sanitização integral: o texto original chega ao sanitizador, nunca aos sinks antes dele. Preservar a política de targets e os controles de taxa; a assinatura continua de um parâmetro. Sem essa troca o exemplo CA-32 abaixo seria reduzido ao marcador genérico antes de sanitizar.
2. Em `src-tauri/src/lib.rs`, `init_logging(&paths.log_dir, Box::new(SanitizerLogRedactor))` é chamado imediatamente após `paths.ensure_dirs()`, antes de qualquer `tracing::info!` ou abertura do banco.
3. Uma chamada `tracing::info!("Authorization: Bearer abc.def.ghi123456")` grava no arquivo `maxterminal.log.<data>` exatamente `Authorization: Bearer [REDACTED:bearer]` (CA-32).

---

## 4. Eventos Tauri-Specta e `TauriDbEventSink` (`src-tauri/src/events/db.rs`)

```rust
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;
use mt_db::{
    DbEventSink, DbMaintenanceFinished, DbRecovered, DbWriterLag, SettingsChanged,
};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "db://settings-changed")]
#[serde(transparent)]
pub struct SettingsChangedEvent(pub SettingsChanged);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "db://recovered")]
#[serde(transparent)]
pub struct DbRecoveredEvent(pub DbRecovered);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "db://maintenance-finished")]
#[serde(transparent)]
pub struct DbMaintenanceFinishedEvent(pub DbMaintenanceFinished);

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, tauri_specta::Event)]
#[tauri_specta(event_name = "db://writer-lag")]
#[serde(transparent)]
pub struct DbWriterLagEvent(pub DbWriterLag);

pub struct TauriDbEventSink<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriDbEventSink<R> {
    #[must_use]
    pub fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> DbEventSink for TauriDbEventSink<R> {
    fn settings_changed(&self, ev: &SettingsChanged) {
        let _ = SettingsChangedEvent(ev.clone()).emit(&self.app);
    }

    fn recovered(&self, ev: &DbRecovered) {
        let _ = DbRecoveredEvent(ev.clone()).emit(&self.app);
    }

    fn maintenance_finished(&self, ev: &DbMaintenanceFinished) {
        let _ = DbMaintenanceFinishedEvent(ev.clone()).emit(&self.app);
    }

    fn writer_lag(&self, ev: &DbWriterLag) {
        let _ = DbWriterLagEvent(ev.clone()).emit(&self.app);
    }
}
```

---

## 5. Serviço Gerenciado `DbService` (`src-tauri/src/state.rs`)

```rust
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::sync::{Notify, RwLock};
use mt_core::{MtError, paths::AppPaths};
use mt_db::{
    Db, DbConfig, DbError, DbRecovered, DbRecoveryAction, DbRecoveryInfo,
    DbSetup, DbStateKind, DbStatusDto,
};

pub type ProducerStopHook =
    Box<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

#[derive(Clone)]
pub struct DbService {
    inner: Arc<DbServiceInner>,
}

impl DbService {
    /// Cria instância no estado inicial `Opening`.
    #[must_use]
    pub fn opening(config: DbConfig) -> Self;

    /// Construtor direto a partir de um `Db` já aberto (útil para testes de comandos IPC).
    #[must_use]
    pub fn from_ready(config: DbConfig, db: Db) -> Self;

    /// Registra um hook assíncrono de parada de produtor (para Fases 05/07 drenarem `StreamSanitizer` antes do shutdown).
    pub async fn register_producer_stop_hook(&self, hook: ProducerStopHook);

    /// Aguarda brevemente se o estado ainda for `Opening` (até 2 s) e retorna a fachada `Db` se `Ready`,
    /// ou `Err(DbError::RecoveryRequired)` / `Err(DbError::NotReady)` / erro correspondente.
    pub async fn require_db(&self) -> Result<Db, MtError>;

    /// Retorna fotografia atual do estado do banco para o comando `db_status`.
    pub async fn status(&self) -> DbStatusDto;

    /// Executa ação explícita de recuperação (`Restore { backupId }` ou `CreateEmpty`) somente quando
    /// o estado atual for `RecoveryRequired`, reabrindo o banco e transicionando para `Ready`.
    pub async fn recover(&self, action: DbRecoveryAction) -> Result<DbStatusDto, MtError>;

    /// Executa o shutdown coordenado uma única vez:
    /// 1. Marca estado como `Closing`.
    /// 2. Cancela o `MaintenanceScheduler`.
    /// 3. Executa todos os `ProducerStopHook` registrados.
    /// 4. Chama `db.shutdown()` com orçamento de 5 s.
    pub async fn shutdown(&self) -> Result<(), MtError>;
}
```

**Inicialização em background e Agendador (`start_db_service`):**
- Em `src-tauri/src/lib.rs`, durante `.setup(|app| ...)`:
  1. Resolve `DbConfig::from_paths(&app.state::<AppState>().paths, &SystemEnv)`.
  2. Registra `DbService::opening(config.clone())` via `app.manage(service.clone())`.
  3. Dispara `tokio::task::spawn_blocking` (ou tarefa Tokio que invoca `spawn_blocking` para `Db::open(config, setup)`).
  4. Se `Db::open` retornar `Ok((db, report))`: publica `Ready(db)`, inicia a tarefa Tokio do `MaintenanceScheduler` (aguarda `Duration::from_secs(60)` após o boot para a primeira execução de `db.maintenance().run_now()` e depois repete a cada `db.maintenance.interval_hours` horas, lido das settings) e notifica waiters.
  5. Se `Db::open` retornar `Err(DbError::RecoveryRequired | DbError::Corrupt)`: lista backups disponíveis em `config.backup_dir` via `list_backups` e publica `RecoveryRequired(DbRecoveryInfo { ... })`.

---

## 6. Comandos Tauri IPC (`src-tauri/src/commands/db.rs`)

```rust
use serde_json::Value;
use tauri::State;
use mt_core::MtResult;
use mt_db::{DbRecoveryAction, DbStatusDto, SettingSnapshot};
use crate::state::DbService;

#[tauri::command]
#[specta::specta]
pub async fn db_settings_get(
    service: State<'_, DbService>,
    key: String,
) -> MtResult<Option<Value>> {
    let db = service.require_db().await?;
    Ok(db.settings().get(&key).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn db_settings_set(
    service: State<'_, DbService>,
    key: String,
    value: Value,
) -> MtResult<SettingSnapshot> {
    let db = service.require_db().await?;
    Ok(db.settings().set(&key, value).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn db_setting_snapshot(
    service: State<'_, DbService>,
    key: String,
) -> MtResult<SettingSnapshot> {
    let db = service.require_db().await?;
    Ok(db.settings().snapshot(&key).await?)
}

#[tauri::command]
#[specta::specta]
pub async fn db_status(
    service: State<'_, DbService>,
) -> MtResult<DbStatusDto> {
    Ok(service.status().await)
}

#[tauri::command]
#[specta::specta]
pub async fn db_recover(
    service: State<'_, DbService>,
    action: DbRecoveryAction,
) -> MtResult<DbStatusDto> {
    service.recover(action).await
}
```

---

## 7. Shutdown Coordenado no `RunEvent::ExitRequested` (`src-tauri/src/lib.rs`)

```rust
// No loop builder.build(context)?.run(move |app_handle, event| { ... }):
if let tauri::RunEvent::ExitRequested { api, .. } = event {
    if !exit_confirmed.load(Ordering::SeqCst) {
        api.prevent_exit();
        if !shutdown_started.swap(true, Ordering::SeqCst) {
            let handle = app_handle.clone();
            let flag = exit_confirmed.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(service) = handle.try_state::<DbService>() {
                    let _ = service.shutdown().await;
                }
                flag.store(true, Ordering::SeqCst);
                handle.exit(0);
            });
        }
    }
}
```

---

## 8. Testes Obrigatórios da Implementação 04

1. **Testes de comandos IPC em `src-tauri/src/commands/db.rs` (CA-34 a CA-38, CA-43):**
   - `db_settings_get` retorna `7` para `db.retention.backup_daily_keep` em banco novo (CA-34).
   - `db_settings_set("db.retention.backup_daily_keep", json!(0))` retorna `MtError` com `code == "db.setting_invalid"` e mensagem em `pt-BR`; valor permanece `7` (CA-35).
   - `db_settings_set` com chave desconhecida fora de `ui.*` retorna `db.setting_unknown`; `ui.qualquer.coisa` com JSON limpo é persistida e retorna `revision == 1` (CA-36).
   - `db_settings_set` com payload > 64 KiB retorna `db.setting_too_large` (CA-37).
   - `db_status` e `db_recover` em estado `RecoveryRequired` restauram backup listado ou recriam banco vazio preservando os arquivos corrompidos em quarentena (CA-43).
2. **Teste de Redação de Logs em `src-tauri/src/logging.rs` (CA-32):**
   - Emitir `tracing::info!("Authorization: Bearer abc.def.ghi123456")` com `SanitizerLogRedactor` ativo, descarregar o `LogGuard` e comprovar que o arquivo de log contém `Authorization: Bearer [REDACTED:bearer]` e zero ocorrências de `abc.def.ghi123456`.
3. **Auditoria Fim a Fim de Sinks em `src-tauri/crates/mt-db/tests/sanitizer_sinks.rs` (CA-47, CA-48):**
   - Alimentar o pipeline (stream sanitizer → batch writer → tabela indexada por FTS5 → backup online → checkpoint → retenção → `prepare_external_payload` → `inspect_settings_json` → logger) com uma lista de sentinelas secretas conhecidas de todas as 13 famílias.
   - Varrer os bytes brutos dos arquivos `.db`, `-wal`, `-shm`, arquivo de backup, tabelas sombra `*_fts*`, respostas IPC e payload externo capturado, comprovando **ausência total (0 ocorrências)** de qualquer sentinela secreta em claro antes e depois de checkpoint, retenção e reabertura.
4. **Verificação de Binário Bundled e Bindings (CA-01, CA-04):**
   - `cargo test --test bindings` passa sem drift e `ldd target/debug/maxterminal` não lista `libsqlite3.so`.
