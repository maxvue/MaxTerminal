# Implementação 04 — Registro de Gatilhos, Sessão de Autocompletar e Barra de Input Rico

> Fase 04: Motor de Terminal Híbrido
> Referências: [`../instructions.md`](../instructions.md) (§2.1 itens 5, 6; §4.1–§4.3; §5.1; §5.2 itens 10, 11) e [`../spec.md`](../spec.md) (§6, §8.1, §8.2, §10, §10.1, §10.2, §11.2, §12, §13, §14, §15.1–§15.2).

---

## 1. Contexto, Objetivo e Escopo

Esta **Implementação 04** entrega a **camada de entrada estruturada estilo Warp** do MaxTerminal: a barra de input rico (`RichInput.vue`), o sistema extensível de gatilhos (`TriggerRegistry`) para `>`, `>>`, `@`, `?`, `/` (e futuros prefixos), a máquina de estados assíncrona de autocompletar (`CompletionSession`) e o popup acessível (`CompletionPopup.vue` + `CompletionItem.vue`).

A Fase 04 fornece toda a infraestrutura, regras de precedência e contratos de UI que serão consumidos pelas Fases 12 (`@` e `?`), 14 (`>` e `/`) e 15 (`>>`):
1. **`fuzzyFilter` (`src/terminal/triggers/fuzzy.ts`):** utilitário puro de filtragem e pontuação por subsequência com bônus para match no início da string, após delimitadores (`-`, `:`, `/`, `_`, espaço) e caracteres consecutivos.
2. **`TriggerRegistry` (`src/terminal/triggers/triggerRegistry.ts`):** registro extensível (`triggerRegistry` singleton + `createTriggerRegistry()` factory para testes) com validação de prefixo, detecção de conflitos (`TriggerConflictError`), **precedência estrita pelo maior prefixo registrado** (`>>` vence `>`) restrita à posição `0` da linha, suporte a escape literal (`\>` ou `\?` suprimem o gatilho preservando a barra para o shell) e regra de não-recaimento (se `>>` estiver indisponível ou recusar a query, **não** recai em `>`).
3. **`CompletionSession` (`src/terminal/triggers/completionSession.ts`):** máquina de estados determinística (`idle` → `querying` → `showing` / `error`) com:
   - Controle de geração (`generation`) invalidando respostas assíncronas obsoletas de `isAvailable`, `provide`, `refreshAfter` e `onSelect`.
   - `AbortController` abortando a consulta anterior a cada digitação (com debounce configurável por gatilho, default `80 ms`) e timeout de `5000 ms` (`"Tempo esgotado ao carregar sugestões"`).
   - Suporte a fluxos multi-etapa (`push-stage` avançando `stage + 1` e `Backspace` em query vazia com `stage > 0` executando `pop-stage`).
   - Ordenação por `section.order` e fixação de itens `pinned: 'bottom'` sempre ao final (mesmo quando a busca não retorna itens regulares, ex.: `"Criar comando"`, `"Criar grupo de comandos local"`).
   - Suporte a `CompletionResult.passthrough === true` (volta a `idle` sem abrir popup) e `CompletionResult.refreshAfter` (reconsulta o provider automaticamente quando o catálogo assíncrono terminar de carregar).
   - **Supressão pós-`Escape`:** pressionar `Escape` com o popup aberto suprime o gatilho naquele painel até que a linha fique vazia ou o prefixo inicial seja apagado/alterado (permitindo digitar `> arquivo.txt` sem o popup reabrir a cada tecla).
   - Proteção single-flight em `onSelect`.
4. **Composables e Componentes (`useCompletion`, `useRichInput`, `CompletionItem.vue`, `CompletionPopup.vue`, `RichInput.vue`):**
   - `CompletionPopup.vue` e `CompletionItem.vue` com acessibilidade WAI-ARIA (`role="listbox"`, `role="option"`, `aria-selected`, `aria-disabled`), navegação completa por teclado (`↑`/`↓` pulando itens `disabled`, `Home`/`End`, `PageUp`/`PageDown`, wrap-around, `Enter`/`Tab`, `Escape`) e **virtualização de lista quando houver mais de 200 itens**.
   - `RichInput.vue` com `role="combobox"`, `aria-expanded`, `aria-controls`, `aria-activedescendant`, `textarea` auto-expansível até 8 linhas, histórico local em memória (até 200 comandos sem duplicatas consecutivas navegável com `↑`/`↓` quando na primeira/última linha e popup fechado), `Ctrl+C` em campo vazio enviando `\x03` ao PTY, colagem editando apenas o rascunho local e submissão via `InputPipeline.sendLine`.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Código-Fonte Frontend (`src/`)
