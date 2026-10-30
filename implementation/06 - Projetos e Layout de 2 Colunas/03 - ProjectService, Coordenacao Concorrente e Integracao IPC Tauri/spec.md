# Implementação 03 — ProjectService, Coordenação Concorrente e Integração IPC Tauri (`spec.md`)

> Parte 03 de 05 da **Fase 06: Projetos e Layout de 2 Colunas**.
> Referências canônicas: [`../instructions.md`](../instructions.md) e [`../spec.md`](../spec.md) §2.10, §3, §4, §7, §8, §9, §10 e §11.1.

---

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega o cérebro do backend de projetos (`ProjectService`) e a integração completa com o crate binário Tauri (`maxterminal`). Ela orquestra o ciclo de vida de watchers, o agendador de reconciliação (inicial, periódica a cada 30 s, no foco e manual), a janela de assentamento (`SETTLE` de 1,5 s com teto de 5 s), o controle de concorrência (`CatalogCoordinator`, `epoch`/`revision`, geração por raiz, semáforo de 8 workers e leases `ProjectActivityGuard` / `ProjectRemovalHook`), além de expor os 17 comandos IPC anotados com `#[specta::specta]`, o `TauriProjectEventSink` e a regeneração de `src/types/bindings.ts`.

### 1.1. Escopo Incluído
- `ProjectService`, `ProjectRemovalHook`, `ProjectActivityGuard` e `CatalogCoordinator` em `src-tauri/crates/mt-projects/src/service.rs`.
- `TauriProjectEventSink` em `src-tauri/src/events/projects.rs` emitindo `project://discovered`, `project://removed`, `project://changed` e `project://root-changed`.
- 17 comandos Tauri em `src-tauri/src/commands/projects.rs`.
- Registro no builder Tauri (`src-tauri/src/lib.rs`), capabilities ACL da janela principal, plugin `@tauri-apps/plugin-dialog` e exportação automática de `src/types/bindings.ts`.
- Testes de integração cobrindo concorrência, hooks de remoção, leases de atividade, reconciliação com relógio simulado (`tokio::time::pause`), aliases/symlinks redirecionados e garantia de nunca apagar arquivos do disco.

### 1.2. Fora de Escopo desta Implementação
- Stores Pinia, composables e componentes Vue — cobertos nas **Implementações 04 e 05**.

---

## 2. Arquivos a Criar ou Modificar

| Ação | Caminho Exato | Descrição |
|------|---------------|-----------|
| Criar | `src-tauri/crates/mt-projects/src/service.rs` | `ProjectService`, `ProjectRemovalHook`, `ProjectActivityGuard`, `CatalogCoordinator` e loops de background |
| Modificar | `src-tauri/crates/mt-projects/src/lib.rs` | Exportar `pub mod service;` e reexportar tipos principais no topo do crate |
| Criar | `src-tauri/src/events/projects.rs` | `TauriProjectEventSink` implementando `ProjectEventSink` via `AppHandle::emit` |
| Modificar | `src-tauri/src/events/mod.rs` | Exportar `pub mod projects;` |
| Criar | `src-tauri/src/commands/projects.rs` | Os 17 comandos IPC `project_*` e `project_root_*` com `#[tauri::command]` e `#[specta::specta]` |
| Modificar | `src-tauri/src/commands/mod.rs` | Exportar `pub mod projects;` |
| Modificar | `src-tauri/src/lib.rs` | Chamar `mt_projects::register_db(&mut db_setup)`, registrar `Arc<ProjectService>`, `collect_commands!`, `collect_events!`, `tauri_plugin_dialog` e `ProjectService::start()` em `setup()` |
| Modificar | `src-tauri/capabilities/default.json` | Autorizar comandos de projetos e `dialog:allow-open` para a janela `main` |
| Modificar | `src/types/bindings.ts` | Regenerado via teste/exportador `tauri-specta` com todos os DTOs, comandos e eventos da Fase 06 |
| Criar | `src-tauri/crates/mt-projects/tests/service_and_concurrency.rs` | Testes de integração de `ProjectService`, concorrência, hooks, leases, aliases e scheduler |

---

## 3. Especificação Técnica Detalhada

### 3.1. `ProjectService`, Hooks e Coordenador (`src-tauri/crates/mt-projects/src/service.rs`)

