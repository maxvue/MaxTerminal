import { FitAddon } from '@xterm/addon-fit';
import { SearchAddon } from '@xterm/addon-search';
import { Unicode11Addon } from '@xterm/addon-unicode11';
import { WebLinksAddon } from '@xterm/addon-web-links';
import { Terminal, type ITheme } from '@xterm/xterm';
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
import { resolveTerminalContext } from '@/terminal/extensions/terminalContext';
import { attachExtensionsToInstance } from '@/terminal/extensions/terminalExtensions';
import { isCopyShortcut, isPasteShortcut } from '@/terminal/keybindings';
import { createOsc8LinkHandler, dispatchTerminalLink } from '@/terminal/links/linkHandlerRegistry';
import { ptyStreamHub, type PtyStreamSubscription } from '@/terminal/stream/ptyStreamHub';
import { themeService } from '@/terminal/theme/themeService';
import { FitController } from './fitController';
import { FlowController } from './flowController';
import { InputPipeline } from './inputPipeline';
import { ModeTracker } from './modeTracker';
import { RendererManager, type WebglAddonLike } from './rendererManager';
import { WriteBatcher } from './writeBatcher';

/** Callbacks emitidos pela `TerminalInstance` para sincronizar a store e componentes. */
export interface TerminalInstanceCallbacks {
    onReady?: ((handle: TerminalInstanceHandle) => void) | undefined;
    onDimensionsChange?: ((cols: number, rows: number) => void) | undefined;
    onRendererChange?: ((renderer: RendererKind) => void) | undefined;
    onModesChange?: ((modes: TerminalModes, prevModes: TerminalModes) => void) | undefined;
    onTitleChange?: ((title: string) => void) | undefined;
    onThemeChange?: ((theme: TerminalTheme) => void) | undefined;
    onFlowStateChange?:
        | ((flow: {
            paused: boolean;
            pendingBytes: number;
            droppedBytes: string;
            cursor: { nextSeq: string; byteOffset: string } | null;
            partialView: boolean;
            frameErrors: number;
        }) => void)
        | undefined;
    onExit?: ((exit: ExitInfo) => void) | undefined;
}

/** Opções de criação da `TerminalInstance`. */
export interface TerminalInstanceOptions {
    paneId: PaneId;
    ptyId: PtyId;
    prefs: TerminalPreferences;
    theme: TerminalTheme;
    flow: FlowConfig;
    callbacks?: TerminalInstanceCallbacks | undefined;
    createTerminal?: ((opts: ConstructorParameters<typeof Terminal>[0]) => Terminal) | undefined;
    createWebglAddon?: (() => WebglAddonLike) | undefined;
}

function toXtermTheme(theme: TerminalTheme): ITheme {
    const ansi = theme.colors.ansi;
    const xtermTheme: ITheme = {
        foreground: theme.colors.foreground,
        background: theme.colors.background,
        cursor: theme.colors.cursor,
        cursorAccent: theme.colors.cursorAccent,
        selectionBackground: theme.colors.selectionBackground,
        black: ansi[0],
        red: ansi[1],
        green: ansi[2],
        yellow: ansi[3],
        blue: ansi[4],
        magenta: ansi[5],
        cyan: ansi[6],
        white: ansi[7],
        brightBlack: ansi[8],
        brightRed: ansi[9],
        brightGreen: ansi[10],
        brightYellow: ansi[11],
        brightBlue: ansi[12],
        brightMagenta: ansi[13],
        brightCyan: ansi[14],
        brightWhite: ansi[15]
    };
    if (theme.colors.selectionForeground !== undefined) {
        xtermTheme.selectionForeground = theme.colors.selectionForeground;
    }
    if (theme.colors.selectionInactiveBackground !== undefined) {
        xtermTheme.selectionInactiveBackground = theme.colors.selectionInactiveBackground;
    }
    return xtermTheme;
}

/**
 * Instância completa do emulador `@xterm/xterm` vinculada a um `PaneId` e um `PtyId`.
 * Gerencia ciclo de vida de addons, stream, batching, backpressure e anexação/desanexação DOM.
 */
