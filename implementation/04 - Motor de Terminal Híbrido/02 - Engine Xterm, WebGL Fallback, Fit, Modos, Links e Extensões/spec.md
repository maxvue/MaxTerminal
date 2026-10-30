# Implementação 02 — Engine Xterm, WebGL Fallback, Fit, Modos, Links e Extensões

> Fase 04: Motor de Terminal Híbrido
> Referências: [`../instructions.md`](../instructions.md) (§2.1 itens 1, 4, 8, 9; §4.1; §5.1) e [`../spec.md`](../spec.md) (§3.1, §7.1, §7.6–§7.9, §8, §11.4, §12, §13, §14).

---

## 1. Contexto, Objetivo e Escopo

Com o pipeline de stream, batching, controle de fluxo e escrita concluído na Implementação 01, esta **Implementação 02** constrói o núcleo do emulador VT baseado em `@xterm/xterm` e seus pontos de extensão:
1. **`RendererManager` (`src/terminal/engine/rendererManager.ts`):** gerencia a aceleração por GPU via `@xterm/addon-webgl`, tratando falhas de inicialização no WebKitGTK (CachyOS), perda de contexto (`onContextLoss`) com fallback imediato (`webgl` → `canvas` se compatível/instalado → `dom`), suspensão de WebGL quando a instância fica oculta (`visible = false`, respeitando o limite de ~16 contextos WebGL por página), retomada após 30 s e fixação definitiva em `dom` se ocorrerem **3 perdas de contexto em 60 segundos**.
2. **`FitController` (`src/terminal/engine/fitController.ts`):** observa o elemento host via `ResizeObserver`, aplica debounce de **50 ms** (trailing com `maxWait: 100 ms` durante arraste contínuo), consulta `fitAddon.proposeDimensions()`, ignora containers ocultos ou `0×0` (`cols < 2` ou `rows < 1`), trunca dentro dos limites `1..=1000` da Fase 02, deduplica dimensões já confirmadas e serializa chamadas `ptyClient.resize(ptyId, size)` mantendo o tamanho desejado separado do confirmado pelo IPC.
3. **`ModeTracker` (`src/terminal/engine/modeTracker.ts`):** monitora transições de alternate screen buffer (`?1049h` / `?1049l` via `xterm.buffer.onBufferChange`), modos VT (`mouseTrackingMode`, `bracketedPasteMode`, `applicationCursorKeysMode` amostrados no máximo 1× por frame após cada lote escrito) e mudanças de título (`xterm.onTitleChange` truncado a 256 caracteres), emitindo eventos apenas quando houver alteração real.
4. **Registros de Contexto, Extensões e Links:**
   - `src/terminal/extensions/terminalContext.ts` (`registerContextProvider`, `resolveTerminalContext`).
   - `src/terminal/extensions/terminalExtensions.ts` (`registerTerminalExtension`, ativação automática em instâncias existentes e futuras com isolamento de exceções).
   - `src/terminal/links/linkHandlerRegistry.ts` (`registerLinkHandler`, handler padrão restrito a `http:` e `https:` validados via `new URL()`, exigindo `Ctrl+Clique` ou `Meta+Clique`, aplicável tanto ao `@xterm/addon-web-links` quanto a `Terminal.options.linkHandler` para escapes OSC 8).