```rust
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use mt_core::{ids::ProjectId, MtError};
use tokio::sync::{mpsc, Mutex, OwnedRwLockReadGuard, RwLock, Semaphore};
use tokio::task::JoinHandle;
use crate::detect::StackDetector;
use crate::events::ProjectEventSink;
use crate::model::{
    DiscoveredFolder, NewProject, NewProjectRoot, Project, ProjectIdentity,
    ProjectPatch, ProjectRoot, ProjectRootId, ProjectRootPatch, ProjectSnapshot,
    RootImportReport, WatchStatus,
};
use crate::store::ProjectStore;
use crate::watcher::RootWatcher;

/// Limite máximo de caminhos aceitos em uma única chamada de `accept_discovered` / `dismiss_discovered`.
pub const MAX_DISCOVERY_BATCH_PATHS: usize = 1000;

/// Hook executado antes de remover um projeto do cadastro (Fase 07 registra encerramento de abas/PTYs).
#[async_trait::async_trait]
pub trait ProjectRemovalHook: Send + Sync + 'static {
    async fn before_remove(&self, id: ProjectId) -> Result<(), MtError>;
}

/// Guard RAII retornado por `acquire_activity`. Enquanto vivo, impede a remoção concorrente do projeto.
pub struct ProjectActivityGuard {
    _guard: OwnedRwLockReadGuard<()>,
    pub project_id: ProjectId,
}

pub struct ProjectService {
    store: Arc<dyn ProjectStore>,
    sink: Arc<dyn ProjectEventSink>,
    detector: StackDetector,
    watcher: StdMutex<RootWatcher>,
    dirty_roots: Arc<StdMutex<HashSet<ProjectRootId>>>,
    removal_hooks: StdMutex<Vec<Arc<dyn ProjectRemovalHook>>>,
    activity_gates: StdMutex<HashMap<ProjectId, Arc<RwLock<()>>>>,
    removing_ids: StdMutex<HashSet<ProjectId>>,
    scan_semaphore: Arc<Semaphore>,
    coordinator: Arc<CatalogCoordinator>,
    background_tasks: StdMutex<Vec<JoinHandle<()>>>,
    shutdown_tx: StdMutex<Option<tokio::sync::watch::Sender<bool>>>,
}

impl ProjectService {
    pub fn new(store: Arc<dyn ProjectStore>, sink: Arc<dyn ProjectEventSink>) -> Arc<Self>;

    /// Inicializa o catálogo em memória, registra watchers para raízes habilitadas e agenda varredura inicial.
    pub async fn start(self: &Arc<Self>) -> Result<(), MtError>;

    /// Cancela timers e tasks de background, fecha fila de sinais e libera debouncers.
    pub async fn shutdown(&self);

    /// Retorna visão atômica consistente do catálogo (`epoch`, `revision`, `projects`, `roots`, `discovered`).
    pub async fn snapshot(&self) -> Result<ProjectSnapshot, MtError>;

    pub fn register_removal_hook(&self, hook: Arc<dyn ProjectRemovalHook>);

    /// Adquire lease compartilhada para operações vinculadas ao projeto (ex.: Fase 07 criando aba).
    /// Recusa se o projeto não existir ou estiver em processo de remoção (`project.not_found` / `project.removing`).
    pub async fn acquire_activity(&self, id: ProjectId) -> Result<ProjectActivityGuard, MtError>;

    pub async fn list(&self) -> Result<Vec<Project>, MtError>;
    pub async fn add(&self, input: NewProject) -> Result<Project, MtError>;
    pub async fn update(&self, id: ProjectId, patch: ProjectPatch) -> Result<Project, MtError>;
    pub async fn remove(&self, id: ProjectId) -> Result<(), MtError>;
    pub async fn reorder(&self, ids: Vec<ProjectId>) -> Result<(), MtError>;
    pub async fn redetect(&self, id: ProjectId) -> Result<Project, MtError>;
    pub async fn preview_identity(&self, path: String) -> Result<ProjectIdentity, MtError>;
    pub async fn refresh_missing(&self) -> Result<Vec<Project>, MtError>;

    pub async fn list_roots(&self) -> Result<Vec<ProjectRoot>, MtError>;
    pub async fn add_root(&self, input: NewProjectRoot) -> Result<RootImportReport, MtError>;
    pub async fn update_root(&self, id: ProjectRootId, patch: ProjectRootPatch) -> Result<ProjectRoot, MtError>;
    pub async fn remove_root(&self, id: ProjectRootId) -> Result<(), MtError>;
    pub async fn rescan_root(&self, id: ProjectRootId) -> Result<RootImportReport, MtError>;

    pub async fn list_discovered(&self) -> Result<Vec<DiscoveredFolder>, MtError>;
    pub async fn accept_discovered(&self, paths: Vec<String>) -> Result<Vec<Project>, MtError>;
    pub async fn dismiss_discovered(&self, paths: Vec<String>) -> Result<(), MtError>;
}
```