export class TerminalInstance implements TerminalInstanceHandle, Disposable {
    public readonly paneId: PaneId;
    public readonly ptyId: PtyId;
    public readonly xterm: Terminal;
    public readonly searchAddon: SearchAddon;
    public readonly inputPipeline: InputPipeline;

    private prefs: TerminalPreferences;
    private currentTheme: TerminalTheme;
    private callbacks: TerminalInstanceCallbacks;

    private readonly innerContainer: HTMLDivElement;
    private readonly fitAddon: FitAddon;
    private readonly fitController: FitController;
    private readonly rendererManager: RendererManager;
    private readonly modeTracker: ModeTracker;
    private readonly writeBatcher: WriteBatcher;
    private readonly flowController: FlowController;
    private readonly extensionsSub: Disposable;
    private readonly themeServiceSub: Disposable;
    private readonly xtermSubs: Array<{ dispose(): void }> = [];
    private readonly afterFlushListeners = new Set<() => void>();
    private readonly customKeyHandlers: Array<(event: KeyboardEvent) => boolean> = [];

    private streamSubscription: PtyStreamSubscription | null = null;
    private subscribingPromise: Promise<void> | null = null;
    private openedOnce = false;
    private visible = true;
    private disposed = false;

    private droppedBytesTotal = 0n;
    private partialView = false;
    private frameErrorsCount = 0;
    private flowPaused = false;
    private flowPendingBytes = 0;
    private flowThrottleTimer: ReturnType<typeof setTimeout> | null = null;

