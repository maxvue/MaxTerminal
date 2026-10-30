# Implementação 03 — Orquestrador, Sugestão Assistida IA, Adaptadores Tauri e Comandos IPC (`spec.md`)

## 1. Contexto, Objetivo e Escopo

Esta implementação integra o domínio e os motores das Implementações 01 e 02 ao runtime do aplicativo Tauri v2 (`maxterminal`), entregando:
1. **`GroupSuggester` (`suggest.rs`):** Sugestão assistida de Grupos de Comandos via `AiService.structured.generate` (Fase 12) com schema fechado `command-group.suggestion`, sanitização da entrada/saída e revalidação completa pelo `validate_group_input`, além da consulta opcional à memória contextual de comandos recorrentes (`RecurringCommandsSource`, Fase 13).
2. **`CommandGroupService` (`service.rs`):** Fachada central de orquestração que serializa a aceitação por `request_id` idempotente, executa preflight completo, recupera execuções interrompidas no boot (`recover_interrupted()`), encerra execuções ativas antes da remoção de um projeto e coordena o shutdown explícito do aplicativo.
3. **Adaptadores Nativos e Camada IPC Tauri (`adapters.rs`, `commands/command_groups.rs`, `events/command_groups.rs`, `capabilities/default.json`):** Implementação concreta de `GroupPtyPort`, `GroupShellPort`, `GroupTabPort`, `GroupAiPort`, `TauriGroupEventSink`, `ChannelRunObserver` e os **14 comandos Tauri** registrados no `tauri-specta`.

### 1.1. Escopo Incluído
- **Itens da Fase 15 cobertos:** `I-09` (14 comandos IPC e registro `tauri-specta`), `I-10` (`TauriGroupEventSink` e `ChannelRunObserver`), `I-14` (`GroupSuggester` com Fase 12 e memória opcional da Fase 13) e `I-15` (`CommandGroupService`, idempotência por `request_id`, `recover_interrupted()` no boot, hook de remoção de projeto e shutdown supervisionado).
- **Critérios de Aceite cobertos:** `CA-01`, `CA-09`, `CA-10` (geração de `src/types/bindings.ts` sem drift), `CA-20` (backend), `CA-21`, `CA-22`, `CA-23`.

### 1.2. Escopo Excluído
- Store Pinia, composables e gatilho `>>` — cobertos na **Implementação 04**.
- Componentes Vue (`CommandGroupModal.vue`, `GroupRunPanel.vue`, etc.) — cobertos na **Implementação 05**.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

| Ação | Caminho Absoluto / Relativo ao Workspace | Descrição |
|------|------------------------------------------|-----------|
| Modificar | `src-tauri/crates/mt-commands/src/groups/mod.rs` | Exportar `suggest` e `service`. |
| Criar | `src-tauri/crates/mt-commands/src/groups/suggest.rs` | `GroupSuggester`, tipos `GroupSuggestRequest`, `GroupSuggestResponse`, `RecurringCommandsResponse`, sanitização do contexto de prompt e validação estrita da sugestão retornada pela IA. |
| Criar | `src-tauri/crates/mt-commands/src/groups/service.rs` | `CommandGroupService`: fachada assíncrona unificando CRUD, `run`, `cancel_run`, `attach_item_output`, `detach_item_output`, `suggest`, `recurring_commands`, `recover_interrupted`, `on_project_deleting` e `shutdown`. |
| Criar | `src-tauri/src/command_groups/mod.rs` | Módulo de bootstrap e estado gerenciado `AppCommandGroupState` no binário Tauri. |
| Criar | `src-tauri/src/command_groups/adapters.rs` | Implementações concretas `TauriGroupPtyAdapter`, `TauriGroupShellAdapter`, `TauriGroupTabAdapter`, `TauriGroupAiAdapter` e `TauriRecurringCommandsAdapter` conectando `mt-commands` aos serviços das Fases 02, 05, 06, 07, 12 e 13. |
| Criar | `src-tauri/src/events/command_groups.rs` | `TauriGroupEventSink` (emissão via `AppHandle::emit`) e `ChannelRunObserver` (implementação de `OutputFrameSubscriber` sobre `tauri::ipc::Channel<GroupOutputFrame>`). |
| Criar | `src-tauri/src/commands/command_groups.rs` | Os 14 comandos `#[tauri::command]` + `#[specta::specta]`. |
| Modificar | `src-tauri/src/commands/mod.rs` | Exportar `pub mod command_groups;`. |
| Modificar | `src-tauri/src/events/mod.rs` | Exportar `pub mod command_groups;`. |
| Modificar | `src-tauri/src/lib.rs` | Registrar `AppCommandGroupState`, invocar `recover_interrupted()` após a recuperação de processos da Fase 02 no startup, conectar o hook de shutdown explícito da janela/app e registrar os 14 comandos e 4 eventos no builder `tauri-specta`. |
| Modificar | `src-tauri/capabilities/default.json` | Adicionar permissões explícitas para os 14 comandos `command_group_*` restritas à janela `main`. |
| Criar | `src-tauri/crates/mt-commands/tests/groups_service.rs` | Testes de integração do `CommandGroupService` e `GroupSuggester` cobrindo `recover_interrupted()`, idempotência concorrente por `request_id`, preflight e sugestão IA validada (`CA-09`, `CA-20`, `CA-23`). |

