# Implementação 03 — Orquestração de Runtime, Ciclo de Vida de PTYs, Split Panes e Sessões

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega o núcleo de orquestração em runtime do workspace (`src-tauri/src/workspace/`): adaptadores com as Fases 02, 05 e 06 (`PtyLifecycleAdapter`, `ShellWorkspaceAdapter`, `BaseSessionAdapter`, `WorkspaceSessionProvider`), resolvedor de ambiente (`EnvironmentRegistry` + `ShellResolver`), supervisão serializada por aba via `TabActor`, e os fluxos transacionais e compensados do `WorkspaceManager` (`tab_open`, `tab_close`, `tab_reorder`, `tab_focus`, `pane_split`, `pane_close`, `pane_restart`, `pane_layout_update`, `session_new`, `workspace_initial_command_run`, `shutdown` e `ProjectRemovalHook`).

### Escopo Exato
1. **Adaptadores de Runtime (`workspace/adapters.rs`):**
   - `PtyLifecycleAdapter`: encapsula `PtyManager::spawn` (sem sink visual inicial — usa ring buffer da Fase 02 até `pty_attach`), `kill_and_wait` via `tokio::task::spawn_blocking`, consulta de `cwd` vivo via `/proc` da Fase 02 e supervisão de saída (`subscribe`).
   - `WorkspaceSessionProvider` + `ShellWorkspaceAdapter`: implementa `SessionProvider` da Fase 05 associando `(PtyId, ExecutionOwner)` **antes** da leitura do primeiro byte do PTY, controla `drain_execution(pty_id)` (aguardando fechamento de blocos pendentes e commit do worker da Fase 05), propaga `ContinuousHint` e correlaciona eventos de prompt confiável OSC 133 A/B a `(PtyId, generation)`.
   - `BaseSessionAdapter`: coordena `SessionsRepo::{prepare, close, abort}` da Fase 05 com `TabSessionsRepo` da Fase 07.
2. **Registro de Ambientes (`workspace/environment.rs`):**
   - Trait `EnvironmentResolver` (ponto de extensão para a Fase 13), `EnvironmentRegistry`, `SpawnContext`, `ResolvedSpawn` e `ShellResolver`.
   - Construção do ambiente efetivo = ambiente base permitido pela Fase 02 + variáveis do template (`Literal` e `Inherited` resolvidas apenas em memória) + overrides + variáveis internas `MAXTERMINAL_TAB_ID`, `MAXTERMINAL_PANE_ID`, `MAXTERMINAL_SESSION_ID`.
3. **Ator por Aba (`workspace/runtime.rs`) e Orquestrador (`workspace/manager.rs`):**
   - `TabActor` com mailbox limitada (`mpsc::channel(64)`), estado operacional em memória (`TabRuntimeState`, mapa de `PaneRuntimeEntry` com `PtyId`, `SessionId`, `generation`, estado de prompt e `initial_dispatch`), evitando qualquer lock SQLite durante chamadas de PTY/E/S.
   - Despacho do comando inicial no primeiro prompt confiável (com reserva atômica `initial_dispatch = claimed` antes de `write(cmd + "\r")` e transição para `sent`), timeout de 5 segundos transicionando para `waiting` sem injeção cega, e comando explícito `workspace_initial_command_run`.
   - Operações de painel (`pane_split` herdando `cwd` sem repetir comando inicial, `pane_close` colapsando árvore ou delegando para `tab_close` no último painel, `pane_restart` exigindo `Exited`/`Failed`, `pane_layout_update`).
   - Encerramento supervisionado e "Iniciar nova Sessão" (`session_new`): parada paralela via `kill_and_wait(Graceful)` com escalada para `Force` da Fase 02, retenção em `TabLifecycle::Blocked` se processos sobreviverem, drenagem da Fase 05 (`drain_execution`; a barreira `finish_execution` do recorder é integrada na Implementação 04), saga de arquivamento do grupo anterior e criação do grupo `seq + 1` preservando `TabId`, `PaneId` e layout, seguida do spawn de novos PTYs com `generation + 1`.
   - Integração com `ProjectService::acquire_activity` / `ProjectActivityGuard` (lease adquirida apenas durante a operação de abertura/reinício e liberada ao final) e `ProjectRemovalHook` da Fase 06 (sem reentrância de lease), mais `WorkspaceLifecycleObserver` (`workspace/observer.rs`, dono: Fase 07) e a consulta pública `WorkspaceManager::pane_pty` para consumidores das Fases 08 e 13.
   - O `ScrollbackRecorder` **não** faz parte desta implementação: o módulo `recorder`, o campo no `WorkspaceManager` e a chamada a `finish_execution` entram na Implementação 04, mantendo esta etapa compilável e testável isoladamente (a barreira de drenagem aqui é apenas `ShellWorkspaceAdapter::drain_execution` da Fase 05).

