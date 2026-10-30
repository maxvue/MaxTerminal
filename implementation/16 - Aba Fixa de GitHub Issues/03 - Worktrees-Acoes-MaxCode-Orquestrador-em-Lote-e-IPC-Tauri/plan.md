# Plano de Execução — Implementação 03: Worktrees, Ações MaxCode, Orquestrador em Lote e Comandos IPC Tauri (`mt-github` + `maxterminal`)

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `03 - Worktrees-Acoes-MaxCode-Orquestrador-em-Lote-e-IPC-Tauri`
> **Especificação Técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Criar Fixtures de Worktree/Bridge e Implementar `worktree.rs`
1. Criar `src-tauri/crates/mt-github/fixtures/maxcode/worktree-cases.v1.json` com vetores de teste da função `nomeDaWorktreeDeIssue` (`wt-implement-issue-<n>`) e casos de validação de caminhos/symlinks.
2. Criar `src-tauri/crates/mt-github/fixtures/maxcode/bridge-cases.v1.json` com snapshots dos envelopes de handshake (`issues_protocol_v1`), `issue.command` (`open`, `plan`, `fix`) e `bugs.cycle.{preview,start,status,progress}`.
3. Implementar `src-tauri/crates/mt-github/src/worktree.rs`:
   - `IssueWorktreeResolver::resolve(project_id, number)` consultando (1) mapeamento do MaxCode se conectado, (2) lista de worktrees registradas no Git via `GitPort::worktree_list` procurando a branch `wt-implement-issue-<n>`, (3) diretório `.max-code-worktrees/wt-implement-issue-<n>`.
   - Validar `common_dir` e rejeitar diretórios homônimos estranhos ou symlinks externos (`IssueWorktreeResolution::Incompatible` / `GithubErrorKind::WorktreeForeign`).
   - Gerar `IssueWorktreeProposal` com TTL de 5 minutos (`BranchOnly` quando a branch local já existe; `Missing` quando nem worktree nem branch existem) e verificar se `.max-code-worktrees/` já está ignorado no `.gitignore`.
   - Implementar `create_worktree(proposal_id, confirmed)` exigindo `confirmed == true`, delegando para `GitPort::worktree_create` (Fase 08) com `ensure_gitignore: true` e lidando com corrida `worktree_exists` por releitura.
   - Implementar `open_issue_tab` delegando para `WorkspacePort` (`tab_open` com `cwd_fallback_to_project: false` e título `Issue #<n>`) ou `AgentPort` cujo adaptador chama `AppAgentState::open_agent_tab` (D-08) com `WorktreeIsolation::Existing { path }`, preservando a worktree criada caso o spawn falhe; nunca chamar o handler Tauri internamente.

### Passo 2: Implementar Ações 1-Clique para o MaxCode (`actions.rs`)
0. Criar `src-tauri/src/github/{mod,adapters}.rs`: implementar `MaxCodePort` usando os quatro métodos de `MaxCodeClient` fornecidos pela 11/06, converter DTO de domínio para wire (`command → action`, `request_id → correlationId`, repo `owner/name`) e registrar o adaptador no composition root. Verificar as duas capabilities D-03; não editar a bridge nesta fase.
1. Implementar `src-tauri/crates/mt-github/src/actions.rs`:
   - Traduzir capabilities da bridge da Fase 11 para `IssueCapabilities`, validando compatibilidade de `protocol_version` e `catalog_hash`.
   - Construir `IssueCommandPayload` para `MaxCodeIssueCommand::{Open, Plan, Fix}` contendo apenas metadados autenticados (`project_path`, `repo`, `issue_number`, `command`, `catalog_hash`, `request_id`), sem concatenar texto não-confiável da issue em prompts.
   - Suportar wake-up via URI (`vscode://johnattas.maxcode/issue?...` com percent-encoding e sem tokens), aguardando handshake da bridge antes de enviar o comando com o mesmo `request_id`.
   - Correlacionar ACK da bridge (`DeliveryState::Accepted | Rejected`) e persistir `DeliveryState::Unknown` em caso de timeout de ACK para evitar disparo duplicado.

### Passo 3: Implementar o Orquestrador Delegado "Corrigir todos os Bugs" (`bulk.rs`)
1. Implementar `BugCycleTracker` em `src-tauri/crates/mt-github/src/bulk.rs`:
   - `preview(project_id)`: consultar `MaxCodePort::preview_bug_cycle` e armazenar o `BugCyclePreview` em memória com validade (`expires_at_ms`).
   - `start(project_id, preview_id, request_id, confirmed)`: exigir `confirmed == true`, validar que o preview não expirou e pertence ao mesmo `repo_id`, gravar `status = 'pending'` em `github_bug_cycles` (protegido pelo índice único parcial `idx_github_one_active_cycle`) e transmitir `bugs.cycle.start`.
   - `handle_peer_progress(progress)` e `status(project_id, cycle_id)`: atualizar `github_bug_cycles` somente quando `incoming.peer_revision >= stored.peer_revision`, emitir `github://bug-cycle-progress` e acionar `NotificationPort::notify_bug_cycle_finished` quando o MaxCode reportar estado terminal (`Completed`, `Failed`, `Cancelled`).
   - Implementar rotina de reconexão e bootstrap que reconcilia ciclos em `pending`, `accepted`, `running` ou `unknown` consultando o mesmo `request_id`/`cycle_id` no MaxCode sem iniciar novo ciclo.