    public constructor(opts: TerminalInstanceOptions) {
        this.paneId = opts.paneId;
        this.ptyId = opts.ptyId;
        this.prefs = { ...opts.prefs, macOptionIsMeta: false };
        this.currentTheme = opts.theme;
        this.callbacks = opts.callbacks ?? {};

        this.innerContainer = document.createElement('div');
        this.innerContainer.className = 'mt-xterm-inner';
        this.innerContainer.style.width = '100%';
        this.innerContainer.style.height = '100%';

        const xtermOptions: ConstructorParameters<typeof Terminal>[0] = {
            cols: 80,
            rows: 24,
            fontFamily: this.prefs.fontFamily,
            fontSize: this.prefs.fontSize,
            lineHeight: this.prefs.lineHeight,
            letterSpacing: this.prefs.letterSpacing,
            scrollback: this.prefs.scrollback,
            cursorStyle: this.prefs.cursorStyle,
            cursorBlink: this.prefs.cursorBlink,
            macOptionIsMeta: false,
            allowProposedApi: true,
            theme: toXtermTheme(this.currentTheme),
            linkHandler: createOsc8LinkHandler(() => this.getContext())
        };

        this.xterm = opts.createTerminal ? opts.createTerminal(xtermOptions) : new Terminal(xtermOptions);

        // 1. Unicode11Addon
        try {
            const unicode11 = new Unicode11Addon();
            this.xterm.loadAddon(unicode11);
            if (this.xterm.unicode) {
                this.xterm.unicode.activeVersion = '11';
            }
        } catch {
            // Segue com versão padrão se ambiente sintético não suportar.
        }

        // 2. FitAddon
        this.fitAddon = new FitAddon();
        try {
            this.xterm.loadAddon(this.fitAddon);
        } catch {
            // Ignora em mocks simplificados.
        }

        // 3. SearchAddon
        this.searchAddon = new SearchAddon({ highlightLimit: 1000 });
        try {
            this.xterm.loadAddon(this.searchAddon);
        } catch {
            // Ignora em mocks simplificados.
        }

        // 4. WebLinksAddon
        try {
            const webLinks = new WebLinksAddon((event, uri) => {
                void dispatchTerminalLink(event, uri, this.getContext());
            });
            this.xterm.loadAddon(webLinks);
        } catch {
            // Ignora em mocks simplificados.
        }

        // 5. RendererManager
        this.rendererManager = new RendererManager({
            xterm: this.xterm,
            webglEnabled: this.prefs.webglEnabled,
            visible: true,
            createWebglAddon: opts.createWebglAddon,
            onRendererChange: (kind) => {
                this.callbacks.onRendererChange?.(kind);
            }
        });

        // 6. ModeTracker
        this.modeTracker = new ModeTracker({
            xterm: this.xterm,
            onModesChange: (modes, prevModes) => {
                this.callbacks.onModesChange?.(modes, prevModes);
            },
            onTitleChange: (title) => {
                this.callbacks.onTitleChange?.(title);
            }
        });

        // 7. FlowController & WriteBatcher
        this.flowController = new FlowController(
            opts.flow,
            {
                ack: async (consumedBytes) => {
                    if (this.streamSubscription) {
                        await this.streamSubscription.ack(consumedBytes);
                    }
                }
            },
            (state) => {
                const pausedChanged = this.flowPaused !== state.paused;
                this.flowPaused = state.paused;
                this.flowPendingBytes = state.pendingBytes;
                this.emitFlowState(pausedChanged);
            }
        );

        this.writeBatcher = new WriteBatcher({
            target: this.xterm,
            flowConfig: opts.flow,
            onChunkEnqueued: (byteLength) => {
                this.flowController.onEnqueued(byteLength);
            },
            onBatchParsed: ({ byteLength }) => {
                this.flowController.onWritten(byteLength);
                this.modeTracker.sampleModes();
                this.emitFlowState(false);
                for (const listener of this.afterFlushListeners) {
                    try {
                        listener();
                    } catch {
                        // Ignora falha de listener individual pós-flush.
                    }
                }
            }
        });

        // 8. InputPipeline
        this.inputPipeline = new InputPipeline({
            ptyId: this.ptyId,
            pasteToXterm: (text) => {
                this.xterm.paste(text);
            },
            isBracketedPasteEnabled: () => {
                return this.modeTracker.getModes().bracketedPaste;
            }
        });

        if (this.xterm.onData) {
            this.xtermSubs.push(
                this.xterm.onData((data) => {
                    this.inputPipeline.onData(data);
                })
            );
        }

        if (this.xterm.onBinary) {
            this.xtermSubs.push(
                this.xterm.onBinary((binary) => {
                    this.inputPipeline.onBinary(binary);
                })
            );
        }

        if (this.xterm.attachCustomKeyEventHandler) {
            this.xterm.attachCustomKeyEventHandler((event: KeyboardEvent) => {
                if (event.type !== 'keydown') {
                    return true;
                }
                if (isCopyShortcut(event)) {
                    const sel = this.xterm.getSelection?.() ?? '';
                    if (sel.length > 0 && typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
                        void navigator.clipboard.writeText(sel).catch(() => {});
                    }
                    return false;
                }
                if (isPasteShortcut(event)) {
                    if (typeof navigator !== 'undefined' && navigator.clipboard?.readText) {
                        void navigator.clipboard
                            .readText()
                            .then((text) => {
                                if (text) {
                                    this.paste(text);
                                }
                            })
                            .catch(() => {});
                    }
                    return false;
                }
                for (const handler of this.customKeyHandlers) {
                    try {
                        const allow = handler(event);
                        if (!allow) {
                            return false;
                        }
                    } catch {
                        // Ignora falhas em handlers externos.
                    }
                }
                return true;
            });
        }

        // 9. FitController
        this.fitController = new FitController({
            ptyId: this.ptyId,
            xterm: this.xterm,
            fitAddon: this.fitAddon,
            onDimensionsChanged: (cols, rows) => {
                this.callbacks.onDimensionsChange?.(cols, rows);
            }
        });

        // 10. Extensões e ThemeService
        this.extensionsSub = attachExtensionsToInstance(this);
        this.themeServiceSub = themeService.onChange((theme) => {
            this.applyTheme(theme);
        });
    }

    /** Registra um interceptor de teclado síncrono descartável no xterm. */
    public registerKeyHandler(handler: (event: KeyboardEvent) => boolean): Disposable {
        this.customKeyHandlers.push(handler);
        return {
            dispose: () => {
                const idx = this.customKeyHandlers.indexOf(handler);
                if (idx !== -1) {
                    this.customKeyHandlers.splice(idx, 1);
                }
            }
        };
    }

