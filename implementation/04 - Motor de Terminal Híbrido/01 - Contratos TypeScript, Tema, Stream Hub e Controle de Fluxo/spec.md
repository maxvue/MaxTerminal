# Implementação 01 — Contratos TypeScript, Tema, Stream Hub e Controle de Fluxo

> Fase 04: Motor de Terminal Híbrido
> Referências: [`../instructions.md`](../instructions.md) (§2.1, §3, §4.1, §5.1) e [`../spec.md`](../spec.md) (§2, §4, §6, §7.2–§7.5, §11.1, §12, §13).

---

## 1. Contexto, Objetivo e Escopo

Esta é a implementação fundacional da **Fase 04 (Motor de Terminal Híbrido)**. Ela estabelece:
1. As dependências npm do motor (`@xterm/xterm`, `@xterm/addon-webgl`, `@xterm/addon-fit`, `@xterm/addon-search`, `@xterm/addon-web-links`, `@xterm/addon-unicode11` e `vue-web-terminal`), além da permissão `opener:allow-open-url` (`http://*` e `https://*`) no Tauri v2 (`src-tauri/capabilities/default.json`).
2. Todos os **contratos TypeScript exclusivos do frontend** (`src/types/terminal.ts`) e erros estruturados (`MtTerminalError`, `TriggerConflictError`).
3. O catálogo de atalhos de teclado (`src/terminal/keybindings.ts`) e o tema padrão **Max Navy/Teal** (`src/terminal/theme/defaultTheme.ts` e `src/terminal/theme/themeService.ts`) derivado dos tokens semânticos `--max-*` e `--background-*`.
4. O adaptador fino de IPC (`src/terminal/stream/ptyClient.ts`), único ponto da Fase 04 que importa `src/ipc/pty.ts` e `src/ipc/ptyFrame.ts` da Fase 02.
5. O pipeline de saída de alto desempenho e controle de fluxo com backpressure real até o Rust:
   - `PtyStreamHub` (`src/terminal/stream/ptyStreamHub.ts`): spawn desanexado, assinatura única por `PtyId` com `ptyAttach`, barreira de geração (`attachmentId`), fila pré-attach limitada, tratamento de `Replay`, `Data`, `Gap` e `End`, e reconciliação com `pty://exit`.
   - `WriteBatcher` (`src/terminal/engine/writeBatcher.ts`): agrupamento por `requestAnimationFrame` (visível) ou `setTimeout(16)` (oculto), orçamento por frame (`256 KiB` concatenando frames inteiros sem partir fronteiras de frame), **no máximo 1 `xterm.write` em voo**, limite de fila de `1 MiB` e avanço do cursor confirmado `{ nextSeq, byteOffset }` somente após o callback de parse do xterm.
   - `FlowController` (`src/terminal/engine/flowController.ts`): contabilização cumulativa em `bigint` de bytes consumidos (`Data` + `Replay`), emissão serializada de `pty_ack(ptyId, attachmentId, consumedBytes)` ao acumular `32 KiB` ou após `16 ms` **somente após o parse**, e estimativa de `paused` em `1 MiB`.
6. O pipeline de entrada (`src/terminal/engine/inputPipeline.ts`): coalescência por `queueMicrotask`, fila FIFO serializada de até `1 MiB`, separação estrita entre `{ kind: 'text', text }` (`onData`) e `{ kind: 'bytes', bytes }` (`onBinary` via `encodeBinary` preservando `0x00..0xff`), fatiamento em blocos de até `64 KiB` sem quebrar pares substitutos UTF-16, retry atômico em `pty.write_queue_full` (10 ms, até 50 tentativas), `paste(text)` via `xterm.paste`, e `sendLine(text)` atômico com deduplicação de duplo `Enter` e bloqueio de IME (`isComposing`).

---

## 2. Arquivos a Criar ou Modificar

### 2.1. Configuração e Dependências
- `package.json` — adição das dependências `@xterm/xterm`, `@xterm/addon-webgl`, `@xterm/addon-fit`, `@xterm/addon-search`, `@xterm/addon-web-links`, `@xterm/addon-unicode11`, `vue-web-terminal` (e `@tauri-apps/plugin-opener` se não presente na Fase 01).
- `src-tauri/capabilities/default.json` — adição da permissão `opener:allow-open-url` restrita aos esquemas `http://*` e `https://*` (e registro de `tauri-plugin-opener` em `src-tauri/Cargo.toml` / `src-tauri/src/lib.rs` caso ausente na Fase 01).