5. **`TerminalInstance` (`src/terminal/engine/terminalInstance.ts`):** classe que encapsula o `Terminal` do `@xterm/xterm`, carrega os addons na ordem canônica (`Unicode11Addon` → `FitAddon` → `SearchAddon` → `WebLinksAddon` → `RendererManager` → extensões), integra `PtyStreamHub`, `WriteBatcher`, `FlowController`, `InputPipeline`, `FitController` e `ModeTracker`, e permite anexar/desanexar do DOM (`attach`/`detach`) sem perder o buffer nem recriar o emulador.
6. **`TerminalInstanceRegistry` (`src/terminal/engine/instanceRegistry.ts`):** `Map<PaneId, TerminalInstance>` não reativa que preserva as instâncias entre trocas de aba/rota, impede reutilização de `paneId` com `ptyId` divergente sem `dispose` explícito e garante limpeza determinística.

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Código-Fonte Frontend (`src/terminal/`)
- `src/terminal/engine/rendererManager.ts` — máquina de estados do renderizador (`webgl` / `canvas` / `dom` / `suspended` / `dom_locked`).
- `src/terminal/engine/fitController.ts` — `ResizeObserver` + debounce 50 ms (`maxWait` 100 ms) + deduplicação + serialização de `pty_resize`.
- `src/terminal/engine/modeTracker.ts` — detector de alternate screen (`?1049h`/`?1049l`), mouse tracking, bracketed paste, application cursor keys e título OSC 0/2.
- `src/terminal/extensions/terminalContext.ts` — registro de `TerminalContextProvider` e merge determinístico de contexto por painel.
- `src/terminal/extensions/terminalExtensions.ts` — registro global `registerTerminalExtension` com ativação retroativa e futura.
- `src/terminal/links/linkHandlerRegistry.ts` — registro `registerLinkHandler`, handler seguro `http(s)` (`Ctrl+Clique` + `@tauri-apps/plugin-opener`) e adaptador para `WebLinksAddon` + OSC 8 `linkHandler`.
- `src/terminal/engine/terminalInstance.ts` — classe `TerminalInstance` implementando `TerminalInstanceHandle`.
- `src/terminal/engine/instanceRegistry.ts` — `TerminalInstanceRegistry` (`getOrCreate`, `get`, `dispose`, `forEach`, `clearAll`).

### 2.2. Testes Unitários (`tests/terminal/`)
- `tests/terminal/rendererManager.spec.ts` — testes de fallback em `onContextLoss`, suspensão em instância oculta e trava em `dom` após 3 perdas em 60 s.
- `tests/terminal/fitController.spec.ts` — testes de debounce (10 resizes em 30 ms ⇒ 1 `pty_resize`), deduplicação de dimensões iguais, container `0×0` ignorado e retry após falha de IPC.
- `tests/terminal/modeTracker.spec.ts` — testes de detecção de `altScreen`, `mouseTracking`, `bracketedPaste`, `applicationCursorKeys` e truncamento de título a 256 chars.
- `tests/terminal/linkHandlerRegistry.spec.ts` — testes de precedência por `priority`, bloqueio de `javascript:`, `file://` e `data:`, exigência de `Ctrl+Clique` e suporte a OSC 8.
- `tests/terminal/instanceRegistry.spec.ts` — testes de ciclo de vida, preservação em `detach`/`attach`, rejeição de `ptyId` conflitante no mesmo `paneId` e **100 montagens/desmontagens consecutivas sem vazamento de assinaturas ou listeners**.

---

## 3. Especificação Técnica Detalhada e Assinaturas

### 3.1. `RendererManager` (`src/terminal/engine/rendererManager.ts`)

```ts
import type { Terminal } from '@xterm/xterm';
import type { Disposable, RendererKind } from '@/types/terminal';

export interface WebglAddonLike extends Disposable {
    onContextLoss(listener: () => void): Disposable;
    clearTextureAtlas?(): void;
}

export interface RendererManagerOptions {
    xterm: Terminal;
    webglEnabled: boolean;
    visible: boolean;
    onRendererChange?: (kind: RendererKind) => void;
    createWebglAddon?: () => WebglAddonLike;
    createCanvasAddon?: (() => Disposable) | null;
    now?: () => number;
}

export class RendererManager implements Disposable {
    public static readonly LOSS_WINDOW_MS = 60_000;
    public static readonly MAX_LOSSES_IN_WINDOW = 3;
    public static readonly RETRY_COOLDOWN_MS = 30_000;

    public constructor(opts: RendererManagerOptions);

    public activate(): RendererKind;
    public setVisible(visible: boolean): void;
    public setWebglEnabled(enabled: boolean): void;
    public suspend(): void;
    public forceContextLossForDebug(): void;
    public getKind(): RendererKind;
    public isLockedToDom(): boolean;
    public getContextLossCountInWindow(): number;
    public dispose(): void;
}
```