### 3.2. Regras de Negócio e Concorrência de `ProjectService`

1. **Coordenador (`CatalogCoordinator`) e Controle de `epoch` / `revision`:**
   - Inicializa `epoch = Uuid::now_v7().to_string()` e `revision = 1`.
   - Toda mutação persistida ou alteração de estado observável (`pending` de sugestões ou `watch_status` de raízes) adquire o `Mutex` assíncrono interno do coordenador **somente para o commit e publicação do estado em memória** (nunca durante E/S de disco de `scan_level1`, `StackDetector` ou execução de `ProjectRemovalHook`).
   - Incrementa `revision` em 1 por alteração observável; se `revision == u32::MAX`, gera novo `epoch = Uuid::now_v7().to_string()` e reinicia `revision = 1`.
   - Todos os eventos (`discovered`, `removed`, `changed`, `root_changed`) derivados de um mesmo commit compartilham exatamente o mesmo par `(epoch, revision)` e são emitidos **após** o commit bem-sucedido no SQLite. Em falha de transação, nenhum evento é emitido e o estado em memória não é alterado.

2. **Leases de Atividade (`acquire_activity`) e Remoção Segura (`remove`):**
   - Cada `ProjectId` possui um `Arc< tokio::sync::RwLock<()> >` em `activity_gates`.
   - `acquire_activity(id)` verifica que `id` não está em `removing_ids`, adquire `read_owned().await` e revalida que o projeto ainda existe no catálogo atual; se não existir ou estiver removendo, libera o guard e retorna `Err`.
   - `remove(id)`:
     1. Insere `id` em `removing_ids` (se já estiver, retorna erro/conflito).
     2. Adquire `write_owned().await` no gate do `id` (aguardando qualquer `ProjectActivityGuard` prévio encerrar e bloqueando novas aquisições).
     3. Executa todos os `ProjectRemovalHook::before_remove(id)` em sequência **fora** do lock do coordenador e fora de transação SQLite. Se qualquer hook retornar `Err(e)`, remove `id` de `removing_ids`, libera o write lock e retorna `MtError::new(ErrorCode("project.removal_blocked"), format!("Não foi possível remover o projeto: {}", e.message)).with_details(...)`.
     4. Coleta todas as origens (`project_root_entries`) que apontam para `project.path` (mais o próprio `project.root_id` se houver) e constrói `Vec<DismissedRow>` para que o projeto removido não volte a ser sugerido nem auto-importado pelas raízes atuais.
     5. Executa `CatalogMutation::Remove { id, dismiss }` em uma única transação e emite `ProjectChangedPayload::Deleted { ids: vec![id] }`.

3. **Ciclo de Vida de Raízes, Geração (`root_generation`) e Assentamento (`SETTLE`):**
   - Cada `ProjectRootId` possui um contador `generation: u64` em memória.
   - Qualquer `update_root`, `remove_root` ou ` watch_enabled = false` incrementa `generation` daquela raiz.
   - Em `add_root`:
     1. Valida o caminho com `paths::validate_root_dir(&input.path)`.
     2. Se `input.watch_enabled == true`, registra o watcher **antes** do scan inicial (sinais que ocorrerem durante o scan inicial caem em `dirty_roots` para um ciclo subsequente). Se `watch()` falhar, define `watch_status = WatchStatus::Error`, registra `ScanWarning` com código `"project_root.watch_failed"`, mas **não** aborta o cadastro da raiz.
     3. Executa `scan_level1` + detecção paralela limitada por `scan_semaphore` (`Semaphore::new(8)` em `spawn_blocking`).
     4. Se `input.import_existing == true` ou `input.discovery_mode == DiscoveryMode::AutoImport`, prepara os projetos elegíveis para inserção imediata na transação `AddRoot`; se `import_existing == false` e `discovery_mode == Suggest`, grava a raiz e as `project_root_entries` e popula `pending` com as `DiscoveredFolder`.
   - Coalescência de `WatchSignal::RootChanged(root_id)`:
     - Ao receber `RootChanged(root_id)` (ou ao drenar `dirty_roots` a cada 1 s), inicia/reinicia um timer de `SETTLE` (1,5 s) para aquela raiz, respeitando um teto máximo de 5 s desde o primeiro sinal do lote (`first_signal_at + Duration::from_secs(5)`).
     - Ao disparar o timer, se já houver um scan em voo para `root_id`, marca `rescan_needed = true` para rodar logo após o término; caso contrário, captura `gen = current_generation(root_id)` e executa a reconciliação da raiz.
     - Antes de commitar o `ReconcileBatch`, o coordenador verifica se `current_generation(root_id) == gen` e se a raiz ainda existe/está habilitada. Se mudou, descarta o resultado silenciosamente!
   - Backoff de Raiz Ausente (`WatchSignal::RootMissing`):
     - Marca `watch_status = WatchStatus::Missing`, emite `project://root-changed` e agenda re-tentativa com backoff exponencial: `5s → 10s → 20s → 40s → 60s` (teto 60 s). Quando o diretório reaparecer, recria o watcher, volta para `WatchStatus::Watching` e executa reconciliação completa.
   - Reconciliação Periódica (a cada 30 s):
     - Re-varre raízes habilitadas (`watch_enabled == true`), reavalia projetos com `identity_source == Auto && stack == Unknown` (caso `Cargo.toml`/`package.json` tenha terminado de ser clonado após o `SETTLE`), reavalia sugestões pendentes e verifica `missing` nos projetos cadastrados.