### 2.2. Código-Fonte Frontend (`src/`)
- `src/types/terminal.ts` — contratos TypeScript completos da Fase 04 e classes de erro `MtTerminalError` e `TriggerConflictError`.
- `src/terminal/keybindings.ts` — constantes e matchers puros para atalhos do terminal, barra de busca, input rico e popup.
- `src/terminal/theme/defaultTheme.ts` — definição do tema padrão `Max Navy/Teal` lendo variáveis CSS `--max-*` com fallback hexadecimal seguro.
- `src/terminal/theme/themeService.ts` — provedor reativo/observável de `TerminalTheme` (`getActiveTheme`, `setThemeProvider`, `onChange`).
- `src/terminal/theme/themeUiVars.ts` — helper puro `themeUiToCssVars(ui: TerminalTheme['ui']): Record<string, string>` que devolve `--mt-block-background`, `--mt-block-border`, `--mt-block-header-background`, `--mt-rich-input-background` e `--mt-rich-input-border` (consumido por `HybridTerminal.vue`, `04/06`).
- `src/terminal/stream/ptyClient.ts` — fachada sobre `src/ipc/pty.ts` e `src/ipc/ptyFrame.ts`.
- `src/terminal/stream/ptyStreamHub.ts` — hub central de assinatura de streams PTY por `PtyId`, controle de gerações e cursores confirmados.
- `src/terminal/engine/flowController.ts` — controlador de crédito e emissão de `pty_ack` cumulativo pós-parse.
- `src/terminal/engine/writeBatcher.ts` — acumulador de frames por rAF/timer com orçamento por frame e 1 write em voo.
- `src/terminal/engine/inputPipeline.ts` — fila de escrita coalescida, fatiamento de 64 KiB, bracketed paste e retry de `pty.write_queue_full`.

### 2.3. Mocks e Testes Unitários (`tests/terminal/`)
- `tests/terminal/mocks/xtermMock.ts` — mock completo e controlável do `@xterm/xterm` e addons para testes unitários em `jsdom`.
- `tests/terminal/writeBatcher.spec.ts` — testes unitários do `WriteBatcher`.
- `tests/terminal/flowController.spec.ts` — testes unitários do `FlowController`.
- `tests/terminal/ptyStreamHub.spec.ts` — testes unitários do `PtyStreamHub`.
- `tests/terminal/inputPipeline.spec.ts` — testes unitários do `InputPipeline`.

---

## 3. Especificação Técnica Detalhada e Assinaturas

### 3.1. Contratos e Erros (`src/types/terminal.ts`)