### Passo 4: Implementar a Fachada `GithubService` (`lib.rs`), Comandos Tauri e Eventos
1. Completar `src-tauri/crates/mt-github/src/lib.rs` com a struct `GithubService` integrando todos os submódulos, validando `(project_id, auth_scope, generation)` antes e depois de cada `await` e tratando o evento `project://removed` para desregistrar vínculos sem cancelar ciclos remotos no MaxCode.
2. Criar `src-tauri/src/events/github.rs` com `TauriGithubEventSink` emitindo os 8 eventos `github://*` após o commit no SQLite.
3. Criar `src-tauri/src/commands/github.rs` com os 20 comandos `github_*` anotados com `#[tauri::command]` e `#[specta::specta]`.
4. Registrar `pub mod github`, comandos, adaptadores de porta e `app.manage(Arc::clone(&github_service))` no `src-tauri/src/lib.rs`; injetar `mt_db::Db` no cache e executar `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings` para atualizar `src/types/bindings.ts`.

### Passo 5: Escrever Testes Automatizados (`tests/bulk.rs`)
1. Criar `src-tauri/crates/mt-github/tests/bulk.rs`:
   - Simular sequência de snapshots de labels (`Waiting Plan` → `Planned`/`Waiting Execute` → `Executed`/`Waiting Check` → `Checked Approved`) e provar que snapshots de labels atualizam apenas observações auxiliares, enquanto somente o ACK/status do MaxCode confirma início e término (`Completed`).
   - Simular perda de ACK no `start` e reinício do `BugCycleTracker`, provando que a recuperação reutiliza o mesmo `requestId` e não dispara um segundo ciclo.
   - Testar descarte de mensagens fora de ordem (`peer_revision` menor que a atual).
   - Testar `IssueWorktreeResolver` nos cenários: worktree existente, apenas branch existente, sem branch, homônimo externo rejeitado e preservação de worktree quando o spawn da aba falha.
   - Testar `actions.rs`: bloqueio quando falta capability, distinção estrita entre `Open`, `Plan` (`BUG PLAN`) e `Fix` (`BUG FIX`), e entrega idempotente por `requestId`.

---

## 2. Comandos de Verificação e Validação

```bash
cd "$(git rev-parse --show-toplevel)"
# 1. Executar todos os testes do crate mt-github (incluindo bulk, worktree e actions)
cargo test --manifest-path src-tauri/Cargo.toml -p mt-github

# 2. Verificar compilação e clippy em mt-github e no binário maxterminal
cargo clippy --manifest-path src-tauri/Cargo.toml -p mt-github -p maxterminal --all-targets -- -D warnings

# 3. Gerar bindings TypeScript e validar ausência de diffs pendentes
cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings && git diff --exit-code src/types/bindings.ts
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

| Critério (Fase 16) | Como é atendido e verificado |
|--------------------|------------------------------|
| **CA-1 (`bulk`)** | `tests/bulk.rs` valida sequência de etapas, autoridade exclusiva do MaxCode para início/fim, recuperação pós-reinício com mesmo `requestId` e ordenação por `peer_revision` |
| **CA-3 (Clippy Workspace)** | `cargo clippy -p mt-github -p maxterminal -- -D warnings` passa sem avisos |
| **CA-4 (Bindings Specta)** | `cargo run --manifest-path src-tauri/Cargo.toml --bin export_bindings` exporta `GithubRepoRef`, `IssueSummary`, `IssueDetail`, `LabelDefinition`, `LabelKind`, `BugCycleProgress` e `MaxCodeIssueCommand` em `src/types/bindings.ts` |
| **CA-14 (Backend MaxCode 1-Clique)** | `actions.rs` exige ACK correlacionado, valida capabilities e garante idempotência por `requestId` |
| **CA-15 (Backend Worktrees)** | `worktree.rs` resolve e cria `.max-code-worktrees/wt-implement-issue-<n>` mediante confirmação e abre aba shell/agente com `cwd` nela |
| **CA-16 (Backend Ciclo em Lote)** | `bulk.rs` exige confirmação com `previewId`, persiste em `github_bug_cycles` e retoma acompanhamento ao reabrir o app |
