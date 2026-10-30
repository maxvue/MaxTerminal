# Implementação 03 — Store do Terminal, Modos de Input, Host Xterm e Busca

> Fase 04: Motor de Terminal Híbrido
> Referências: [`../instructions.md`](../instructions.md) (§2.1 itens 1, 5, 10; §4.1–§4.3; §5.1) e [`../spec.md`](../spec.md) (§8, §9.1, §10, §10.1, §11.3, §11.4, §13, §15.1–§15.2).

---

## 1. Contexto, Objetivo e Escopo

Com o motor `TerminalInstance` e o `TerminalInstanceRegistry` prontos na Implementação 02, esta **Implementação 03** conecta o engine ao estado reativo serializável do Vue/Pinia e entrega os componentes de apresentação do emulador base:
1. **`InputModeResolver` (`src/terminal/input/inputModeResolver.ts`):** resolve deterministicamente o modo de entrada (`'rich'` vs. `'passthrough'`) de cada painel segundo a hierarquia de precedência obrigatória:
   - `altScreen === true` ou `mouseTracking !== 'none'` ⇒ **`'passthrough'` forçado** (acima de qualquer override manual ou dica de extensão).
   - Transição de `altScreen` (`false ↔ true`) limpa automaticamente o `inputModeOverride` do painel para não prender o usuário em um modo manual após fechar um TUI (`vim`, `htop`, `claude`, `agy`).
   - Fora de `altScreen`/`mouseTracking`, `inputModeOverride` manual vence dicas registradas (`InputModeHintProvider`).
   - Entre dicas registradas via `registerInputModeHint(provider)`, vence a de maior `priority` (em empate, a última registrada).
   - Fallback padrão: `'rich'` quando `preferences.richInputEnabled === true`, caso contrário `'passthrough'`.
2. **`useTerminalStore` (`src/stores/terminal.ts`):** store Pinia contendo exclusivamente estado serializável por `PaneId` (`TerminalInstanceState`), preferências globais em memória (`TerminalPreferences`), configuração de fluxo (`FlowConfig`), getters (`instance`, `isPassthrough`, `activeInstance`, `webglCount`) e actions (`register`, `unregister`, `patch`, `setInputModeOverride`, `toggleInputMode`, `setPreferences`, `openSearch`, `closeSearch`, `setActive`).
3. **Composables do Emulador e Busca:**
   - `src/composables/terminal/useTerminal.ts`: gerencia a ligação entre o ciclo de vida do componente Vue (`onMounted` → `attach`, `onBeforeUnmount` → `detach` sem `dispose`, `watch(visible)` → `setVisible`) e a `TerminalInstance` (envolvida em `markRaw`).
   - `src/composables/terminal/useTerminalSearch.ts`: integra o `@xterm/addon-search` (`findNext`, `findPrevious`, `onDidChangeResults`, `clearDecorations`), resolvendo tokens CSS `--max-warning-*` / `--max-primary-*` para cores hexadecimais `#RRGGBB` exigidas pelas decorações do `SearchAddon`, tratando regex inválida sem lançar exceção e convertendo índice 0-based para 1-based (`n de m` ou `"Limite de destaques atingido"`).
4. **Componentes Vue Base (`src/components/terminal/`):**
   - `TerminalView.vue` — host DOM do `@xterm/xterm` com `role="application"`, `aria-label="Terminal"`, `contain: strict` e import único de `@xterm/xterm/css/xterm.css`.
   - `TerminalSearchBar.vue` — barra flutuante de busca incremental com toggles `"Aa"` (caseSensitive), `"Palavra"` (wholeWord), `".*"` (regex), navegação `↑`/`↓` (`Enter`/`Shift+Enter`), `Escape` devolvendo foco ao terminal e contador `"n de m"` / `"Sem resultados"`.
   - `TerminalExitBanner.vue` — banner exibido após drenagem completa do stream (`End`), informando `"Processo encerrado (código X)"` ou `"Processo encerrado pelo sinal SIGKILL"` e botão `"Reiniciar"` (`@restart-requested`).
   - `InputModeToggle.vue` — botão indicador/alternador de modo (`"Input rico"` vs. `"Modo direto (raw)"`) com desabilitação contextual quando um TUI em `altScreen`/`mouseTracking` força modo direto.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Código-Fonte Frontend (`src/`)
