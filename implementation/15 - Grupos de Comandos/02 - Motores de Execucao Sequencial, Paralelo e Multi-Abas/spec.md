# Implementação 02 — Motores de Execução Sequencial, Paralelo e Multi-Abas (`spec.md`)

## 1. Contexto, Objetivo e Escopo

Esta implementação constrói os três motores de execução de Grupos de Comandos e as portas de integração (`ports.rs`) e eventos (`events.rs`) no crate `mt-commands`:
1. **Modo Sequencial na mesma aba (`&&` ou `;`):** `SequentialComposer` (quoting seguro específico para `bash`, `zsh` e `fish` + isolamento com `env -C` quando há overrides de `cwd`/`env`) e `SequentialRunner` (reserva exclusiva do painel, submissão item a item aguardando fim de bloco OSC 133 e novo prompt confirmado no mesmo PTY).
2. **Modo Paralelo em Background:** `ParallelRunner` e `GroupRunSink` (fila FIFO com semáforo global de 16 PTYs headless supervisionados com `PtyOwner { tab_id: None, pane_id: None }`, ring buffer sanitizado de 256 KiB por item, streaming ao vivo sob demanda e cancelamento sem processos órfãos).
3. **Modo Multi-Abas:** `MultiTabPlanner` e `MultiTabRunner` (seleção de template shell compatível com o flag `continuous` no preflight, geração de `TabOpenRequest` + `TabOpenOverrides`, abertura sequencial de abas com tratamento de falhas parciais sem destruir abas anteriores já abertas).

### 1.1. Escopo Incluído
- **Itens da Fase 15 cobertos:** `I-05` (`SequentialComposer` + `SequentialRunner`), `I-06` (`ParallelRunner` + `GroupRunSink`), `I-07` (`MultiTabPlanner` + `MultiTabRunner`), `I-10` (contratos de eventos e frames de saída em `events.rs`) e `I-15` (consistência de transições, reconciliação de eventos precoces de spawn/abertura e cancelamento).
- **Critérios de Aceite cobertos:** `CA-01`, `CA-05`, `CA-06`, `CA-07`, `CA-08`, `CA-17` (backend), `CA-18` (backend), `CA-19` (backend), `CA-21` (supervisão/shutdown dos runners), `CA-24`.

