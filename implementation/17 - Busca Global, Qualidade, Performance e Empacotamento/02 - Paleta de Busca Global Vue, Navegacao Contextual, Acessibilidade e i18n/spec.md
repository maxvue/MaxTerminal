# Implementação 02 — Paleta de Busca Global Vue, Navegação Contextual, Acessibilidade e i18n

> **Fase:** 17 — Busca Global, Qualidade, Performance e Empacotamento
> **Itens de Escopo Cobertos:** `I-04`, `I-05`, `I-06`, `I-12`
> **Critérios de Aceite Cobertos:** `CA-09`, `CA-11`, `CA-12`, `CA-13`, `CA-14`, `CA-15`, `CA-29`

---

## 1. Contexto, Objetivo e Escopo

Esta implementação entrega toda a camada frontend em **Vue 3.6 + TypeScript (`tsgo`) + Pinia + UnoCSS + SCSS Scoped** para a **Busca Global FTS5 instantânea**, incluindo:
- Wrappers IPC tipados (`src/ipc/search.ts` e `src/ipc/backup.ts`) consumindo `src/types/bindings.ts`.
- Store reativa em memória `useSearchStore` com controle estrito de geração monotônica (`generation`), debounce de 120 ms, proteção contra corridas assíncronas (respostas fora de ordem, `finally` atrasado, fechamento/reabertura da paleta), suporte a composição IME e paginação incremental por seção (`loadMore`).
- Paleta modal acessível (`GlobalSearchPalette.vue`) acionada por `Ctrl+K`, por escalonamento `Ctrl+Shift+F` a partir da barra de busca local da Fase 04, ou pelo botão `"Buscar em tudo…"` na `AppTitleBar.vue` da Fase 01, sem jamais roubar teclas de TUIs em `alt-screen` / `passthrough`.
- Renderização segura de trechos destacados (`HighlightedText.vue`) sem `v-html` (`vue/no-v-html: error`).
- Orquestração de navegação contextual (`useSearchNavigation.ts`) até blocos de comando vivos, sessões arquivadas (`SessionReadonlyViewer` da Fase 07), chunks de scrollback, projetos, templates, grupos de comandos e issues do GitHub, com realce temporário de 1,5 s.
- Conformidade de acessibilidade WCAG AA (`axe-core`) e localização 100% em `pt-BR`.

---

## 2. Lista Exata de Arquivos a Criar ou Modificar

### 2.1. Criar em `src/`
- `src/ipc/search.ts` — Wrappers IPC (`searchGlobal`, `cancelSearch`, `releaseSearch`, `getSearchIndexStats`, `rebuildSearchIndex`, `onSearchIndexProgress`, `onSearchInvalidated`).
- `src/ipc/backup.ts` — Wrappers IPC (`listDbBackups`, `createDbBackup`, `restoreDbBackup`).
- `src/stores/search.ts` — Store Pinia `useSearchStore` (estado puramente em memória, sem persistência em `localStorage`).
- `src/composables/search/useGlobalSearch.ts` — Coordenação de debounce (120 ms), eventos IME (`compositionstart`/`compositionend`), listeners de invalidação e ciclo de vida do modal.
- `src/composables/search/useHighlightSegments.ts` — Normalização e validação segura de `HighlightSegment[]`.
- `src/composables/search/useSearchNavigation.ts` — Resolução assíncrona de `SearchTarget` (seleção de projeto, foco em aba viva, abertura de viewer de histórico somente leitura, rolagem até bloco/chunk e realce de 1.500 ms).
- `src/composables/search/useSearchShortcut.ts` — Registro de `Ctrl+K` e escalonamento `Ctrl+Shift+F` junto ao `keybindings.ts` da Fase 04.
- `src/components/search/HighlightedText.vue` — Renderizador seguro de `HighlightSegment[]` usando `<mark>` e interpolação textual.
- `src/components/search/SearchFilterBar.vue` — Barra de filtros (projeto, período, aba/sessão, código de saída, fontes) com aviso de fontes ocultas por incompatibilidade de filtro.
- `src/components/search/SearchResultItem.vue` — Item individual com ícone semântico da fonte, título e excerto destacados, data relativa/absoluta via `Intl.DateTimeFormat('pt-BR')` e badge de truncamento.
- `src/components/search/SearchResultSection.vue` — Cabeçalho da seção com rótulo em `pt-BR`, lista de itens e botão acessível `"Carregar mais"`.
- `src/components/search/GlobalSearchPalette.vue` — Modal principal com padrão ARIA `dialog` + `combobox` + `listbox`, navegação por setas entre seções, estados `loading`/`empty`/`error`/`cursor_expired`.