- `src/terminal/input/inputModeResolver.ts` — resolução de modo de entrada e registro `registerInputModeHint`.
- `src/stores/terminal.ts` — store Pinia `useTerminalStore`.
- `src/composables/terminal/useTerminal.ts` — composable de montagem, foco, colagem e sincronização entre `TerminalInstance` e `useTerminalStore`.
- `src/composables/terminal/useTerminalSearch.ts` — composable de busca com `@xterm/addon-search`.
- `src/components/terminal/TerminalView.vue` — componente host do `@xterm/xterm`.
- `src/components/terminal/TerminalSearchBar.vue` — barra de busca no scrollback.
- `src/components/terminal/TerminalExitBanner.vue` — banner de término de processo.
- `src/components/terminal/InputModeToggle.vue` — alternador visual de modo de entrada (`rich` / `passthrough`).

### 2.2. Testes Unitários e de Componentes (`tests/terminal/`)
- `tests/terminal/inputModeResolver.spec.ts` — testes de precedência de modo, forçamento em `altScreen`/`mouseTracking`, limpeza de override e prioridade de `InputModeHintProvider`.
- `tests/terminal/terminalStore.spec.ts` — testes de estado inicial, `patch`, `setInputModeOverride`, `toggleInputMode`, propagação de `setPreferences` e `webglCount`.
- `tests/terminal/TerminalSearchBar.spec.ts` — testes de componente (`@vue/test-utils`): digitação, toggles `Aa`/`Palavra`/`.*`, teclas `Enter`/`Shift+Enter`/`Escape`, contador `n de m` e regex inválida.
- `tests/terminal/TerminalExitBanner.spec.ts` — testes de componente: exibição por `code`, por `signalName` e emissão de `restart-requested`.

---

## 3. Especificação Técnica Detalhada e Assinaturas

### 3.1. `InputModeResolver` (`src/terminal/input/inputModeResolver.ts`)

```ts
import type {
    Disposable,
    InputMode,
    InputModeHint,
    InputModeHintProvider,
    TerminalContext,
    TerminalModes
} from '@/types/terminal';

export interface ResolveInputModeParams {
    context: TerminalContext;
    modes: TerminalModes;
    override: InputMode | null;
    richInputEnabled: boolean;
}

export interface InputModeResolution {
    mode: InputMode;
    forcedByTui: boolean;
    source: 'tui' | 'override' | 'hint' | 'default';
    reason: string;
}

export function registerInputModeHint(provider: InputModeHintProvider): Disposable;
export function resolveInputMode(params: ResolveInputModeParams): InputModeResolution;
export function shouldClearOverrideOnModesChange(
    prevModes: TerminalModes,
    nextModes: TerminalModes
): boolean;
export function clearInputModeHintsForTests(): void;
```

**Regras de Negócio:**
1. Se `params.modes.altScreen === true` ou `params.modes.mouseTracking !== 'none'`, retorna imediatamente `{ mode: 'passthrough', forcedByTui: true, source: 'tui', reason: 'Modo direto ativado automaticamente para aplicação interativa (TUI)' }`.
2. `shouldClearOverrideOnModesChange(prev, next)` retorna `true` sempre que `prev.altScreen !== next.altScreen` (entrar ou sair de alternate screen limpa `inputModeOverride`).
3. Se não forçado por TUI e `params.override !== null`, retorna `{ mode: params.override, forcedByTui: false, source: 'override', reason: 'Modo selecionado manualmente pelo usuário' }`.
4. Caso contrário, avalia todos os `InputModeHintProvider` registrados na ordem de registro. Ignora retornos `null`. Escolhe o `InputModeHint` com maior `priority`; em caso de empate (`hint.priority === best.priority`), o **último registrado** vence (`>=`).
5. Se nenhum provider retornar dica, retorna `{ mode: params.richInputEnabled ? 'rich' : 'passthrough', forcedByTui: false, source: 'default', reason: 'Configuração padrão do terminal' }`.

---

### 3.2. `useTerminalStore` (`src/stores/terminal.ts`)

```ts
import { defineStore } from 'pinia';
import type { PaneId, PtyId } from '@/types/bindings';
import type {
    FlowConfig,
    InputMode,
    TerminalInstanceState,
    TerminalPreferences
} from '@/types/terminal';

export interface TerminalStoreState {
    instances: Record<PaneId, TerminalInstanceState>;
    preferences: TerminalPreferences;
    flowConfig: FlowConfig;
    activePaneId: PaneId | null;
}

export const useTerminalStore = defineStore('terminal', {
    // state, getters e actions conforme spec.md §9.1
});
```