### 1.2. Escopo Excluído
- `GroupSuggester` (IA Fase 12 / Memória Fase 13), `CommandGroupService` unificado, adaptadores concretos do binário Tauri (`src-tauri/src/command_groups/adapters.rs`) e comandos IPC — cobertos na **Implementação 03**.
- Store Pinia, composables, gatilho `>>` e componentes Vue — cobertos nas **Implementações 04 e 05**.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Absoluto / Relativo ao Workspace | Descrição |
|------|------------------------------------------|-----------|
| Modificar | `src-tauri/crates/mt-commands/src/groups/mod.rs` | Exportar `ports`, `events`, `compose`, `sequential`, `runner`, `plan` e `multi_tab`. |
| Criar | `src-tauri/crates/mt-commands/src/groups/ports.rs` | Traits das portas de infraestrutura (`GroupPtyPort`, `GroupShellPort`, `GroupTabPort`, `GroupAiPort`, `RecurringCommandsSource`) e tipos de lease/observação. |
| Criar | `src-tauri/crates/mt-commands/src/groups/events.rs` | Payloads de eventos (`GroupChangedEvent`, `GroupRunEvent`, `GroupRunItemEventDelta`, `GroupOutputFrame`, `GroupOutputFrameKind`) e traits `GroupEventSink` / `OutputFrameSubscriber`. |
| Criar | `src-tauri/crates/mt-commands/src/groups/compose.rs` | `SequentialComposer`: quoting por dialeto (`Bash`, `Zsh`, `Fish`), verificação de capacidade `env -C` e montagem do comando isolado quando houver overrides de `cwd`/`env`. |
| Criar | `src-tauri/crates/mt-commands/src/groups/sequential.rs` | `SequentialRunner`: aquisição de `PaneExecutionLease`, submissão unitária por item, correlação com `(PtyId, SessionId, CommandBlockId, generation)`, políticas `&&` e `;`, e cancelamento via `Ctrl+C` (`\x03`). |
| Criar | `src-tauri/crates/mt-commands/src/groups/runner.rs` | `ParallelRunner`, `RunHandle` e `GroupRunSink`: semáforo global de 16 PTYs, buffer de eventos precoces antes do retorno de `spawn`, ring buffer sanitizado de 256 KiB com `StreamSanitizer`, attach/detach de replay+live atômico e cancelamento sem órfãos. |
| Criar | `src-tauri/crates/mt-commands/src/groups/plan.rs` | `MultiTabPlanner`: resolução de templates shell compatíveis com `continuous`, construção de `PlannedTabOpen` (`TabOpenRequest` + `TabOpenOverrides` da Fase 07). |
| Criar | `src-tauri/crates/mt-commands/src/groups/multi_tab.rs` | `MultiTabRunner`: abertura sequencial de abas, correlação do comando inicial da aba, preservação de abas anteriores em falha parcial e cancelamento seguro (`close_tab` apenas se ainda executando o comando inicial). |
| Criar | `src-tauri/crates/mt-commands/tests/groups_compose.rs` | Testes em shells reais (`bash`, `zsh`, `fish`) e unitários cobrindo quoting, comentários, pipes, `cd`, `exit`, overrides isolados com `env -C` e políticas `&&` vs `;` (`CA-05`). |
| Criar | `src-tauri/crates/mt-commands/tests/groups_runner.rs` | Testes de integração dos runners (`ParallelRunner`, `SequentialRunner`, `MultiTabRunner`) cobrindo limite de 16 PTYs, saída de 2 MiB truncada em 256 KiB e sanitizada, cancelamento sem órfãos em `/proc` em < 3,5 s e falha parcial em Multi-Abas (`CA-06`, `CA-07`, `CA-08`, `CA-24`). |

---

## 3. Assinaturas Exatas de Portas, Eventos e Motores Rust

### 3.1. `src-tauri/crates/mt-commands/src/groups/ports.rs`

