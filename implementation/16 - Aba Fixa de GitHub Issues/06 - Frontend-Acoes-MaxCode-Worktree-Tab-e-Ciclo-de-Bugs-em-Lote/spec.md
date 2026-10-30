# Especificação Técnica — Implementação 06: Frontend Ações MaxCode, Abertura de Aba na Worktree e Ciclo Automatizado de Bugs em Lote

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `06 - Frontend-Acoes-MaxCode-Worktree-Tab-e-Ciclo-de-Bugs-em-Lote`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §5.2, §6 e §7, [`../instructions.md`](../instructions.md) §2.1 e §5.1–§5.2, [`../spec.md`](../spec.md) §8, §9, §10.3 e §12.

---

## 1. Contexto, Objetivo e Escopo

Esta sexta e última implementação conclui 100% da Fase 16, entregando os componentes de integração operacional entre as Issues do GitHub, o workspace de terminais/agentes do MaxTerminal e a extensão MaxCode:

1. **Menu de Ações 1-Clique por Issue (`src/components/github/IssueActionsMenu.vue`):** Botões e menu de ação rápida no detalhe e na lista da issue para:
   - Enviar para o MaxCode com 1 clique: **Abrir (`open`)**, **Analisar (`BUG PLAN` / `plan`)** e **Corrigir (`BUG FIX` / `fix`)**.
   - Inspeção reativa das capacidades anunciadas pelo MaxCode (`IssueCapabilities` da bridge da Fase 11) e do status do catálogo (`CatalogStatus`): quando uma ação não for suportada pelo peer conectado ou o catálogo estiver incompatível, o botão fica desabilitado com tooltip explicativo em `pt-BR`.
   - Rastreamento do `DeliveryReceipt` (`Pending`, `Accepted`, `Rejected`, `Unknown`) por `requestId`, garantindo que abrir a janela do VS Code via URI não seja confundido com execução concluída e impedindo disparos duplicados.
2. **Abertura de Aba no MaxTerminal na Worktree/Branch da Issue (`src/components/github/IssueOpenTabModal.vue`):**
   - Fluxo acionado pelo botão **"Abrir Aba no MaxTerminal"**: chama primeiro `github_issue_worktree_resolve(projectId, number)`.
   - Se a resolução retornar `existing`, abre imediatamente a aba com o template/agente escolhido (ou abre direto com o padrão quando acionado pelo botão rápido, sem exibir diálogo de confirmação de criação de worktree — `CA-15`).
   - Se a resolução retornar `branch_only` ou `missing`, exibe o modal de confirmação obrigatório detalhando o caminho `.max-code-worktrees/wt-implement-issue-<n>`, a branch `wt-implement-issue-<n>` e se o `.gitignore` será atualizado, só executando `github_issue_worktree_create(proposalId, true)` após clique explícito em **"Confirmar e Abrir Aba"**.
   - Permite escolher entre **Terminal (Shell com Template do projeto)** ou **Agente CLI (`Claude Code`, `Codex`, `Antigravity (agy)`, `OpenCode`)** e foca automaticamente a aba recém-criada no workspace da Fase 07.
3. **Ciclo Automatizado em Lote "Corrigir todos os Bugs" (`src/components/github/BugCycleConfirmModal.vue`, `src/components/github/BugCycleProgress.vue`):**
   - Botão global **"Corrigir todos os Bugs"** no cabeçalho da aba `IssuesTab.vue`.
   - Ao clicar, consulta `github_bug_cycle_preview(projectId)` e abre `BugCycleConfirmModal.vue` exibindo o total de issues candidatas, a contagem deduplicada, a distribuição por etapa do workflow e o critério de seleção autoritativo do MaxCode.
   - Somente após confirmação explícita do usuário (`confirmed: true`), invoca `github_bug_cycle_start(projectId, previewId, requestId, true)`.
   - Exibe `BugCycleProgress.vue` acompanhando em tempo real o status (`Pending`, `Accepted`, `Running`, `Unknown`, `Completed`, `Failed`, `Cancelled`), os contadores por etapa e o indicador de atraso (`observation_stale`) caso a conexão com o MaxCode oscile, retomando automaticamente o acompanhamento ao fechar e reabrir o MaxTerminal (`github_bug_cycle_status`).