---

## 2. Arquivos a Criar ou Modificar

### Criar
- `src-tauri/src/workspace/mod.rs`
- `src-tauri/src/workspace/errors.rs`
- `src-tauri/src/workspace/adapters.rs`
- `src-tauri/src/workspace/environment.rs`
- `src-tauri/src/workspace/observer.rs`
- `src-tauri/src/workspace/spawn_hooks.rs`
- `src-tauri/src/workspace/runtime.rs`
- `src-tauri/src/workspace/manager.rs`

### Modificar
- `src-tauri/src/lib.rs` (registrar `pub mod workspace;`)

---

## 3. Especificação Técnica Detalhada

### 3.1. Propriedade de Execução e Adaptadores (`workspace/adapters.rs`)

```rust
use bytes::Bytes;
use dashmap::DashMap;
use mt_core::error::MtError;
use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_pty::{ExitInfo, KillMode, PtyOwner, SpawnSpec};
use mt_db::Db;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ExecutionOwner {
    pub project_id: ProjectId,
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub session_id: SessionId,
    pub tab_session_id: TabSessionId,
    pub generation: i64,
}

pub struct PtyLifecycleAdapter {
    pty_manager: Arc<mt_pty::PtyManager>,
}

impl PtyLifecycleAdapter {
    pub fn new(pty_manager: Arc<mt_pty::PtyManager>) -> Self;

    /// Executa spawn do PTY sem OutputSink inicial (bytes ficam retidos no ring buffer da Fase 02 até `pty_attach`),
    /// invocando `pre_read_hook(pty_id)` ANTES que a thread reader do PTY processe o primeiro byte.
    pub async fn spawn_supervised<F>(
        &self,
        spec: SpawnSpec,
        owner: ExecutionOwner,
        pre_read_hook: F,
    ) -> Result<PtyId, MtError>
    where
        F: FnOnce(PtyId) -> Result<(), MtError> + Send + 'static;

    pub async fn write(&self, pty_id: PtyId, bytes: Bytes) -> Result<(), MtError>;

    /// Executa `kill_and_wait` dentro de `tokio::task::spawn_blocking`.
    /// Tenta `KillMode::Graceful` pelo orçamento configurado e, em caso de timeout, escala para `KillMode::Force`.
    /// Retorna `Err(MtError::workspace_stop_failed(...))` se qualquer processo descendente (mesmo com `setsid`)
    /// permanecer vivo em `/proc` pelo par `(pid, start_time)`.
    pub async fn terminate_and_wait(
        &self,
        pty_id: PtyId,
        tab_id: TabId,
        graceful_timeout: Duration,
        force_timeout: Duration,
    ) -> Result<ExitInfo, MtError>;

    /// Obtém o cwd atual do processo em foreground do PTY via `/proc` da Fase 02.
    pub fn query_live_cwd(&self, pty_id: PtyId) -> Option<PathBuf>;
}

pub struct WorkspaceSessionProvider {
    prepared_by_pty: DashMap<PtyId, ExecutionOwner>,
    db: Db,
}

impl WorkspaceSessionProvider {
    pub fn new(db: Db) -> Self;

    /// Registra o owner no índice em memória dentro do `pre_read_hook` do spawn.
    pub fn bind_before_reader(&self, pty_id: PtyId, owner: ExecutionOwner);
    pub fn owner_of(&self, pty_id: PtyId) -> Option<ExecutionOwner>;
    pub fn remove_pty(&self, pty_id: PtyId) -> Option<ExecutionOwner>;
}

pub struct ShellWorkspaceAdapter {
    shell_runtime: Arc<mt_shell::ShellRuntime>,
    session_provider: Arc<WorkspaceSessionProvider>,
    early_prompts: DashMap<(PtyId, i64), ()>,
}

impl ShellWorkspaceAdapter {
    pub fn new(
        shell_runtime: Arc<mt_shell::ShellRuntime>,
        session_provider: Arc<WorkspaceSessionProvider>,
    ) -> Self;

    /// Prepara o contexto de shell integration (Fase 05) antes do início do reader do PTY.
    /// Inclui `ContinuousHint` quando `continuous == true`.
    pub fn attach_pre_read(
        &self,
        pty_id: PtyId,
        owner: ExecutionOwner,
        continuous: bool,
        has_initial_command: bool,
    ) -> Result<(), MtError>;

    /// Verifica ou consome prontidão de prompt OSC 133 A/B retida para `(pty_id, generation)`
    /// (cobrindo o caso em que o primeiro prompt chega antes do retorno assíncrono do spawn).
    pub fn take_retained_prompt_ready(&self, pty_id: PtyId, generation: i64) -> bool;

    /// Fecha blocos em aberto da execução `pty_id`, aguarda a fila do worker da Fase 05 e confirma
    /// o commit de todas as linhas em `command_blocks` e `sessions` (`status = 'closed'`).
    pub async fn drain_execution(&self, pty_id: PtyId, exit: Option<&ExitInfo>) -> Result<(), MtError>;

    /// Notifica a Fase 05 para limpar o estado de status da aba antes de iniciar os eventos de uma nova sessão.
    pub fn reset_tab_status_for_new_session(&self, tab_id: TabId, new_tab_session_id: TabSessionId);

    /// Remove a aba do registry de status da Fase 05 após fechamento definitivo.
    pub fn remove_tab_status(&self, tab_id: TabId);
}
```