```ts
import type { Terminal } from '@xterm/xterm';
import type {
    AttachmentId,
    CommandBlockId,
    ExitInfo,
    OutputCursor,
    PaneId,
    ProjectId,
    PtyId
} from './bindings';

export interface Disposable {
    dispose(): void;
}

export type RendererKind = 'webgl' | 'canvas' | 'dom';
export type InputMode = 'rich' | 'passthrough';
export type MouseTrackingMode = 'none' | 'x10' | 'vt200' | 'drag' | 'any';

export type TerminalErrorCode =
    | 'terminal.renderer_fallback'
    | 'terminal.pane_conflict'
    | 'terminal.queue_overflow'
    | 'stream.already_subscribed'
    | 'stream.replay_gap'
    | 'stream.frame_invalid'
    | 'stream.protocol_violation'
    | 'input.queue_exceeded'
    | 'input.write_timeout'
    | 'input.invalid_binary'
    | 'trigger.provider_failed'
    | 'trigger.timeout'
    | 'trigger.conflict'
    | 'trigger.invalid_prefix'
    | 'block_action.failed'
    | 'link.rejected'
    | 'extension.activate_failed';

export class MtTerminalError extends Error {
    public readonly code: TerminalErrorCode;
    public readonly details?: unknown;

    public constructor(code: TerminalErrorCode, message: string, details?: unknown) {
        super(message);
        this.name = 'MtTerminalError';
        this.code = code;
        this.details = details;
    }
}

export class TriggerConflictError extends MtTerminalError {
    public constructor(prefixOrId: string, details?: unknown) {
        super(
            'trigger.conflict',
            `Conflito no registro de gatilho: "${prefixOrId}" já está registrado.`,
            details
        );
        this.name = 'TriggerConflictError';
    }
}

export interface TerminalPreferences {
    fontFamily: string;          // default: 'JetBrainsMono Nerd Font, monospace'
    fontSize: number;            // default: 13
    lineHeight: number;          // default: 1.15
    letterSpacing: number;       // default: 0
    scrollback: number;          // default: 10000
    cursorStyle: 'block' | 'bar' | 'underline';
    cursorBlink: boolean;
    webglEnabled: boolean;       // default: true
    richInputEnabled: boolean;   // default: true
    macOptionIsMeta: false;      // fixo: Linux
}

export const DEFAULT_TERMINAL_PREFERENCES: Readonly<TerminalPreferences> = Object.freeze({
    fontFamily: 'JetBrainsMono Nerd Font, monospace',
    fontSize: 13,
    lineHeight: 1.15,
    letterSpacing: 0,
    scrollback: 10000,
    cursorStyle: 'block',
    cursorBlink: true,
    webglEnabled: true,
    richInputEnabled: true,
    macOptionIsMeta: false
});

export interface FlowConfig {
    ackThresholdBytes: number;   // 32 * 1024 (32768)
    ackIntervalMs: number;       // 16
    pausedHintBytes: number;     // 1024 * 1024 (1048576)
    frameBudgetBytes: number;    // 256 * 1024 (262144)
}

export const DEFAULT_FLOW_CONFIG: Readonly<FlowConfig> = Object.freeze({
    ackThresholdBytes: 32 * 1024,
    ackIntervalMs: 16,
    pausedHintBytes: 1024 * 1024,
    frameBudgetBytes: 256 * 1024
});

export interface TerminalModes {
    altScreen: boolean;
    mouseTracking: MouseTrackingMode;
    bracketedPaste: boolean;
    applicationCursorKeys: boolean;
}

export interface TerminalFlowState {
    paused: boolean;
    pendingBytes: number;
    droppedBytes: string;        // u64 acumulado em string decimal
    cursor: OutputCursor | null; // { nextSeq: string, byteOffset: string }
    partialView?: boolean;       // true após Gap ou truncamento de replay
    frameErrors?: number;        // contador de falhas de frame
}

export interface TerminalSearchState {
    open: boolean;
    query: string;
    resultIndex: number;
    resultCount: number;
    limitExceeded?: boolean;
    regexError?: string | null;
}

export interface TerminalInstanceState {
    paneId: PaneId;
    ptyId: PtyId;
    cols: number;
    rows: number;
    renderer: RendererKind;
    modes: TerminalModes;
    inputMode: InputMode;
    inputModeOverride: InputMode | null;
    title: string;
    visible: boolean;
    focused: boolean;
    flow: TerminalFlowState;
    search: TerminalSearchState;
    exit: ExitInfo | null;
    /** Cores locais de UI do tema aplicado a este painel (espelho de `TerminalTheme['ui']`); alimenta as variáveis CSS do `.hybrid-terminal`. */
    themeUi: TerminalTheme['ui'];
}

export interface TerminalTheme {
    id: string;
    name: string;
    source: 'builtin' | 'tabby' | 'custom';
    colors: {
        foreground: string;
        background: string;
        cursor: string;
        cursorAccent: string;
        selectionBackground: string;
        selectionForeground?: string;
        selectionInactiveBackground?: string;
        ansi: [
            string, string, string, string, string, string, string, string,
            string, string, string, string, string, string, string, string
        ];
    };
    /** Cores locais do painel para `CommandBlockLayer` e `RichInput` (contrato canônico de 5 campos; Fase 10 `toTerminalTheme` sempre os preenche). */
    ui: {
        blockBackground: string;
        blockBorder: string;
        blockHeaderBackground: string;
        richInputBackground: string;
        richInputBorder: string;
    };
}

export interface TerminalThemeProvider {
    getActiveTheme(): TerminalTheme;
    onChange(cb: (theme: TerminalTheme) => void): Disposable;
}

export interface TerminalContext {
    paneId: PaneId;
    ptyId: PtyId;
    projectId: ProjectId | null;
    cwd: string | null;
    extra: Readonly<Record<string, unknown>>;
}

export type TerminalContextProvider = (
    base: Pick<TerminalContext, 'paneId' | 'ptyId'>
) => Partial<TerminalContext>;

export interface TerminalExtension {
    id: string;
    activate(instance: TerminalInstanceHandle): Disposable | void;
}

export interface TerminalInstanceHandle {
    readonly paneId: PaneId;
    readonly ptyId: PtyId;
    readonly xterm: Terminal;
    sendInput(text: string): Promise<void>;
    getContext(): TerminalContext;
}

export interface TerminalLinkHandler {
    id: string;
    priority: number;
    match: RegExp;
    accepts(uri: string): boolean;
    activate(event: MouseEvent, uri: string, ctx: TerminalContext): void | Promise<void>;
}

export interface InputModeHint {
    mode: InputMode;
    priority: number;
    reason: string;
}

export type InputModeHintProvider = (
    ctx: TerminalContext,
    modes: TerminalModes
) => InputModeHint | null;

export interface CompletionItem {
    id: string;
    label: string;
    description?: string;
    detail?: string;
    icon?: string;
    section?: string;
    kind: 'value' | 'action';
    pinned?: 'bottom';
    disabled?: boolean;
    payload?: unknown;
}

export interface CompletionSection {
    id: string;
    label: string;
    order: number;
}

export interface CompletionResult {
    items: CompletionItem[];
    sections?: CompletionSection[];
    emptyMessage?: string;
    stageLabel?: string;
    passthrough?: boolean;
    refreshAfter?: Promise<unknown>;
}

export interface TriggerQuery {
    prefix: string;
    query: string;
    input: string;
    cursor: number;
    stage: number;
    stageState: unknown;
}

export type TriggerSelectOutcome =
    | { kind: 'replace-input'; text: string; cursor?: number; keepOpen?: boolean }
    | { kind: 'submit'; text: string }
    | { kind: 'push-stage'; stageState: unknown; initialQuery?: string }
    | { kind: 'handled' }
    | { kind: 'close' };

export interface TriggerDefinition {
    id: string;
    prefix: string;
    label: string;
    debounceMs?: number;
    isAvailable(ctx: TerminalContext): boolean | Promise<boolean>;
    provide(
        q: TriggerQuery,
        ctx: TerminalContext,
        signal: AbortSignal
    ): CompletionResult | Promise<CompletionResult>;
    onSelect(
        item: CompletionItem,
        q: TriggerQuery,
        ctx: TerminalContext
    ): TriggerSelectOutcome | Promise<TriggerSelectOutcome>;
    onSubmitWithoutSelection?(
        q: TriggerQuery,
        ctx: TerminalContext
    ): TriggerSelectOutcome | Promise<TriggerSelectOutcome>;
}

export type CommandBlockStatus = 'running' | 'success' | 'error' | 'aborted';

export interface CommandBlockModel {
    id: CommandBlockId;
    paneId: PaneId;
    command: string;
    cwd: string | null;
    startedAt: number;
    finishedAt: number | null;
    exitCode: number | null;
    status: CommandBlockStatus;
    promptLine: number;
    outputStartLine: number | null;
    outputEndLine: number | null;
    meta: Readonly<Record<string, unknown>>;
}

export interface CommandBlockFeed {
    begin(
        paneId: PaneId,
        init: {
            id: CommandBlockId;
            command: string;
            cwd: string | null;
            startedAt: number;
            line?: number;
        }
    ): void;
    markOutputStart(paneId: PaneId, id: CommandBlockId, line?: number): void;
    end(
        paneId: PaneId,
        id: CommandBlockId,
        result: { exitCode: number | null; finishedAt: number; line?: number }
    ): void;
    patchMeta(paneId: PaneId, id: CommandBlockId, meta: Record<string, unknown>): void;
    clear(paneId: PaneId): void;
}

export interface BlockAction {
    id: string;
    label: string;
    icon: string;
    order: number;
    when(block: CommandBlockModel): boolean;
    run(block: CommandBlockModel, ctx: TerminalContext): void | Promise<void>;
}

export type { AttachmentId };
```