---

## 2. Arquivos a Criar ou Modificar

| Caminho | Ação | Descrição |
|---------|------|-----------|
| `src/components/github/IssueActionsMenu.vue` | Criar | Barra/menu de ações por issue: Abrir, Analisar (`BUG PLAN`), Corrigir (`BUG FIX`) no MaxCode e Abrir Aba no MaxTerminal |
| `src/components/github/IssueOpenTabModal.vue` | Criar | Modal de resolução/confirmação de criação da worktree `.max-code-worktrees/wt-implement-issue-<n>` e seleção de Terminal ou Agente CLI |
| `src/components/github/BugCycleConfirmModal.vue` | Criar | Modal de confirmação obrigatória de `"Corrigir todos os Bugs"` com resumo por etapa retornado por `github_bug_cycle_preview` |
| `src/components/github/BugCycleProgress.vue` | Criar | Painel/barra de progresso do ciclo automatizado de bugs em lote com contadores por etapa e status confirmado pelo MaxCode |
| `src/stores/githubIssues.ts` | Modificar | Completar e conectar actions `sendToMaxCode`, `resolveIssueWorktree`, `createIssueWorktree`, `openIssueTab`, `previewCycle`, `startCycle` e `reconcileCycle` |
| `src/components/github/IssueDetail.vue` | Modificar | Integrar `IssueActionsMenu.vue` e `IssueOpenTabModal.vue` no cabeçalho do detalhe da issue |
| `src/components/github/IssuesTab.vue` | Modificar | Integrar o botão global `"Corrigir todos os Bugs"`, `BugCycleConfirmModal.vue` e `BugCycleProgress.vue` |
| `tests/frontend/github/actions-worktree-and-cycle.spec.ts` | Criar | Testes Vitest validando ações MaxCode (ACK, capabilities, idempotência), confirmação de worktree, confirmação de `"Corrigir todos os Bugs"` e retomada de `BugCycleProgress` |

---

## 3. Contratos TypeScript e Regras dos Componentes Vue

### 3.1. Ações 1-Clique para MaxCode (`src/components/github/IssueActionsMenu.vue`)

```ts
export interface IssueActionsMenuProps {
    projectId: ProjectId;
    issueNumber: number;
    capabilities: IssueCapabilities | null;
    catalogStatus: CatalogStatus;
}
```

Regras de negócio de `IssueActionsMenu.vue`:
1. Renderiza 4 botões de ação com ícones `MaxIcon` e rótulos em `pt-BR`:
   - **Abrir no MaxCode** (`action: 'open'`)
   - **Analisar (`BUG PLAN`)** (`action: 'plan'`)
   - **Corrigir (`BUG FIX`)** (`action: 'fix'`)
   - **Abrir Aba no MaxTerminal** (aciona resolução de worktree)
2. **Gate de Capacidade e Catálogo:**
   - Se `capabilities == null` ou a ação específica (`canOpenIssue`, `canPlanIssue`, `canFixIssue`) for `false` (e `supportsUriWakeup` também for `false`), o botão correspondente fica `:disabled="true"` com tooltip em `pt-BR` exibindo `capabilities?.unavailableReason ?? 'Extensão MaxCode desconectada ou sem suporte ao protocolo de issues.'`.
   - Para `plan` e `fix`, se `catalogStatus !== 'compatible'`, desabilita o botão com tooltip `"Catálogo de 24 labels do MaxCode incompatível ou pendente de validação."`.
3. **Prevenção de Duplicidade e Feedback de Entrega:**
   - Enquanto um `requestId` para `(projectId, issueNumber, action)` estiver em estado `Pending` ou `Unknown`, o botão exibe spinner/estado de reconciliação e não gera novo `requestId`.
   - Se retornar `DeliveryState::Accepted`, exibe toast de confirmação `"Comando enviado e confirmado pelo MaxCode."`.
   - Se retornar `DeliveryState::Unknown` (ex.: VS Code acordou via URI mas o ACK ainda não chegou), exibe alerta informativo `"O MaxCode foi acionado, mas a confirmação de recebimento ainda não chegou."` com botão `"Verificar entrega"` (que reconsulta com o mesmo `requestId`).