---

### 3.2. Resolvedor de Ambiente (`workspace/environment.rs`)

```rust
use mt_core::error::MtError;
use mt_pty::SpawnSpec;
use mt_core::workspace::template::{EnvValue, EnvironmentKind, LaunchSnapshot};
use crate::workspace::adapters::ExecutionOwner;
use dashmap::DashMap;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SpawnContext {
    pub owner: ExecutionOwner,
    pub snapshot: LaunchSnapshot,
    pub cwd: PathBuf,
    pub is_primary: bool,
    pub emit_initial_command: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedSpawn {
    pub spawn_spec: SpawnSpec,
    pub shell_integration_enabled: bool,
    pub initial_command: Option<String>,
}

pub trait EnvironmentResolver: Send + Sync + 'static {
    fn resolve(&self, ctx: &SpawnContext) -> Result<ResolvedSpawn, MtError>;
}

pub struct EnvironmentRegistry {
    shell_resolver: Arc<dyn EnvironmentResolver>,
    agent_resolvers: DashMap<String, Arc<dyn EnvironmentResolver>>,
}

impl EnvironmentRegistry {
    pub fn new(shell_resolver: Arc<dyn EnvironmentResolver>) -> Self;

    /// Ponto de extensão para a Fase 13 registrar os resolvedores de agentes CLI (`claude`, `codex`, `agy`, `opencode`).
    pub fn register_agent_resolver(&self, agent_id: impl Into<String>, resolver: Arc<dyn EnvironmentResolver>);

    pub fn has_agent_resolver(&self, agent_id: &str) -> bool;

    /// Resolve o `SpawnContext`. Se `snapshot.environment.kind == EnvironmentKind::Agent` e não houver
    /// resolvedor registrado para `agent_id`, retorna imediatamente `Err(MtError::workspace_environment_unavailable(agent_id))`
    /// antes de abrir qualquer PTY.
    pub fn resolve(&self, ctx: &SpawnContext) -> Result<ResolvedSpawn, MtError>;
}
```