---

## 3. Assinaturas Exatas de Tipos, Serviços e Comandos IPC

### 3.1. `src-tauri/crates/mt-commands/src/groups/suggest.rs`

```rust
use std::path::Path;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use specta::Type;
use mt_core::{MtResult, PaneId, ProjectId};
use crate::groups::model::{CommandGroupInput, GroupScope};
use crate::groups::ports::{GroupAiPort, GroupShellPort, GroupTabPort, RecurringCommandEntry, RecurringCommandsSource};

pub const AI_SUGGEST_SCHEMA_ID: &str = "command-group.suggestion";

/// Requisição IPC para `command_group_suggest`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GroupSuggestRequest {
    pub request_id: String, // AiRequestId da Fase 12
    pub project_id: Option<ProjectId>,
    pub pane_id: Option<PaneId>,
    pub scope: GroupScope,
    pub goal: String,
    pub draft: Option<CommandGroupInput>,
    pub draft_revision: u64,
}

/// Resposta IPC de `command_group_suggest`: rascunho validado e nunca persistido/executado automaticamente.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupSuggestResponse {
    pub request_id: String,
    pub draft_revision: u64,
    pub suggestion: CommandGroupInput,
}

/// Resposta IPC de `command_group_recurring_commands`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RecurringCommandsResponse {
    pub available: bool,
    pub items: Vec<RecurringCommandEntry>,
}

pub struct GroupSuggester {
    ai_port: Arc<dyn GroupAiPort>,
    tab_port: Arc<dyn GroupTabPort>,
    shell_port: Arc<dyn GroupShellPort>,
    recurring_source: Option<Arc<dyn RecurringCommandsSource>>,
}

impl GroupSuggester {
    pub fn new(
        ai_port: Arc<dyn GroupAiPort>,
        tab_port: Arc<dyn GroupTabPort>,
        shell_port: Arc<dyn GroupShellPort>,
        recurring_source: Option<Arc<dyn RecurringCommandsSource>>,
    ) -> Self;

    /// Gera um rascunho validado via Fase 12:
    /// 1. Valida `goal` (1..=2000 caracteres após trim) e sanitiza `goal` e `draft` com `mt_db::sanitizer::sanitize`.
    /// 2. Força `scope` e `project_id` a partir da requisição do editor (ignorando qualquer tentativa do modelo de alterar escopo/projeto).
    /// 3. Submete ao `GroupAiPort::generate_group_suggestion`.
    /// 4. Passa o `CommandGroupInput` retornado por `validate_group_input(&suggestion, project_root.as_deref())`,
    ///    garantindo que limites de 1–50 itens, `cwd` relativo confinado, chaves POSIX de `env` e ausência de segredos sejam respeitados.
    pub async fn suggest(&self, req: GroupSuggestRequest) -> MtResult<GroupSuggestResponse>;

    /// Consulta os comandos recorrentes da Fase 13 se a fonte opcional estiver registrada e disponível;
    /// caso contrário retorna `{ available: false, items: vec![] }` sem erro.
    pub async fn list_recurring(
        &self,
        project_id: &ProjectId,
        limit: u32,
    ) -> MtResult<RecurringCommandsResponse>;
}
```