### 3.2. Abertura de Aba na Worktree da Issue (`src/components/github/IssueOpenTabModal.vue`)

Fluxo de decisão determinístico (`CA-15`):
1. Ao clicar em **"Abrir Aba no MaxTerminal"** (ou escolher se deseja abrir Terminal Padrão, Template específico ou Agente CLI `claude-code`, `codex`, `agy`, `opencode`):
   - A store executa `githubIpc.issueWorktreeResolve(projectId, issueNumber)`.
2. **Caso `status === 'existing'`:**
   - Nenhuma worktree precisa ser criada. Se o usuário clicou na ação rápida padrão (ou já escolheu o destino), chama `githubIpc.issueOpenTab(projectId, issueNumber, resolution.workspace.workspaceId, target, templateId, agent)` **diretamente, sem abrir diálogo de confirmação de criação de worktree**.
   - A aba é aberta na Fase 07 / Fase 13 com `cwd` em `.max-code-worktrees/wt-implement-issue-<n>` e a seleção do workspace muda para `{ kind: 'terminal', tabId: result.tabId }`.
3. **Caso `status === 'branch_only'` ou `status === 'missing'`:**
   - Abre obrigatoriamente o `IssueOpenTabModal.vue` exibindo os detalhes da proposta (`proposal.targetPath`, `proposal.branch`, `proposal.createNewBranch`, `proposal.baseRef`, aviso se `.gitignore` será atualizado) e os seletores de destino (Shell + Template ou Agente CLI).
   - Somente quando o usuário clica em **"Criar Worktree e Abrir Aba"**, chama `githubIpc.issueWorktreeCreate(proposal.proposalId, true)` seguido de `githubIpc.issueOpenTab(...)`.
4. **Caso `status === 'ambiguous'`:**
   - Exibe a lista de worktrees candidatas para o usuário escolher explicitamente qual utilizar (sem fazer checkout forçado).
5. **Caso `status === 'incompatible'`:**
   - Exibe o motivo do bloqueio em `pt-BR` (ex.: diretório homônimo pertencente a outro repositório) e mantém o botão de criação desabilitado.

### 3.3. Ciclo em Lote "Corrigir todos os Bugs" (`BugCycleConfirmModal.vue` e `BugCycleProgress.vue`)

1. **`BugCycleConfirmModal.vue`:**
   - Antes de iniciar qualquer ciclo, busca `github_bug_cycle_preview(projectId)`.
   - Exibe:
     - Repositório de destino (`owner/name`).
     - Quantidade total de issues candidatas e quantidade após deduplicação pelo MaxCode.
     - Tabela/chips com a quantidade de issues por etapa (`preview.stages`: `stage`, `label`, `count`).
     - Critério de seleção reportado pelo MaxCode (`preview.selectionCriteria`).
   - Botão **"Confirmar e Iniciar Ciclo"** chama `store.startCycle(projectId, preview.previewId)` passando `confirmed: true` e um `requestId` único.
2. **`BugCycleProgress.vue`:**
   - Visível no topo de `IssuesTab.vue` sempre que `store.activeCycleByProject[projectId]` existir.
   - Exibe o badge de estado do ciclo (`Aguardando confirmação`, `Aceito pelo MaxCode`, `Em execução`, `Estado incerto / Reconectando`, `Concluído`, `Falhou`, `Cancelado`), o progresso por etapa (`stages`) e o timestamp da última atualização.
   - Se `progress.observationStale === true` ou `progress.status === 'unknown'`, exibe indicador visual de que a conexão com o MaxCode está sendo reconciliada sem reiniciar a fila.
   - Ao montar `IssuesTab.vue` (inclusive após fechar e reabrir o MaxTerminal), `store.reconcileCycle(projectId)` chama `github_bug_cycle_status(projectId, null)` para restaurar imediatamente o painel do ciclo ativo (`CA-16`).
