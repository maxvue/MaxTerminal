# Especificação Técnica — Implementação 03: Worktrees, Ações MaxCode, Orquestrador em Lote e Comandos IPC Tauri (`mt-github` + `maxterminal`)

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `03 - Worktrees-Acoes-MaxCode-Orquestrador-em-Lote-e-IPC-Tauri`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §6 e §7, [`../instructions.md`](../instructions.md) §2.1 e §5.1–§5.2, [`../spec.md`](../spec.md) §3, §5, §6, §8 e §9.

---

## 1. Contexto, Objetivo e Escopo

Esta terceira implementação completa o backend Rust da Fase 16, integrando o crate `mt-github` às Fases 06, 07, 08, 09, 11 e 13 e expondo todos os 20 comandos IPC Tauri e os 8 eventos globais `github://*`:

1. **Resolução e Criação de Worktree/Branch da Issue (`worktree.rs` + `fixtures/maxcode/worktree-cases.v1.json`):** Implementação do `IssueWorktreeResolver` seguindo a ordem estrita de prioridade (mapeamento retornado pelo MaxCode → worktree Git registrada na branch da issue → convenção `.max-code-worktrees/wt-implement-issue-<n>` e branch `wt-implement-issue-<n>`), validação de `common_dir` e symlinks, propostas com expiração de 5 minutos (`existing`, `branch_only`, `missing`, `ambiguous`, `incompatible`), verificação de `.gitignore` (`ensure_gitignore` da Fase 08), criação somente mediante confirmação explícita do usuário e abertura de aba de Terminal (`WorkspacePort` / Fase 07) ou Agente CLI (`AgentPort` / Fase 13 com `WorktreeIsolation::Existing { path }`).
2. **Integração 1-Clique com MaxCode (`actions.rs` + `fixtures/maxcode/bridge-cases.v1.json`):** Implementação do contrato versionado `issues_protocol_v1` sobre a bridge da Fase 11 (`MaxCodePort`), verificação de `IssueCapabilities` anunciadas no handshake, envio das ações **Abrir (`open`)**, **Analisar (`plan` / `BUG PLAN`)** e **Corrigir (`fix` / `BUG FIX`)** com `requestId` idempotente, correlação de `DeliveryReceipt` (`Pending`, `Accepted`, `Rejected`, `Unknown`) e wake-up via URI (`vscode://johnattas.maxcode/issue?...`) aguardando handshake antes de despachar o comando.
3. **Orquestrador Delegado do Ciclo em Lote "Corrigir todos os Bugs" (`bulk.rs`):** Implementação do `BugCycleTracker` que consulta a prévia autoritativa no MaxCode (`github_bug_cycle_preview`), dispara `bugs.cycle.start` uma única vez após confirmação (`github_bug_cycle_start`) respeitando o índice único parcial `idx_github_one_active_cycle`, acompanha o progresso confirmado por `peer_revision` e `requestId`/`cycleId` (`github_bug_cycle_status` e `github://bug-cycle-progress`), retoma o acompanhamento após queda da bridge ou reinício do aplicativo sem reiniciar a fila, e dispara notificação nativa do SO (`NotificationPort` / Fase 09) apenas quando o MaxCode confirma término (`completed`, `failed` ou `cancelled`).
4. **Fachada `GithubService`, Comandos IPC e Eventos Tauri (`lib.rs`, `src-tauri/src/commands/github.rs`, `src-tauri/src/events/github.rs`):** Composição completa de `GithubService`, registro dos 20 comandos `github_*` no `tauri::Builder` e exportação automática de tipos TypeScript para `src/types/bindings.ts` via `tauri-specta`.

---

## 2. Arquivos a Criar ou Modificar

