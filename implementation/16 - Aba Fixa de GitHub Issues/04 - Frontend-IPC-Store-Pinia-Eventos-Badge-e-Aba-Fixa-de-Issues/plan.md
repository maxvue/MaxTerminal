# Plano de Execução — Implementação 04: Frontend IPC, Store Pinia, Eventos, Badge 🟣 e Aba Fixa de Issues

> **Fase:** 16 — Aba Fixa de GitHub Issues
> **Unidade:** `04 - Frontend-IPC-Store-Pinia-Eventos-Badge-e-Aba-Fixa-de-Issues`
> **Especificação Técnica:** [`./spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Implementar a Camada IPC (`src/ipc/github.ts`) e Helpers de Catálogo (`src/constants/githubLabels.ts`)
1. Criar `src/ipc/github.ts` exportando `githubIpc` (com os 20 métodos mapeados para os comandos de `src/types/bindings.ts`) e `listenGithubEvents(handlers)` assinando os 8 eventos `github://*`.
2. Verificar que nenhuma interface `GithubRepoRef`, `IssueSummary`, `IssueDetail`, `LabelDefinition`, `LabelKind`, `BugCycleProgress` ou `MaxCodeIssueCommand` foi redeclarada manualmente em `src/`.
3. Criar `src/constants/githubLabels.ts` com funções auxiliares para agrupar `LabelDefinition[]` do `LabelCatalog` em `State` ("Estado"), `Closure` ("Encerramento") e `Category` ("Categoria"), além da paleta de cores hexadecimais sugeridas para criação de labels faltantes quando o export não definir cor.

### Passo 2: Implementar a Store Pinia (`src/stores/githubIssues.ts`) e Eventos (`src/composables/useGithubIssueEvents.ts`)
1. Criar `src/stores/githubIssues.ts` (`useGithubIssuesStore`) seguindo os padrões do projeto (4 espaços, aspas simples, ponto e vírgula, tipagem estrita):
   - Estado indexado por `ProjectId`, `PartitionKey` e `IssueKey`.
   - Actions `resolveProject`, `loadCounts`, `loadList`, `loadMoreList`, `loadDetail`, `loadMoreComments`, `refresh`, `loadCatalog`, `loadRepoLabels`, `ensureLabels`, `setAuthToken`, `clearAuthToken`, `applyEvent` e `disposeProject`.
   - Garantir que `setAuthToken` nunca armazene a string do token no state da Pinia.
2. Criar `src/composables/useGithubIssueEvents.ts`:
   - Registrar os listeners dos 8 eventos `github://*` e do evento `project://removed` antes da carga inicial.
   - Em caso de gap na sequência de `revision` do envelope, acionar resync automático via `store.loadList(projectId, { reset: true })`.
   - Limpar todos os `UnlistenFn` no `onScopeDispose` / HMR.

### Passo 3: Implementar Filtros Puros (`useIssueFilters.ts`) e o Badge 🟣 (`useIssueBadgeProvider.ts`)
1. Criar `src/composables/useIssueFilters.ts`:
   - Implementar filtragem reativa combinando estado (`open`/`closed`/`all`), labels de estado, labels de encerramento, labels de categoria e busca textual (`#number`, título, autor).
   - Otimizar comparação com `Set` pré-computado por issue para garantir tempo `< 50 ms` com 1000 issues em memória.
2. Criar `src/composables/useIssueBadgeProvider.ts`:
   - Registrar o provider `id: 'github-issues'`, `order: 60` no `useProjectBadgeRegistry()` da Fase 06.
   - Retornar `null` quando o projeto não for GitHub, quando a contagem ainda for desconhecida (`null`) ou quando for `0`.
   - Exibir o contador roxo com tooltip em `pt-BR` indicando estado atualizado ou atrasado (`stale`), sem ser afetado pelos filtros da UI.

### Passo 4: Implementar a Aba Fixa Virtual no `TabBar #pinned` e Componentes de Lista/Filtros/Auth/Banner
1. Criar `src/components/github/IssueFilters.vue`, `src/components/github/IssueListItem.vue` e `src/components/github/IssueList.vue`:
   - Seguir ordem obrigatória `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`, tags de abertura em linha única e recuo de 4 espaços (`baseIndent: 1`).
   - Implementar navegação completa por teclado (`ArrowDown`, `ArrowUp`, `Home`, `End`, `Enter`) com atributos `role="listbox"` / `role="option"` e `aria-selected`.
2. Criar `src/components/github/GithubAuthPanel.vue`:
   - Exibir fonte ativa (`gh` CLI, Keyring do sistema ou Anônimo) e conta conectada.
   - Permitir salvar um Personal Access Token no Keyring (`github_auth_set_token`) limpando imediatamente a variável local do input após o submit, ou remover o token salvo (`github_auth_clear_token`).
3. Criar `src/components/github/LabelSyncBanner.vue`:
   - Calcular quantas das 24 labels canônicas do MaxCode faltam nas labels remotas do repositório (`github_repo_labels`).
   - Quando `missingCount > 0`, exibir o banner `"Faltam N labels do MaxCode neste repositório"` com botão `"Criar labels"` que abre confirmação (e solicita cor hex se alguma label faltante não tiver cor no catálogo) e invoca `github_labels_ensure`.
4. Criar `src/components/github/IssuesTab.vue` coordenando `LabelSyncBanner`, `IssueFilters`, `IssueList`, `GithubAuthPanel`, indicadores de `rateLimit` e o slot/painel lateral de detalhe.
5. Integrar no componente de workspace da Fase 07 (`TabBar` slot `#pinned`):
   - Renderizar o botão fixo `"Issues"` com ícone e contador apenas quando `store.contexts[selectedProjectId]` existir.
   - Garantir que a aba `"Issues"` seja sempre o índice 0, não possua botão de fechar (`×`), não possa ser arrastada/reordenada e não crie PTY ou `TabId`.

### Passo 5: Escrever Testes Vitest (`store-and-filters.spec.ts` e `badge-and-pinned-tab.spec.ts`)
1. Criar `tests/frontend/github/store-and-filters.spec.ts`:
   - Testar filtro combinado (`Waiting Check` + categoria `Seguranca` + busca textual) sobre fixture de 1000 issues validando exatidão e tempo de execução `< 50 ms`.
   - Testar `applyEvent('github://issues-updated')` aplicando atualização incremental sem recarregar toda a lista quando a revisão é contígua, e forçando resync quando há gap de revisão.
   - Testar descarte de resposta IPC atrasada após troca de projeto ou rotação de `authScope`.
2. Criar `tests/frontend/github/badge-and-pinned-tab.spec.ts`:
   - Comprovar que o badge 🟣 não renderiza para `count: 0`, `count: null` ou projeto sem GitHub, e renderiza com `count > 0` ignorando filtros ativos na UI.
   - Comprovar que a aba `"Issues"` no `#pinned` está na posição 0, não renderiza botão de fechar e preserva `activeTabId` das abas de terminal ao alternar.

---

## 2. Comandos de Verificação e Validação

```bash
cd "$(git rev-parse --show-toplevel)"
# 1. Validar que nenhum tipo exportado pelo Specta foi duplicado manualmente em src/
rg -n "interface (GithubRepoRef|IssueSummary|IssueDetail|LabelDefinition|BugCycleProgress)|type (LabelKind|MaxCodeIssueCommand)" src/ --glob '!src/types/bindings.ts'

# 2. Executar typecheck com tsgo e lint/stylelint
npm run typecheck
npm run lint

# 3. Executar testes unitários da Implementação 04 no Vitest
npx --no-install vitest run tests/frontend/github/store-and-filters.spec.ts tests/frontend/github/badge-and-pinned-tab.spec.ts
```

---

## 3. Critérios de Aceite Atendidos nesta Implementação

| Critério (Fase 16) | Como é atendido e verificado |
|--------------------|------------------------------|
| **CA-4 (Sem duplicação de tipos)** | `src/ipc/github.ts` e `src/stores/githubIssues.ts` consomem exclusivamente `src/types/bindings.ts` |
| **CA-5 (Typecheck e Lint)** | `tsgo --noEmit`, ESLint e Stylelint passam sem erros nos arquivos criados |
| **CA-6 (Filtros, Store incremental, Badge 🟣 e Aba Fixa)** | Coberto por `store-and-filters.spec.ts` e `badge-and-pinned-tab.spec.ts` |
| **CA-7 (Aba fixa na 1ª posição apenas p/ projetos GitHub)** | Composição no slot `TabBar #pinned` condicionada a `RepoContext` não-nulo |
| **CA-8 e CA-10 (Badge independente de filtro e busca < 50 ms)** | Validado com fixture de 1000 issues em `store-and-filters.spec.ts` |
| **CA-17 (`LabelSyncBanner`)** | Exibe `"Faltam N labels do MaxCode"` e cria somente as faltantes via `github_labels_ensure` |