### 2.2. Modificar em `src/`
- `src/types/bindings.ts` — Regenerado automaticamente a partir do `tauri-specta` com os tipos da Implementação 01.
- `src/composables/terminal/keybindings.ts` (Fase 04) — Registrar a ação `search.global.open` (`Ctrl+K`) e a regra de escalonamento de `terminal.search.open` (`Ctrl+Shift+F` quando a `TerminalSearchBar` já estiver aberta).
- `src/components/terminal/TerminalSearchBar.vue` (Fase 04) — Expor evento/ação de escalonamento para abrir a paleta global levando a query atual.
- `src/components/shell/AppTitleBar.vue` (Fase 01) — Adicionar botão `"Buscar em tudo…"` (`Ctrl+K`) no slot `actions`.
- `src/app/App.vue` — Montar `<GlobalSearchPalette />` e inicializar `useSearchShortcut()`.
- `eslint.config.js` — Garantir que a regra `'vue/no-v-html': 'error'` esteja ativa sem exceções em `src/components/search/**`.

### 2.3. Criar em `tests/`
- `tests/unit/search/useSearchStore.spec.ts` — Testes de máquina de estados, debounce de 120 ms (10 teclas a cada 30 ms = 1 chamada IPC), corridas A/B com resolução invertida, `finally` atrasado, paginação `loadMore` idempotente e invalidação.
- `tests/unit/search/HighlightedText.spec.ts` — Testes de renderização literal de payloads XSS (`<img src=x onerror=alert(1)>`), emojis, acentos e texto RTL.
- `tests/unit/search/useSearchShortcut.spec.ts` — Testes de precedência de `Ctrl+K` e `Ctrl+Shift+F` (bloqueado em xterm `passthrough`/alt-screen, permitido fora do terminal ou no input rico vazio).
- `tests/unit/search/useSearchNavigation.spec.ts` — Testes de navegação para bloco em aba viva, sessão arquivada, chunk de scrollback, template (`Enter` vs `Ctrl+Enter`), grupo, issue e alvo removido (`search.target_missing`).
- `tests/a11y/search-palette.a11y.spec.ts` e `tests/a11y/workspace-modals.a11y.spec.ts` — Suíte `vitest-axe` / `axe-core` validando zero violações `serious`/`critical` na paleta, lista de projetos, barra de abas e modais.
- `tests/i18n/ptbr-literals.spec.ts` — Verificação estática e de componentes garantindo os rótulos canônicos em `pt-BR` e formatação `Intl.DateTimeFormat('pt-BR')`.

---

## 3. Especificação Técnica Exaustiva

### 3.1. Wrappers IPC (`src/ipc/search.ts` e `src/ipc/backup.ts`)

