# Implementação 05 — Camada de Blocos de Comando, Adaptador VueWebTerminal e Ações de Bloco

> Fase 04: Motor de Terminal Híbrido
> Referências: [`../instructions.md`](../instructions.md) (§2.1 itens 7, 11; §4.1–§4.3; §5.1 itens 3, 7; §5.2 item 12) e [`../spec.md`](../spec.md) (§3 ADR-04-01, §6, §8, §9.2, §9.3, §10, §10.1, §12, §13, §14, §15.1–§15.2).

---

## 1. Contexto, Objetivo e Escopo

Esta **Implementação 05** constrói a **Camada de Blocos de Comando (estilo Warp)** sobre o emulador `@xterm/xterm`, integrando `vue-web-terminal` por meio do `VueWebTerminalAdapter` conforme o **ADR-04-01** (`spec.md` §3):
1. **Spike e Implementação do `VueWebTerminalAdapter` (`src/terminal/blocks/vueWebTerminalAdapter.ts`):**
   - Valida e encapsula a API pública do pacote `vue-web-terminal` (`3.x` compatível com Vue 3.6) na camada de apresentação de blocos/mensagens, garantindo que o `@xterm/xterm` permaneça o **único** emulador VT e o **único** consumidor do stream do PTY.
   - O adaptador recebe projeções de `CommandBlockModel` (oriundas do `CommandBlockFeed`) e converte em registros/estruturas de exibição compatíveis com `vue-web-terminal` (texto puro, nunca HTML externo), além de encaminhar intenções de comando para o `InputPipeline` sem abrir PTY próprio, sem registrar outro leitor e sem duplicar eco.
2. **`BlockActionRegistry` (`src/terminal/blocks/blockActionRegistry.ts`):**
   - Registro extensível (`registerBlockAction(action): Disposable`) para ações por bloco (onde a Fase 11 registrará `"Inspecionar"` e a Fase 12 registrará `"Diagnóstico"`).
   - Inclui duas ações nativas da Fase 04:
     - `copy-command` (`"Copiar comando"`): copia `block.command` para a área de transferência.
     - `copy-output` (`"Copiar saída"`): extrai o **texto renderizado ainda retido** no `buffer.normal` do xterm entre os markers válidos do bloco (`translateToString(true)`, concatenando linhas com `isWrapped === true` sem inserir quebra artificial e preservando quebras reais). Caso o início do bloco já tenha sofrido trim/eviction no scrollback, informa ao usuário via Toast que a saída copiada é parcial ou indisponível.
3. **`useCommandBlocksStore` (`src/stores/commandBlocks.ts`) e Ancoragem por `IMarker`:**
   - Store Pinia mantendo apenas dados serializáveis (`byPane[paneId] = { order, blocks }`, limite de `maxBlocksPerPane: 500`) e expondo o objeto `feed: CommandBlockFeed` (`begin`, `markOutputStart`, `end`, `patchMeta`, `clear`) que a Fase 05 alimentará via OSC 133 (e que o playground alimenta via simulador).
   - Registro auxiliar não-reativo de `IMarker` (`src/terminal/blocks/blockMarkerRegistry.ts` ou interno ao módulo): cada bloco cria `IMarker`s reais no `xterm.registerMarker(...)` para `promptMarker`, `outputStartMarker` e `outputEndMarker`. `marker.line` acompanha reflow/trim automaticamente e `marker.onDispose` aciona `pruneEvicted` para remover blocos cujas linhas foram descartadas do buffer normal (sem usar `baseY` como contador de linhas descartadas).