- `src/terminal/triggers/fuzzy.ts` — algoritmo `fuzzyFilter` e `fuzzyScore`.
- `src/terminal/triggers/triggerRegistry.ts` — classe `TriggerRegistry`, `createTriggerRegistry` e singleton `triggerRegistry`.
- `src/terminal/triggers/completionSession.ts` — máquina de estados `CompletionSession`.
- `src/composables/terminal/useCompletion.ts` — composable reativo sobre `CompletionSession`.
- `src/composables/terminal/useRichInput.ts` — composable de estado do input rico, histórico local de 200 entradas, integração com `useCompletion` e envio ao `InputPipeline`.
- `src/components/terminal/CompletionItem.vue` — item individual do popup de autocompletar.
- `src/components/terminal/CompletionPopup.vue` — popup de autocompletar com seções, breadcrumb de etapa (`stageLabel`), itens fixos no rodapé e virtualização (> 200 itens).
- `src/components/terminal/RichInput.vue` — barra de input rico estilo Warp.

### 2.2. Testes Unitários e de Componentes (`tests/terminal/`)
- `tests/terminal/fuzzy.spec.ts` — testes de correspondência por subsequência, bônus de início de palavra e ordenação estável.
- `tests/terminal/triggerRegistry.spec.ts` — testes de precedência `>>` vs `>`, posição 0, `TriggerConflictError`, `dispose()` e escape `\>` / `\?`.
- `tests/terminal/completionSession.spec.ts` — testes de `AbortSignal`, `push-stage`/`pop-stage`, `pinned: 'bottom'`, `passthrough: true`, `refreshAfter`, timeout de 5 s, supressão pós-`Escape` e single-flight em `onSelect`.
- `tests/terminal/CompletionPopup.spec.ts` — testes de renderização de seções, itens `pinned`, estados `loading`/`error`/`empty`, virtualização > 200 itens e emissões de clique/hover.
- `tests/terminal/RichInput.spec.ts` — testes de teclado (`↑`/`↓`, `Enter`, `Tab`, `Escape`, `Home`/`End`, wrap-around), `aria-activedescendant`, envio de `texto + '\r'` ao PTY, bloqueio de IME (`isComposing`), `Ctrl+C` vazio enviando `\x03` e navegação no histórico local.

---

## 3. Especificação Técnica Detalhada e Assinaturas

### 3.1. Utilitário Fuzzy (`src/terminal/triggers/fuzzy.ts`)

```ts
export interface FuzzyMatchResult<T> {
    item: T;
    score: number;
    matchedIndices: number[];
}

export function fuzzyScore(text: string, query: string): { score: number; matchedIndices: number[] } | null;

export function fuzzyFilter<T>(
    items: readonly T[],
    query: string,
    key?: keyof T | ((item: T) => string)
): T[];
```
- Se `query.trim() === ''`, `fuzzyFilter` retorna uma cópia superficial de `items` mantendo a ordem original.
- Comparação case-insensitive com normalização de acentos opcional, priorizando matches exatos de prefixo, seguidos por início de palavra (`-`, `:`, `/`, `_`, espaço) e contiguidade.

---

### 3.2. `TriggerRegistry` (`src/terminal/triggers/triggerRegistry.ts`)

```ts
import type { Disposable, TriggerDefinition } from '@/types/terminal';

export interface TriggerMatch {
    def: TriggerDefinition;
    query: string;
}

export class TriggerRegistry {
    public static readonly MAX_PREFIX_LENGTH = 32;

    public register(def: TriggerDefinition): Disposable;
    public list(): readonly TriggerDefinition[];
    public getById(id: string): TriggerDefinition | undefined;
    public match(input: string): TriggerMatch | null;
    public isEscapedTriggerLiteral(input: string): boolean;
    public onChange(cb: () => void): Disposable;
    public clearForTests(): void;
}

export function createTriggerRegistry(): TriggerRegistry;
export const triggerRegistry: TriggerRegistry;
```

