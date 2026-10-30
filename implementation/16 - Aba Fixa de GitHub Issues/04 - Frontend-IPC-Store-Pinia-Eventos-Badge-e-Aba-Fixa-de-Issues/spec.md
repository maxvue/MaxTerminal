# Especificação Técnica — Implementação 04: Frontend IPC, Store Pinia, Eventos, Badge 🟣 e Aba Fixa de Issues

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `04 - Frontend-IPC-Store-Pinia-Eventos-Badge-e-Aba-Fixa-de-Issues`
> **Referências Canônicas:** [`../../AGENTS.md`](../../../AGENTS.md) §3, §4.1, §4.2 e §7, [`../instructions.md`](../instructions.md) §2.1 e §5.1–§5.2, [`../spec.md`](../spec.md) §5, §6, §10.1–§10.3.

---

## 1. Contexto, Objetivo e Escopo

Esta quarta implementação constrói a fundação frontend da Fase 16 (TypeScript, Pinia, composables reativos e componentes estruturais da aba fixa e listagem), conectando o backend `mt-github` aos pontos de extensão das Fases 06 (Lista de Projetos) e 07 (Barra de Abas):

1. **Camada IPC e Metadados de Labels (`src/ipc/github.ts`, `src/constants/githubLabels.ts`):** Wrappers tipados para os 20 comandos `github_*` e listeners dos 8 eventos `github://*`, consumindo exclusivamente os tipos gerados pelo Specta em `src/types/bindings.ts` (zero duplicação manual de DTOs).
2. **Store Pinia Centralizada (`src/stores/githubIssues.ts` — `useGithubIssuesStore`):** Única autoridade no frontend para estado de repositórios GitHub por projeto, resumos e detalhes de issues, contadores de issues abertas, filtros ativos, catálogo de labels, autenticação, recibos de mutação e progresso do ciclo de bugs, com proteção contra respostas obsoletas (verificação de `authScope`, `generation` e token de requisição) e coalescência de chamadas em voo.
3. **Composables de Eventos, Filtros Puros e Badge 🟣 (`useGithubIssueEvents.ts`, `useIssueFilters.ts`, `useIssueBadgeProvider.ts`):**
   - Assinatura global única dos eventos `github://*` com detecção de gap de `revision` e resync automático.
   - Filtragem, busca textual e ordenação puras em memória capazes de filtrar 1000 issues combinando estado + label de workflow + label de categoria + texto em `< 50 ms`.
   - Registro do contador 🟣 **Issues Abertas** no `useProjectBadgeRegistry` da Fase 06 (`id: 'github-issues'`, `order: 60`), ocultando o badge quando `count === 0` ou quando o projeto não é um repositório GitHub, e preservando o último valor com indicação de atraso/erro sem transformar falha de rede em zero falso.
4. **Aba Fixa Virtual "Issues" e Componentes de Listagem/Autenticação/Sync de Labels (`IssuesTab.vue`, `IssueList.vue`, `IssueListItem.vue`, `IssueFilters.vue`, `GithubAuthPanel.vue`, `LabelSyncBanner.vue` + integração no `TabBar #pinned`):** Renderização da aba fixa na 1ª posição (índice 0, não fechável, não reordenável, sem criar PTY/SessionId/TabId), preservando a aba de terminal ativa ao alternar entre "Issues" e terminais.

---

## 2. Arquivos a Criar ou Modificar