4. **Composable e Componentes Visuais (`useCommandBlocks.ts`, `CommandBlock.vue`, `CommandBlockLayer.vue`):**
   - `useCommandBlocks.ts` **instancia e é o único dono** de um `VueWebTerminalAdapter` por painel (via `createVueWebTerminalAdapter`) e o usa para projetar cada `CommandBlockModel` em `VueWebTerminalProjectedEntry` (`projectedEntries`), de modo que o adaptador integra o pipeline real de renderização de blocos (e não existe como classe órfã). O adaptador é descartado (`dispose()`) quando o composable sai de escopo.
   - `CommandBlock.vue`: cabeçalho visual compacto exibindo comando (truncado a 4096 caracteres, sem `v-html`), `cwd`, duração formatada em `pt-BR`, badge semântico de status (`running` → `--max-success-500`, `success` → `--max-success-500` contorno, `error` → `--max-danger-500`, `aborted` → `--max-warning-500`), botões das ações filtradas por `action.when(block)` e slot `<slot name="actions" :block="block" />`.
   - `CommandBlockLayer.vue`: gerencia `xterm.registerDecoration` para os blocos visíveis no **viewport ± 1 tela** (atualizado via `xterm.onRender`, sem polling), monta `CommandBlock.vue` via `<Teleport>` nos containers das decorações com `pointer-events: none` no overlay e `pointer-events: auto` apenas nos controles interativos, e oculta toda a camada quando `modes.altScreen === true` (preservando os markers no buffer normal).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Código-Fonte Frontend (`src/`)
- `src/terminal/blocks/vueWebTerminalAdapter.ts` — adaptador de apresentação e integração com `vue-web-terminal` conforme ADR-04-01.
- `src/terminal/blocks/blockActionRegistry.ts` — registro de ações de bloco + ações nativas `"Copiar comando"` (`copy-command`) e `"Copiar saída"` (`copy-output`) + helper de extração de texto renderizado do buffer xterm.
- `src/stores/commandBlocks.ts` — store Pinia `useCommandBlocksStore` implementando `CommandBlockFeed` e gerenciando o vínculo com `IMarker` fora do estado reativo.
- `src/composables/terminal/useCommandBlocks.ts` — composable expondo blocos do painel, **entradas projetadas pelo `VueWebTerminalAdapter` (`projectedEntries`)**, ações aplicáveis por bloco e execução protegida com Toast de erro; importa e instancia `createVueWebTerminalAdapter`.
- `src/components/terminal/CommandBlock.vue` — componente visual do cabeçalho e ações de um bloco de comando.
- `src/components/terminal/CommandBlockLayer.vue` — camada de ancoragem sobre o xterm (`registerMarker` + `registerDecoration` + `Teleport`, restrita ao viewport ± 1 tela, oculta em `altScreen`); consome `projectedEntries` de `useCommandBlocks` e repassa cada entrada projetada a `CommandBlock.vue`. É composta por `HybridTerminal.vue` (`../06`).

### 2.2. Testes Unitários, de Integração e de Componentes (`tests/terminal/`)
- `tests/terminal/vueWebTerminalAdapter.spec.ts` — teste de integração real com `vue-web-terminal` validando projeção de blocos do `CommandBlockFeed`, ausência de segundo leitor/PTY e unicidade do pipeline de entrada.
- `tests/terminal/blockActionRegistry.spec.ts` — testes de registro/remoção de ações, filtro `when(block)`, cópia de comando e extração de saída do buffer normal com tratamento de `isWrapped` e detecção de trim parcial.
- `tests/terminal/commandBlocksStore.spec.ts` — testes do `CommandBlockFeed` (`begin`, `markOutputStart`, `end` com `0`/`!=0`/`null`, `patchMeta`, `clear`), idempotência, limite de 500 blocos e descarte via `IMarker.onDispose`.
- `tests/terminal/CommandBlock.spec.ts` — testes de componente (`@vue/test-utils`): renderização de status (`running`/`success`/`error`/`aborted`), duração, filtro de ações do `BlockActionRegistry` e slot `actions` recebendo `{ block }`.
- `tests/terminal/useCommandBlocks.spec.ts` — teste do composable: instancia exatamente 1 `VueWebTerminalAdapter` por painel, `projectedEntries` reflete `begin`/`end` do `CommandBlockFeed` (1 entrada por bloco, na ordem de `order`), `dispose()` do adaptador ao desmontar e ausência de HTML em `content`.