| Caminho | Ação | Descrição |
|---------|------|-----------|
| `src-tauri/crates/mt-github/fixtures/maxcode/worktree-cases.v1.json` | Criar | Vetores de teste da convenção `wt-implement-issue-<n>` e resolução de caminhos/branches |
| `src-tauri/crates/mt-github/fixtures/maxcode/bridge-cases.v1.json` | Criar | Fixtures de handshake `issues_protocol_v1`, payloads `issue.command` e `bugs.cycle.*` |
| `src-tauri/crates/mt-github/src/worktree.rs` | Criar | `IssueWorktreeResolver`, gerenciamento de propostas com TTL de 5 min e abertura de aba shell/agente |
| `src-tauri/crates/mt-github/src/actions.rs` | Criar | `MaxCodeIssueCommand`, `IssueCommandPayload`, `IssueCapabilities`, `DeliveryReceipt` e serviço de envio |
| `src-tauri/crates/mt-github/src/bulk.rs` | Criar | `BugCycleTracker`, persistência em `github_bug_cycles`, reconciliação por `peer_revision` e notificação |
| `src-tauri/crates/mt-github/src/model.rs` | Modificar | Adicionar DTOs de worktree, actions MaxCode e ciclo em lote (`BugCyclePreview`, `BugCycleProgress`, etc.) |
| `src-tauri/crates/mt-github/src/lib.rs` | Modificar | Implementar a fachada `GithubService` integrando `remote`, `auth`, `client`, `labels`, `cache`, `sync`, `worktree`, `actions` e `bulk` |
| `src-tauri/crates/mt-github/tests/bulk.rs` | Criar | Testes de máquina de estados de `BugCycleTracker`, perda de ACK, reinício sem novo ciclo, ordenação por `peer_revision` e testagem de worktree/actions |
| `src-tauri/src/events/github.rs` | Criar | `TauriGithubEventSink` implementando `GithubEventSink` via `AppHandle::emit` |
| `src-tauri/src/commands/github.rs` | Criar | Implementação dos 20 comandos `#[tauri::command] #[specta::specta]` |
| `src-tauri/src/commands/mod.rs` | Modificar | Registrar o submódulo `pub mod github;` |
| `src-tauri/src/events/mod.rs` | Modificar | Registrar o submódulo `pub mod github;` |
| `src-tauri/src/lib.rs` | Modificar | Instanciar `GithubService`, injetar portas concretas (`mt-git`, `mt-bridge`, `mt-agents`, `mt-os`) e registrar os 20 comandos no `tauri_specta::Builder` |
| `src/types/bindings.ts` | Modificar (gerado) | Atualizado via `npm run bindings` com todos os tipos de `mt-github` |

---

## 3. Assinaturas Exatas e Contratos Rust

### 3.1. Resolução de Worktree e Abertura de Aba (`src-tauri/crates/mt-github/src/worktree.rs`)

```rust
use mt_core::{ProjectId, TabId, TemplateId};
use serde::{Deserialize, Serialize};
use specta::Type;
use crate::error::GithubError;
use crate::model::{RepoId, RequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceSource {
    MaxCodeMapping,
    GitRegisteredBranch,
    MaxCodeConvention,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedIssueWorkspace {
    pub workspace_id: String,
    pub repo_id: RepoId,
    pub number: u32,
    pub path: String,
    pub branch: String,
    pub common_dir: String,
    pub source: WorkspaceSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueWorktreeProposal {
    pub proposal_id: String,
    pub project_id: ProjectId,
    pub repo_id: RepoId,
    pub number: u32,
    pub target_path: String,
    pub branch: String,
    pub create_new_branch: bool,
    pub base_ref: Option<String>,
    pub gitignore_needs_update: bool,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueWorktreeCandidate {
    pub candidate_id: String,
    pub path: String,
    pub branch: String,
    pub source: WorkspaceSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IssueWorktreeResolution {
    Existing { workspace: ResolvedIssueWorkspace },
    BranchOnly { proposal: IssueWorktreeProposal },
    Missing { proposal: IssueWorktreeProposal },
    Ambiguous { candidates: Vec<IssueWorktreeCandidate> },
    Incompatible { reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueTabTargetKind {
    Shell,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueOpenTabResult {
    pub tab_id: TabId,
    pub workspace_path: String,
    pub branch: String,
    pub target: IssueTabTargetKind,
}

pub fn issue_worktree_convention_name(issue_number: u32) -> String {
    format!("wt-implement-issue-{issue_number}")
}
```

Regras de negócio de `IssueWorktreeResolver`:
1. **Convenção Canônica:** O nome da branch e da pasta em `.max-code-worktrees/` para a issue `<n>` é `wt-implement-issue-<n>` (`<project_root>/.max-code-worktrees/wt-implement-issue-<n>`).
2. **Validação de Pertencimento (`common_dir`):** Antes de aceitar qualquer diretório existente como worktree da issue, verifica via `GitPort` que ele é uma worktree Git registrada cujo `common_dir` canônico coincide exatamente com o `common_dir` do repositório do projeto e que não contém symlink escapando de `.max-code-worktrees/`. Caso exista um diretório homônimo não registrado no Git ou pertencente a outro repositório, retorna `GithubErrorKind::WorktreeForeign` / `IssueWorktreeResolution::Incompatible`.
3. **Expiração e Confirmação:** Propostas (`proposal_id`) ficam em cache em memória por 5 minutos (`300_000 ms`). `create_worktree(proposal_id, confirmed)` exige `confirmed == true`, revalida o estado do Git no momento da criação, aplica a política `ensure_gitignore` da Fase 08 para `.max-code-worktrees/` e trata condição de corrida (`worktree_exists`) relendo as worktrees e retornando `ResolvedIssueWorkspace` se pertencer ao mesmo repositório/branch.
4. **Abertura de Aba:**
   - Se `target == IssueTabTargetKind::Shell`: invoca `WorkspacePort::open_tab` (Fase 07 `tab_open`) passando `overrides.cwd = Some(workspace.path)`, `overrides.cwd_fallback_to_project = false` e `overrides.title = Some(format!("Issue #{}", number))`. Nunca injeta texto da issue como comando shell inicial.
   - Se `target == IssueTabTargetKind::Agent`: invoca `AgentPort::open_agent_tab` (Fase 13 `agent_tab_open`) com `AgentKind` (`claude-code`, `codex`, `agy` ou `opencode`) e `WorktreeIsolation::Existing { path: workspace.path }`. Se o spawn do PTY falhar após a criação da worktree, preserva a worktree criada no disco e inclui o caminho nos detalhes do erro.