#### Regras de Resolução de Variáveis de Ambiente em `ShellResolver`
1. Parte do conjunto de variáveis permitidas do processo pai fornecido pela Fase 02 (`mt_pty::allowed_host_env()`).
2. Para cada `(key, env_val)` em `ctx.snapshot.env_vars`:
   - `EnvValue::Literal { value }`: insere `(key, value)` (já validado como não secreto).
   - `EnvValue::Inherited { source }`: busca `source` no `allowed_host_env()` em memória. Se `source` não existir no ambiente do processo, retorna `Err(MtError::template_invalid("envVars", format!("Variável de ambiente herdada '{}' não está definida no sistema", source)))`. O valor resolvido fica **apenas** na memória do `SpawnSpec` e nunca é persistido no SQLite nem incluído em logs/eventos.
3. Injeta variáveis internas somente leitura:
   - `MAXTERMINAL_PROJECT_ID`, `MAXTERMINAL_TAB_ID`, `MAXTERMINAL_PANE_ID`, `MAXTERMINAL_SESSION_ID`, `MAXTERMINAL_TAB_SESSION_ID`.
4. `initial_command`: somente incluído em `ResolvedSpawn.initial_command` quando `ctx.is_primary && ctx.emit_initial_command`.

---

### 3.3. `TabActor`, `WorkspaceLifecycleObserver` e `WorkspaceManager` (`runtime.rs`, `observer.rs`, `manager.rs`)

#### Assinaturas de `WorkspaceLifecycleObserver` (`workspace/observer.rs`)
```rust
use crate::workspace::adapters::ExecutionOwner;
use mt_core::ids::{PaneId, ProjectId, PtyId, TabId, TabSessionId};
use mt_pty::ExitInfo;
use mt_core::workspace::session::SessionSummary;
use mt_core::workspace::tab::TabView;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum WorkspaceLifecycleSignal {
    TabOpened { tab: TabView },
    TabLayoutChanged { tab: TabView },
    TabClosed { tab_id: TabId, project_id: ProjectId, revision: i64 },
    PaneSpawned { owner: ExecutionOwner, pty_id: PtyId, cwd: PathBuf, revision: i64 },
    PaneExited { owner: ExecutionOwner, pty_id: PtyId, exit: ExitInfo, revision: i64 },
    SessionArchived { summary: SessionSummary },
}

pub trait WorkspaceLifecycleObserver: Send + Sync + 'static {
    fn on_signal(&self, signal: WorkspaceLifecycleSignal);
}

/// Fila de despacho ordenada por `TabId`, executada fora do lock/mailbox do `TabActor`.
/// Pertence ao `WorkspaceManager`; consumidores se registram via `WorkspaceManager::register_observer`.
pub struct ObserverDispatcher { /* observers: RwLock<Vec<Arc<dyn WorkspaceLifecycleObserver>>>, filas por TabId */ }

impl ObserverDispatcher {
    pub fn register(&self, observer: Arc<dyn WorkspaceLifecycleObserver>);
    pub fn dispatch(&self, signal: WorkspaceLifecycleSignal);
}
```
- Os sinais de `WorkspaceLifecycleObserver` são despachados fora do lock/mailbox do `TabActor` em uma fila ordenada por `TabId`, permitindo que a Fase 08 (Git/Portas) e a Fase 13 (Agentes) observem spawns/exits validando `owner.generation`.