**Regras de Negócio do `TriggerRegistry`:**
1. **Validação no `register(def)`:**
   - `def.prefix` deve ser string não vazia (`length >= 1 && length <= 32`) e **não** pode conter espaços em branco ou caracteres de controle (`/\s|[\x00-\x1f\x7f]/`). Caso inválido, lança `MtTerminalError('trigger.invalid_prefix', ...)`.
   - Se já existir um gatilho registrado com o mesmo `def.prefix` ou mesmo `def.id`, lança `new TriggerConflictError(def.prefix)`.
2. **Ordenação por Comprimento Decrescente (`prefix.length desc`):**
   - Internamente, a lista é ordenada por `b.prefix.length - a.prefix.length` (mantendo ordem de inserção para prefixos de mesmo tamanho). Assim, `>>` (tamanho 2) é sempre testado antes de `>` (tamanho 1).
3. **Regra de Match na Posição 0 e Escape Literal (`\>` / `\?`):**
   - Se `input.startsWith('\\')` e `input.slice(1)` começar com qualquer prefixo registrado, `match(input)` retorna `null` (`isEscapedTriggerLiteral(input) === true`). A barra invertida **não** é removida do input pelo registry; o texto segue literal para o shell.
   - `match(input)` verifica apenas `input.startsWith(def.prefix)` (posição `0` exata; espaços antes do prefixo desativam o match).
   - Ao encontrar o **maior prefixo registrado** que começa em `0`, retorna `{ def, query: input.slice(def.prefix.length) }` imediatamente.
   - **Proibição de recaimento (`>>` → `>`):** a decisão de qual prefixo casou é puramente léxica pelo maior prefixo registrado. Se `match('>>foo')` retornar o gatilho `>>` e, posteriormente, `def.isAvailable(ctx)` retornar `false` (ou `provide` retornar `passthrough: true`), a `CompletionSession` trata a linha como entrada literal e **jamais** consulta o gatilho `>`.

---

### 3.3. `CompletionSession` (`src/terminal/triggers/completionSession.ts`)

```ts
import type { PaneId } from '@/types/bindings';
import type {
    CompletionItem,
    CompletionSection,
    Disposable,
    TerminalContext,
    TriggerDefinition,
    TriggerQuery,
    TriggerSelectOutcome
} from '@/types/terminal';
import type { TriggerRegistry } from './triggerRegistry';

export type CompletionSessionStatus = 'idle' | 'querying' | 'showing' | 'error';

export interface CompletionSessionSnapshot {
    status: CompletionSessionStatus;
    open: boolean;                   // status === 'querying' || status === 'showing' || status === 'error'
    loading: boolean;                // status === 'querying'
    trigger: TriggerDefinition | null;
    stage: number;
    stageLabel: string | undefined;
    items: CompletionItem[];         // já ordenados (seções + pinned: 'bottom' ao final)
    sections: CompletionSection[];
    activeIndex: number;             // índice em items (-1 se nenhum item selecionável)
    error: string | null;
    emptyMessage: string;
    selecting: boolean;              // true enquanto onSelect assíncrono está em voo
}

export interface CompletionSessionOptions {
    paneId: PaneId;
    registry?: TriggerRegistry;
    getContext: () => TerminalContext;
    providerTimeoutMs?: number;      // default: 5000
    onStateChange?: (snapshot: CompletionSessionSnapshot) => void;
}

export class CompletionSession implements Disposable {
    public static readonly DEFAULT_DEBOUNCE_MS = 80;
    public static readonly DEFAULT_TIMEOUT_MS = 5000;

    public constructor(opts: CompletionSessionOptions);

    public updateInput(input: string, cursor: number): void;
    public moveSelection(delta: number): void;
    public setSelectionIndex(index: number): void;
    public selectFirst(): void;
    public selectLast(): void;
    public handleBackspaceOnEmptyQuery(): boolean; // retorna true se fez pop-stage
    public confirmSelection(): Promise<TriggerSelectOutcome | null>;
    public submitWithoutSelection(): Promise<TriggerSelectOutcome | null>;
    public suppressCurrentTrigger(): void;         // chamado no Escape
    public cancel(): void;                         // reseta para idle (ex.: modo passthrough)
    public getSnapshot(): CompletionSessionSnapshot;
    public dispose(): void;
}
```