4. **Regras de Aliases, Raízes Sobrepostas e `missing`:**
   - Dois symlinks na mesma raiz ou duas raízes apontando para o mesmo `canonical_path` resultam em **um único `Project`** (ou uma única `DiscoveredFolder` pendente).
   - A raiz representante (`Project.root_id` na criação ou `DiscoveredFolder.root_id`) é sempre a raiz elegível mais antiga por `(created_at, id)`.
   - Se várias raízes observam o mesmo destino não cadastrado e pelo menos uma origem não foi dispensada: se qualquer origem ativa tiver `discovery_mode == AutoImport`, o destino é auto-importado; caso contrário, é sugerido uma única vez.
   - Remover um symlink/alias em uma raiz remove apenas a linha em `project_root_entries`. O campo `Project.missing` é determinado exclusivamente por `std::fs::metadata(&project.path)`: apenas `ErrorKind::NotFound` ou `ErrorKind::NotADirectory` marca `missing = true` (emitindo `ProjectRemovedPayload { kind: RemovedKind::ProjectMissing, ... }` e `ProjectChangedPayload::Upserted`); `PermissionDenied` preserva o valor anterior de `missing` e registra warning.

5. **Aceitação e Dispensa em Lote (`accept_discovered` / `dismiss_discovered`):**
   - Rejeita chamadas com `paths.len() > MAX_DISCOVERY_BATCH_PATHS` (1000) ou strings vazias/NUL com `project.invalid_argument` antes de qualquer I/O.
   - Deduplica `paths` mantendo ordem.
   - Para cada `path` em `accept_discovered`:
     - Se o destino já estiver cadastrado em `projects`, trata como sucesso idempotente (retorna o projeto existente e garante limpeza da sugestão pendente).
     - Caso contrário, exige que exista sugestão pendente elegível (senão retorna `project.discovery_not_found`) e re-executa `canonical_dir(&path)` imediatamente antes do commit: se o caminho não existir mais ou se o destino canônico mudou (ex.: symlink redirecionado), aborta o lote inteiro com `project.discovery_stale` e agenda rescan da raiz.

### 3.3. Comandos Tauri (`src-tauri/src/commands/projects.rs`)