**Regras de Negócio do `RendererManager`:**
1. Somente ativa `WebglAddon` depois que o `xterm` já foi aberto em um elemento DOM (`xterm.element` presente), quando `webglEnabled === true`, `visible === true` e `!lockedToDom`.
2. A criação e `xterm.loadAddon(webglAddon)` ocorrem dentro de `try / catch`: se o WebKitGTK/driver lançar exceção (ex.: DMABUF/WebGL2 indisponível), registra a falha na janela deslizante de 60 s e cai imediatamente para o fallback (`canvas` se `createCanvasAddon` fornecido, senão `dom` nativo do xterm).
3. Quando `onContextLoss` dispara:
   - Descarta imediatamente o addon WebGL (`webglAddon.dispose()`), remove o listener, registra o timestamp da perda e executa `xterm.refresh(0, xterm.rows - 1)`.
   - Se o número de perdas nos últimos `60_000 ms` atingir `3`, marca `lockedToDom = true` permanentemente até recarregar a aplicação, desativa qualquer addon secundário e fixa o renderer em `'dom'`.
   - Caso contrário (`< 3` perdas em 60 s), alterna para `'canvas'` (se disponível) ou `'dom'` e agenda um timer de `30_000 ms` (`RETRY_COOLDOWN_MS`) para tentar restaurar `'webgl'` caso a instância continue visível e `webglEnabled === true`.
4. Quando `setVisible(false)` ou `suspend()` é chamado:
   - Cancela qualquer timer de retry pendente.
   - Se um `webglAddon` estiver ativo, chama `webglAddon.dispose()` para liberar o contexto GPU imediatamente e notifica `onRendererChange('dom')` (nunca mantém contexto WebGL em abas ocultas).
5. Quando `setVisible(true)` é chamado novamente (e `webglEnabled && !lockedToDom`): recria e carrega o `WebglAddon` e chama `xterm.refresh(0, xterm.rows - 1)`.

---

### 3.2. `FitController` (`src/terminal/engine/fitController.ts`)

```ts
import type { Terminal } from '@xterm/xterm';
import type { FitAddon } from '@xterm/addon-fit';
import type { PtyId, PtySize } from '@/types/bindings';
import type { Disposable } from '@/types/terminal';

export interface FitControllerOptions {
    ptyId: PtyId;
    xterm: Terminal;
    fitAddon: Pick<FitAddon, 'proposeDimensions'>;
    onDimensionsChanged?: (cols: number, rows: number) => void;
    debounceMs?: number; // default: 50
    maxWaitMs?: number;  // default: 100
}

export class FitController implements Disposable {
    public static readonly MIN_COLS = 2;
    public static readonly MIN_ROWS = 1;
    public static readonly MAX_DIM = 1000;

    public constructor(opts: FitControllerOptions);

    public observe(host: HTMLElement): void;
    public unobserve(): void;
    public setVisible(visible: boolean): void;
    public requestFit(immediate?: boolean): Promise<void>;
    public markPtyExited(): void;
    public getConfirmedSize(): { cols: number; rows: number } | null;
    public dispose(): void;
}
```

**Regras de Negócio do `FitController`:**
1. Observa `host` via `ResizeObserver`. Eventos de resize passam por debounce trailing de `50 ms` com `maxWait` de `100 ms` (garantindo no máximo 1 chamada a cada 50–100 ms durante arraste contínuo de divisor/janela).
2. Em cada avaliação:
   - Se `!visible`, ou `!host.isConnected`, ou `host.clientWidth === 0`, ou `host.clientHeight === 0`, ignora silenciosamente.
   - Obtém `dims = fitAddon.proposeDimensions()`. Se `!dims` ou `!Number.isFinite(dims.cols)` ou `!Number.isFinite(dims.rows)` ou `dims.cols < 2` ou `dims.rows < 1`, ignora.
   - Trunca `cols = Math.min(1000, Math.floor(dims.cols))` e `rows = Math.min(1000, Math.floor(dims.rows))`.
3. **Separação entre dimensão do xterm e dimensão confirmada pelo IPC (`confirmedCols`, `confirmedRows`):**
   - Se `xterm.cols !== cols || xterm.rows !== rows`, chama `xterm.resize(cols, rows)` e `onDimensionsChanged?.(cols, rows)`.
   - Se `confirmedCols === cols && confirmedRows === rows`, não chama `ptyClient.resize` (deduplicação).
   - Caso contrário, serializa o envio de `ptyClient.resize(ptyId, { rows, cols, pixelWidth: 0, pixelHeight: 0 })` (se novas dimensões surgirem enquanto uma chamada IPC está em voo, apenas a última pendente é enviada em seguida).
   - `confirmedCols` e `confirmedRows` só são atualizados **após** a Promise de `ptyClient.resize` resolver com sucesso. Se falhar por erro transitório, mantém `confirmedSize` inalterado para que o próximo `requestFit()` tente novamente mesmo que `xterm.cols/rows` já tenham mudado. Se falhar com `pty.already_exited` / `pty.not_found`, chama `markPtyExited()`.