```rust
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use async_trait::async_trait;
use mt_core::{
    CommandBlockId, MtResult, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId
};
use crate::groups::model::{
    CommandGroupInput, GroupScope, ShellDialect
};

/// Especificação de spawn de um PTY headless em background (espelha `mt_pty::SpawnSpec` com `PtyOwner` vazio).
#[derive(Debug, Clone)]
pub struct HeadlessSpawnSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
    pub login: bool,
}

/// Informação de encerramento de processo supervisionado (espelha `mt_pty::ExitInfo` após limpeza da árvore).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisedExitInfo {
    pub pty_id: PtyId,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub tree_cleaned: bool,
}

/// Sink de saída bruta anexado ao PTY antes/durante o spawn.
pub trait RawOutputTap: Send + Sync {
    fn on_bytes(&self, chunk: &[u8]);
    fn on_eof(&self);
}

/// Porta para `mt_pty::PtyManager` (Fase 02).
#[async_trait]
pub trait GroupPtyPort: Send + Sync {
    /// Cria um PTY headless (`PtyOwner { tab_id: None, pane_id: None }`) com `output_tap` instalado
    /// desde o primeiro byte e retorna o `PtyId`.
    async fn spawn_headless(
        &self,
        spec: HeadlessSpawnSpec,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<PtyId>;

    /// Aguarda o encerramento completo do processo líder E a limpeza supervisionada de todos os
    /// processos descendentes em `/proc` pela Fase 02.
    async fn wait_exit(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo>;

    /// Escreve bytes no PTY de um painel existente (usado pelo modo Sequencial).
    async fn write_to_pty(&self, pty_id: &PtyId, data: &[u8]) -> MtResult<()>;

    /// Encerra um PTY headless (`KillMode::Graceful` com escalonamento para `SIGKILL` na árvore).
    async fn kill_graceful(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo>;

    /// Anexa um tap temporário de leitura aos bytes produzidos por um PTY existente (Sequencial/Multi-Abas)
    /// durante a execução do item correlacionado.
    async fn attach_item_tap(
        &self,
        pty_id: &PtyId,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<Box<dyn Send + Sync>>;
}

/// Estado instantâneo verificado no backend para um painel de terminal (Fase 05).
#[derive(Debug, Clone)]
pub struct PaneShellState {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub pty_id: PtyId,
    pub session_id: SessionId,
    pub shell_path: PathBuf,
    pub generation: u64,
    pub prompt_ready: bool,
    pub in_alt_screen: bool,
    pub foreground_busy: bool,
    pub tracking_available: bool,
}

/// Evento de ciclo de vida de bloco OSC 133 e prompt emitido pela Fase 05.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellLifecycleEvent {
    BlockStarted {
        pty_id: PtyId,
        session_id: SessionId,
        block_id: CommandBlockId,
        generation: u64,
    },
    BlockFinished {
        pty_id: PtyId,
        session_id: SessionId,
        block_id: CommandBlockId,
        generation: u64,
        exit_code: i32,
        signal: Option<i32>,
    },
    PromptReady {
        pty_id: PtyId,
        session_id: SessionId,
        generation: u64,
    },
    GenerationInvalidated {
        pane_id: PaneId,
        pty_id: PtyId,
        reason: String,
    },
}

/// Lease exclusivo de submissão de comandos sobre um painel durante uma fila Sequencial.
#[async_trait]
pub trait PaneExecutionLease: Send + Sync {
    fn state(&self) -> &PaneShellState;
    async fn next_event(&mut self) -> Option<ShellLifecycleEvent>;
    async fn release(self: Box<Self>);
}

/// Porta para integração com Shell Integration (Fase 05) e coordenação de input (Fase 04).
#[async_trait]
pub trait GroupShellPort: Send + Sync {
    /// Inspeciona o estado atual de um painel sem adquirir lease.
    async fn inspect_pane(&self, pane_id: &PaneId) -> MtResult<PaneShellState>;

    /// Adquire lease exclusivo `(pane_id, pty_id, session_id, generation)` se e somente se:
    /// - `tracking_available == true` (senão retorna `commands.group.tracking_unavailable`)
    /// - `prompt_ready == true && !in_alt_screen && !foreground_busy` e nenhum outro lease ativo
    ///   (senão retorna `commands.group.pane_busy`).
    async fn acquire_pane_lease(&self, pane_id: &PaneId) -> MtResult<Box<dyn PaneExecutionLease>>;

    /// Assina eventos de ciclo de vida OSC 133 de uma sessão/aba recém-aberta pelo modo Multi-Abas.
    async fn subscribe_session_events(
        &self,
        session_id: &SessionId,
    ) -> MtResult<tokio::sync::mpsc::UnboundedReceiver<ShellLifecycleEvent>>;

    /// Verifica se o binário `env` do sistema suporta a flag `-C` (GNU coreutils >= 8.28).
    fn supports_env_chdir(&self) -> bool;
}

/// Descritor de template retornado pela Fase 07 para validação de preflight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectShellTemplate {
    pub id: TemplateId,
    pub project_id: ProjectId,
    pub name: String,
    pub is_default: bool,
    pub is_shell_environment: bool,
    pub continuous: bool,
    pub shell_path: PathBuf,
    pub base_env: BTreeMap<String, String>,
}

/// Requisição real de abertura de aba enviada para `WorkspaceManager::open_tab` (Fase 07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTabOpenSpec {
    pub project_id: ProjectId,
    pub template_id: TemplateId,
    pub focus: bool,
    pub title: String,
    pub cwd: PathBuf,
    pub initial_command: String,
    pub extra_env: BTreeMap<String, String>,
    pub cwd_fallback_to_project: bool,
}

/// Identificadores retornados por `WorkspaceManager::open_tab` (Fase 07).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedGroupTab {
    pub tab_id: TabId,
    pub pane_id: PaneId,
    pub pty_id: PtyId,
    pub session_id: SessionId,
    pub initial_block_id: Option<CommandBlockId>,
}

/// Porta para o `WorkspaceManager` (Fase 07) e informações de projeto (Fase 06).
#[async_trait]
pub trait GroupTabPort: Send + Sync {
    async fn resolve_project_root(&self, project_id: &ProjectId) -> MtResult<PathBuf>;
    async fn list_project_templates(&self, project_id: &ProjectId) -> MtResult<Vec<ProjectShellTemplate>>;
    async fn open_tab(&self, spec: GroupTabOpenSpec) -> MtResult<OpenedGroupTab>;

    /// Fecha uma aba aberta pelo modo Multi-Abas somente se ela ainda estiver executando o comando
    /// inicial do item (`session_id` e `pty_id` inalterados e comando inicial não finalizado).
    async fn close_tab_if_running_initial(
        &self,
        tab_id: &TabId,
        expected_session_id: &SessionId,
        expected_pty_id: &PtyId,
    ) -> MtResult<bool>;
}

/// Item de comando recorrente retornado pela memória contextual opcional (Fase 13).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecurringCommandEntry {
    pub command: String,
    pub cwd: Option<String>,
    pub frequency: u32,
    pub last_used_at: i64,
}

/// Porta opcional para comandos recorrentes da Fase 13.
#[async_trait]
pub trait RecurringCommandsSource: Send + Sync {
    fn is_available(&self) -> bool;
    async fn list_recurring(
        &self,
        project_id: &ProjectId,
        limit: u32,
    ) -> MtResult<Vec<RecurringCommandEntry>>;
}

/// Contexto sanitizado enviado ao `AiService::generate_structured` (Fase 12).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestPromptContext {
    pub project_id: Option<ProjectId>,
    pub pane_id: Option<PaneId>,
    pub scope: GroupScope,
    pub project_name: Option<String>,
    pub project_stacks: Vec<String>,
    pub shell_dialect: ShellDialect,
    pub goal: String,
    pub existing_draft: Option<CommandGroupInput>,
    pub recurring_commands: Vec<String>,
}

/// Porta para `AiService::generate_structured` (Fase 12), schema `command-group.suggestion.v1`.
#[async_trait]
pub trait GroupAiPort: Send + Sync {
    async fn generate_group_suggestion(
        &self,
        ai_request_id: &str,
        context: SuggestPromptContext,
    ) -> MtResult<CommandGroupInput>;
}
```