### 3.2. `src-tauri/crates/mt-commands/src/groups/service.rs`

```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use mt_core::{CommandGroupId, MtResult, ProjectId};
use crate::groups::events::{
    GroupEventSink, OutputAttachReceipt, OutputFrameSubscriber
};
use crate::groups::model::{
    CommandGroup, CommandGroupCatalog, CommandGroupInput, DeleteGroupReceipt, GroupRun,
    GroupRunId, GroupRunItemId, GroupRunListPage, GroupRunRequest, GroupScope,
    OutputSubscriptionId, ReorderGroupEntry
};
use crate::groups::ports::{
    GroupAiPort, GroupPtyPort, GroupShellPort, GroupTabPort, RecurringCommandsSource
};
use crate::groups::repo::CommandGroupRepo;
use crate::groups::suggest::{
    GroupSuggestRequest, GroupSuggestResponse, GroupSuggester, RecurringCommandsResponse
};

pub struct CommandGroupService {
    repo: Arc<dyn CommandGroupRepo>,
    pty_port: Arc<dyn GroupPtyPort>,
    shell_port: Arc<dyn GroupShellPort>,
    tab_port: Arc<dyn GroupTabPort>,
    events: Arc<dyn GroupEventSink>,
    suggester: GroupSuggester,
    sequential_runner: Arc<crate::groups::sequential::SequentialRunner>,
    parallel_runner: Arc<crate::groups::runner::ParallelRunner>,
    multi_tab_runner: Arc<crate::groups::multi_tab::MultiTabRunner>,
    accept_mutex: Mutex<()>,
}

impl CommandGroupService {
    pub fn new(
        repo: Arc<dyn CommandGroupRepo>,
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        tab_port: Arc<dyn GroupTabPort>,
        ai_port: Arc<dyn GroupAiPort>,
        recurring_source: Option<Arc<dyn RecurringCommandsSource>>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self>;

    // Operações de CRUD (persistem no SQLite antes de emitir `command-group://changed`):
    pub async fn list(&self, project_id: Option<ProjectId>) -> MtResult<CommandGroupCatalog>;
    pub async fn get(&self, group_id: CommandGroupId) -> MtResult<CommandGroup>;
    pub async fn create(&self, input: CommandGroupInput) -> MtResult<CommandGroup>;
    pub async fn update(
        &self,
        group_id: CommandGroupId,
        expected_revision: u64,
        input: CommandGroupInput,
    ) -> MtResult<CommandGroup>;
    pub async fn delete(
        &self,
        group_id: CommandGroupId,
        expected_revision: u64,
    ) -> MtResult<DeleteGroupReceipt>;
    pub async fn reorder(
        &self,
        scope: GroupScope,
        project_id: Option<ProjectId>,
        entries: Vec<ReorderGroupEntry>,
    ) -> MtResult<CommandGroupCatalog>;

    // Execução, Cancelamento, Histórico e Saída:
    pub async fn run(self: &Arc<Self>, request: GroupRunRequest) -> MtResult<GroupRun>;
    pub async fn cancel_run(
        self: &Arc<Self>,
        run_id: GroupRunId,
        item_id: Option<GroupRunItemId>,
    ) -> MtResult<GroupRun>;
    pub async fn list_runs(
        &self,
        project_id: ProjectId,
        cursor: Option<String>,
        limit: u32,
        active_only: Option<bool>,
    ) -> MtResult<GroupRunListPage>;
    pub async fn get_run(&self, run_id: GroupRunId) -> MtResult<GroupRun>;
    pub async fn attach_item_output(
        &self,
        run_id: GroupRunId,
        item_id: GroupRunItemId,
        after_seq: Option<u64>,
        subscriber: Arc<dyn OutputFrameSubscriber>,
    ) -> MtResult<OutputAttachReceipt>;
    pub async fn detach_item_output(&self, subscription_id: OutputSubscriptionId) -> MtResult<()>;

    // Sugestão IA e Memória Contextual:
    pub async fn suggest(&self, request: GroupSuggestRequest) -> MtResult<GroupSuggestResponse>;
    pub async fn recurring_commands(
        &self,
        project_id: ProjectId,
        limit: u32,
    ) -> MtResult<RecurringCommandsResponse>;

    // Ciclo de Vida e Supervisão:
    /// Executado no startup do aplicativo (após a limpeza de órfãos da Fase 02):
    /// marca runs e itens não terminais como `interrupted` e aplica retenção de 30 dias em runs terminais.
    pub async fn recover_interrupted(&self) -> MtResult<Vec<GroupRun>>;

    /// Executado antes de remover um projeto (Fase 06): cancela e aguarda todas as execuções ativas daquele projeto.
    pub async fn on_project_deleting(self: &Arc<Self>, project_id: &ProjectId) -> MtResult<()>;

    /// Executado no encerramento explícito do aplicativo: cancela todas as filas e PTYs de grupos e aguarda supervisão.
    pub async fn shutdown(self: &Arc<Self>);
}
```

### 3.3. Os 14 Comandos IPC Tauri (`src-tauri/src/commands/command_groups.rs`)

```rust
use std::sync::Arc;
use tauri::{ipc::Channel, State};
use mt_core::{CommandGroupId, MtResult, ProjectId};
use mt_commands::groups::{
    events::{GroupOutputFrame, OutputAttachReceipt},
    model::{
        CommandGroup, CommandGroupCatalog, CommandGroupInput, DeleteGroupReceipt, GroupRun,
        GroupRunId, GroupRunItemId, GroupRunListPage, GroupRunRequest, GroupScope,
        OutputSubscriptionId, ReorderGroupEntry
    },
    suggest::{GroupSuggestRequest, GroupSuggestResponse, RecurringCommandsResponse}
};
use crate::command_groups::AppCommandGroupState;
use crate::events::command_groups::ChannelRunObserver;