4. Também escuta `document.fonts?.ready` (quando disponível no navegador) para disparar `requestFit()` após o carregamento da fonte Nerd Font.

---

### 3.3. `ModeTracker` (`src/terminal/engine/modeTracker.ts`)

```ts
import type { Terminal } from '@xterm/xterm';
import type { Disposable, MouseTrackingMode, TerminalModes } from '@/types/terminal';

export interface ModeTrackerOptions {
    xterm: Terminal;
    onModesChange: (modes: TerminalModes, prevModes: TerminalModes) => void;
    onTitleChange: (title: string) => void;
}

export class ModeTracker implements Disposable {
    public static readonly MAX_TITLE_LENGTH = 256;

    public constructor(opts: ModeTrackerOptions);

    public sampleModes(): TerminalModes;
    public getModes(): TerminalModes;
    public getTitle(): string;
    public dispose(): void;
}
```

**Regras de Negócio do `ModeTracker`:**
1. Estado inicial: `{ altScreen: false, mouseTracking: 'none', bracketedPaste: false, applicationCursorKeys: false }`.
2. Assina `xterm.buffer.onBufferChange((buf) => ...)` para detectar imediatamente a entrada e saída do alternate screen buffer (`buf.type === 'alternate'`, disparado pelas sequências `?1049h`, `?1047h`, `?47h` e seus respectivos `l`).
3. Expõe `sampleModes()` que é invocado no callback `onBatchParsed` do `WriteBatcher` (no máximo 1× por frame renderizado) e também em `onBufferChange`:
   - Lê `xterm.buffer.active.type === 'alternate'`.
   - Lê `xterm.modes.mouseTrackingMode` (`'none' | 'x10' | 'vt200' | 'drag' | 'any'`).
   - Lê `Boolean(xterm.modes.bracketedPasteMode)`.
   - Lê `Boolean(xterm.modes.applicationCursorKeysMode)`.
   - Compara campo a campo com o snapshot anterior; só invoca `onModesChange(next, prev)` quando pelo menos um campo mudou.
4. Assina `xterm.onTitleChange((rawTitle) => ...)`: sanitiza caracteres de controle C0/C1, trunca a `256` caracteres (`rawTitle.slice(0, 256)`) e emite `onTitleChange(title)` quando diferente do anterior.

---

### 3.4. Contexto, Extensões e Links (`src/terminal/extensions/` e `src/terminal/links/`)

#### `src/terminal/extensions/terminalContext.ts`
```ts
import type { PaneId, PtyId } from '@/types/bindings';
import type { Disposable, TerminalContext, TerminalContextProvider } from '@/types/terminal';

export function registerContextProvider(provider: TerminalContextProvider): Disposable;
export function resolveTerminalContext(base: { paneId: PaneId; ptyId: PtyId }): TerminalContext;
export function clearContextProvidersForTests(): void;
```
- Faz o merge dos objetos parciais retornados pelos `TerminalContextProvider`s na ordem de registro, combinando o dicionário `extra` de forma imutável (`Object.freeze`).

#### `src/terminal/extensions/terminalExtensions.ts`
```ts
import type { Disposable, TerminalExtension, TerminalInstanceHandle } from '@/types/terminal';

export function registerTerminalExtension(ext: TerminalExtension): Disposable;
export function attachExtensionsToInstance(instance: TerminalInstanceHandle): Disposable;
export function clearTerminalExtensionsForTests(): void;
```
- Quando `registerTerminalExtension(ext)` é chamado, ativa a extensão imediatamente em todas as `TerminalInstanceHandle` vivas e nas que forem criadas depois.
- Qualquer exceção lançada em `ext.activate(instance)` é capturada e registrada via `console.error` com código `'extension.activate_failed'` sem interromper a inicialização do terminal nem as demais extensões.
- Quando o `Disposable` retornado por `registerTerminalExtension` (ou o `Disposable` da instância) é descartado, todos os `Disposable`s produzidos por `ext.activate` são invocados com segurança.