---

## 3. Especificação Técnica Detalhada e Assinaturas

### 3.1. `VueWebTerminalAdapter` (`src/terminal/blocks/vueWebTerminalAdapter.ts`)

```ts
import type { PaneId } from '@/types/bindings';
import type {
    CommandBlockModel,
    Disposable,
    TerminalContext
} from '@/types/terminal';

export interface VueWebTerminalProjectedEntry {
    key: string;
    blockId: CommandBlockModel['id'];
    type: 'normal' | 'ansi';
    class: 'success' | 'error' | 'warning' | 'info' | 'system';
    tag: string;
    content: string;               // sempre texto plano sanitizado para UI (nunca HTML)
    command: string;
    cwd: string | null;
    status: CommandBlockModel['status'];
    exitCode: number | null;
    durationMs: number | null;
}

export interface VueWebTerminalAdapterOptions {
    paneId: PaneId;
    getContext: () => TerminalContext;
    onInputIntent?: (commandText: string) => Promise<void>;
}

export class VueWebTerminalAdapter implements Disposable {
    public constructor(opts: VueWebTerminalAdapterOptions);

    public getLibraryMetadata(): { packageName: 'vue-web-terminal'; available: boolean };
    public projectBlock(block: CommandBlockModel, outputPreview?: string): VueWebTerminalProjectedEntry;
    public projectBlocks(blocks: readonly CommandBlockModel[]): VueWebTerminalProjectedEntry[];
    public handleExecCmd(
        cmdKey: string,
        command: string,
        success?: (msg?: unknown) => void,
        failed?: (msg?: string) => void
    ): Promise<void>;
    public dispose(): void;
}

export function createVueWebTerminalAdapter(
    opts: VueWebTerminalAdapterOptions
): VueWebTerminalAdapter;
```

**Regras de Negócio do `VueWebTerminalAdapter` (ADR-04-01):**
1. Importa os símbolos públicos reais do pacote `vue-web-terminal` instalado (validando sua presença e compatibilidade com Vue 3.6) sem instanciar emulador paralelo de PTY e sem interceptar eventos globais de teclado do xterm.
2. `projectBlock(block, outputPreview)` converte um `CommandBlockModel` para o formato de mensagem estruturada do `vue-web-terminal`, truncando `command` a 4096 caracteres e garantindo que `content` seja texto puro (jamais HTML injetado).
3. `handleExecCmd` captura qualquer intenção de execução disparada pela interface `exec-cmd` da biblioteca, delega para `opts.onInputIntent(command)` (que envia pelo `InputPipeline` único da instância) e resolve o callback `success()` imediatamente sem gerar eco local duplicado.
4. **Integração obrigatória ao pipeline de blocos (sem classe órfã):** o único ponto de instanciação em produção é `useCommandBlocks.ts` (§3.4), que importa `createVueWebTerminalAdapter`, injeta `getContext` (contexto do painel) e `onInputIntent` (delegando ao `InputPipeline` único da instância) e expõe `projectedEntries`; `CommandBlockLayer.vue` consome essas entradas e as entrega a `CommandBlock.vue`. Nenhum outro módulo cria adaptadores. O adaptador não monta o componente visual `<VueWebTerminal>` como emulador: apenas projeta/valida a estrutura de mensagens da biblioteca para a camada de blocos.
5. **Dependência `vue-web-terminal`:** não adicionar `overrides` no `package.json` (a versão `3.4.2` não declara peer de `vue`). Somente se `npm ci` comprovar conflito de peer dependency, registrar a nota correspondente em `implementation/` e então adicionar o `overrides` mínimo necessário.

---

### 3.2. `BlockActionRegistry` e Extração de Saída (`src/terminal/blocks/blockActionRegistry.ts`)