**Regras de Negócio da `CompletionSession`:**
1. **Ordenação de Itens e `pinned: 'bottom'`:**
   - Separa `regularItems = items.filter(i => i.pinned !== 'bottom')` e `pinnedItems = items.filter(i => i.pinned === 'bottom')`.
   - Se `sections` for fornecido, ordena as seções por `section.order asc` e agrupa `regularItems` na ordem das seções preservando a ordem relativa do provider dentro de cada seção.
   - Concatena `[...orderedRegularItems, ...pinnedItems]`. Mesmo que `orderedRegularItems` esteja vazio, `pinnedItems` continuam visíveis e selecionáveis no final da lista.
2. **Navegação Pulando `disabled: true`:**
   - `activeIndex` aponta inicialmente para o primeiro item com `!item.disabled` (ou `-1` se todos estiverem desabilitados / lista vazia).
   - `moveSelection(delta)` avança/retrocede com wrap-around circulando apenas entre itens com `!item.disabled`.
3. **Multi-etapa (`push-stage` e `pop-stage`):**
   - Mantém uma pilha `stageHistory: Array<{ stageState: unknown; query: string }>`.
   - Quando `onSelect` retorna `{ kind: 'push-stage', stageState, initialQuery }`: empilha o estado atual, incrementa `stage += 1`, define `currentStageState = outcome.stageState` e dispara nova consulta imediata.
   - Quando o usuário pressiona `Backspace` com a `query` da etapa atual vazia e `stage > 0` (`handleBackspaceOnEmptyQuery()`): desempilha a etapa anterior (`stage -= 1`), restaura `stageState` e reexecuta a consulta.
4. **Supressão pós-`Escape` (`suppressCurrentTrigger`):**
   - Ao pressionar `Escape` com o popup aberto, grava `suppressedPrefix = activeTrigger.prefix` e volta ao estado `idle` **sem alterar o texto digitado no input**.
   - Enquanto `input.startsWith(suppressedPrefix)` e `input.length > 0`, novas digitações continuam em `idle` sem reabrir o popup.
   - A supressão só é limpa quando `input === ''` ou quando o início da linha deixar de começar com `suppressedPrefix`.
5. **`refreshAfter` e `passthrough`:**
   - Se `result.passthrough === true`: transita imediatamente para `idle` (`open: false`) sem exibir popup.
   - Se `result.refreshAfter` estiver presente: renderiza os itens atuais imediatamente e anexa `.then(() => ...)` verificando se `generation` e `input` permanecem idênticos; em caso positivo, reexecuta `provide` silenciosamente para atualizar a lista. Erros em `refreshAfter` são ignorados (mantendo a lista já exibida).
6. **Barreira de geração (`generation`) e `Single-Flight` em `onSelect`:**
   - Qualquer mudança de input, modo, contexto, `cancel()`, `suppressCurrentTrigger()` ou `dispose()` incrementa `generation` e chama `abortController.abort()`.
   - Enquanto `confirmSelection()` está aguardando a Promise de `trigger.onSelect(...)`, `selecting = true` impede chamadas concorrentes; se a geração mudar antes da resolução, o `TriggerSelectOutcome` tardio é descartado.

---

### 3.4. Composables e Componentes (`useCompletion`, `useRichInput`, `CompletionItem.vue`, `CompletionPopup.vue`, `RichInput.vue`)

#### `src/composables/terminal/useCompletion.ts`
```ts
import type { ComputedRef, Ref } from 'vue';
import type { PaneId } from '@/types/bindings';
import type {
    CompletionItem,
    CompletionSection,
    TriggerSelectOutcome
} from '@/types/terminal';

export interface UseCompletionReturn {
    open: ComputedRef<boolean>;
    loading: ComputedRef<boolean>;
    selecting: ComputedRef<boolean>;
    title: ComputedRef<string>;
    stage: ComputedRef<number>;
    stageLabel: ComputedRef<string | undefined>;
    items: ComputedRef<CompletionItem[]>;
    sections: ComputedRef<CompletionSection[]>;
    activeIndex: ComputedRef<number>;
    activeItem: ComputedRef<CompletionItem | null>;
    error: ComputedRef<string | null>;
    emptyMessage: ComputedRef<string>;
    move: (delta: number) => void;
    setActiveIndex: (index: number) => void;
    selectFirst: () => void;
    selectLast: () => void;
    handleBackspace: () => boolean;
    confirm: () => Promise<TriggerSelectOutcome | null>;
    submitWithoutSelection: () => Promise<TriggerSelectOutcome | null>;
    suppress: () => void;
    cancel: () => void;
}

export function useCompletion(
    paneId: Ref<PaneId> | PaneId,
    textRef: Ref<string>,
    cursorRef: Ref<number>
): UseCompletionReturn;
```