    /** Anexa (ou reanexa) a instância a um container DOM sem perder o buffer nem reabrir o xterm. */
    public async attach(host: HTMLElement): Promise<void> {
        if (this.disposed) {
            return;
        }

        if (this.innerContainer.parentElement !== host) {
            host.appendChild(this.innerContainer);
        }

        if (!this.openedOnce) {
            this.openedOnce = true;
            this.xterm.open(this.innerContainer);
        }

        this.visible = true;
        this.writeBatcher.setVisible(true);
        this.fitController.setVisible(true);
        const activeRenderer = this.rendererManager.activate();
        this.callbacks.onRendererChange?.(activeRenderer);
        this.callbacks.onThemeChange?.(this.currentTheme);

        this.fitController.observe(host);
        await this.ensureSubscribed();
        await this.fitController.requestFit(true);

        this.callbacks.onReady?.(this);
    }

    /** Desanexa do container DOM preservando o buffer e o stream em segundo plano. */
    public detach(): void {
        if (this.disposed) {
            return;
        }
        this.fitController.unobserve();
        this.setVisible(false);
        if (this.innerContainer.parentElement) {
            this.innerContainer.remove();
        }
    }

    /** Atualiza visibilidade da instância (suspende WebGL e muda batcher para timer quando oculta). */
    public setVisible(visible: boolean): void {
        if (this.disposed || this.visible === visible) {
            return;
        }
        this.visible = visible;
        this.writeBatcher.setVisible(visible);
        this.rendererManager.setVisible(visible);
        this.fitController.setVisible(visible);
        this.callbacks.onRendererChange?.(this.rendererManager.getKind());
    }

    public focus(): void {
        if (this.disposed) {
            return;
        }
        this.xterm.focus();
    }

    /** Aplica um novo `TerminalTheme` criando um novo objeto `ITheme` e propagando `theme.ui`. */
    public applyTheme(theme: TerminalTheme): void {
        if (this.disposed) {
            return;
        }
        this.currentTheme = theme;
        this.xterm.options.theme = toXtermTheme(theme);
        this.callbacks.onThemeChange?.(theme);
    }

    public getTheme(): TerminalTheme {
        return this.currentTheme;
    }

    /** Aplica preferências visuais em tempo real na instância ativa. */
    public applyPreferences(prefs: Partial<TerminalPreferences>): void {
        if (this.disposed) {
            return;
        }
        this.prefs = {
            ...this.prefs,
            ...prefs,
            macOptionIsMeta: false
        };
        this.xterm.options.fontFamily = this.prefs.fontFamily;
        this.xterm.options.fontSize = this.prefs.fontSize;
        this.xterm.options.lineHeight = this.prefs.lineHeight;
        this.xterm.options.letterSpacing = this.prefs.letterSpacing;
        this.xterm.options.scrollback = this.prefs.scrollback;
        this.xterm.options.cursorStyle = this.prefs.cursorStyle;
        this.xterm.options.cursorBlink = this.prefs.cursorBlink;

        this.rendererManager.setWebglEnabled(this.prefs.webglEnabled);
        this.callbacks.onRendererChange?.(this.rendererManager.getKind());
        void this.fitController.requestFit(false);
    }

    public paste(text: string): void {
        if (this.disposed) {
            return;
        }
        this.inputPipeline.paste(text);
    }

    public async sendInput(text: string): Promise<void> {
        if (this.disposed) {
            return;
        }
        this.inputPipeline.onData(text);
        await this.inputPipeline.flushNow();
    }

    public async sendLine(text: string, opts?: { isComposing?: boolean }): Promise<void> {
        if (this.disposed) {
            return;
        }
        await this.inputPipeline.sendLine(text, opts);
    }

    public getContext(): TerminalContext {
        return resolveTerminalContext({
            paneId: this.paneId,
            ptyId: this.ptyId
        });
    }

    /** Registra um callback invocado após cada lote de escrita confirmado no Xterm. */
    public onAfterFlush(listener: () => void): () => void {
        if (this.disposed) {
            return () => {};
        }
        this.afterFlushListeners.add(listener);
        return () => {
            this.afterFlushListeners.delete(listener);
        };
    }