```ts
import type { IBufferLine, IMarker, Terminal } from '@xterm/xterm';
import type {
    BlockAction,
    CommandBlockModel,
    Disposable,
    TerminalContext
} from '@/types/terminal';

export interface BlockMarkersRecord {
    promptMarker: IMarker | null;
    outputStartMarker: IMarker | null;
    outputEndMarker: IMarker | null;
    partialTrimmed: boolean;
}

export interface ExtractedBlockOutput {
    text: string;
    partial: boolean;
    available: boolean;
}

export function extractRenderedBlockOutput(
    xterm: Terminal,
    block: CommandBlockModel,
    markers?: BlockMarkersRecord
): ExtractedBlockOutput;

export function registerBlockAction(action: BlockAction): Disposable;
export function getBlockActionsFor(block: CommandBlockModel): BlockAction[];
export function executeBlockAction(
    actionId: string,
    block: CommandBlockModel,
    ctx: TerminalContext
): Promise<void>;
export function resetBlockActionsForTests(): void;
```

**Regras de Negócio de `extractRenderedBlockOutput` e Ações Nativas:**
1. **`copy-command` (`order: 10`, `icon: 'mdi:content-copy'`, `label: 'Copiar comando'`):**
   - `when(block)`: `block.command.trim().length > 0`.
   - `run(block)`: copia `block.command` via `navigator.clipboard.writeText(block.command)`.
2. **`copy-output` (`order: 20`, `icon: 'mdi:text-box-outline'`, `label: 'Copiar saída'`):**
   - `when(block)`: `block.status !== 'running' && block.outputStartLine !== null`.
   - `extractRenderedBlockOutput(xterm, block, markers)`:
     - Usa `xterm.buffer.normal`.
     - Determina a linha inicial a partir de `markers?.outputStartMarker?.line` (se não descartado e `line >= 0`); se o `outputStartMarker` já foi descartado (`isDisposed` ou `line < 0`), marca `partial = true` e inicia da linha `0` do `buffer.normal` caso `outputEndMarker` ainda seja válido (senão retorna `{ text: '', partial: true, available: false }`).
     - Determina a linha final a partir de `markers?.outputEndMarker?.line` (ou `buffer.normal.baseY + buffer.normal.cursorY` como limite superior).
     - Percorre `i` de `startLine` até `endLine`: lê `line = buffer.normal.getLine(i)`. Se `i > startLine` e `!line.isWrapped`, adiciona `'\n'` antes de concatenar `line.translateToString(true)`. Se `line.isWrapped === true`, concatena diretamente sem `'\n'` (reconstruindo linhas longas que sofreram wrap visual).
   - Se `!available`, exibe `Toast.show({ title: 'A saída deste comando já foi descartada do histórico do terminal.', severity: 'warn' })`. Se `partial`, copia o texto disponível e exibe `Toast.show({ title: 'Saída copiada parcialmente (linhas antigas atingiram o limite do buffer).', severity: 'info' })`.
3. **Tratamento de erro em `executeBlockAction`:**
   - Qualquer exceção lançada por `action.run(block, ctx)` é capturada, encapsulada em `MtTerminalError('block_action.failed', ...)` e exibida via `Toast.show({ title: err.message, severity: 'error' })` conforme `spec.md` §12.

---

### 3.3. `useCommandBlocksStore` (`src/stores/commandBlocks.ts`)

```ts
import { defineStore } from 'pinia';
import type { IMarker } from '@xterm/xterm';
import type { CommandBlockId, PaneId } from '@/types/bindings';
import type {
    CommandBlockFeed,
    CommandBlockModel,
    CommandBlockStatus
} from '@/types/terminal';
import type { BlockMarkersRecord } from '@/terminal/blocks/blockActionRegistry';

export interface PaneBlocksBucket {
    order: CommandBlockId[];
    blocks: Record<CommandBlockId, CommandBlockModel>;
}

export interface CommandBlocksStoreState {
    byPane: Record<PaneId, PaneBlocksBucket>;
    maxBlocksPerPane: number; // default: 500
}

export function getBlockMarkers(
    paneId: PaneId,
    blockId: CommandBlockId
): BlockMarkersRecord | undefined;

export const useCommandBlocksStore = defineStore('commandBlocks', {
    // state, getters (blocksOf, lastBlock, runningBlock) e actions (feed, pruneEvicted)
});
```

