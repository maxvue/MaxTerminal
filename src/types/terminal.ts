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

/** Contrato de recurso descartável do Motor de Terminal Híbrido. */
export interface Disposable {
    dispose(): void;
}

/** Tipo de renderizador ativo na instância do Xterm. */
export type RendererKind = 'webgl' | 'canvas' | 'dom';

/** Modo de entrada do painel (barra estruturada estilo Warp ou envio direto ao PTY). */
export type InputMode = 'rich' | 'passthrough';

/** Modo de rastreamento de mouse reportado pelo Xterm. */
export type MouseTrackingMode = 'none' | 'x10' | 'vt200' | 'drag' | 'any';

/** Códigos canônicos de erro do Motor de Terminal Híbrido. */
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

/** Erro estruturado do domínio do Motor de Terminal Híbrido. */
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

/** Erro lançado quando dois gatilhos disputam o mesmo prefixo ou identificador. */
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

/** Preferências visuais e comportamentais do emulador de terminal. */
export interface TerminalPreferences {
    fontFamily: string;
    fontSize: number;
    lineHeight: number;
    letterSpacing: number;
    scrollback: number;
    cursorStyle: 'block' | 'bar' | 'underline';
    cursorBlink: boolean;
    webglEnabled: boolean;
    richInputEnabled: boolean;
    macOptionIsMeta: false;
}

/** Valores padrão imutáveis para as preferências do terminal. */
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

/** Parâmetros do controle de fluxo entre o frontend e o PTY. */
export interface FlowConfig {
    ackThresholdBytes: number;
    ackIntervalMs: number;
    pausedHintBytes: number;
    frameBudgetBytes: number;
}

/** Configuração padrão de controle de fluxo (ACK a cada 32 KiB / 16 ms, lote de 256 KiB). */
export const DEFAULT_FLOW_CONFIG: Readonly<FlowConfig> = Object.freeze({
    ackThresholdBytes: 32 * 1024,
    ackIntervalMs: 16,
    pausedHintBytes: 1024 * 1024,
    frameBudgetBytes: 256 * 1024
});

/** Modos VT ativos no emulador. */
export interface TerminalModes {
    altScreen: boolean;
    mouseTracking: MouseTrackingMode;
    bracketedPaste: boolean;
    applicationCursorKeys: boolean;
}

/** Estado serializável do controle de fluxo de um painel. */
export interface TerminalFlowState {
    paused: boolean;
    pendingBytes: number;
    droppedBytes: string;
    cursor: OutputCursor | null;
    partialView?: boolean | undefined;
    frameErrors?: number | undefined;
}

/** Estado serializável da busca incremental no scrollback. */
export interface TerminalSearchState {
    open: boolean;
    query: string;
    resultIndex: number;
    resultCount: number;
    limitExceeded?: boolean | undefined;
    regexError?: string | null | undefined;
}

/** Paleta e tokens visuais de um tema de terminal. */
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
        selectionForeground?: string | undefined;
        selectionInactiveBackground?: string | undefined;
        ansi: [
            string, string, string, string, string, string, string, string,
            string, string, string, string, string, string, string, string
        ];
    };
    /** Cores locais do painel para `CommandBlockLayer` e `RichInput`. */
    ui: {
        blockBackground: string;
        blockBorder: string;
        blockHeaderBackground: string;
        richInputBackground: string;
        richInputBorder: string;
    };
}

/** Estado reativo serializável de uma instância de terminal mantido na `useTerminalStore`. */
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
    /** Cores locais de UI do tema aplicado a este painel; alimenta as variáveis CSS do `.hybrid-terminal`. */
    themeUi: TerminalTheme['ui'];
}

/** Provedor observável de temas de terminal. */
export interface TerminalThemeProvider {
    getActiveTheme(): TerminalTheme;
    onChange(cb: (theme: TerminalTheme) => void): Disposable;
}

/** Contexto operacional resolvido para um painel de terminal. */
export interface TerminalContext {
    paneId: PaneId;
    ptyId: PtyId;
    projectId: ProjectId | null;
    cwd: string | null;
    extra: Readonly<Record<string, unknown>>;
    inputMode?: InputMode | undefined;
    tabId?: string | null | undefined;
    shell?: string | null | undefined;
}

/** Função provedora de metadados contextuais para um painel. */
export type TerminalContextProvider = (
    base: Pick<TerminalContext, 'paneId' | 'ptyId'>
) => Partial<TerminalContext>;

/** Contrato de extensão conectável às instâncias de terminal. */
export interface TerminalExtension {
    id: string;
    activate(instance: TerminalInstanceHandle): Disposable | void;
}