#### Assinaturas de `WorkspaceManager` (`workspace/manager.rs`)
```rust
use crate::workspace::adapters::{ExecutionOwner, PtyLifecycleAdapter, ShellWorkspaceAdapter, WorkspaceSessionProvider};
use crate::workspace::environment::EnvironmentRegistry;
use crate::workspace::observer::{ObserverDispatcher, WorkspaceLifecycleObserver};
use crate::workspace::runtime::TabActorHandle;
use crate::workspace::spawn_hooks::WorkspaceSpawnHook;
use dashmap::DashMap;
use mt_core::error::MtError;
use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId, TemplateId};
use mt_core::workspace::layout::{LayoutSizePatch, SplitDirection};
use mt_core::workspace::session::{
    CursorPage, ScrollbackChunk, SessionDetail, SessionEndReason, SessionSearchHit, SessionSummary,
    WorkspaceRestoreReport, WorkspaceSnapshot,
};
use mt_core::workspace::tab::{PaneCloseOutcome, TabView};
use mt_core::workspace::template::{TabOpenRequest, TabTemplate, TabTemplateInput};
use mt_db::Db;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;

pub struct WorkspaceManager {
    boot_id: String,
    event_seq: AtomicI64,
    db: Db,
    pty: Arc<PtyLifecycleAdapter>,
    shell: Arc<ShellWorkspaceAdapter>,
    session_provider: Arc<WorkspaceSessionProvider>,
    environments: Arc<EnvironmentRegistry>,
    /// Serviço de projetos da Fase 06: `acquire_activity(project_id)` é chamado apenas durante a operação
    /// de abertura/reinício (tab_open, pane_split, pane_restart, session_new e restauração) e o guard é liberado ao final.
    project_service: Arc<mt_projects::service::ProjectService>,
    observers: Arc<ObserverDispatcher>,
    spawn_hooks: Vec<Arc<dyn WorkspaceSpawnHook>>,
    tabs: DashMap<TabId, TabActorHandle>,
    pty_index: DashMap<PtyId, ExecutionOwner>,
    in_flight_requests: DashMap<String, Arc<tokio::sync::Notify>>,
    restore_cell: tokio::sync::OnceCell<WorkspaceRestoreReport>,
}

impl WorkspaceManager {
    pub fn boot_id(&self) -> &str;
    pub fn next_event_seq(&self) -> i64;

    // Consultas públicas e integração para consumidores (Fases 08 e 13)
    /// Resolve o `PtyId` atualmente vivo do painel `pane_id` da aba `tab_id` (`None` se a aba/painel não existir,
    /// se o painel não estiver `Live` ou se o PTY já tiver saído). Leitura em memória via `pty_index`/`TabActor`,
    /// sem tocar o SQLite. Usado por `git_track_pane` e `PtyProcessTreeAdapter::verify_pane_ownership` (Fase 08).
    pub async fn pane_pty(&self, tab_id: TabId, pane_id: PaneId) -> Option<PtyId>;
    /// Registra um observador de ciclo de vida; sinais chegam fora do lock/mailbox do `TabActor`, ordenados por `TabId`.
    pub fn register_observer(&self, observer: Arc<dyn WorkspaceLifecycleObserver>);
    /// Builder consumido antes de Arc/app.manage/restore; não permite instalação tardia em runtime.
    pub fn with_spawn_hooks(self, hooks: Vec<Arc<dyn WorkspaceSpawnHook>>) -> Self;

    // Templates
    pub async fn template_list(&self, project_id: ProjectId) -> Result<Vec<TabTemplate>, MtError>;
    pub async fn template_get(&self, template_id: TemplateId) -> Result<TabTemplate, MtError>;
    pub async fn template_create(&self, project_id: ProjectId, input: TabTemplateInput) -> Result<TabTemplate, MtError>;
    pub async fn template_update(
        &self,
        template_id: TemplateId,
        expected_revision: i64,
        input: TabTemplateInput,
    ) -> Result<TabTemplate, MtError>;
    pub async fn template_delete(&self, template_id: TemplateId, expected_revision: i64) -> Result<(), MtError>;
    pub async fn template_set_default(&self, template_id: TemplateId) -> Result<Vec<TabTemplate>, MtError>;

    // Abas
    pub async fn tab_list(&self, project_id: ProjectId) -> Result<Vec<TabView>, MtError>;
    pub async fn tab_open(&self, request: TabOpenRequest) -> Result<TabView, MtError>;
    pub async fn tab_close(&self, tab_id: TabId, request_id: String) -> Result<(), MtError>;
    pub async fn tab_reorder(&self, project_id: ProjectId, ordered_tab_ids: Vec<TabId>) -> Result<Vec<TabId>, MtError>;
    pub async fn tab_focus(&self, tab_id: TabId, pane_id: Option<PaneId>) -> Result<TabView, MtError>;

    // Painéis e Split Panes
    pub async fn pane_split(
        &self,
        pane_id: PaneId,
        direction: SplitDirection,
        expected_layout_revision: i64,
        request_id: String,
    ) -> Result<TabView, MtError>;
    pub async fn pane_close(&self, pane_id: PaneId, request_id: String) -> Result<PaneCloseOutcome, MtError>;
    pub async fn pane_restart(
        &self,
        pane_id: PaneId,
        cwd_fallback_to_project: bool,
        request_id: String,
    ) -> Result<TabView, MtError>;
    pub async fn pane_layout_update(
        &self,
        tab_id: TabId,
        expected_layout_revision: i64,
        patches: Vec<LayoutSizePatch>,
    ) -> Result<TabView, MtError>;

    // Sessões e Histórico
    pub async fn session_new(&self, tab_id: TabId, request_id: String) -> Result<TabView, MtError>;
    pub async fn session_list(
        &self,
        tab_id: TabId,
        cursor: Option<String>,
        limit: usize,
    ) -> Result<CursorPage<SessionSummary>, MtError>;
    pub async fn session_get(&self, tab_session_id: TabSessionId) -> Result<SessionDetail, MtError>;
    pub async fn session_scrollback_get(
        &self,
        tab_session_id: TabSessionId,
        session_id: SessionId,
        cursor: Option<String>,
        limit: usize,
    ) -> Result<CursorPage<ScrollbackChunk>, MtError>;
    pub async fn session_search(
        &self,
        tab_id: TabId,
        query: String,
        limit: usize,
    ) -> Result<Vec<SessionSearchHit>, MtError>;

    // Workspace e Comando Inicial
    pub async fn workspace_snapshot(&self) -> Result<WorkspaceSnapshot, MtError>;
    pub async fn workspace_initial_command_run(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        generation: i64,
        request_id: String,
    ) -> Result<TabView, MtError>;

    // Ciclo de vida global e integração com Fases 06 e 09
    pub async fn shutdown(&self) -> Result<(), MtError>;
    pub async fn close_project_tabs_for_removal(&self, project_id: ProjectId) -> Result<(), MtError>;
}
```