#### `src/terminal/links/linkHandlerRegistry.ts`
```ts
import type { ILinkHandler } from '@xterm/xterm';
import type { Disposable, TerminalContext, TerminalLinkHandler } from '@/types/terminal';

export function isSafeHttpUrl(rawUri: string): boolean;
export function hasActivationModifier(event: MouseEvent): boolean; // event.ctrlKey || event.metaKey
export function registerLinkHandler(handler: TerminalLinkHandler): Disposable;
export function getRegisteredLinkHandlers(): readonly TerminalLinkHandler[];
export function dispatchTerminalLink(
    event: MouseEvent,
    uri: string,
    ctx: TerminalContext
): Promise<boolean>;
export function createOsc8LinkHandler(
    getContext: () => TerminalContext,
    onHoverChange?: (uri: string | null) => void
): ILinkHandler;
export function clearLinkHandlersForTests(): void;
```
- **Segurança de Links (§14):**
  - O handler padrão (`id: 'builtin.http'`, `priority: 0`) valida a URI com `new URL(uri)` e aceita **exclusivamente** `url.protocol === 'http:' || url.protocol === 'https:'`.
  - Esquemas `javascript:`, `file:`, `data:`, `vbscript:`, `mailto:`, etc. são rejeitados por `isSafeHttpUrl` e ignorados caso nenhum handler customizado de fase futura (ex.: Fase 09 para `caminho:linha`) os aceite.
  - A ativação exige obrigatoriamente `event.ctrlKey || event.metaKey` (`Ctrl+Clique`). Sem o modificador, o clique não abre o link.
  - A abertura externa usa `@tauri-apps/plugin-opener` (`openUrl(url.toString())`) com fallback seguro em ambiente de teste.
  - `createOsc8LinkHandler` aplica exatamente a mesma política de validação e exigência de `Ctrl+Clique` para links OSC 8 (`Terminal.options.linkHandler`).

---

### 3.5. `TerminalInstance` e `TerminalInstanceRegistry`

#### `src/terminal/engine/terminalInstance.ts`
```ts
import type { Terminal } from '@xterm/xterm';
import type { SearchAddon } from '@xterm/addon-search';
import type { ExitInfo, PaneId, PtyId } from '@/types/bindings';
import type {
    Disposable,
    FlowConfig,
    RendererKind,
    TerminalContext,
    TerminalInstanceHandle,
    TerminalModes,
    TerminalPreferences,
    TerminalTheme
} from '@/types/terminal';
import type { InputPipeline } from './inputPipeline';

export interface TerminalInstanceCallbacks {
    onReady?: (handle: TerminalInstanceHandle) => void;
    onDimensionsChange?: (cols: number, rows: number) => void;
    onRendererChange?: (renderer: RendererKind) => void;
    onModesChange?: (modes: TerminalModes, prevModes: TerminalModes) => void;
    onTitleChange?: (title: string) => void;
    onFlowStateChange?: (flow: {
        paused: boolean;
        pendingBytes: number;
        droppedBytes: string;
        cursor: { nextSeq: string; byteOffset: string } | null;
        partialView: boolean;
        frameErrors: number;
    }) => void;
    onExit?: (exit: ExitInfo) => void;
}

export interface TerminalInstanceOptions {
    paneId: PaneId;
    ptyId: PtyId;
    prefs: TerminalPreferences;
    theme: TerminalTheme;
    flow: FlowConfig;
    callbacks?: TerminalInstanceCallbacks;
}

export class TerminalInstance implements TerminalInstanceHandle, Disposable {
    public readonly paneId: PaneId;
    public readonly ptyId: PtyId;
    public readonly xterm: Terminal;
    public readonly searchAddon: SearchAddon;
    public readonly inputPipeline: InputPipeline;

    public constructor(opts: TerminalInstanceOptions);

    public attach(host: HTMLElement): Promise<void>;
    public detach(): void;
    public setVisible(visible: boolean): void;
    public focus(): void;
    public applyTheme(theme: TerminalTheme): void;
    public applyPreferences(prefs: Partial<TerminalPreferences>): void;
    public paste(text: string): void;
    public sendInput(text: string): Promise<void>;
    public sendLine(text: string, opts?: { isComposing?: boolean }): Promise<void>;
    public getContext(): TerminalContext;
    public setCallbacks(callbacks: TerminalInstanceCallbacks): void;
    public forceWebglContextLossForDebug(): void;
    public isDisposed(): boolean;
    public dispose(): void;
}
```