### 3.2. `src-tauri/crates/mt-commands/src/groups/events.rs`

```rust
use serde::{Deserialize, Serialize};
use specta::Type;
use mt_core::{
    CommandBlockId, CommandGroupId, PaneId, ProjectId, PtyId, SessionId, TabId
};
use crate::groups::model::{
    GroupRunCounts, GroupRunId, GroupRunItemId, GroupScope, ItemStatus,
    OutputSubscriptionId, RunStatus
};

pub const EVENT_GROUP_CHANGED: &str = "command-group://changed";
pub const EVENT_RUN_STARTED: &str = "command-group://run-started";
pub const EVENT_RUN_ITEM_CHANGED: &str = "command-group://run-item-changed";
pub const EVENT_RUN_FINISHED: &str = "command-group://run-finished";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GroupChangeOperation {
    Created,
    Updated,
    Deleted,
    Reordered,
}

/// Payload emitido em `command-group://changed` (sem comandos, env ou output).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupChangedEvent {
    pub group_id: Option<CommandGroupId>,
    pub scope: GroupScope,
    pub project_id: Option<ProjectId>,
    pub revision: u64,
    pub operation: GroupChangeOperation,
}

/// Delta compacto de um item incluído em `command-group://run-item-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunItemEventDelta {
    pub id: GroupRunItemId,
    pub position: u32,
    pub status: ItemStatus,
    pub version: u64,
    pub pty_id: Option<PtyId>,
    pub tab_id: Option<TabId>,
    pub pane_id: Option<PaneId>,
    pub session_id: Option<SessionId>,
    pub command_block_id: Option<CommandBlockId>,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub started_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub output_truncated: bool,
}

