# Plano de Execução — Implementação 02: Paleta de Busca Global Vue, Navegação Contextual, Acessibilidade e i18n

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Especificação de Referência:** [`spec.md`](./spec.md)

---

## 1. Checklist Sequencial de Implementação

### Passo 1: Wrappers IPC e Regeneração de Bindings
1. Regenerar `src/types/bindings.ts` garantindo a presença de `SearchRequest`, `SearchFilters`, `SearchResponse`, `SearchSection`, `SearchHit`, `SearchTarget`, `HighlightSegment`, `CancelOutcome`, `SearchIndexStat` e DTOs de backup.
2. Criar `src/ipc/search.ts` e `src/ipc/backup.ts` utilizando o helper `call()` de `src/ipc/core.ts`.

### Passo 2: Store `useSearchStore` e Composable `useHighlightSegments`
1. Criar `src/composables/search/useHighlightSegments.ts` para sanitizar a estrutura de `HighlightSegment[]` (mesclando segmentos adjacentes de mesmo estado `matched` e rejeitando entradas vazias).
2. Criar `src/stores/search.ts` implementando `useSearchStore` puramente em memória, com controle de `generation`, debounce configurável (padrão 120 ms), bloqueio durante composição IME, cancelamento explícito de requisições substituídas, liberação de snapshots atrasados/fechados e paginação incremental por seção (`loadMore`).

### Passo 3: Composables `useGlobalSearch`, `useSearchNavigation` e `useSearchShortcut`
1. Criar `src/composables/search/useGlobalSearch.ts` conectando listeners `onSearchInvalidated` e `onSearchIndexProgress`, foco anterior (`document.activeElement`) e limpeza no `onScopeDispose`.
2. Criar `src/composables/search/useSearchNavigation.ts` integrando `useProjectsStore` (06), `useTabsStore` / `SessionReadonlyViewer` (07), `CommandBlockLayer.scrollToBlock` (04/05), `useCommandGroupsStore` (15) e `useGithubIssuesStore` (16), além do realce visual temporário de 1,5 s.
3. Criar `src/composables/search/useSearchShortcut.ts` e atualizar `src/composables/terminal/keybindings.ts` e `src/components/terminal/TerminalSearchBar.vue` (Fase 04) para suportar `Ctrl+K` (fora de `passthrough` e com input rico vazio) e o escalonamento `Ctrl+Shift+F` levando a query da barra local.

### Passo 4: Componentes Vue da Paleta de Busca e Integração na `AppTitleBar`
1. Criar `src/components/search/HighlightedText.vue` sem `v-html`, usando exclusivamente `<mark>` e interpolação textual.
2. Criar `src/components/search/SearchFilterBar.vue` com seletores de projeto, período, código de saída (`Qualquer`, `Sucesso (0)`, `Com erro (!= 0)`, `Desconhecido`) e fontes, exibindo explicação em `pt-BR` quando um filtro oculta fontes incompatíveis.
3. Criar `src/components/search/SearchResultItem.vue` e `src/components/search/SearchResultSection.vue` com formatação `Intl.DateTimeFormat('pt-BR')` e botão `"Carregar mais"`.
4. Criar `src/components/search/GlobalSearchPalette.vue` com atributos ARIA completos (`role="dialog"`, `aria-modal="true"`, input `role="combobox"`, `aria-expanded`, `aria-controls`, `aria-activedescendant`, lista `role="listbox"`), navegação por `ArrowDown`/`ArrowUp`/`Home`/`End`, `Enter` (abrir), `Ctrl+Enter` (abrir template em nova aba) e `Escape` (fechar e devolver foco).
5. Adicionar o botão `"Buscar em tudo…"` no slot `actions` de `src/components/shell/AppTitleBar.vue` e montar `<GlobalSearchPalette />` em `src/app/App.vue`.

### Passo 5: Testes Unitários, Acessibilidade (`axe-core`) e Auditoria `pt-BR`
1. Criar `tests/unit/search/useSearchStore.spec.ts`, `HighlightedText.spec.ts`, `useSearchShortcut.spec.ts` e `useSearchNavigation.spec.ts`.
2. Criar `tests/a11y/search-palette.a11y.spec.ts` e `tests/a11y/workspace-modals.a11y.spec.ts` executando `axe-core` sobre a paleta aberta (estados vazio, com resultados e com filtros), lista de projetos, barra de abas e modais.
3. Criar `tests/i18n/ptbr-literals.spec.ts` verificando todos os textos obrigatórios e datas em `pt-BR`.

---

## 2. Comandos Exatos de Verificação e Testes

Executar a partir da raiz da worktree ativa (`cd "$(git rev-parse --show-toplevel)"`):

```bash
# 1. Lint de código (incluindo regra vue/no-v-html) e estilos SCSS
npm run lint
npm run lint:style

# 2. Typecheck estrito com tsgo e vue-tsc
npm run typecheck
npm run typecheck:vue

# 3. Executar suíte unitária da busca global (CA-09, CA-11 a CA-14)
npx --no-install vitest run tests/unit/search/

# 4. Executar suíte de acessibilidade axe-core e localização pt-BR (CA-15, CA-29)
npx --no-install vitest run tests/a11y/ tests/i18n/
```

---

## 3. Mapeamento para os Critérios de Aceite da Fase 17

| Critério | Teste / Verificação Responsável | Resultado Esperado |
|----------|---------------------------------|--------------------|
| `CA-09` | `tests/unit/search/useSearchStore.spec.ts` | 10 teclas espaçadas por 30 ms com debounce de 120 ms disparam 1 única chamada `searchGlobal`; respostas atrasadas ou de geração anterior são descartadas e seus snapshots liberados. |
| `CA-11` | `tests/unit/search/useSearchShortcut.spec.ts` | `Ctrl+K` abre a paleta com foco no input fora de `passthrough`; `Ctrl+Shift+F` com barra local aberta escalona levando o texto; em TUI (`alt-screen`) nenhuma tecla é interceptada. |
| `CA-12` | `tests/unit/search/useSearchNavigation.spec.ts` + `GlobalSearchPalette` | Setas percorrem itens entre todas as seções; `Enter` abre; `Ctrl+Enter` abre nova aba para template; `Esc` fecha e restaura o foco ao elemento original. |
| `CA-13` | `tests/unit/search/useSearchNavigation.spec.ts` | Resultado de bloco vivo seleciona o projeto, ativa a aba, rola até o bloco e aplica realce de 1,5 s; sessão arquivada abre o viewer de histórico da Fase 07 posicionado no bloco/chunk. |
| `CA-14` | `tests/unit/search/HighlightedText.spec.ts` + `npm run lint` | `HighlightedText.vue` renderiza sem `v-html`; payload `<img src=x onerror=alert(1)>` é exibido como texto literal inofensivo. |
| `CA-15` | `tests/i18n/ptbr-literals.spec.ts` | Textos `"Buscar em tudo…"`, `"Nenhum resultado para “x”"`, `"Carregar mais"`, `"Blocos de comando"`, `"Histórico de sessões"`, `"Projetos"`, `"Templates"`, `"Grupos de comandos"`, `"Issues"` e datas `Intl.DateTimeFormat('pt-BR')` validados. |
| `CA-29` | `tests/a11y/search-palette.a11y.spec.ts` + `workspace-modals.a11y.spec.ts` | Zero violações `serious` ou `critical` do `axe-core` na paleta, lista de projetos, barra de abas e modais. |