```ts
// src/ipc/search.ts
import type { UnlistenFn } from '@tauri-apps/api/event';
import { listen } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import { commands } from '@/types/bindings';
import type {
    CancelOutcome,
    RebuildJobHandle,
    SearchIndexProgressEvent,
    SearchIndexStat,
    SearchInvalidatedEvent,
    SearchRequest,
    SearchResponse,
    SearchSourceKind
} from '@/types/bindings';

export async function searchGlobal(request: SearchRequest): Promise<SearchResponse> {
    return call('search_global', () => commands.searchGlobal(request));
}

export async function cancelSearch(requestId: string): Promise<CancelOutcome> {
    return call('search_cancel', () => commands.searchCancel(requestId));
}

export async function releaseSearch(snapshotId: string): Promise<void> {
    return call('search_release', () => commands.searchRelease(snapshotId));
}

export async function getSearchIndexStats(): Promise<SearchIndexStat[]> {
    return call('search_index_stats', () => commands.searchIndexStats());
}

export async function rebuildSearchIndex(sources: SearchSourceKind[]): Promise<RebuildJobHandle> {
    return call('search_index_rebuild', () => commands.searchIndexRebuild(sources));
}

export async function onSearchIndexProgress(
    handler: (payload: SearchIndexProgressEvent) => void
): Promise<UnlistenFn> {
    return listen<SearchIndexProgressEvent>('search://index-progress', (event) => {
        handler(event.payload);
    });
}

export async function onSearchInvalidated(
    handler: (payload: SearchInvalidatedEvent) => void
): Promise<UnlistenFn> {
    return listen<SearchInvalidatedEvent>('search://invalidated', (event) => {
        handler(event.payload);
    });
}
```

### 3.2. Store `useSearchStore` (`src/stores/search.ts`)

```ts
import { defineStore } from 'pinia';
import { computed, ref, shallowRef } from 'vue';
import { cancelSearch, releaseSearch, searchGlobal } from '@/ipc/search';
import type {
    MtError,
    SearchFilters,
    SearchHit,
    SearchSection,
    SearchSort,
    SearchSourceKind
} from '@/types/bindings';

export type SearchUiStatus = 'idle' | 'debouncing' | 'loading' | 'ready' | 'empty' | 'error';

export const SEARCH_SECTION_LABELS: Record<SearchSourceKind['kind'], string> = {
    commands: 'Blocos de comando',
    output: 'Histórico de sessões',
    sessions: 'Sessões',
    projects: 'Projetos',
    templates: 'Templates',
    groups: 'Grupos de comandos',
    group_runs: 'Execuções de grupos',
    issues: 'Issues',
    comments: 'Comentários de issues'
};

export const useSearchStore = defineStore('search', () => {
    const open = ref<boolean>(false);
    const query = ref<string>('');
    const filters = ref<SearchFilters>({
        projectId: null,
        tabId: null,
        sessionId: null,
        paneId: null,
        fromMs: null,
        untilMs: null,
        exit: { kind: 'any' },
        sources: []
    });
    const sort = ref<SearchSort>({ kind: 'relevance' });
    const generation = ref<number>(0);
    const activeRequestId = ref<string | null>(null);
    const snapshotId = ref<string | null>(null);
    const sections = shallowRef<SearchSection[]>([]);
    const selectedKey = ref<string | null>(null);
    const status = ref<SearchUiStatus>('idle');
    const error = ref<MtError | null>(null);
    const loadingMoreSources = ref<Set<SearchSourceKind['kind']>>(new Set());
    const isComposingIme = ref<boolean>(false);

    // Estado não serializável mantido fora de refs exportadas
    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    const inFlightLoadMoreKeys = new Set<string>();

    const flatItems = computed<SearchHit[]>(() =>
        sections.value.flatMap((section) => section.items)
    );

    function show(initialQuery?: string, initialProjectId?: string | null): void;
    function updateQuery(nextQuery: string, debounceMs?: number): void;
    function updateFilters(partial: Partial<SearchFilters>, debounceMs?: number): void;
    function setComposingIme(composing: boolean): void;
    async function run(): Promise<void>;
    async function loadMore(source: SearchSourceKind): Promise<void>;
    function select(key: string | null): void;
    function moveSelection(delta: 1 | -1): void;
    function hide(): void;
    function invalidate(reason?: string): void;
    function dispose(): void;

    return {
        open,
        query,
        filters,
        sort,
        generation,
        activeRequestId,
        snapshotId,
        sections,
        selectedKey,
        status,
        error,
        loadingMoreSources,
        isComposingIme,
        flatItems,
        show,
        updateQuery,
        updateFilters,
        setComposingIme,
        run,
        loadMore,
        select,
        moveSelection,
        hide,
        invalidate,
        dispose
    };
});
```