---

### 3.2. Tema Padrão e Serviço de Tema (`src/terminal/theme/defaultTheme.ts` e `themeService.ts`)

- **`defaultTheme.ts`:**
  - Exporta `resolveCssToken(tokenName: string, fallbackHex: string): string` (lê `getComputedStyle(document.documentElement).getPropertyValue(tokenName).trim()`, retornando `fallbackHex` em ambiente SSR/jsdom vazio ou se não for cor hexadecimal/RGB válida para o canvas/WebGL do xterm).
  - Exporta `createMaxNavyTealTheme(): TerminalTheme` com `id: 'max-navy-teal'`, `name: 'Max Navy/Teal'`, `source: 'builtin'`, mapeando paleta 16 ANSI coerente com `--max-primary-*`, `--max-success-*`, `--max-warning-*`, `--max-danger-*`, `--max-info-*` e `--background-*`, e preenchendo `ui` com os 5 campos (`blockBackground`, `blockBorder`, `blockHeaderBackground`, `richInputBackground`, `richInputBorder`) resolvidos via `resolveCssToken` a partir de tokens `--background-*`.
- **`themeService.ts`:**
  ```ts
  import type { Disposable, TerminalTheme, TerminalThemeProvider } from '@/types/terminal';

  export interface ThemeService extends TerminalThemeProvider {
      setThemeProvider(provider: TerminalThemeProvider | null): Disposable;
      notifyThemeChanged(theme?: TerminalTheme): void;
  }

  export const themeService: ThemeService;
  ```

---

### 3.3. Atalhos (`src/terminal/keybindings.ts`)

```ts
export interface KeybindingDescriptor {
    id:
        | 'terminal.copy'
        | 'terminal.paste'
        | 'terminal.search'
        | 'terminal.toggleInputMode'
        | 'richInput.interrupt';
    key: string;
    ctrlKey: boolean;
    shiftKey: boolean;
    altKey: boolean;
}

export const TERMINAL_KEYBINDINGS: Readonly<Record<KeybindingDescriptor['id'], KeybindingDescriptor>>;

export function isCopyShortcut(event: KeyboardEvent): boolean;       // Ctrl+Shift+C
export function isPasteShortcut(event: KeyboardEvent): boolean;      // Ctrl+Shift+V
export function isSearchShortcut(event: KeyboardEvent): boolean;     // Ctrl+Shift+F
export function isToggleInputModeShortcut(event: KeyboardEvent): boolean; // Ctrl+Shift+I
export function isInterruptShortcut(event: KeyboardEvent): boolean;  // Ctrl+C (sem Shift/Alt)
```