#[tauri::command]
#[specta::specta]
pub async fn command_group_list(
    state: State<'_, AppCommandGroupState>,
    project_id: Option<ProjectId>,
) -> MtResult<CommandGroupCatalog> {
    state.service.list(project_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_get(
    state: State<'_, AppCommandGroupState>,
    group_id: CommandGroupId,
) -> MtResult<CommandGroup> {
    state.service.get(group_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_create(
    state: State<'_, AppCommandGroupState>,
    input: CommandGroupInput,
) -> MtResult<CommandGroup> {
    state.service.create(input).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_update(
    state: State<'_, AppCommandGroupState>,
    group_id: CommandGroupId,
    expected_revision: u64,
    input: CommandGroupInput,
) -> MtResult<CommandGroup> {
    state.service.update(group_id, expected_revision, input).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_delete(
    state: State<'_, AppCommandGroupState>,
    group_id: CommandGroupId,
    expected_revision: u64,
) -> MtResult<DeleteGroupReceipt> {
    state.service.delete(group_id, expected_revision).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_reorder(
    state: State<'_, AppCommandGroupState>,
    scope: GroupScope,
    project_id: Option<ProjectId>,
    entries: Vec<ReorderGroupEntry>,
) -> MtResult<CommandGroupCatalog> {
    state.service.reorder(scope, project_id, entries).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_run(
    state: State<'_, AppCommandGroupState>,
    request: GroupRunRequest,
) -> MtResult<GroupRun> {
    state.service.run(request).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_run_cancel(
    state: State<'_, AppCommandGroupState>,
    run_id: GroupRunId,
    item_id: Option<GroupRunItemId>,
) -> MtResult<GroupRun> {
    state.service.cancel_run(run_id, item_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_run_list(
    state: State<'_, AppCommandGroupState>,
    project_id: ProjectId,
    cursor: Option<String>,
    limit: u32,
    active_only: Option<bool>,
) -> MtResult<GroupRunListPage> {
    state.service.list_runs(project_id, cursor, limit, active_only).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_run_get(
    state: State<'_, AppCommandGroupState>,
    run_id: GroupRunId,
) -> MtResult<GroupRun> {
    state.service.get_run(run_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_run_item_attach(
    state: State<'_, AppCommandGroupState>,
    run_id: GroupRunId,
    item_id: GroupRunItemId,
    after_seq: Option<u64>,
    channel: Channel<GroupOutputFrame>,
) -> MtResult<OutputAttachReceipt> {
    let observer = Arc::new(ChannelRunObserver::new(channel));
    state.service.attach_item_output(run_id, item_id, after_seq, observer).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_run_item_detach(
    state: State<'_, AppCommandGroupState>,
    subscription_id: OutputSubscriptionId,
) -> MtResult<()> {
    state.service.detach_item_output(subscription_id).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_suggest(
    state: State<'_, AppCommandGroupState>,
    request: GroupSuggestRequest,
) -> MtResult<GroupSuggestResponse> {
    state.service.suggest(request).await
}

#[tauri::command]
#[specta::specta]
pub async fn command_group_recurring_commands(
    state: State<'_, AppCommandGroupState>,
    project_id: ProjectId,
    limit: u32,
) -> MtResult<RecurringCommandsResponse> {
    state.service.recurring_commands(project_id, limit).await
}
```

---

## 4. Regras de Negócio e Casos de Borda Desta Implementação

1. **Ordem Estrita de Idempotência em `CommandGroupService::run` (§4.1 item 1):**
   - Ao receber `GroupRunRequest`, o serviço adquire `accept_mutex` e consulta **primeiro** `repo.find_run_by_request_id(&request.request_id)` **antes** de verificar a existência ou a revisão atual do grupo.
   - Se o `request_id` já existir no banco:
     - Se `stored_hash == compute_request_hash(&request)`: retorna imediatamente `Ok(existing_run)` sem abrir novos PTYs ou abas (mesmo que o grupo tenha sido editado ou excluído depois do primeiro aceite).
     - Se `stored_hash != compute_request_hash(&request)`: retorna `Err(GroupErrorCode::RequestConflict)`.
2. **Preflight Completo Antes de Aceitar Execução:**
   - Se o `request_id` for inédito, `CommandGroupService::run` carrega o grupo, valida `group.revision == request.expected_revision` (`commands.group.revision_conflict`), valida que um grupo `Local` pertence a `request.project_id` (`commands.group.scope_mismatch`), resolve a pasta raiz do projeto e canonicaliza o `cwd` de todos os itens (`CwdResolver::resolve_effective_cwd`).
   - Para `Sequential`: exige `origin_pane_id.is_some()`, adquire o `PaneExecutionLease` validando prompt pronto, ausência de alt-screen/foreground e suporte a `env -C` (se algum item ou o grupo definir `cwd`).
   - Para `MultiTabs`: resolve todos os templates shell compatíveis via `MultiTabPlanner::resolve_templates_for_items`.
   - Somente após todo o preflight passar, persiste o snapshot (`GroupRun` + `GroupRunItem`s em `pending`), libera a transação do banco, emite `command-group://run-started` e despacha ao runner correspondente sem bloquear a resposta IPC.
3. **Recuperação no Boot (`recover_interrupted`) vs Shutdown Explícito:**
   - No startup do app (`lib.rs`), após a rotina da Fase 02 limpar eventuais processos órfãos de sessão anterior, `service.recover_interrupted().await` marca todas as linhas de `command_group_runs` e `command_group_run_items` que ainda estavam em estados não terminais (`queued`, `pending`, `starting`, `running`, `cancelling`) como `interrupted`, preservando itens que já haviam alcançado `succeeded`, `failed`, `skipped` ou `cancelled`, e **jamais relança comandos automaticamente** (`CA-09`, `CA-21`).
   - Quando a janela é apenas minimizada para a bandeja (Fase 09), as execuções continuam vivas normalmente. Quando o usuário aciona "Sair" explicitamente, `service.shutdown().await` é invocado antes de encerrar o `PtyManager` e o pool SQLite.