Regras de concorrência obrigatórias na `useSearchStore`:
1. `updateQuery` / `updateFilters` incrementam `generation.value += 1` **sincronamente**, limpam `debounceTimer`, disparam `cancelSearch(activeRequestId)` e `releaseSearch(snapshotId)` se existirem, limpam `inFlightLoadMoreKeys` e agendam novo timer de `120 ms` (salvo se `isComposingIme.value === true`, caso em que aguarda `setComposingIme(false)`).
2. `run()` captura `const myGen = generation.value` e `const reqId = crypto.randomUUID()`. Quando `await searchGlobal(...)` resolve, se `!open.value || generation.value !== myGen || activeRequestId.value !== reqId`, chama `void releaseSearch(res.snapshotId)` e retorna imediatamente sem tocar em `sections`, `status` ou `error`. O bloco `catch` e o encerramento de estado também verificam `generation.value === myGen`.
3. `loadMore(source)` monta a chave `const dedupKey = `${snapshotId.value}:${source.kind}:${section.nextCursor}``; se já estiver em `inFlightLoadMoreKeys`, ignora. Ao resolver, verifica `generation.value === myGen && snapshotId.value === mySnapshotId` e anexa somente `SearchHit` cujas `key` ainda não existam naquela seção.

### 3.3. Navegação Contextual (`src/composables/search/useSearchNavigation.ts`)

```ts
import type { SearchTarget } from '@/types/bindings';

export type SearchNavigationAction = 'default' | 'open-in-new-tab';

export interface SearchNavigationResult {
    ok: boolean;
    reason?: 'target_missing' | 'navigation_failed';
}

export function useSearchNavigation() {
    async function openTarget(
        target: SearchTarget,
        action: SearchNavigationAction = 'default'
    ): Promise<SearchNavigationResult>;

    return { openTarget };
}
```

Fluxo por variante de `SearchTarget`:
- **`Command`:**
  1. Se `target.projectId` existir, seleciona o projeto via `useProjectsStore.select(target.projectId)`.
  2. Verifica se `target.tabId` e `target.sessionId` correspondem a uma aba ativa na `useTabsStore` e se o bloco `target.blockId` ainda reside na `CommandBlockLayer` da Fase 04. Se sim: ativa a aba, foca o painel `target.paneId`, executa `scrollToBlock(target.blockId)` e aplica a classe de realce `.max-search-highlight-flash` por `1500 ms` (desativando animação se `prefers-reduced-motion: reduce`).
  3. Caso a sessão esteja arquivada ou o bloco tenha saído do buffer vivo: resolve `tabSessionId` (ou abre em modo `"Histórico não vinculado"` se legado sem `tab_sessions`) e abre o `SessionReadonlyViewer` da Fase 07 posicionado em `target.blockId` sem instanciar PTY/stdin.
- **`Output`:**
  1. Seleciona `target.projectId` e abre o `SessionReadonlyViewer` (ou aba correspondente) carregando a página de scrollback que contém `(target.sessionId, target.paneId, target.chunkSeq)`, aplicando realce temporário de `1500 ms` no chunk.
- **`Session`:**
  1. Se a sessão estiver ativa numa aba viva, foca a aba; caso contrário, abre o `SessionReadonlyViewer` para `target.sessionId`.
- **`Project`:**
  1. Seleciona `target.projectId` na `useProjectsStore`.
- **`Template`:**
  1. Se `action === 'open-in-new-tab'` (`Ctrl+Enter`), invoca o fluxo da Fase 07 para abrir uma nova aba a partir de `target.templateId`; se `action === 'default'` (`Enter`), seleciona o projeto e abre o modal/detalhes do template.