/** Handle público reduzido exposto para extensões e componentes externos. */
export interface TerminalInstanceHandle {
    readonly paneId: PaneId;
    readonly ptyId: PtyId;
    readonly xterm: Terminal;
    sendInput(text: string): Promise<void>;
    getContext(): TerminalContext;
    onAfterFlush?(listener: () => void): () => void;
}

/** Manipulador customizável de links clicáveis no terminal. */
export interface TerminalLinkHandler {
    id: string;
    priority: number;
    match: RegExp;
    accepts(uri: string): boolean;
    activate(event: MouseEvent, uri: string, ctx: TerminalContext): void | Promise<void>;
}

/** Dica de modo de entrada fornecida por extensões (ex.: perfis de agentes CLI). */
export interface InputModeHint {
    mode: InputMode;
    priority: number;
    reason: string;
}

/** Provedor de dicas de modo de entrada para um painel. */
export type InputModeHintProvider = (
    ctx: TerminalContext,
    modes: TerminalModes
) => InputModeHint | null;

/** Item individual exibido no popup de autocompletar. */
export interface CompletionItem {
    id: string;
    label: string;
    description?: string | undefined;
    detail?: string | undefined;
    icon?: string | undefined;
    section?: string | undefined;
    kind: 'value' | 'action';
    pinned?: 'bottom' | undefined;
    disabled?: boolean | undefined;
    disabledReason?: string | undefined;
    payload?: unknown;
}

/** Seção de agrupamento no popup de autocompletar. */
export interface CompletionSection {
    id: string;
    label: string;
    order: number;
}

/** Resultado retornado por um provedor de gatilho de autocompletar. */
export interface CompletionResult {
    items: CompletionItem[];
    sections?: CompletionSection[] | undefined;
    emptyMessage?: string | undefined;
    stageLabel?: string | undefined;
    passthrough?: boolean | undefined;
    refreshAfter?: Promise<unknown> | undefined;
}

/** Consulta repassada ao provedor de gatilho durante a sessão de autocompletar. */
export interface TriggerQuery {
    prefix: string;
    query: string;
    input: string;
    cursor: number;
    stage: number;
    stageState: unknown;
}

/** Ação resultante da seleção ou submissão em um gatilho. */
export type TriggerSelectOutcome =
    | { kind: 'replace-input'; text: string; cursor?: number | undefined; keepOpen?: boolean | undefined }
    | { kind: 'submit'; text: string }
    | { kind: 'push-stage'; stageState: unknown; initialQuery?: string | undefined }
    | { kind: 'handled' }
    | { kind: 'close' };

/** Definição de um gatilho de entrada rica (`>`, `>>`, `@`, `?`, `/`). */
export interface TriggerDefinition {
    id: string;
    prefix: string;
    label: string;
    debounceMs?: number | undefined;
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
    onSecondarySelect?(
        item: CompletionItem,
        q: TriggerQuery,
        ctx: TerminalContext
    ): TriggerSelectOutcome | Promise<TriggerSelectOutcome>;
    onSubmitWithoutSelection?(
        q: TriggerQuery,
        ctx: TerminalContext
    ): TriggerSelectOutcome | Promise<TriggerSelectOutcome>;
}

/** Status semântico de execução de um bloco de comando. */
export type CommandBlockStatus = 'running' | 'success' | 'error' | 'aborted';

/** Modelo serializável de um bloco de comando ancorado no buffer do terminal. */
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

/** Interface de alimentação de eventos de blocos de comando (consumida pela Fase 05 / OSC 133). */
export interface CommandBlockFeed {
    begin(
        paneId: PaneId,
        init: {
            id: CommandBlockId;
            command: string;
            cwd: string | null;
            startedAt: number;
            line?: number | undefined;
        }
    ): void;
    markOutputStart(paneId: PaneId, id: CommandBlockId, line?: number): void;
    end(
        paneId: PaneId,
        id: CommandBlockId,
        result: { exitCode: number | null; finishedAt: number; line?: number | undefined }
    ): void;
    patchMeta(paneId: PaneId, id: CommandBlockId, meta: Record<string, unknown>): void;
    clear(paneId: PaneId): void;
}

/** Ação contextual registrável para exibição no cabeçalho de um bloco de comando. */
export interface BlockAction {
    id: string;
    label: string;
    icon: string;
    order: number;
    when(block: CommandBlockModel): boolean;
    run(block: CommandBlockModel, ctx: TerminalContext): void | Promise<void>;
}

export type { AttachmentId };