**Regras de Negócio da `useCommandBlocksStore` (§9.2 e §9.3):**
1. **`feed.begin(paneId, init)`:**
   - Idempotente por `init.id`: se o bloco já existir no painel, atualiza apenas campos ausentes sem duplicar em `order`.
   - Determina `promptLine`: se `init.line !== undefined`, usa `init.line`; caso contrário, consulta a instância no `terminalInstanceRegistry.get(paneId)` e usa `xterm.buffer.normal.baseY + xterm.buffer.normal.cursorY` (ou `0` se instância não montada).
   - Se a instância estiver em `buffer.active.type === 'normal'`, registra `promptMarker = xterm.registerMarker(offset)` fora do Pinia (em um `Map` de módulo `markersByPane`) e assina `promptMarker.onDispose(() => store.pruneEvictedBlock(paneId, init.id))`.
   - Se `order.length > maxBlocksPerPane` (`500`), remove e descarta os markers dos blocos mais antigos (FIFO).
2. **`feed.markOutputStart(paneId, id, line?)`:**
   - Se o bloco existir, define `outputStartLine` e registra `outputStartMarker` no buffer normal.
3. **`feed.end(paneId, id, result)`:**
   - Se o bloco **não** existir em `byPane[paneId]`, ignora com `console.warn` de diagnóstico (nunca fabrica bloco órfão sem `begin`).
   - Se o bloco já estiver finalizado (`status !== 'running'`), operação é idempotente.
   - Calcula `status`:
     - `result.exitCode === 0` ⇒ `'success'`
     - `result.exitCode !== null && result.exitCode !== 0` ⇒ `'error'`
     - `result.exitCode === null` ⇒ `'aborted'`
   - Define `finishedAt = result.finishedAt`, `exitCode = result.exitCode`, `outputEndLine` e cria `outputEndMarker`.
4. **`feed.patchMeta(paneId, id, meta)`:** mescla `meta` de forma imutável no bloco correspondente.
5. **`feed.clear(paneId)`:** descarta todos os `IMarker`s do painel e zera `{ order: [], blocks: {} }`.
6. **`pruneEvicted(paneId, firstValidLine)`:** remove blocos cujo `promptMarker` foi descartado pelo xterm ou cuja linha ancorada ficou inválida (`< firstValidLine`).

---

### 3.4. Composable e Componentes (`useCommandBlocks.ts`, `CommandBlock.vue`, `CommandBlockLayer.vue`)

1. **`src/composables/terminal/useCommandBlocks.ts`:**
   ```ts
   import type { ComputedRef, Ref } from 'vue';
   import type { CommandBlockId, PaneId } from '@/types/bindings';
   import type { BlockAction, CommandBlockModel } from '@/types/terminal';

   import type {
       VueWebTerminalAdapter,
       VueWebTerminalProjectedEntry
   } from '@/terminal/blocks/vueWebTerminalAdapter';
   import type { TerminalContext } from '@/types/terminal';

   export interface UseCommandBlocksOptions {
       getContext: () => TerminalContext;
       onInputIntent?: (commandText: string) => Promise<void>;
   }

   export interface UseCommandBlocksReturn {
       adapter: VueWebTerminalAdapter;
       blocks: ComputedRef<CommandBlockModel[]>;
       /** Projeção de `blocks` (mesma ordem) feita por `adapter.projectBlocks(...)`. */
       projectedEntries: ComputedRef<VueWebTerminalProjectedEntry[]>;
       runningBlock: ComputedRef<CommandBlockModel | null>;
       lastBlock: ComputedRef<CommandBlockModel | null>;
       actionsFor: (block: CommandBlockModel) => BlockAction[];
       runAction: (actionId: string, block: CommandBlockModel) => Promise<void>;
       clearBlocks: () => void;
       getMarkerLine: (blockId: CommandBlockId) => number;
   }

   export function useCommandBlocks(
       paneId: Ref<PaneId> | PaneId,
       options: UseCommandBlocksOptions
   ): UseCommandBlocksReturn;
   ```
   - Internamente executa `import { createVueWebTerminalAdapter } from '@/terminal/blocks/vueWebTerminalAdapter'`, cria **um** adaptador por chamada (`{ paneId: unref(paneId), getContext, onInputIntent }`), calcula `projectedEntries = computed(() => adapter.projectBlocks(blocks.value))` e chama `adapter.dispose()` em `onScopeDispose`. Se receber `Ref<PaneId>` e o valor mudar, descarta o adaptador anterior antes de criar o novo e invalida a projeção; nunca mantém dois adaptadores ativos para a mesma chamada. Em produção, `CommandBlockLayer` é montado com `:key="paneId"` e é o único consumidor desse composable por painel; `HybridTerminal` apenas fornece as props.