---

### 3.4. Reserva Pré-Spawn (`workspace/spawn_hooks.rs`)

Contrato da Fase 07, independente de Git, aplicado a cada execução criada por `tab_open`, `pane_split`,
`pane_restart`, `session_new` e restauração. Os tipos `SpawnContext`/`ResolvedSpawn` de §3.2 permanecem
inalterados; o hook recebe um contexto próprio, após resolver o ambiente e validar o cwd efetivo.

```rust
use crate::workspace::adapters::ExecutionOwner;
use mt_core::{MtError, ids::PtyId};
use std::{future::Future, path::PathBuf, pin::Pin};

pub type SpawnHookFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, MtError>> + Send + 'a>>;

#[derive(Debug, Clone)]
pub struct SpawnReservationRequest {
    pub owner: ExecutionOwner,
    pub cwd: PathBuf,
}

pub trait WorkspaceSpawnHook: Send + Sync + 'static {
    // A implementação clona seus Arcs antes de construir a future; não captura &self.
    fn reserve(&self, request: SpawnReservationRequest)
        -> SpawnHookFuture<'static, Box<dyn WorkspaceSpawnGuard>>;
}

pub trait WorkspaceSpawnGuard: Send + 'static {
    // Confirma diretamente o registro externo, sem aguardar WorkspaceLifecycleObserver.
    fn register(&mut self, pty_id: PtyId) -> SpawnHookFuture<'_, ()>;
    // Idempotente, síncrono, sem E/S: desfaz somente o registro desta execução, após sua parada.
    fn rollback(&mut self);
}
```

1. Declarar `pub mod spawn_hooks;` em `workspace/mod.rs`. O builder `with_spawn_hooks` recebe a lista
   completa antes de compartilhar o manager em `Arc`/`app.manage` e antes de qualquer restauração ou
   abertura. A lista fica imutável; na Fase 07 isolada ela é vazia, e testes usam hooks fake.