/// Payload emitido em `command-group://run-started`, `run-item-changed` e `run-finished`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunEvent {
    pub run_id: GroupRunId,
    pub project_id: ProjectId,
    pub seq: u64,
    pub status: RunStatus,
    pub counts: GroupRunCounts,
    pub last_exit_code: Option<i32>,
    pub started_at: Option<i64>,
    pub cancel_requested_at: Option<i64>,
    pub finished_at: Option<i64>,
    pub item: Option<GroupRunItemEventDelta>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GroupOutputFrameKind {
    Replay,
    Data,
    Gap,
    End,
}

/// Frame enviado exclusivamente via `tauri::ipc::Channel<GroupOutputFrame>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupOutputFrame {
    pub subscription_id: OutputSubscriptionId,
    pub item_id: GroupRunItemId,
    pub seq: u64,
    pub kind: GroupOutputFrameKind,
    pub text: Option<String>,
    pub dropped_bytes: Option<u64>,
}

/// Resposta do comando `command_group_run_item_attach`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OutputAttachReceipt {
    pub subscription_id: OutputSubscriptionId,
    pub replay_start_seq: u64,
    pub replay_end_seq: u64,
    pub truncated: bool,
}

/// Emissor abstrato de eventos globais para desacoplar `mt-commands` do `tauri::AppHandle`.
pub trait GroupEventSink: Send + Sync {
    fn emit_group_changed(&self, event: GroupChangedEvent);
    fn emit_run_started(&self, event: GroupRunEvent);
    fn emit_run_item_changed(&self, event: GroupRunEvent);
    fn emit_run_finished(&self, event: GroupRunEvent);
}

/// Assinante de frames de saída (`Channel<GroupOutputFrame>`).
pub trait OutputFrameSubscriber: Send + Sync {
    fn send_frame(&self, frame: GroupOutputFrame) -> bool;
}
```

### 3.3. `src-tauri/crates/mt-commands/src/groups/compose.rs`

```rust
use std::collections::BTreeMap;
use std::path::Path;
use mt_core::MtResult;
use crate::groups::model::ShellDialect;

/// Compositor de submissão unitária de item para o modo Sequencial na mesma aba.
pub struct SequentialComposer;

impl SequentialComposer {
    /// Aplica quoting de um argumento literal para o dialeto de shell alvo:
    /// - `Bash` / `Zsh` (POSIX): envolve em `'...'` e substitui cada `'` interno por `'\''`.
    /// - `Fish`: envolve em `'...'` e substitui cada `\` por `\\` e cada `'` por `\'`.
    pub fn quote_arg(dialect: ShellDialect, raw: &str) -> String;

    /// Prepara os bytes de uma única submissão de item para escrita no PTY do painel:
    /// - Sem overrides (`resolved_cwd == None` e `merged_env.is_empty()`): envia exatamente
    ///   `format!("{command}\n")` sem modificar a sintaxe original do usuário (preservando `cd`,
    ///   `export`/`set -gx`, aliases e funções no shell ativo).
    /// - Com overrides (`resolved_cwd.is_some()` ou `!merged_env.is_empty()`):
    ///   - Exige `env_chdir_supported == true` quando `resolved_cwd.is_some()` (senão retorna
    ///     `Err(GroupErrorCode::EnvCwdUnavailable)`).
    ///   - Compõe `env [-C <cwd_quoted>] [<K=V_quoted>...] <shell_path_quoted> -c <command_quoted>\n`
    ///     onde cada argumento é protegido por `quote_arg(dialect, ...)`, isolando `cwd` e `env`
    ///     em um processo shell filho sem vazar para o shell pai do painel.
    pub fn compose_submission(
        dialect: ShellDialect,
        shell_path: &Path,
        command: &str,
        resolved_cwd_override: Option<&Path>,
        env_overrides: &BTreeMap<String, String>,
        env_chdir_supported: bool,
    ) -> MtResult<Vec<u8>>;
}
```

### 3.4. `src-tauri/crates/mt-commands/src/groups/runner.rs`

```rust
use std::sync::Arc;
use tokio::sync::Semaphore;
use mt_core::MtResult;
use crate::groups::events::{
    GroupEventSink, OutputAttachReceipt, OutputFrameSubscriber
};
use crate::groups::model::{
    GroupRun, GroupRunId, GroupRunItemId, OutputSubscriptionId
};
use crate::groups::ports::GroupPtyPort;
use crate::groups::repo::CommandGroupRepo;