| Caminho | Ação | Descrição |
|---------|------|-----------|
| `src/ipc/github.ts` | Criar | Wrappers tipados sobre `bindings.ts` para os 20 comandos `github_*` e assinatura tipada dos 8 eventos `github://*` |
| `src/constants/githubLabels.ts` | Criar | Helpers visuais e metadados de UI derivados do catálogo exposto por `github_labels_catalog` (cores padrão para criação, rótulos de grupo `State`/`Closure`/`Category` em pt-BR) |
| `src/stores/githubIssues.ts` | Criar | Store Pinia `useGithubIssuesStore` com estado normalizado por `IssueKey` e todas as actions |
| `src/composables/useGithubIssueEvents.ts` | Criar | Composable de assinatura global dos eventos `github://*` e do evento `project://removed` |
| `src/composables/useIssueFilters.ts` | Criar | Composable puro de filtragem combinada (estado, labels de estado/encerramento, categoria e busca textual) |
| `src/composables/useIssueBadgeProvider.ts` | Criar | Registro do provedor de badge 🟣 (`id: 'github-issues'`, `order: 60`) no `useProjectBadgeRegistry` da Fase 06 |
| `src/components/github/IssuesTab.vue` | Criar | Painel mestre da aba fixa de Issues (barra superior, banners de auth/rate-limit/labels, split lista + detalhe) |
| `src/components/github/IssueList.vue` | Criar | Lista virtualizada/paginada de issues com navegação por teclado (`ArrowUp`/`ArrowDown`/`Enter`) e gatilho `load-more` |
| `src/components/github/IssueListItem.vue` | Criar | Item da lista exibindo `#number`, título, estado, chips de labels, contagem de comentários e tempo relativo |
| `src/components/github/IssueFilters.vue` | Criar | Barra de filtros (estado Aberta/Fechada/Todas, seletor de etapa do workflow, seletor de categoria e input de busca) |
| `src/components/github/GithubAuthPanel.vue` | Criar | Painel/modal de configuração de autenticação (`gh` CLI vs. token no Keyring, limpeza imediata do input após envio) |
| `src/components/github/LabelSyncBanner.vue` | Criar | Banner exibido quando faltam labels canônicas do MaxCode no repositório, com botão "Criar labels" e confirmação |
| `src/components/workspace/ProjectWorkspace.vue` (ou host do `TabBar`) | Modificar | Injetar o botão virtual "Issues" no slot `#pinned` do `TabBar` da Fase 07 e alternar exibição entre `<IssuesTab>` e painéis de terminal |
| `tests/frontend/github/store-and-filters.spec.ts` | Criar | Testes Vitest cobrindo filtros combinados (< 50 ms p/ 1000 itens), aplicação incremental de `github://issues-updated` e descarte de respostas obsoletas |
| `tests/frontend/github/badge-and-pinned-tab.spec.ts` | Criar | Testes Vitest cobrindo o badge 🟣 (oculto em `0` e não-GitHub, preservado em erro/stale) e a aba fixa `IssuesTab` (índice 0, sem botão fechar, sem PTY) |

---

## 3. Contratos TypeScript, Store Pinia e Componentes Vue

### 3.1. Camada IPC (`src/ipc/github.ts`)

Todos os tipos de domínio são importados diretamente de `@/types/bindings`:

```ts
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type AuthStatus,
    type BugCyclePreview,
    type BugCycleProgress,
    type Cached,
    type DeliveryReceipt,
    type IssueCreateRequest,
    type IssueDetail,
    type IssueListQuery,
    type IssueOpenTabResult,
    type IssuePage,
    type IssueTabTargetKind,
    type IssueWorktreeResolution,
    type LabelCatalog,
    type LabelEnsureResult,
    type LabelPatchRequest,
    type LabelPatchResult,
    type MaxCodeIssueCommand,
    type MissingLabelMetadata,
    type MutationReceipt,
    type OpenIssueCount,
    type ProjectId,
    type RepoContext,
    type RepoLabelsPage,
    type RequestId,
    type ResolvedIssueWorkspace,
    type SyncTarget,
    type SyncTicket,
    type TemplateId
} from '@/types/bindings';

export const githubIpc = {
    repoResolve: (projectId: ProjectId): Promise<RepoContext | null> =>
        unwrap(commands.githubRepoResolve(projectId), 'github_repo_resolve'),
    authStatus: (): Promise<AuthStatus> =>
        unwrap(commands.githubAuthStatus(), 'github_auth_status'),
    authSetToken: (token: string): Promise<AuthStatus> =>
        unwrap(commands.githubAuthSetToken(token), 'github_auth_set_token'),
    authClearToken: (): Promise<AuthStatus> =>
        unwrap(commands.githubAuthClearToken(), 'github_auth_clear_token'),
    issuesList: (projectId: ProjectId, query: IssueListQuery): Promise<Cached<IssuePage>> =>
        unwrap(commands.githubIssuesList(projectId, query), 'github_issues_list'),
    issueDetail: (projectId: ProjectId, number: number, commentsCursor?: string | null): Promise<Cached<IssueDetail>> =>
        unwrap(commands.githubIssueDetail(projectId, number, commentsCursor ?? null), 'github_issue_detail'),
    issueCounts: (projectIds: ProjectId[]): Promise<Record<ProjectId, OpenIssueCount>> =>
        unwrap(commands.githubIssueCounts(projectIds), 'github_issue_counts'),
    refresh: (projectId: ProjectId, target: SyncTarget, number?: number | null): Promise<SyncTicket> =>
        unwrap(commands.githubRefresh(projectId, target, number ?? null), 'github_refresh'),
    labelsCatalog: (): Promise<LabelCatalog> =>
        unwrap(commands.githubLabelsCatalog(), 'github_labels_catalog'),
    repoLabels: (projectId: ProjectId, cursor?: string | null): Promise<RepoLabelsPage> =>
        unwrap(commands.githubRepoLabels(projectId, cursor ?? null), 'github_repo_labels'),
    labelsEnsure: (
        projectId: ProjectId,
        requestId: RequestId,
        missingMetadata: MissingLabelMetadata[],
        confirmed: boolean
    ): Promise<LabelEnsureResult> =>
        unwrap(commands.githubLabelsEnsure(projectId, requestId, missingMetadata, confirmed), 'github_labels_ensure'),
    issueCreate: (request: IssueCreateRequest): Promise<MutationReceipt<IssueDetail>> =>
        unwrap(commands.githubIssueCreate(request), 'github_issue_create'),
    issueLabelsUpdate: (request: LabelPatchRequest): Promise<MutationReceipt<LabelPatchResult>> =>
        unwrap(commands.githubIssueLabelsUpdate(request), 'github_issue_labels_update'),
    issueSendMaxcode: (
        projectId: ProjectId,
        number: number,
        action: MaxCodeIssueCommand,
        requestId: RequestId
    ): Promise<DeliveryReceipt> =>
        unwrap(commands.githubIssueSendMaxcode(projectId, number, action, requestId), 'github_issue_send_maxcode'),
    issueWorktreeResolve: (projectId: ProjectId, number: number): Promise<IssueWorktreeResolution> =>
        unwrap(commands.githubIssueWorktreeResolve(projectId, number), 'github_issue_worktree_resolve'),
    issueWorktreeCreate: (proposalId: string, confirmed: boolean): Promise<ResolvedIssueWorkspace> =>
        unwrap(commands.githubIssueWorktreeCreate(proposalId, confirmed), 'github_issue_worktree_create'),
    issueOpenTab: (
        projectId: ProjectId,
        number: number,
        workspaceId: string,
        target: IssueTabTargetKind,
        templateId?: TemplateId | null,
        agent?: string | null
    ): Promise<IssueOpenTabResult> =>
        unwrap(
            commands.githubIssueOpenTab(projectId, number, workspaceId, target, templateId ?? null, agent ?? null),
            'github_issue_open_tab'
        ),
    bugCyclePreview: (projectId: ProjectId): Promise<BugCyclePreview> =>
        unwrap(commands.githubBugCyclePreview(projectId), 'github_bug_cycle_preview'),
    bugCycleStart: (
        projectId: ProjectId,
        previewId: string,
        requestId: RequestId,
        confirmed: boolean
    ): Promise<BugCycleProgress> =>
        unwrap(commands.githubBugCycleStart(projectId, previewId, requestId, confirmed), 'github_bug_cycle_start'),
    bugCycleStatus: (projectId: ProjectId, cycleId?: string | null): Promise<BugCycleProgress | null> =>
        unwrap(commands.githubBugCycleStatus(projectId, cycleId ?? null), 'github_bug_cycle_status')
};
```

### 3.2. Store Pinia (`src/stores/githubIssues.ts`)

```ts
export type IssueKey = `${string}:${string}:${number}`; // `${repoId}:${authScope}:${number}`
export type PartitionKey = `${string}:${string}`;      // `${repoId}:${authScope}`
export type WorkspaceSelection = { kind: 'issues' } | { kind: 'terminal'; tabId: string };

export interface IssueFilterState {
    state: 'open' | 'closed' | 'all';
    stateLabels: string[];
    closureLabels: string[];
    categoryLabels: string[];
    searchText: string;
}
```