2. Na operação supervisionada, reservar a geração e copiar o cwd final sob serialização da aba, encerrar
   a transação curta e aguardar todos os `reserve` na ordem da lista **antes** de `spawn_supervised`.
   Revalidar existência/canonicalização do cwd após obter as reservas; não trocar o cwd depois disso.
   Se a aquisição falhar, nenhum PTY é criado e os guards já adquiridos são descartados em ordem inversa.
3. A operação possui `Vec<Box<dyn WorkspaceSpawnGuard>>`; nenhum guard empresta campos do manager,
   referências de `DashMap`, transações ou locks síncronos. Futures de `reserve` são `Send + 'static`;
   `register` pode emprestar exclusivamente o guard por `'_`, também com `Send`, sem exigir `Sync` do guard.
   Não executar essas futures em tasks destacadas sem supervisão.
4. Após spawn e estabelecimento do ownership/runtime, aguardar `register(pty_id)` de **todos** os guards.
   O adaptador confirma seu registro em memória antes de retornar sucesso; só então publicar a view/sinal
   `PaneSpawned` e descartar as reservas temporárias. Não existe intervalo entre liberação da reserva e
   registro, e a confirmação não depende da fila de observadores nem chama o próprio `TabActor`.
5. Falha de spawn/registro/publicação aciona compensação: manter guards, encerrar e drenar qualquer PTY
   criado, chamar `rollback` em ordem inversa (incluindo o guard cujo registro falhou parcialmente) e
   descartar os guards. Sem PTY criado, `Drop` libera diretamente. `Drop` é síncrono, não bloqueante,
   idempotente e não agenda limpeza assíncrona; remove somente a reserva temporária, nunca um vínculo
   ativo confirmado. O registro ativo tem seu ciclo de vida normal pelo observador após sucesso.
6. Cancelamento do caller não cancela a operação gerenciada. Antes do spawn, descartar a future libera
   reservas sem efeitos externos; após spawn, a operação/guards permanecem na supervisão até registro ou
   compensação. É proibido `abort` da task que possui um PTY sem transferir antes a operação e seus guards
   ao supervisor de recuperação. Se o encerramento falhar, reter os `WorkspaceSpawnGuard`s em operação
   `Blocked` até retry confirmar a parada; nunca liberar proteção de um processo sobrevivente. A lease
   temporária `ProjectActivityGuard` é liberada ao concluir a operação em erro após registrar esse estado,
   permitindo que remoção/retry alcancem a recuperação sem deadlock; não a guardar junto às reservas.
   Shutdown aguarda essas operações.
7. Reservar/registrar por painel, sem manter locks de repositório entre painéis. Na Fase 08, o guard carrega
   um token RAII de reserva (não o lock exclusivo de Git). Fases posteriores implementam esta trait no
   binário; `workspace/spawn_hooks.rs` não importa `mt_git`, e `EnvironmentResolver` não é alterado.

### 3.5. Regras de Concorrência, Ordem de Locks e Compensação

1. **Hierarquia Estrita de Aquisição:**
   - Barreira de boot/restore → `self.project_service.acquire_activity(project_id).await?` da Fase 06 → serialização da operação pelo `TabActor`. Dentro dessa operação: transação curta concluída → hooks pré-spawn → spawn/registro → transação curta/publicação. A serialização não implica manter um mutex/guard de mapa durante os awaits; hooks não reentram no ator. O `ProjectActivityGuard` é liberado ao final, nunca mantido pela vida da aba.
   - O hook `ProjectRemovalHook::before_remove(project_id)` da Fase 06 já é invocado sob o lock exclusivo do gate de atividade do projeto; portanto, `close_project_tabs_for_removal` **não** readquire a lease do `ProjectActivityGuard`, evitando deadlock.
   - O guard cobre toda operação que pode criar PTY (`tab_open`, `pane_split`, `pane_restart`, `session_new` e abertura na restauração), incluindo compensação; não fica retido durante a vida da aba. Fluxos internos reutilizam o guard da operação externa sem readquisição.
   - Nunca segurar uma transação SQLite aberta enquanto aguarda hooks, `spawn`, `terminate_and_wait` ou `drain_execution`; na Implementação 04, a mesma regra se aplica ao flush do recorder/`BatchWriter`. O caminho Git de remoção não adquire gate de projeto nem chama o `WorkspaceManager`, evitando inversão com o caminho de abertura.