2. **`src/components/terminal/CommandBlock.vue`:**
   - Props: `{ block: CommandBlockModel; entry: VueWebTerminalProjectedEntry; actions: BlockAction[]; compact?: boolean }` (`entry` é a projeção produzida pelo `VueWebTerminalAdapter`; o cabeçalho usa `entry.command` já truncado, `entry.class`/`entry.tag` para o badge semântico e `entry.durationMs`/`entry.exitCode`).
   - Emits: `{ action: [actionId: string] }`.
   - Slot: `<slot name="actions" :block="block" />` renderizado logo após as ações registradas.
   - Exibe:
     - Indicador semântico de status (`running`, `success`, `error`, `aborted`) com as cores exatas de `spec.md` §10.
     - Comando truncado a 4096 caracteres (`{{ entry.command }}`, truncado pelo adaptador).
     - Diretório `block.cwd` (quando presente).
     - Duração formatada em `pt-BR` (ex.: `"120 ms"`, `"2,4 s"`, `"1 min 15 s"` ou `"Em execução..."`).
     - Código de saída (`exit != 0` destacado em vermelho).
   - SCSS scoped: fundo, borda e fundo do cabeçalho vêm das variáveis do painel `var(--mt-block-background, var(--background-900))`, `var(--mt-block-border, var(--background-700))` e `var(--mt-block-header-background, var(--background-800))`, expostas por `HybridTerminal.vue` em `.hybrid-terminal` a partir de `TerminalTheme.ui` (`../02` §3.5 e `../06`); os badges de status continuam nos tokens `--max-*`.
3. **`src/components/terminal/CommandBlockLayer.vue`:**
   - Props: `{ paneId: PaneId; getContext: () => TerminalContext; onInputIntent: (commandText: string) => Promise<void> }`, fornecidas pelo `HybridTerminal` após a instância estar pronta.
   - Obtém `blocks`/`projectedEntries` de `useCommandBlocks(paneId, { getContext, onInputIntent })` (que contém o `VueWebTerminalAdapter`) e passa a `CommandBlock.vue` o par `block` + `entry`. Faz o pareamento por `blockId` e renderiza somente quando ambos existem, sem repassar `undefined` à prop obrigatória `entry`.
   - Slot repassado: `block-actions` com `{ block }`.
   - Oculto imediatamente (`v-if="!isAltScreen"`) quando `modes.altScreen === true`.
   - Assina `xterm.onRender` (e mudanças na lista de blocos) para manter `IDecoration` ativas **somente para blocos cuja linha (`promptMarker.line`) esteja dentro do intervalo `[viewportY - rows, viewportY + 2 * rows]`** (viewport ± 1 tela).
   - Monta cada `CommandBlock.vue` via `<Teleport :to="decorationElement">` sem alterar a altura das linhas da grade do xterm e sem interceptar eventos de mouse fora dos botões do cabeçalho (`pointer-events: none` no container da decoração, `pointer-events: auto` no cabeçalho/botões). O destino fica dentro do host xterm do mesmo `.hybrid-terminal`, preservando a herança das variáveis CSS locais do painel; não teleportar para `body`.