**Regras de Negócio do `TerminalInstance`:**
1. Cria um `innerContainer: HTMLDivElement` próprio (`width: 100%; height: 100%`). Na primeira chamada a `attach(host)`, anexa `innerContainer` ao `host`, chama `xterm.open(innerContainer)` **uma única vez**, carrega `RendererManager.activate()`, observa o `host` no `FitController`, assina o `ptyStreamHub.subscribe(ptyId, ...)` e executa o `requestFit(true)` inicial.
2. Nas chamadas subsequentes a `attach(newHost)` (ex.: após troca de aba/rota onde o componente Vue foi desmontado e remontado), apenas move `innerContainer` para `newHost` (`newHost.appendChild(innerContainer)`), restaura visibilidade (`setVisible(true)`) e reexecuta `fitController.observe(newHost)` + `requestFit(true)` — **sem** chamar `xterm.open` novamente e **sem** perder scrollback, seleção ou estado VT.
3. Em `detach()` (chamado no `onBeforeUnmount` do componente Vue ao trocar de aba): desobserva o `host` no `FitController`, chama `setVisible(false)` (o que suspende o `WebglAddon` e muda o `WriteBatcher` para `setTimeout(16)`), e remove `innerContainer` do DOM, mas **mantém** a assinatura do `ptyStreamHub` viva para continuar recebendo e confirmando saída em segundo plano.
4. Atualiza `onFlowStateChange` com **throttle de 250 ms** (exceto em mudanças críticas como `Gap`, `partialView`, `paused` ou `Exit`, que notificam imediatamente).
5. Instala `xterm.attachCustomKeyEventHandler` respeitando `src/terminal/keybindings.ts`:
   - `Ctrl+Shift+C`: copia `xterm.getSelection()` para `navigator.clipboard.writeText` e retorna `false`.
   - `Ctrl+Shift+V`: retorna `false` para não enviar controle bruto ao PTY (o evento `paste` do navegador/atalho chama `paste()`).
   - `Ctrl+C` **sem** `Shift`: retorna sempre `true` para que o xterm envie `\x03` diretamente ao processo no PTY.

#### `src/terminal/engine/instanceRegistry.ts`
```ts
import type { PaneId } from '@/types/bindings';
import type { TerminalInstance, TerminalInstanceOptions } from './terminalInstance';

export class TerminalInstanceRegistry {
    public getOrCreate(opts: TerminalInstanceOptions): TerminalInstance;
    public get(paneId: PaneId): TerminalInstance | undefined;
    public has(paneId: PaneId): boolean;
    public dispose(paneId: PaneId): boolean;
    public forEach(cb: (instance: TerminalInstance, paneId: PaneId) => void): void;
    public size(): number;
    public disposeAll(): void;
}

export const terminalInstanceRegistry: TerminalInstanceRegistry;
export function createTerminalInstanceRegistry(): TerminalInstanceRegistry;
```
- Se `getOrCreate(opts)` encontrar uma instância já registrada para `opts.paneId`, verifica se `existing.ptyId === opts.ptyId`. Se o `ptyId` for diferente, lança `MtTerminalError('terminal.pane_conflict', 'Painel já possui uma instância vinculada a outro PtyId. Descarte a instância anterior antes de recriar.')`.

---

## 4. Casos de Borda, Memória e Segurança

- **Isolamento de Reatividade Vue:** `TerminalInstanceRegistry` usa um `Map` nativo fora do Pinia, e toda referência a `TerminalInstance` ou `Terminal` exposta a composables/componentes deve ser envolvida com `markRaw(...)`.
- **Proibição de APIs Privadas do Xterm:** não acessar `xterm._core` nem configurar opções obsoletas (`windowsMode`).
- **Prevenção de XSS e Clipboard Remoto (OSC 52):** nenhum addon de OSC 52 é carregado; títulos OSC 0/2 são truncados a 256 caracteres e higienizados de controles.