### 3.2. Integração 1-Clique com MaxCode (`src-tauri/crates/mt-github/src/actions.rs`)

```rust
use serde::{Deserialize, Serialize};
use specta::Type;
use crate::model::{GithubRepoRef, RequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum MaxCodeIssueCommand {
    Open,
    Plan,
    Fix,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueCapabilities {
    pub protocol_version: Option<u16>,
    pub catalog_hash: Option<String>,
    pub can_open_issue: bool,
    pub can_plan_issue: bool,
    pub can_fix_issue: bool,
    pub can_preview_bug_cycle: bool,
    pub can_start_bug_cycle: bool,
    pub can_query_bug_cycle_status: bool,
    pub supports_uri_wakeup: bool,
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueCommandPayload {
    pub schema_version: u16,
    pub request_id: RequestId,
    pub project_path: String,
    pub repo: GithubRepoRef,
    pub issue_number: u32,
    pub command: MaxCodeIssueCommand,
    pub catalog_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryState {
    Pending,
    Accepted,
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryReceipt {
    pub request_id: RequestId,
    pub state: DeliveryState,
    pub peer_operation_id: Option<String>,
    pub message: Option<String>,
}
```

Regras de entrega em `actions.rs`:
- Verifica `MaxCodePort::issue_capabilities()`. Se a capability específica (`can_open_issue`, `can_plan_issue` ou `can_fix_issue`) for `false` ou o `catalog_hash` divergir para `Plan`/`Fix`, retorna `GithubErrorKind::BridgeUnsupported` com motivo claro em `pt-BR`. **Nunca** degrada silenciosamente `Fix` (`BUG FIX`) para `maxcode_open_plan`.
- Se o peer MaxCode estiver desconectado mas `supports_uri_wakeup` estiver habilitado, aciona a URI `vscode://johnattas.maxcode/issue?requestId=<id>&repo=<owner>/<name>&number=<n>&action=<open|plan|fix>` apenas para acordar a extensão, aguarda o handshake da bridge (até timeout de 10s) e transmite o `IssueCommandPayload` com o **mesmo** `requestId`. Abrir a janela do VS Code sem receber ACK da bridge resulta em `DeliveryState::Unknown` (nunca `Accepted`).

### 3.3. Ciclo em Lote "Corrigir todos os Bugs" (`src-tauri/crates/mt-github/src/bulk.rs`)

```rust
use serde::{Deserialize, Serialize};
use specta::Type;
use crate::model::{GithubRepoRef, RequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum BugCycleStatus {
    Pending,
    Accepted,
    Running,
    Unknown,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugStageCount {
    pub stage: String,
    pub label: String,
    pub count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCyclePreview {
    pub preview_id: String,
    pub repo: GithubRepoRef,
    pub catalog_hash: String,
    pub total_candidates: u32,
    pub deduplicated_count: u32,
    pub stages: Vec<BugStageCount>,
    pub selection_criteria: String,
    pub expires_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCycleProgress {
    pub request_id: RequestId,
    pub cycle_id: Option<String>,
    pub status: BugCycleStatus,
    pub peer_revision: u32,
    pub total: Option<u32>,
    pub stages: Vec<BugStageCount>,
    pub updated_at_ms: i64,
    pub observation_stale: bool,
}
```

Regras de negócio de `BugCycleTracker`:
1. **Delegação Estrita ao MaxCode:** O MaxTerminal **não** implementa fila própria de correção, não deduplica issues por conta própria e não altera labels no GitHub para avançar etapas.
2. **Idempotência e Exclusão Mútua (`idx_github_one_active_cycle`):** Ao chamar `start_cycle(project_id, preview_id, request_id, confirmed)`:
   - Exige `confirmed == true` e `preview` dentro da janela de validade (`now_ms <= expires_at_ms`).
   - Insere registro `status = 'pending'` em `github_bug_cycles` antes de enviar `bugs.cycle.start` na bridge. Se já existir um ciclo ativo para aquele `repo_id` com outro `request_id`, retorna `GithubErrorKind::Conflict`. Se for o mesmo `request_id`, reaproveita e consulta o status atual sem enviar novo comando de início.