---

### 3.4. Adaptador IPC (`src/terminal/stream/ptyClient.ts`)

Único módulo da Fase 04 que conhece `src/ipc/pty.ts` e `src/ipc/ptyFrame.ts`.

```ts
import type { Channel } from '@tauri-apps/api/core';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type {
    AttachInfo,
    AttachmentId,
    KillMode,
    OutputCursor,
    PtyCleanupFailedEvent,
    PtyExitEvent,
    PtyId,
    PtyInput,
    PtySize,
    PtySummary,
    SpawnSpec
} from '@/types/bindings';
import type { PtyFrame } from '@/ipc/ptyFrame';

export {
    PtyFrameKind,
    PtyFrameError,
    decodeFrame,
    decodeGap,
    decodeEnd,
    encodeBinary
} from '@/ipc/ptyFrame';
export type { PtyFrame };

export interface PtyClient {
    spawn(spec: SpawnSpec): Promise<PtySummary>;
    attach(
        ptyId: PtyId,
        onFrame: (frame: PtyFrame) => void,
        cursor?: OutputCursor
    ): Promise<{ info: AttachInfo; channel: Channel<ArrayBuffer> }>;
    detach(ptyId: PtyId, attachmentId: AttachmentId): Promise<void>;
    ack(ptyId: PtyId, attachmentId: AttachmentId, consumedBytes: string): Promise<void>;
    write(ptyId: PtyId, input: PtyInput): Promise<void>;
    resize(ptyId: PtyId, size: PtySize): Promise<void>;
    kill(ptyId: PtyId, mode?: KillMode): Promise<void>;
    list(): Promise<PtySummary[]>;
    onExit(cb: (event: PtyExitEvent) => void): Promise<UnlistenFn>;
    onCleanupFailed(cb: (event: PtyCleanupFailedEvent) => void): Promise<UnlistenFn>;
}

export const ptyClient: PtyClient;
```

---

### 3.5. `FlowController` (`src/terminal/engine/flowController.ts`)

```ts
import type { Disposable, FlowConfig } from '@/types/terminal';

export interface AckSink {
    ack(consumedBytes: string): Promise<void>;
}

export interface FlowControllerStateSnapshot {
    paused: boolean;
    pendingBytes: number;
    consumedTotal: bigint;
    unflushedAckBytes: bigint;
}

export class FlowController implements Disposable {
    public constructor(
        cfg: FlowConfig,
        sink: AckSink,
        onState: (state: { paused: boolean; pendingBytes: number }) => void
    );

    public onEnqueued(bytesLength: number): void;
    public onWritten(bytesLength: number): void;
    public tick(): void;
    public flushAck(): Promise<void>;
    public setAttachmentReady(ready: boolean): void;
    public reset(): void;
    public updateConfig(cfg: Partial<FlowConfig>): void;
    public getSnapshot(): FlowControllerStateSnapshot;
    public dispose(): void;
}
```

**Regras de Negócio do `FlowController`:**
1. `onEnqueued(n)` soma `pendingBytes += n` e reavalia `paused = (pendingBytes + Number(unflushedAckBytes)) >= cfg.pausedHintBytes`, mas **NUNCA** agenda nem envia `pty_ack`.
2. `onWritten(n)` (chamado exclusivamente no callback de parse concluído do `xterm.write`) subtrai `pendingBytes = Math.max(0, pendingBytes - n)`, soma `consumedTotal += BigInt(n)` e `unflushedAckBytes += BigInt(n)`.
3. Somente frames `Data` e `Replay` invocam `onEnqueued` e `onWritten`. Frames `Gap` e `End` **não** entram em `consumedTotal`.
4. Se `unflushedAckBytes >= BigInt(cfg.ackThresholdBytes)` (32 KiB) e `attachmentReady === true`, dispara `flushAck()` imediatamente. Caso contrário, se `unflushedAckBytes > 0n`, garante um timer de `cfg.ackIntervalMs` (16 ms) que chama `tick() -> flushAck()`.
5. **Serialização de ACKs (1 em voo):** se um `sink.ack(...)` já estiver em voo quando `consumedTotal` crescer novamente, coalesce o novo valor acumulado e envia um único `sink.ack(consumedTotal.toString())` adicional assim que o em voo resolver.
6. `flushAck()` **nunca** zera `consumedTotal` dentro da mesma geração (`attachmentId`): o contrato da Fase 02 exige o total cumulativo desde o `pty_attach`.
7. Tratamento de erro no `sink.ack`:
   - Se o erro tiver `code === 'pty.attachment_stale'`, ignora silenciosamente (geração antiga já substituída).
   - Se `code === 'pty.ack_invalid'`, não faz retry cego; registra erro de protocolo.