pub const GLOBAL_PARALLEL_PTY_LIMIT: usize = 16;
pub const MAX_ITEM_OUTPUT_TAIL_BYTES: usize = 256 * 1024; // 256 KiB
pub const MAX_UNTERMINATED_LINE_BYTES: usize = 16 * 1024; // 16 KiB

/// Sink de saída por item com sanitização incremental (`mt_db::sanitizer::StreamSanitizer`),
/// ring buffer UTF-8 limitado a 256 KiB e suporte a `attach`/`detach` atômico sem bloquear o leitor do PTY.
pub struct GroupRunSink { /* estado interno protegido por Mutex leve */ }

impl GroupRunSink {
    pub fn new(run_id: GroupRunId, item_id: GroupRunItemId) -> Arc<Self>;

    /// Retorna `(cauda_sanitizada, foi_truncada)` para persistência em `command_group_run_items`.
    pub fn snapshot_tail(&self) -> (String, bool);

    /// Captura atomicamente o watermark do ring buffer, envia frames `Replay` (e `Gap` se truncado
    /// ou se `after_seq` já saiu da janela de 256 KiB) e registra o assinante para frames `Data`/`End`
    /// subsequentes sem duplicar nem perder bytes na fronteira replay/live.
    pub fn attach(
        &self,
        subscription_id: OutputSubscriptionId,
        after_seq: Option<u64>,
        subscriber: Arc<dyn OutputFrameSubscriber>,
    ) -> OutputAttachReceipt;

    /// Remove a assinatura sem cancelar o item em execução.
    pub fn detach(&self, subscription_id: &OutputSubscriptionId);
}

/// Executor do modo Paralelo em Background com semáforo global de 16 PTYs.
pub struct ParallelRunner {
    semaphore: Arc<Semaphore>,
    pty_port: Arc<dyn GroupPtyPort>,
    repo: Arc<dyn CommandGroupRepo>,
    events: Arc<dyn GroupEventSink>,
    /* índice local (GroupRunId, GroupRunItemId) -> estado/PtyId/GroupRunSink */
}