Regras de funcionamento da `useGithubIssuesStore`:
1. **Chave de Partição (`PartitionKey`):** Todo resumo (`summaries[issueKey]`), detalhe (`details[issueKey]`), lista ordenada de números (`orderedNumbersByPartition[partitionKey]`) e revisão (`revisionByPartition[partitionKey]`) é isolado por `(repo.id, authScope)`. Quando `github://auth-changed` chega, seleções e caches de escopos privados anteriores são limpos da memória imediatamente.
2. **Controle de Geração e Concorrência:** Antes de aplicar o retorno de qualquer `githubIpc.*`, a action verifica se `contexts[projectId]?.generation === capturedGeneration` e `contexts[projectId]?.authScope === capturedScope`. Respostas de gerações anteriores são descartadas silenciosamente.
3. **Aplicação Incremental de Eventos (`applyEvent`):**
   - `github://issues-updated`: se `revision > currentRevision + 1` ou `payload.invalidate === true`, dispara `loadList(projectId, { reset: true })`. Caso contrário, atualiza `counts[projectId] = payload.count`, remove `removedNumbers` da partição e recarrega os itens modificados.
   - `github://issue-updated`: se a issue estiver aberta no painel de detalhe (`selectedIssueByProject[projectId] === payload.number`), recarrega o detalhe.
4. **Alternância de Aba Virtual (`selectIssuesView` / `selectTerminalTab`):**
   - `workspaceSelectionByProject[projectId]` controla se o workspace exibe `<IssuesTab>` (`{ kind: 'issues' }`) ou a árvore de painéis de terminal da Fase 07 (`{ kind: 'terminal', tabId }`).
   - Ao clicar na aba fixa "Issues", `useTabsStore.activeTabId` do projeto é preservado para que, ao clicar de volta na aba de terminal ou fechar o painel, o foco retorne ao terminal anterior sem recriar PTY.

### 3.3. Composable de Filtros (`src/composables/useIssueFilters.ts`)

- Opera de forma pura e síncrona sobre os `IssueSummary[]` carregados na partição do projeto + `LabelCatalog`.
- **Semântica booleana:**
  - Entre dimensões distintas (`state`, `stateLabels`, `closureLabels`, `categoryLabels`, `searchText`): **AND** (interseção).
  - Dentro de uma mesma dimensão de labels (quando o usuário seleciona mais de uma label no filtro): configurável com suporte a interseção/união usando a função de normalização do catálogo.
  - `searchText`: busca case-insensitive em `#<number>`, `title` e `authorLogin`.
- **Desempenho:** Pré-indexa os conjuntos de labels normalizadas por issue para filtrar 1000 issues em `< 5 ms` (muito abaixo do orçamento de `50 ms` do `CA-10`).

### 3.4. Provedor do Badge 🟣 (`src/composables/useIssueBadgeProvider.ts`)

- Registra no `useProjectBadgeRegistry()` da Fase 06 o provider:
  - `id: 'github-issues'`
  - `order: 60` (6º badge, após Executando, Execução Contínua, Finalizado, Com Erro e Parado)
  - `icon: 'lucide:circle-dot'` (via `MaxIcon`)
  - Token semântico roxo verificado no tema `@maxvue/max-components-ui` / `presetMaxUno` (sem assumir `--max-help-500` inexistente; usa variável CSS com fallback seguro `var(--max-purple-500, var(--max-primary-400, #a855f7))`).
- Regra de exibição (`resolve(projectId)`):
  - Se o projeto não possui `RepoContext` GitHub ou `counts[projectId]?.count == null`: retorna `null` (oculto).
  - Se `counts[projectId].count === 0`: retorna `null` (oculto conforme `CA-6`).
  - Se `counts[projectId].count > 0`: retorna `{ count, tooltip, stale: counts[projectId].stale }` com tooltip em `pt-BR` (`"N issues abertas no GitHub"` ou `"N issues abertas no GitHub (sincronização atrasada)"`).
  - Independente dos filtros aplicados em `IssueFilters.vue`, o badge exibe **sempre** o total de issues abertas do repositório (`CA-8`).