- **`Group` / `GroupRun`:**
  1. Abre o painel/histórico do grupo na `useCommandGroupsStore` (Fase 15) focado em `groupId` ou `runKey` (nunca reexecuta o grupo automaticamente).
- **`Issue`:**
  1. Seleciona `target.projectId`, ativa a aba fixa de GitHub Issues (Fase 16) e abre o detalhe da issue `target.number` (rolando até `target.commentKey` se presente), sem disparar `BUG FIX` automaticamente.
- Se qualquer alvo tiver sido removido por retenção/deleção (`search.target_missing`), exibe toast em `pt-BR`: `"Este resultado não está mais disponível"` e oferece ação `"Atualizar busca"`.

### 3.4. Precedência de Atalhos (`src/composables/search/useSearchShortcut.ts`)

1. **`Ctrl+K`:**
   - Se o foco atual estiver dentro de uma superfície xterm em modo `passthrough` ou `alt-screen` (TUI ativa como `vim`, `htop`, `claude`, `opencode`, `agy`, `codex`), **não intercepta** (preserva o atalho nativo da TUI/shell).
   - Se o foco estiver no input rico da Fase 04 e o texto do input rico **não** estiver vazio, **não intercepta** (preserva `kill-line`).
   - Nos demais casos (foco fora do terminal ou no input rico vazio), previne o default e chama `searchStore.show()`.
2. **`Ctrl+Shift+F`:**
   - Se o terminal estiver em `alt-screen` / `passthrough`, **não intercepta**.
   - Se a `TerminalSearchBar.vue` (busca local da Fase 04) já estiver aberta e visível, fecha a barra local e chama `searchStore.show(localSearchQuery)` (escalonamento direto).
   - Caso contrário, mantém o comportamento da Fase 04 (abrir busca local na aba ativa).

### 3.5. Padrões de Componentes Vue 3.6 (`GlobalSearchPalette.vue` e `HighlightedText.vue`)

Todos os SFCs seguem estritamente:
1. `<template>` (tags de abertura em linha única, sem `v-html`).
2. `<script setup lang="ts">` (`baseIndent: 1`, 4 espaços, aspas simples, ponto e vírgula, sem vírgula final).
3. `<style lang="scss" scoped>` (usando tokens `--max-primary-*`, `--max-warning-*`, `--max-danger-*`, `--background-*`).

Em `HighlightedText.vue`:
```vue
<template>
    <span class="highlighted-text">
        <template v-for="(segment, index) in safeSegments" :key="index">
            <mark v-if="segment.matched" class="highlighted-text__mark">{{ segment.text }}</mark>
            <template v-else>{{ segment.text }}</template>
        </template>
    </span>
</template>
```

---

## 4. Plano de Testes e Casos de Borda

- **Debounce exato (`CA-09`):** Com `vi.useFakeTimers()`, simular 10 chamadas `updateQuery` espaçadas por 30 ms. Verificar que `searchGlobal` não é chamado durante os 270 ms de digitação e é chamado exatamente 1 vez 120 ms após a 10ª tecla.
- **Segurança XSS (`CA-14`):** Passar `<img src=x onerror=alert(1)>` tanto em segmentos `matched: true` quanto `matched: false` em `HighlightedText.vue`; confirmar que `wrapper.find('img').exists()` é `false` e `wrapper.text()` contém a string literal.
- **Localização `pt-BR` (`CA-15`):** Verificar todas as strings obrigatórias (`"Buscar em tudo…"`, `"Nenhum resultado para “x”"`, `"Carregar mais"`, `"Blocos de comando"`, `"Histórico de sessões"`, `"Projetos"`, `"Templates"`, `"Grupos de comandos"`, `"Issues"`) e formatação de datas com `Intl.DateTimeFormat('pt-BR')`.