impl ParallelRunner {
    pub fn new(
        pty_port: Arc<dyn GroupPtyPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self>;

    /// Inicia o processamento assíncrono dos itens da execução em background.
    pub async fn launch(self: &Arc<Self>, run: GroupRun) -> MtResult<()>;

    /// Cancela toda a execução ou um item específico de forma idempotente:
    /// itens `pending` na fila mudam imediatamente para `cancelled`;
    /// itens `starting`/`running` mudam para `cancelling`, invocam `kill_graceful` e só mudam para
    /// `cancelled` após confirmação de limpeza da árvore em `/proc`.
    pub async fn cancel(
        self: &Arc<Self>,
        run_id: &GroupRunId,
        item_id: Option<&GroupRunItemId>,
    ) -> MtResult<GroupRun>;

    /// Anexa um assinante ao `GroupRunSink` ativo ou faz replay a partir do `output_tail` persistido no SQLite
    /// caso o item já tenha terminado.
    pub fn attach_output(
        self: &Arc<Self>,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
        after_seq: Option<u64>,
        subscriber: Arc<dyn OutputFrameSubscriber>,
    ) -> MtResult<OutputAttachReceipt>;

    pub fn detach_output(&self, subscription_id: &OutputSubscriptionId);

    /// Cancela todas as execuções ativas e aguarda a limpeza supervisionada de todos os PTYs no shutdown explícito do app.
    pub async fn shutdown_all(self: &Arc<Self>);
}
```

### 3.5. `src-tauri/crates/mt-commands/src/groups/sequential.rs`, `plan.rs` e `multi_tab.rs`

```rust
// Em sequential.rs:
pub struct SequentialRunner {
    pty_port: Arc<dyn GroupPtyPort>,
    shell_port: Arc<dyn GroupShellPort>,
    repo: Arc<dyn CommandGroupRepo>,
    events: Arc<dyn GroupEventSink>,
}

impl SequentialRunner {
    pub fn new(
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self>;

    /// Executa a fila sequencial sob um `PaneExecutionLease` já adquirido no preflight.
    pub async fn launch_with_lease(
        self: &Arc<Self>,
        run: GroupRun,
        lease: Box<dyn PaneExecutionLease>,
    ) -> MtResult<()>;

    /// Cancela a fila sequencial ou um item pendente específico. Se cancelar o item ativo,
    /// envia `\x03` (Ctrl+C) apenas para a `generation` do lease e aguarda `BlockFinished` + `PromptReady`.
    pub async fn cancel(
        self: &Arc<Self>,
        run_id: &GroupRunId,
        item_id: Option<&GroupRunItemId>,
    ) -> MtResult<GroupRun>;

    pub async fn shutdown_all(self: &Arc<Self>);
}

// Em plan.rs:
pub struct MultiTabPlanner;

impl MultiTabPlanner {
    /// No preflight, seleciona para cada item um template de ambiente Shell cujo `continuous`
    /// coincida exatamente com `item.continuous`:
    /// 1. Se `template_by_item` especificar um `TemplateId` para o item, valida que o template
    ///    pertence ao projeto, é `is_shell_environment == true` e possui `template.continuous == item.continuous`.
    /// 2. Caso contrário, usa o template padrão do projeto se for Shell e tiver `continuous == item.continuous`,
    ///    ou o primeiro template Shell do projeto compatível se houver exatamente um inequívoco.
    /// 3. Se nenhum template compatível existir, retorna `Err(GroupErrorCode::TemplateIncompatible)`.
    pub fn resolve_templates_for_items(
        items: &[crate::groups::model::CommandGroupItem],
        available_templates: &[crate::groups::ports::ProjectShellTemplate],
        explicit_map: & std::collections::BTreeMap<crate::groups::model::GroupItemId, mt_core::TemplateId>,
    ) -> MtResult< std::collections::BTreeMap<crate::groups::model::GroupItemId, crate::groups::ports::ProjectShellTemplate>>;
}

// Em multi_tab.rs:
pub struct MultiTabRunner {
    tab_port: Arc<dyn GroupTabPort>,
    pty_port: Arc<dyn GroupPtyPort>,
    shell_port: Arc<dyn GroupShellPort>,
    repo: Arc<dyn CommandGroupRepo>,
    events: Arc<dyn GroupEventSink>,
}

impl MultiTabRunner {
    pub fn new(
        tab_port: Arc<dyn GroupTabPort>,
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self>;

    pub async fn launch(self: &Arc<Self>, run: GroupRun) -> MtResult<()>;

    pub async fn cancel(
        self: &Arc<Self>,
        run_id: &GroupRunId,
        item_id: Option<&GroupRunItemId>,
    ) -> MtResult<GroupRun>;

    pub async fn shutdown_all(self: &Arc<Self>);
}
```

---

## 4. Regras de Negócio, Supervisão e Casos de Borda Desta Implementação

1. **Proibição de Concatenação Textual (`commands.join(" && ")`):**
   - Cada item no modo Sequencial é submetido individualmente ao PTY somente após o término correlacionado (`BlockFinished`) **e** o próximo prompt pronto (`PromptReady`) do item anterior.
   - Com `SequentialSeparator::And`, se um item terminar com `exit_code != 0`, todos os itens subsequentes ainda em `Pending` são marcados imediatamente como `Skipped` na mesma transação e a fila termina.
   - Com `SequentialSeparator::Semicolon` e OSC 133 confiável, se um item terminar com `exit_code != 0`, o item fica `Failed`, a fila prossegue e a mistura de `Succeeded`/`Failed` resulta em `Partial` (`last_exit_code` é apenas o último item).
   - Sem OSC 133, `cmd1 ; cmd2` fornece somente o exit da linha/último comando: resultados intermediários são `unknown`, nunca `Succeeded`. O lease recusa execução sequencial com `commands.group.tracking_unavailable`; oferecer Multi-Abas em vez de concatenar silenciosamente. Cadeia manual dentro de um item é observada como linha única, sem inventar status por subcomando (T-18).
2. **Isolamento de Overrides no Modo Sequencial (`env -C`):**
   - Quando um item sequencial ou o grupo define `cwd` ou `env`, o comando é executado em um processo shell filho via `env -C <cwd> <K=V...> <shell> -c <command>`. Isso garante que alterações de diretório ou variáveis do item com override **nunca vazem** para o item seguinte, inclusive no `fish` (sem usar `begin; cd ...; cd -; end`).
   - Quando o item não possui override de `cwd` nem de `env`, o comando é enviado diretamente ao shell ativo do painel, permitindo que `cd pasta` no Item 1 afete naturalmente o Item 2.
3. **Semáforo Global de 16 PTYs e Corrida de Spawn Rápido no Modo Paralelo:**
   - O semáforo de 16 vagas pertence à instância de `ParallelRunner` compartilhada por todo o aplicativo. Se dois grupos de 10 itens forem disparados ao mesmo tempo, no máximo 16 PTYs existirão simultaneamente; os 4 restantes aguardam em `Pending`.
   - O `GroupRunSink` é passado para `spawn_headless` **antes** do processo nascer. Se o processo terminar em microssegundos (antes do retorno de `spawn_headless` gravar `pty_id` no SQLite), o `ParallelRunner` reconcilia o `pty_id` retornado com o resultado de `wait_exit`, garantindo que nenhum evento de saída ou término seja perdido.
4. **Sanitização Incremental e Sem Backpressure no PTY (`GroupRunSink`):**
   - Os bytes recebidos em `on_bytes` passam por `mt_db::sanitizer::StreamSanitizer` (que retém uma janela deslizante nas fronteiras de chunks para capturar tokens como `ghp_...` ou `Bearer ...` partidos ao meio entre duas leituras do PTY).
   - Linhas sem `\n` maiores que 16 KiB sofrem flush sanitizado conservador para evitar consumo de memória.
   - Se um `OutputFrameSubscriber` (`Channel`) estiver lento (fila cheia), o `GroupRunSink` descarta frames intermediários daquele assinante e emite um frame `GroupOutputFrameKind::Gap { dropped_bytes }` na próxima janela livre, **jamais bloqueando** a thread de leitura do PTY.
5. **Multi-Abas: Falha Parcial e Comando Inicial Único:**
   - A Fase 07 (`WorkspaceManager::open_tab`) é a única responsável por executar `initial_command`. O `MultiTabRunner` nunca escreve o comando uma segunda vez no PTY.
   - Se o Item 1 abrir a aba com sucesso e o Item 2 falhar ao abrir (`commands.group.tab_open_failed`), o Item 1 e sua aba permanecem abertos e rastreados; o Item 2 é marcado como `Failed` e o `MultiTabRunner` prossegue para o Item 3.
   - Se o usuário cancelar uma execução Multi-Abas enquanto uma chamada `open_tab` está em voo, assim que `open_tab` retornar, o `MultiTabRunner` vincula os IDs retornados e invoca `close_tab_if_running_initial` imediatamente antes de marcar o item como `Cancelled`.