8. `reset()` só é chamado ao trocar `attachmentId` (nova conexão `pty_attach`): zera `consumedTotal = 0n`, `unflushedAckBytes = 0n`, `pendingBytes = 0`, cancela timer e invalida callbacks de ACK em voo da geração anterior.

---

### 3.6. `WriteBatcher` (`src/terminal/engine/writeBatcher.ts`)

```ts
import type { Disposable, FlowConfig } from '@/types/terminal';

export interface BatchFrameInput {
    seq: bigint;
    byteOffset: bigint;
    bytes: Uint8Array;
    onParsed?: () => void;
}

export interface XtermWriteTarget {
    write(data: Uint8Array, callback?: () => void): void;
}

export interface WriteBatcherOptions {
    target: XtermWriteTarget;
    flowConfig: FlowConfig;
    onChunkEnqueued?: (byteLength: number) => void;
    onBatchParsed?: (info: {
        byteLength: number;
        confirmedCursor: { nextSeq: string; byteOffset: string };
    }) => void;
    onQueueOverflow?: (error: Error) => void;
}

export class WriteBatcher implements Disposable {
    public constructor(opts: WriteBatcherOptions);

    public push(frame: BatchFrameInput): void;
    public flush(): void;
    public setVisible(visible: boolean): void;
    public updateFlowConfig(cfg: Partial<FlowConfig>): void;
    public whenIdle(): Promise<void>;
    public clearUnparsed(): void;
    public getQueuedBytes(): number;
    public getConfirmedCursor(): { nextSeq: string; byteOffset: string } | null;
    public isWriteInFlight(): boolean;
    public dispose(): void;
}
```

**Regras de Negócio do `WriteBatcher`:**
1. **Limite de fila + write em voo (`1 MiB` = `1048576` bytes):** se `queuedBytes + inFlightBytes + frame.bytes.byteLength > 1048576`, rejeita com `MtTerminalError('terminal.queue_overflow', ...)` e aciona `onQueueOverflow` sem descartar silenciosamente frames ao vivo.
2. **Preservação de fronteira de frame:** `flush()` retira da fila frames inteiros enquanto a soma não ultrapassar `frameBudgetBytes` (`256 KiB`). Se o primeiro frame da fila tiver exatamente até `256 KiB` (limite máximo do frame v1 na Fase 02), ele é processado inteiro. Nunca divide um único frame em dois lotes, garantindo que `confirmedCursor` sempre reflita uma fronteira exata `{ nextSeq: (lastFrame.seq + 1n).toString(), byteOffset: (lastFrame.byteOffset + BigInt(lastFrame.bytes.byteLength)).toString() }`.
3. **Concatenação em `Uint8Array` único pré-alocado:** se houver 1 frame no lote, passa `frame.bytes` diretamente ao `target.write`; se houver múltiplos frames no lote, aloca um único `new Uint8Array(totalBatchBytes)` e copia via `.set(...)` (proibido `Array.concat` ou `TextDecoder`).
4. **Exatamente 1 write em voo (`inFlight === true`):** enquanto o callback de `target.write(batch, cb)` não disparar, nenhum novo `target.write` é invocado. Quando `cb` dispara:
   - Se a instância/geração não foi descartada nem limpa (`clearUnparsed`/`dispose`), atualiza `confirmedCursor`, chama `onBatchParsed({ byteLength, confirmedCursor })` e executa os callbacks `frame.onParsed()` daquele lote.
   - Se ainda houver frames na fila, agenda o próximo flush no próximo ciclo (`requestAnimationFrame` quando `visible && !document.hidden`, ou `setTimeout(16)` quando oculto).
5. **Agendamento adaptativo:** quando `setVisible(false)` é chamado e há um `requestAnimationFrame` pendente sem `write` em voo, cancela o `rAF` e agenda `setTimeout(16)` para que abas em segundo plano continuem drenando o buffer e enviando `pty_ack` (evitando timeout de 5 s no Rust).

---

### 3.7. `PtyStreamHub` (`src/terminal/stream/ptyStreamHub.ts`)