```rust
use std::sync::Arc;
use mt_core::{ids::ProjectId, MtError};
use mt_projects::model::{
    DiscoveredFolder, NewProject, NewProjectRoot, Project, ProjectIdentity,
    ProjectPatch, ProjectRoot, ProjectRootId, ProjectRootPatch, ProjectSnapshot,
    RootImportReport,
};
use mt_projects::service::ProjectService;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub async fn project_snapshot(svc: State<'_, Arc<ProjectService>>) -> Result<ProjectSnapshot, MtError> {
    svc.snapshot().await
}

#[tauri::command]
#[specta::specta]
pub async fn project_list(svc: State<'_, Arc<ProjectService>>) -> Result<Vec<Project>, MtError> {
    svc.list().await
}

#[tauri::command]
#[specta::specta]
pub async fn project_add(svc: State<'_, Arc<ProjectService>>, input: NewProject) -> Result<Project, MtError> {
    svc.add(input).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_update(
    svc: State<'_, Arc<ProjectService>>,
    id: ProjectId,
    patch: ProjectPatch,
) -> Result<Project, MtError> {
    svc.update(id, patch).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_remove(svc: State<'_, Arc<ProjectService>>, id: ProjectId) -> Result<(), MtError> {
    svc.remove(id).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_reorder(svc: State<'_, Arc<ProjectService>>, ids: Vec<ProjectId>) -> Result<(), MtError> {
    svc.reorder(ids).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_redetect(svc: State<'_, Arc<ProjectService>>, id: ProjectId) -> Result<Project, MtError> {
    svc.redetect(id).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_preview_identity(
    svc: State<'_, Arc<ProjectService>>,
    path: String,
) -> Result<ProjectIdentity, MtError> {
    svc.preview_identity(path).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_refresh_missing(svc: State<'_, Arc<ProjectService>>) -> Result<Vec<Project>, MtError> {
    svc.refresh_missing().await
}

#[tauri::command]
#[specta::specta]
pub async fn project_root_list(svc: State<'_, Arc<ProjectService>>) -> Result<Vec<ProjectRoot>, MtError> {
    svc.list_roots().await
}

#[tauri::command]
#[specta::specta]
pub async fn project_root_add(
    svc: State<'_, Arc<ProjectService>>,
    input: NewProjectRoot,
) -> Result<RootImportReport, MtError> {
    svc.add_root(input).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_root_update(
    svc: State<'_, Arc<ProjectService>>,
    id: ProjectRootId,
    patch: ProjectRootPatch,
) -> Result<ProjectRoot, MtError> {
    svc.update_root(id, patch).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_root_remove(svc: State<'_, Arc<ProjectService>>, id: ProjectRootId) -> Result<(), MtError> {
    svc.remove_root(id).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_root_rescan(
    svc: State<'_, Arc<ProjectService>>,
    id: ProjectRootId,
) -> Result<RootImportReport, MtError> {
    svc.rescan_root(id).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_discovery_list(
    svc: State<'_, Arc<ProjectService>>,
) -> Result<Vec<DiscoveredFolder>, MtError> {
    svc.list_discovered().await
}

#[tauri::command]
#[specta::specta]
pub async fn project_discovery_accept(
    svc: State<'_, Arc<ProjectService>>,
    paths: Vec<String>,
) -> Result<Vec<Project>, MtError> {
    svc.accept_discovered(paths).await
}

#[tauri::command]
#[specta::specta]
pub async fn project_discovery_dismiss(
    svc: State<'_, Arc<ProjectService>>,
    paths: Vec<String>,
) -> Result<(), MtError> {
    svc.dismiss_discovered(paths).await
}
```

### 3.4. Emissor de Eventos Tauri (`src-tauri/src/events/projects.rs`)

```rust
use mt_projects::events::{
    ProjectChangedPayload, ProjectDiscoveredPayload, ProjectEvent, ProjectEventSink,
    ProjectRemovedPayload, ProjectRootChangedPayload,
};
use tauri::{AppHandle, Emitter};

pub const EVENT_PROJECT_DISCOVERED: &str = "project://discovered";
pub const EVENT_PROJECT_REMOVED: &str = "project://removed";
pub const EVENT_PROJECT_CHANGED: &str = "project://changed";
pub const EVENT_PROJECT_ROOT_CHANGED: &str = "project://root-changed";

pub struct TauriProjectEventSink {
    app: AppHandle,
}

impl TauriProjectEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ProjectEventSink for TauriProjectEventSink {
    fn discovered(&self, event: ProjectEvent<ProjectDiscoveredPayload>) {
        if let Err(err) = self.app.emit(EVENT_PROJECT_DISCOVERED, &event) {
            tracing::warn!("Falha ao emitir evento {}: {}", EVENT_PROJECT_DISCOVERED, err);
        }
    }

    fn removed(&self, event: ProjectEvent<ProjectRemovedPayload>) {
        if let Err(err) = self.app.emit(EVENT_PROJECT_REMOVED, &event) {
            tracing::warn!("Falha ao emitir evento {}: {}", EVENT_PROJECT_REMOVED, err);
        }
    }

    fn changed(&self, event: ProjectEvent<ProjectChangedPayload>) {
        if let Err(err) = self.app.emit(EVENT_PROJECT_CHANGED, &event) {
            tracing::warn!("Falha ao emitir evento {}: {}", EVENT_PROJECT_CHANGED, err);
        }
    }

    fn root_changed(&self, event: ProjectEvent<ProjectRootChangedPayload>) {
        if let Err(err) = self.app.emit(EVENT_PROJECT_ROOT_CHANGED, &event) {
            tracing::warn!("Falha ao emitir evento {}: {}", EVENT_PROJECT_ROOT_CHANGED, err);
        }
    }
}
```