#### `src/composables/terminal/useRichInput.ts`
```ts
import type { ComputedRef, Ref } from 'vue';
import type { PaneId } from '@/types/bindings';
import type { UseCompletionReturn } from './useCompletion';

export interface UseRichInputReturn {
    text: Ref<string>;
    cursor: Ref<number>;
    history: Readonly<Ref<readonly string[]>>;
    historyIndex: Readonly<Ref<number>>;
    completion: UseCompletionReturn;
    activeDescendantId: ComputedRef<string | undefined>;
    listboxId: string;
    onKeydown: (event: KeyboardEvent) => Promise<void>;
    onPaste: (event: ClipboardEvent) => void;
    submit: () => Promise<void>;
    selectCompletionItem: (item: import('@/types/terminal').CompletionItem) => Promise<void>;
}

export function useRichInput(paneId: Ref<PaneId> | PaneId): UseRichInputReturn;
```
- **Histórico Local:** armazena até `200` entradas submetidas no painel, sem duplicar entradas consecutivas idênticas e ignorando strings em branco. Quando o popup está fechado, `ArrowUp` na primeira linha do textarea recupera o comando anterior (salvando o rascunho atual) e `ArrowDown` na última linha avança até restaurar o rascunho.
- **`Ctrl+C` com campo vazio:** quando `text.value === ''` e o usuário pressiona `Ctrl+C` sem seleção, envia `'\x03'` ao PTY via `instance.sendInput('\x03')`.
- **Aplicação de `TriggerSelectOutcome`:**
  - `replace-input`: substitui `text.value = outcome.text`, posiciona `cursor.value = outcome.cursor ?? outcome.text.length` e, se `!outcome.keepOpen`, fecha o popup.
  - `submit`: envia `outcome.text` ao PTY via `instance.sendLine(outcome.text)`, registra no histórico local, limpa `text.value = ''` e fecha o popup.
  - `push-stage`: limpa a query da nova etapa mantendo o prefixo (`text.value = trigger.prefix + (outcome.initialQuery ?? '')`).
  - `handled` / `close`: fecha o popup (no `handled`, limpa o campo caso a ação tenha assumido o fluxo, como abrir modal).

#### Componentes Vue (`src/components/terminal/`)
1. **`CompletionItem.vue`:**
   - Props: `{ item: CompletionItem; active: boolean; id: string }`.
   - Emits: `{ select: [item: CompletionItem]; hover: [] }`.
   - Atributos: `role="option"`, `:id="id"`, `:aria-selected="active"`, `:aria-disabled="Boolean(item.disabled)"`. Renderiza ícone (`MaxIcon` quando `item.icon` presente), `item.label`, `item.description` e badge/detalhe `item.detail`.
2. **`CompletionPopup.vue`:**
   - Props: `{ listboxId?: string; items: CompletionItem[]; sections?: CompletionSection[]; activeIndex: number; loading: boolean; error: string | null; title: string; stageLabel?: string; emptyMessage?: string }`.
   - Emits: `{ select: [item: CompletionItem]; hover: [index: number] }`.
   - Slot `item` com `{ item, active }`.
   - `role="listbox"`, cabeçalho exibindo `title` e breadcrumb `stageLabel` (quando presente).
   - **Virtualização acima de 200 itens:** quando `items.length > 200`, renderiza janela virtualizada baseada em `scrollTop` e altura fixa de linha (`32px`), mantendo o item `activeIndex` visível ao navegar com setas/PageUp/PageDown/Home/End.
3. **`RichInput.vue`:**
   - Props: `{ paneId: PaneId; disabled?: boolean }`.
   - Emits: `{ submit: [text: string]; 'mode-toggle': [] }`.
   - Textarea com `role="combobox"`, `:aria-expanded="completion.open.value"`, `:aria-controls="listboxId"`, `:aria-activedescendant="activeDescendantId"`, auto-dimensionável de 1 até 8 linhas, acompanhada do `CompletionPopup.vue` ancorado acima da barra e do `InputModeToggle.vue`.
   - Todas as tags de abertura em linha única, 4 espaços, SCSS scoped com tokens `--max-*` e `--background-*`.