3. **Ordenação Monotônica por `peer_revision`:** Atualizações de progresso vindas do MaxCode (`bugs.cycle.progress` ou resposta de `bugs.cycle.status`) só sobrescrevem o estado persistido se `incoming.peer_revision >= stored.peer_revision`.
4. **Observação Auxiliar vs. Término Confirmado:** Mudanças nas labels das issues observadas durante sincronizações do GitHub podem atualizar os contadores auxiliares de visualização quando `observation_stale == false`, mas **jamais** alteram `status` para `Completed`. Apenas um snapshot assinado pelo MaxCode com `status: Completed | Failed | Cancelled` encerra o ciclo e dispara a notificação nativa via `NotificationPort::notify_bug_cycle_finished`.
5. **Recuperação Pós-Reinício:** Na inicialização do `GithubService` e ao reconectar a bridge da Fase 11, carrega todos os ciclos de `github_bug_cycles` com `status IN ('pending', 'accepted', 'running', 'unknown')` e consulta `bugs.cycle.status` no MaxCode passando `request_id` e `cycle_id`.

### 3.4. Lista Completa dos 20 Comandos Tauri (`src-tauri/src/commands/github.rs`)

```rust
#[tauri::command] #[specta::specta]
pub async fn github_repo_resolve(state: State<'_, Arc<GithubService>>, project_id: ProjectId) -> Result<Option<RepoContext>, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_auth_status(state: State<'_, Arc<GithubService>>) -> Result<AuthStatus, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_auth_set_token(state: State<'_, Arc<GithubService>>, token: String) -> Result<AuthStatus, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_auth_clear_token(state: State<'_, Arc<GithubService>>) -> Result<AuthStatus, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issues_list(state: State<'_, Arc<GithubService>>, project_id: ProjectId, query: IssueListQuery) -> Result<Cached<IssuePage>, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_detail(state: State<'_, Arc<GithubService>>, project_id: ProjectId, number: u32, comments_cursor: Option<String>) -> Result<Cached<IssueDetail>, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_counts(state: State<'_, Arc<GithubService>>, project_ids: Vec<ProjectId>) -> Result<std::collections::HashMap<ProjectId, OpenIssueCount>, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_refresh(state: State<'_, Arc<GithubService>>, project_id: ProjectId, target: SyncTarget, number: Option<u32>) -> Result<SyncTicket, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_labels_catalog(state: State<'_, Arc<GithubService>>) -> Result<LabelCatalog, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_repo_labels(state: State<'_, Arc<GithubService>>, project_id: ProjectId, cursor: Option<String>) -> Result<RepoLabelsPage, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_labels_ensure(state: State<'_, Arc<GithubService>>, project_id: ProjectId, request_id: RequestId, missing_metadata: Vec<MissingLabelMetadata>, confirmed: bool) -> Result<LabelEnsureResult, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_create(state: State<'_, Arc<GithubService>>, request: IssueCreateRequest) -> Result<MutationReceipt<IssueDetail>, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_labels_update(state: State<'_, Arc<GithubService>>, request: LabelPatchRequest) -> Result<MutationReceipt<LabelPatchResult>, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_send_maxcode(state: State<'_, Arc<GithubService>>, project_id: ProjectId, number: u32, action: MaxCodeIssueCommand, request_id: RequestId) -> Result<DeliveryReceipt, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_worktree_resolve(state: State<'_, Arc<GithubService>>, project_id: ProjectId, number: u32) -> Result<IssueWorktreeResolution, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_worktree_create(state: State<'_, Arc<GithubService>>, proposal_id: String, confirmed: bool) -> Result<ResolvedIssueWorkspace, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_issue_open_tab(state: State<'_, Arc<GithubService>>, project_id: ProjectId, number: u32, workspace_id: String, target: IssueTabTargetKind, template_id: Option<TemplateId>, agent: Option<String>) -> Result<IssueOpenTabResult, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_bug_cycle_preview(state: State<'_, Arc<GithubService>>, project_id: ProjectId) -> Result<BugCyclePreview, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_bug_cycle_start(state: State<'_, Arc<GithubService>>, project_id: ProjectId, preview_id: String, request_id: RequestId, confirmed: bool) -> Result<BugCycleProgress, MtError>;

#[tauri::command] #[specta::specta]
pub async fn github_bug_cycle_status(state: State<'_, Arc<GithubService>>, project_id: ProjectId, cycle_id: Option<String>) -> Result<Option<BugCycleProgress>, MtError>;
```