2. **Despacho de Comando Inicial (`initial_dispatch`):**
   - Ao abrir uma aba (ou iniciar nova sessão) cujo `LaunchSnapshot.initial_command` seja `Some(cmd)`:
     - O painel primário (`primary_pane_id`) inicia com `initial_dispatch = 'pending'`.
     - O `TabActor` agenda um timer de 5 segundos associado a `(pane_id, generation)`.
     - Se o evento de prompt confiável OSC 133 A/B da Fase 05 chegar antes de 5 s para o mesmo `(pty_id, generation)`:
       1. Executa `TabSessionsRepo::claim_initial_dispatch(tab_session_id, pane_id, generation)`.
       2. Se retornar `true`, envia `pty.write(pty_id, Bytes::from(format!("{}\r", cmd)))`.
       3. Se `write` tiver sucesso, marca `mark_initial_dispatch_sent`. Se `write` falhar de forma ambígua, mantém `claimed` (sem retry automático) e registra aviso recuperável no painel.
     - Se o timer de 5 s expirar sem prompt confiável:
       - Transiciona `pending -> waiting` via `mark_initial_dispatch_waiting`, atualiza a view da aba e expõe na UI a ação explícita **"Executar comando inicial"** (`workspace_initial_command_run`), que valida `(tab_id, pane_id, generation)` antes de reivindicar `claimed` e escrever no PTY.
3. **Isolamento de Geração (`generation`):**
   - Todo callback de saída de PTY (`on_exit`), evento de prompt ou mudança de status da Fase 05 carrega `(PtyId, generation)`.
   - Se o `PaneRuntimeEntry` atual do `TabActor` já estiver em uma `generation` maior ou outro `PtyId`, o evento tardio é descartado silenciosamente.
4. **Falha de Encerramento (`TabLifecycle::Blocked`):**
   - Se `terminate_and_wait` falhar ou atingir timeout em `tab_close`, `pane_close`, `pane_restart`, `session_new`, `shutdown` ou `close_project_tabs_for_removal`, o `TabActor`:
     - Mantém os `ExecutionOwner`s vivos em `pty_index` e no supervisor.
     - Coloca a aba em `TabLifecycle::Blocked` e o estágio da operação em `OperationStage::Blocked`.
     - **Não** arquiva o agrupamento, **não** cria novos PTYs substitutos e **bloqueia** a exclusão do projeto na Fase 06 retornando `Err(MtError::workspace_stop_failed(...))`.

---

## 4. Casos de Borda e Tratamento de Erros

1. **Cancelamento de `invoke` pela WebView:** Todas as mutações críticas (`tab_open`, `tab_close`, `pane_split`, `pane_close`, `session_new`) são executadas em tasks gerenciadas pelo `WorkspaceManager` / `TabActor`; se o caller IPC cancelar a future da requisição (ex.: reload da página), a operação e seus guards de compensação continuam até um ponto consistente no backend.
2. **Fechamento do Último Painel em `pane_close`:** Quando `PanesRepo` detecta que o painel alvo é a única folha aberta na árvore da aba, `pane_close` delega internamente para o fluxo completo de `tab_close` no mesmo `TabActor` e retorna `PaneCloseOutcome::TabClosed { tab_id }`.
3. **Falha Parcial de Spawn em `session_new`:** Quando uma aba com múltiplos painéis inicia uma nova sessão, cada painel recebe seu novo `SessionId` e tenta `spawn_supervised` independentemente. Se um painel falhar ao iniciar (ex.: `cwd` removido do disco), apenas aquele `PaneView` entra em `PaneRuntimeState::Failed` com `error: Some(MtError)` e sua sessão base é fechada com `SpawnFailed`, enquanto os demais painéis da aba entram em `PaneRuntimeState::Live` normalmente.