**Assinaturas exatas de Getters e Actions:**
- **Getters:**
  - `instance: (state) => (paneId: PaneId): TerminalInstanceState | undefined`
  - `isPassthrough: (state) => (paneId: PaneId): boolean` (`state.instances[paneId]?.inputMode === 'passthrough'`)
  - `activeInstance: (state): TerminalInstanceState | null`
  - `webglCount: (state): number` (quantidade de instâncias em `state.instances` com `renderer === 'webgl' && visible === true`)
- **Actions:**
  - `register(paneId: PaneId, ptyId: PtyId): TerminalInstanceState` — cria o estado inicial se não existir (ou atualiza se mesmo `ptyId`), calculando `inputMode` inicial via `resolveInputMode` e `themeUi` inicial a partir de `themeService.getActiveTheme().ui`.
  - `unregister(paneId: PaneId): void` — remove `instances[paneId]` e limpa `activePaneId` se era o painel removido.
  - `patch(paneId: PaneId, partial: Partial<TerminalInstanceState>): void` — mescla alterações parciais; se `modes` ou `inputModeOverride` mudarem, recalcula `inputMode` automaticamente via `resolveInputMode` (e limpa `inputModeOverride` se `shouldClearOverrideOnModesChange(prev.modes, nextModes)` for `true`).
  - `setInputModeOverride(paneId: PaneId, mode: InputMode | null): void` — se a instância estiver em `altScreen` ou `mouseTracking !== 'none'`, ignora a tentativa de forçar `'rich'` (permanece `'passthrough'`); caso contrário define `inputModeOverride = mode` e recalcula `inputMode`.
  - `toggleInputMode(paneId: PaneId): void` — alterna entre `'rich'` e `'passthrough'` via `setInputModeOverride`.
  - `setPreferences(partial: Partial<TerminalPreferences>): void` — atualiza `this.preferences` (mantendo `macOptionIsMeta: false`), recalcula `inputMode` de todas as instâncias registradas e propaga `instance.applyPreferences(this.preferences)` para todas as instâncias no `terminalInstanceRegistry`.
  - `openSearch(paneId: PaneId): void` e `closeSearch(paneId: PaneId): void` — atualiza `instances[paneId].search.open`.
  - `setActive(paneId: PaneId | null): void` — define `activePaneId` e sincroniza `focused`.

---

### 3.3. Composables (`src/composables/terminal/useTerminal.ts` e `useTerminalSearch.ts`)

#### `useTerminal.ts`
```ts
import type { ComputedRef, Ref, ShallowRef } from 'vue';
import type { ExitInfo, PaneId, PtyId } from '@/types/bindings';
import type {
    TerminalInstanceHandle,
    TerminalInstanceState,
    TerminalModes
} from '@/types/terminal';

export interface UseTerminalCallbacks {
    onReady?: (handle: TerminalInstanceHandle) => void;
    onExit?: (exit: ExitInfo) => void;
    onTitleChange?: (title: string) => void;
    onModesChange?: (modes: TerminalModes) => void;
}

export interface UseTerminalReturn {
    handle: ShallowRef<TerminalInstanceHandle | null>;
    state: ComputedRef<TerminalInstanceState | undefined>;
    focus: () => void;
    paste: (text: string) => void;
}

export function useTerminal(
    paneId: Ref<PaneId> | PaneId,
    ptyId: Ref<PtyId> | PtyId,
    hostRef: Ref<HTMLElement | null>,
    visibleRef?: Ref<boolean>,
    callbacks?: UseTerminalCallbacks
): UseTerminalReturn;
```
- No `onMounted`: registra o painel em `useTerminalStore`, obtém ou cria a `TerminalInstance` no `terminalInstanceRegistry` usando `markRaw`, conecta callbacks que sincronizam `store.patch(paneId, ...)` (incluindo `onThemeChange` → `store.patch(paneId, { themeUi: theme.ui })`) e chama `await instance.attach(hostRef.value)`.
- No `onBeforeUnmount`: chama `instance.detach()` (nunca `dispose()`, preservando a instância na troca de abas).
- Observa `visibleRef` com `watch` e chama `instance.setVisible(visible)` + `store.patch(paneId, { visible })`.