```ts
import type { ExitInfo, OutputCursor, PtyId, PtySummary, SpawnSpec } from '@/types/bindings';
import type { Disposable } from '@/types/terminal';

export interface PtySink {
    onData(frame: {
        seq: bigint;
        byteOffset: bigint;
        bytes: Uint8Array;
        isReplay: boolean;
    }): Promise<void>;
    onGap(droppedBytes: bigint, nextCursor: { nextSeq: string; byteOffset: string }): void;
    onExit(exit: ExitInfo): void;
    onFrameError?(error: Error): void;
    onCleanupFailed?(survivorsCount: number): void;
}

export interface SubscribeOptions {
    cursor?: OutputCursor | null;
    onAttachmentReady?: (attachmentId: string, truncated: boolean) => void;
    onAckError?: (error: unknown) => void;
}

export interface PtyStreamSubscription extends Disposable {
    readonly attachmentId: string | null;
    ack(consumedBytes: string): Promise<void>;
}

export interface PtyStreamHub {
    spawn(spec: SpawnSpec): Promise<PtySummary>;
    subscribe(
        ptyId: PtyId,
        sink: PtySink,
        options?: SubscribeOptions
    ): Promise<PtyStreamSubscription>;
    release(ptyId: PtyId): void;
    getActiveSubscriptionCount(): number;
    resetForTests(): void;
}

export function createPtyStreamHub(): PtyStreamHub;
export const ptyStreamHub: PtyStreamHub;
```

**Regras de Negócio do `PtyStreamHub`:**
1. **`spawn(spec)` desanexado:** apenas delega para `ptyClient.spawn(spec)`. Não cria `Channel`, não chama `pty_attach` e não envia `pty_ack`.
2. **Assinante único por `PtyId`:** se já houver uma assinatura ativa para `ptyId`, `subscribe` lança imediatamente `new MtTerminalError('stream.already_subscribed', ...)` tanto em DEV quanto em PROD.
3. **Barreira de geração e fila pré-`AttachInfo`:**
   - Antes de invocar `ptyClient.attach(ptyId, onFrame, cursor)`, o hub aloca um `generationToken` único e registra o slot da assinatura.
   - Como o `Channel<ArrayBuffer>` do Tauri pode entregar frames antes de a Promise de `ptyClient.attach` resolver com `AttachInfo.attachmentId`, os frames recebidos nesse intervalo entram na fila FIFO interna daquela geração, mas qualquer envio de `ack(consumedBytes)` aguarda `attachmentId` tornar-se conhecido.
   - Se `dispose()` for chamado enquanto `ptyClient.attach` ainda está em voo, o `generationToken` é invalidado imediatamente; assim que a Promise de `attach` resolver, o hub chama `ptyClient.detach(ptyId, info.attachmentId).catch(() => {})` automaticamente para não deixar conexão órfã no Rust.
4. **Validação de continuidade e tratamento de `Gap`, `Replay`, `Data` e `End`:**
   - `Gap`: decodifica `droppedBytes = decodeGap(frame)`, avança o cursor esperado para `{ nextSeq: frame.seq, byteOffset: frame.byteOffset }` e chama `sink.onGap(droppedBytes, ...)`. Nunca injeta bytes sintéticos no `sink.onData`.
   - `Replay` e `Data`: valida continuidade monotônica (`frame.byteOffset` e `frame.seq` em `Data`). Enfileira na FIFO assíncrona chamando `await sink.onData(...)`.
   - `End`: decodifica `ExitInfo = decodeEnd(frame)` e entra na mesma FIFO; só invoca `sink.onExit(exit)` **depois** que todas as Promises `sink.onData` anteriores resolverem.
5. **Correlação com `pty://exit`:**
   - O listener global `ptyClient.onExit` (instalado sob demanda enquanto houver rotas no hub e removido quando a última rota fizer `release`) registra `exitInfo` e `finalSeq`, mas **não** descarta a fila de output nem dispara `sink.onExit` antes de drenar os frames até `End`/`finalSeq`. `onExit` no `sink` é garantidamente idempotente (chamado no máximo 1 vez por encerramento).
6. **Erros de Frame (`PtyFrameError`):**
   - Se `decodeFrame` lançar erro (ou se o cliente reportar frame corrompido), o hub interrompe a geração atual, chama `sink.onFrameError?.(err)` e desanexa a conexão sem avançar cursor.

---

### 3.8. `InputPipeline` (`src/terminal/engine/inputPipeline.ts`)

```ts
import type { PtyId } from '@/types/bindings';
import type { Disposable } from '@/types/terminal';

export interface InputPipelineOptions {
    ptyId: PtyId;
    pasteToXterm: (text: string) => void;
    isBracketedPasteEnabled: () => boolean;
    onMultilineUnbracketedWarning?: () => void;
    onWriteError?: (error: unknown, acceptedBytes: number) => void;
    onProcessExited?: () => void;
}

export class InputPipeline implements Disposable {
    public static readonly MAX_CHUNK_BYTES = 64 * 1024;       // 65536 bytes UTF-8 / binários
    public static readonly MAX_QUEUE_BYTES = 1024 * 1024;     // 1 MiB
    public static readonly WRITE_FULL_RETRY_MS = 10;
    public static readonly WRITE_FULL_MAX_RETRIES = 50;

    public constructor(opts: InputPipelineOptions);

    public onData(text: string): void;
    public onBinary(binaryString: string): void;
    public paste(text: string): void;
    public sendLine(text: string, opts?: { isComposing?: boolean }): Promise<void>;
    public cancelPendingPasteAndInterrupt(): Promise<void>;
    public markExited(): void;
    public isBlocked(): boolean;
    public getQueuedBytes(): number;
    public flushNow(): Promise<void>;
    public dispose(): void;
}
```