    public setCallbacks(callbacks: TerminalInstanceCallbacks): void {
        this.callbacks = callbacks;
    }

    public forceWebglContextLossForDebug(): void {
        this.rendererManager.forceContextLossForDebug();
    }

    public isDisposed(): boolean {
        return this.disposed;
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;

        if (this.flowThrottleTimer !== null) {
            clearTimeout(this.flowThrottleTimer);
            this.flowThrottleTimer = null;
        }

        this.themeServiceSub.dispose();
        this.extensionsSub.dispose();
        this.afterFlushListeners.clear();
        this.fitController.dispose();
        this.modeTracker.dispose();
        this.inputPipeline.dispose();
        this.writeBatcher.dispose();
        this.flowController.dispose();
        this.rendererManager.dispose();

        if (this.streamSubscription) {
            this.streamSubscription.dispose();
            this.streamSubscription = null;
        }
        ptyStreamHub.release(this.ptyId);

        for (const sub of this.xtermSubs) {
            sub.dispose();
        }
        this.xtermSubs.length = 0;

        try {
            this.xterm.dispose();
        } catch {
            // Ignora falhas de dispose do xterm em ambientes de teste.
        }

        if (this.innerContainer.parentElement) {
            this.innerContainer.remove();
        }
    }

    private async ensureSubscribed(): Promise<void> {
        if (this.streamSubscription || this.disposed) {
            return;
        }
        if (this.subscribingPromise) {
            await this.subscribingPromise;
            return;
        }

        this.subscribingPromise = (async () => {
            this.flowController.reset();
            const sub = await ptyStreamHub.subscribe(
                this.ptyId,
                {
                    onData: (frame) => {
                        return new Promise<void>((resolve) => {
                            try {
                                this.writeBatcher.push({
                                    seq: frame.seq,
                                    byteOffset: frame.byteOffset,
                                    bytes: frame.bytes,
                                    onParsed: () => {
                                        resolve();
                                    }
                                });
                            } catch (err: unknown) {
                                this.frameErrorsCount += 1;
                                this.emitFlowState(true);
                                resolve();
                                throw err;
                            }
                        });
                    },
                    onGap: (droppedBytes) => {
                        this.droppedBytesTotal += droppedBytes;
                        this.partialView = true;
                        this.emitFlowState(true);
                    },
                    onExit: (exit) => {
                        void this.writeBatcher.whenIdle().then(() => {
                            if (this.disposed) {
                                return;
                            }
                            this.fitController.markPtyExited();
                            this.inputPipeline.markExited();
                            this.emitFlowState(true);
                            this.callbacks.onExit?.(exit);
                        });
                    },
                    onFrameError: () => {
                        this.frameErrorsCount += 1;
                        this.emitFlowState(true);
                    }
                },
                {
                    cursor: this.writeBatcher.getConfirmedCursor(),
                    onAttachmentReady: (_attachmentId, truncated) => {
                        if (truncated) {
                            this.partialView = true;
                        }
                        this.flowController.setAttachmentReady(true);
                        this.emitFlowState(true);
                    }
                }
            );

            if (this.disposed) {
                sub.dispose();
                return;
            }
            this.streamSubscription = sub;
        })();

        try {
            await this.subscribingPromise;
        } finally {
            this.subscribingPromise = null;
        }
    }

    private emitFlowState(immediate: boolean): void {
        if (this.disposed) {
            return;
        }
        const send = (): void => {
            if (this.flowThrottleTimer !== null) {
                clearTimeout(this.flowThrottleTimer);
                this.flowThrottleTimer = null;
            }
            this.callbacks.onFlowStateChange?.({
                paused: this.flowPaused,
                pendingBytes: this.flowPendingBytes,
                droppedBytes: this.droppedBytesTotal.toString(10),
                cursor: this.writeBatcher.getConfirmedCursor(),
                partialView: this.partialView,
                frameErrors: this.frameErrorsCount
            });
        };

        if (immediate) {
            send();
            return;
        }

        if (this.flowThrottleTimer === null) {
            this.flowThrottleTimer = setTimeout(() => {
                this.flowThrottleTimer = null;
                send();
            }, 250);
        }
    }
}
