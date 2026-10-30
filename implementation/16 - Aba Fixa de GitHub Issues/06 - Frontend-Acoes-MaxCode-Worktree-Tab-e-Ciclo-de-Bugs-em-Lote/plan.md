# Plano de Execução — Implementação 06: Frontend Ações MaxCode, Abertura de Aba na Worktree e Ciclo Automatizado de Bugs em Lote

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `06 - Frontend-Acoes-MaxCode-Worktree-Tab-e-Ciclo-de-Bugs-em-Lote`
> **Especificação Técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar `IssueActionsMenu.vue` (Integração 1-Clique com MaxCode)
1. Criar `src/components/github/IssueActionsMenu.vue`:
   - Renderizar os botões **Abrir**, **Analisar (`BUG PLAN`)**, **Corrigir (`BUG FIX`)** e **Abrir Aba no MaxTerminal**.
   - Desabilitar individualmente as ações do MaxCode quando `IssueCapabilities` indicar ausência de suporte ou quando `CatalogStatus !== 'compatible'`, exibindo tooltip explicativo em `pt-BR`.
   - Conectar à action `store.sendToMaxCode(projectId, issueNumber, command)` mantendo `requestId` estável por intenção e exibindo o estado real do `DeliveryReceipt` (`Pending`, `Accepted`, `Rejected`, `Unknown`).

### Passo 2: Implementar `IssueOpenTabModal.vue` (Worktree/Branch da Issue + Aba de Terminal/Agente)
1. Criar `src/components/github/IssueOpenTabModal.vue`:
   - Ao acionar abertura de aba para a issue `#<n>`, consultar `store.resolveIssueWorktree(projectId, issueNumber)`.
   - Quando `resolution.status === 'existing'`, abrir a aba diretamente via `store.openIssueTab(...)` sem diálogo de confirmação de criação de worktree (`CA-15`) e focar a nova aba no workspace.
   - Quando `resolution.status === 'branch_only'` ou `'missing'`, apresentar o modal de confirmação detalhando `.max-code-worktrees/wt-implement-issue-<n>`, a branch `wt-implement-issue-<n>`, o ajuste de `.gitignore` (se necessário) e a escolha entre **Terminal (Template)** ou **Agente CLI (`claude-code`, `codex`, `agy`, `opencode`)**.
   - Executar `github_issue_worktree_create(proposalId, true)` somente após confirmação explícita do usuário e, se o spawn da aba falhar após criar a worktree, exibir mensagem em `pt-BR` informando que a worktree criada foi preservada.

### Passo 3: Implementar `BugCycleConfirmModal.vue` e `BugCycleProgress.vue` ("Corrigir todos os Bugs")
1. Criar `src/components/github/BugCycleConfirmModal.vue`:
   - Carregar `BugCyclePreview` via `store.previewCycle(projectId)`.
   - Exibir a contagem de issues por etapa (`preview.stages`), total de candidatos, total deduplicado e critério de seleção.
   - Exigir confirmação explícita antes de chamar `store.startCycle(projectId, preview.previewId)`.
2. Criar `src/components/github/BugCycleProgress.vue`:
   - Renderizar o painel de acompanhamento do ciclo ativo (`BugCycleProgress`) com contadores por etapa, indicador de `observationStale` quando a bridge estiver desconectada e status terminal confirmado exclusivamente pelo MaxCode.
3. Integrar `BugCycleConfirmModal.vue` e `BugCycleProgress.vue` em `src/components/github/IssuesTab.vue` e `IssueActionsMenu.vue` / `IssueDetail.vue`, garantindo que `store.reconcileCycle(projectId)` seja chamado ao selecionar o projeto ou reabrir o app.

### Passo 4: Escrever Testes Vitest (`actions-worktree-and-cycle.spec.ts`) e Validar Toda a Fase 16
1. Criar `tests/frontend/github/actions-worktree-and-cycle.spec.ts`:
   - Testar `IssueActionsMenu.vue`: botões desabilitados com tooltip em `pt-BR` quando falta capability; envio de `open`, `plan` (`BUG PLAN`) e `fix` (`BUG FIX`) exigindo ACK correlacionado; estado `Unknown` quando URI acorda o VS Code sem ACK; ausência de duplicação em duplo clique.
   - Testar `IssueOpenTabModal.vue`: abertura direta sem diálogo quando a worktree `.max-code-worktrees/wt-implement-issue-<n>` já existe (`existing`), e exigência de confirmação explícita antes de `github_issue_worktree_create` quando `missing` ou `branch_only`.
   - Testar `BugCycleConfirmModal.vue` e `BugCycleProgress.vue`: confirmação obrigatória antes de `github_bug_cycle_start`, atualização reativa dos contadores por etapa via `github://bug-cycle-progress`, retomada do ciclo ativo via `github_bug_cycle_status` ao reabrir o app sem reiniciar a fila, e garantia de que mudanças de labels sozinhas não marcam o ciclo como concluído.
2. Revisar todos os arquivos da Fase 16 (`src-tauri/crates/mt-github`, `src-tauri/src/commands/github.rs`, `src/ipc/github.ts`, `src/stores/githubIssues.ts`, `src/composables/*`, `src/components/github/*.vue`) quanto à conformidade com `AGENTS.md` (textos, tooltips, `aria-label` e mensagens de erro 100% em `pt-BR`).

---

## 2. Comandos de Verificação e Validação Final da Fase 16

```bash
# 1. Executar toda a suíte de testes Rust do crate mt-github
cargo test -p mt-github

# 2. Validar clippy sem avisos em mt-github e maxterminal
cargo clippy -p mt-github -p maxterminal --all-targets -- -D warnings

# 3. Validar sincronia de bindings TypeScript gerados via tauri-specta
npm run bindings && git diff --exit-code src/types/bindings.ts

# 4. Validar ausência de tipos duplicados manualmente no frontend
rg -n "interface (GithubRepoRef|IssueSummary|IssueDetail|LabelDefinition|BugCycleProgress)|type (LabelKind|MaxCodeIssueCommand)" src/ --glob '!src/types/bindings.ts'

# 5. Executar verificação de tipos TypeScript (tsgo) e linters (ESLint + Stylelint)
npm run typecheck:tsgo
npm run lint

# 6. Executar toda a suíte Vitest da Fase 16
npx --no-install vitest run tests/frontend/github/
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

| Critério (Fase 16) | Como é atendido e verificado |
|--------------------|------------------------------|
| **CA-6 (Confirmação antes de `github_bug_cycle_start` e `github_issue_worktree_create`)** | Validado por testes automatizados em `tests/frontend/github/actions-worktree-and-cycle.spec.ts` |
| **CA-14 (Ações Abrir, `BUG PLAN` e `BUG FIX` com ACK e URI)** | `IssueActionsMenu.vue` valida capabilities, correlaciona ACK por `requestId` e distingue despertar de URI de ação confirmada |
| **CA-15 (Abrir Aba na Worktree da Issue)** | `IssueOpenTabModal.vue` pede confirmação e cria `.max-code-worktrees/wt-implement-issue-<n>` quando inexistente, ou abre direto sem diálogo quando já existe |
| **CA-16 (Corrigir todos os Bugs e retomada pós-reinício)** | `BugCycleConfirmModal.vue` exige confirmação listando issues por etapa; `BugCycleProgress.vue` acompanha etapas e retoma ao reabrir o app via consulta ao MaxCode |
| **CA-18 (Idioma 100% pt-BR na UI e erros)** | Todos os rótulos, tooltips, `aria-label` e `MtError.message` revisados e validados em Português do Brasil (`pt-BR`) |