**Regras de Negócio do `InputPipeline`:**
1. **Separação texto vs. binário:**
   - `onData(text)` enfileira item `{ kind: 'text', text }`. Coalesce itens consecutivos do tipo `'text'` que ainda não saíram da fila antes do flush da microtask (`queueMicrotask`).
   - `onBinary(binaryString)` converte imediatamente via `encodeBinary(binaryString)` (rejeitando code units > 255 com `MtTerminalError('input.invalid_binary', ...)`) e enfileira `{ kind: 'bytes', bytes: Array.from(uint8) }`. Nunca concatena item binário com item de texto antes de codificar.
2. **Orçamento de 1 MiB na fila de entrada:**
   - Mede o tamanho codificado (`TextEncoder` para texto, `.length` para bytes). Se adicionar uma nova operação ultrapassar `1 MiB` (`1048576` bytes), lança `MtTerminalError('input.queue_exceeded', 'Limite da fila de entrada (1 MiB) excedido.')` sem enfileirar metade da entrada (preservando o rascunho no `RichInput`).
3. **Fatiamento em blocos de até 64 KiB (`splitTextUtf8Safe`):**
   - Textos cujo UTF-8 exceda `65536` bytes são fatiados em pedaços de até `65536` bytes UTF-8 **sem jamais cortar no meio de um code point Unicode / par substituto UTF-16**.
   - Arrays binários maiores que `65536` bytes são fatiados em fatias de `65536` bytes.
4. **Retry exclusivo para `pty.write_queue_full`:**
   - Ao enviar uma fatia via `ptyClient.write(ptyId, chunk)`, se o erro retornado tiver `code === 'pty.write_queue_full'`, aguarda `10 ms` e retenta **exatamente a mesma fatia** até `50` vezes (pois o backend rejeita atomicamente). Se exceder 50 tentativas, interrompe a fila, limpa itens pendentes daquela operação e notifica `onWriteError(err, acceptedBytes)`.
   - Se o erro for `pty.already_exited` ou `pty.not_found`, marca `markExited()`, bloqueia novas entradas e notifica `onProcessExited()`.
   - Qualquer outro erro ambíguo não é retentado (evita duplicar comando).
5. **`paste(text)` e `sendLine(text)`:**
   - `paste(text)` apenas chama `opts.pasteToXterm(text)` (que invoca `xterm.paste(text)`), deixando o próprio `xterm.onData` encaminhar os bytes (já com `\x1b[200~...\x1b[201~` quando bracketed paste estiver ativo) ao `InputPipeline.onData`.
   - `sendLine(text, { isComposing })`:
     - Se `isComposing === true`, retorna imediatamente sem enviar nada.
     - Deduplica duplo `Enter` enquanto o `sendLine` anterior ainda está sendo enfileirado/disparado no mesmo tick.
     - Se `text` contiver `\n` ou `\r` e `!opts.isBracketedPasteEnabled()`, dispara `opts.onMultilineUnbracketedWarning?.()`.
     - Se `text.length > 0`, usa `opts.pasteToXterm(text)` e em seguida enfileira `'\r'`; se `text === ''`, enfileira apenas `'\r'`.
6. **Cancelamento de colagem longa com `Ctrl+C` (`cancelPendingPasteAndInterrupt`):**
   - Remove da fila todas as fatias de colagem ainda não enviadas. Se um envelope bracketed paste estava aberto na operação interrompida, envia o fechamento `\x1b[201~` antes de enviar `\x03` (`Ctrl+C`), garantindo que o TUI/shell não fique preso dentro do modo de colagem.

---

## 4. Casos de Borda e Requisitos de Performance

- **Inteiros de 64 bits (`u64`):** `seq`, `byteOffset` e `consumedBytes` nunca passam por `Number(...)` para cálculo de cursor ou ACK; toda aritmética usa `bigint` e serializa com `.toString(10)` (suporta valores acima de `Number.MAX_SAFE_INTEGER`).
- **UTF-8 e escapes ANSI partidos entre frames:** o `WriteBatcher` entrega `Uint8Array` bruto ao `xterm.write(Uint8Array)` sem decodificar para `string` no caminho de dados.
- **Zero vazamento de timers/listeners:** `dispose()` em `FlowController`, `WriteBatcher`, `InputPipeline` e `PtyStreamSubscription` é idempotente e limpa `requestAnimationFrame`, `setTimeout` e callbacks pendentes.