#### `useTerminalSearch.ts`
```ts
import type { ComputedRef, Ref } from 'vue';
import type { PaneId } from '@/types/bindings';

export interface SearchOptionsState {
    caseSensitive: boolean;
    wholeWord: boolean;
    regex: boolean;
}

export interface UseTerminalSearchReturn {
    query: Ref<string>;
    options: Ref<SearchOptionsState>;
    resultIndex: ComputedRef<number>; // 1-based quando há seleção ativa; 0 quando nenhum
    resultCount: ComputedRef<number>;
    limitExceeded: ComputedRef<boolean>;
    regexError: ComputedRef<string | null>;
    next: () => boolean;
    prev: () => boolean;
    close: () => void;
}

export function useTerminalSearch(paneId: Ref<PaneId> | PaneId): UseTerminalSearchReturn;
```
- Antes de invocar `searchAddon.findNext` / `findPrevious`, se `options.value.regex === true` e `query.value.length > 0`, valida a expressão com `new RegExp(query.value)` dentro de `try/catch`; se inválida, preenche `regexError` (`"Expressão regular inválida"`) sem chamar o addon e sem derrubar o terminal.
- Passa opções `decorations` com cores `#RRGGBB` resolvidas dos tokens CSS (`matchBackground`, `matchBorder`, `matchOverviewRuler`, `activeMatchBackground`, `activeMatchBorder`, `activeMatchColorOverviewRuler`) para habilitar o disparo do evento `searchAddon.onDidChangeResults`.
- No `close()`: chama `searchAddon.clearDecorations()`, fecha a busca no `useTerminalStore.closeSearch(paneId)` e devolve o foco ao terminal (`instance.focus()`).

---

### 3.4. Componentes Vue (`src/components/terminal/`)

Todos seguem rigorosamente as diretrizes do `AGENTS.md`:
- Ordem `<template>` → `<script setup lang="ts">` → `<style lang="scss" scoped>`.
- Recuo de 4 espaços (`baseIndent: 1` no `<script setup>`), aspas simples, ponto e vírgula obrigatório, sem vírgula final, **tags de abertura em linha única**.

1. **`TerminalView.vue`:**
   - Props: `{ paneId: PaneId; ptyId: PtyId; visible?: boolean }` (default `visible: true`).
   - Emits: `{ ready: [handle: TerminalInstanceHandle]; exit: [exit: ExitInfo]; 'title-change': [title: string]; 'modes-change': [modes: TerminalModes] }`.
   - Importa `@xterm/xterm/css/xterm.css`.
   - Container raiz com `role="application"`, `aria-label="Terminal"`, `tabindex="-1"`, estilo `.terminal-view { position: relative; width: 100%; height: 100%; overflow: hidden; contain: strict; }` e clique focando o xterm (`@mousedown="onHostMouseDown"`).
2. **`TerminalSearchBar.vue`:**
   - Props: `{ paneId: PaneId }`.
   - Emits: `{ close: [] }`.
   - Contém campo de texto de busca (auto-focado ao abrir), 3 botões de toggle com `aria-pressed` (`"Aa"` com tooltip `"Diferenciar maiúsculas/minúsculas"`, `"Palavra"` com tooltip `"Palavra inteira"`, `".*"` com tooltip `"Expressão regular"`), contador (`"{{ resultIndex }} de {{ resultCount }}"`, `"Sem resultados"`, `"Limite de destaques atingido"` ou mensagem de `regexError`), botões `"Resultado anterior"` (`↑`), `"Próximo resultado"` (`↓`) e `"Fechar busca"` (`Escape`).
3. **`TerminalExitBanner.vue`:**
   - Props: `{ exit: ExitInfo }`.
   - Emits: `{ 'restart-requested': [] }`.
   - Exibe mensagem clara em pt-BR:
     - Se `exit.signalName`: `"Processo encerrado pelo sinal {{ exit.signalName }}"`
     - Senão se `exit.signal !== null`: `"Processo encerrado pelo sinal {{ exit.signal }}"`
     - Senão: `"Processo encerrado (código {{ exit.code ?? 0 }})"`
   - Botão `"Reiniciar"` emitindo `restart-requested`.
4. **`InputModeToggle.vue`:**
   - Props: `{ paneId: PaneId }`.
   - Alterna `terminalStore.toggleInputMode(paneId)` quando não estiver bloqueado por TUI (`altScreen` ou `mouseTracking !== 'none'`), exibindo rótulo/tooltip `"Input rico"` ou `"Modo direto (raw)"`.

---

## 4. Casos de Borda e Segurança

- **Preservação de foco ao fechar busca ou sair de TUI:** fechar `TerminalSearchBar` com `Escape` devolve o foco ao painel ativo; sair de `altScreen` limpa `inputModeOverride` e restaura o modo padrão sem roubar foco se uma barra de busca ou modal estiver focado.
- **Zero `v-html`:** todas as mensagens de erro de regex, contadores e sinais de saída usam interpolação textual `{{ }}`.
