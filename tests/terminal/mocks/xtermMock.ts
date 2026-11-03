import type {
    IBuffer,
    IBufferLine,
    IBufferNamespace,
    IDecoration,
    IDecorationOptions,
    IDisposable,
    IMarker,
    ITerminalAddon,
    ITerminalOptions,
    Terminal
} from '@xterm/xterm';

/** Linha simulada do buffer do Xterm para testes unitários. */
export class MockBufferLine implements IBufferLine {
    public isWrapped: boolean;
    public length: number;
    private readonly text: string;

    public constructor(text: string, isWrapped = false) {
        this.text = text;
        this.isWrapped = isWrapped;
        this.length = text.length;
    }

    public getCell(_x: number): undefined {
        return undefined;
    }

    public translateToString(trimRight?: boolean, startColumn?: number, endColumn?: number): string {
        const sliced = this.text.slice(startColumn ?? 0, endColumn);
        return trimRight ? sliced.replace(/\s+$/, '') : sliced;
    }
}

/** Marker simulado do Xterm com suporte a `line`, `isDisposed` e `onDispose`. */
export class MockMarker implements IMarker {
    public readonly id: number;
    public line: number;
    public isDisposed = false;
    private readonly disposeListeners = new Set<() => void>();

    public constructor(id: number, line: number) {
        this.id = id;
        this.line = line;
    }

    public onDispose(listener: () => void): IDisposable {
        this.disposeListeners.add(listener);
        return {
            dispose: () => {
                this.disposeListeners.delete(listener);
            }
        };
    }

    public dispose(): void {
        if (this.isDisposed) {
            return;
        }
        this.isDisposed = true;
        this.line = -1;
        for (const cb of this.disposeListeners) {
            cb();
        }
        this.disposeListeners.clear();
    }
}

/** Decoração simulada do Xterm com elemento DOM real. */
export class MockDecoration implements IDecoration {
    public readonly marker: IMarker;
    public element: HTMLElement | undefined;
    public options: IDecorationOptions;
    public isDisposed = false;
    private readonly renderListeners = new Set<(el: HTMLElement) => void>();
    private readonly disposeListeners = new Set<() => void>();

    public constructor(marker: IMarker, options: IDecorationOptions) {
        this.marker = marker;
        this.options = options;
        this.element = document.createElement('div');
    }

    public onRender(listener: (element: HTMLElement) => void): IDisposable {
        this.renderListeners.add(listener);
        if (this.element) {
            listener(this.element);
        }
        return {
            dispose: () => {
                this.renderListeners.delete(listener);
            }
        };
    }

    public onDispose(listener: () => void): IDisposable {
        this.disposeListeners.add(listener);
        return {
            dispose: () => {
                this.disposeListeners.delete(listener);
            }
        };
    }

    public emitRender(): void {
        if (!this.isDisposed && this.element) {
            for (const cb of this.renderListeners) {
                cb(this.element);
            }
        }
    }

    public dispose(): void {
        if (this.isDisposed) {
            return;
        }
        this.isDisposed = true;
        for (const cb of this.disposeListeners) {
            cb();
        }
        this.renderListeners.clear();
        this.disposeListeners.clear();
    }
}

/** Buffer simulado do Xterm (`normal` ou `alternate`). */
export class MockBuffer implements IBuffer {
    public readonly type: 'normal' | 'alternate';
    public cursorY = 0;
    public cursorX = 0;
    public viewportY = 0;
    public baseY = 0;
    public lines: MockBufferLine[] = [new MockBufferLine('')];

    public constructor(type: 'normal' | 'alternate') {
        this.type = type;
    }

    public get length(): number {
        return this.lines.length;
    }

    public getLine(y: number): IBufferLine | undefined {
        return this.lines[y];
    }

    public getNullCell(): never {
        throw new Error('getNullCell não implementado no mock');
    }

    public setLines(entries: Array<{ text: string; isWrapped?: boolean }>): void {
        this.lines = entries.map((e) => new MockBufferLine(e.text, Boolean(e.isWrapped)));
        this.cursorY = Math.max(0, this.lines.length - 1);
    }
}

/** Mock controlável da classe `Terminal` de `@xterm/xterm`. */
export class MockXtermTerminal {
    public cols = 80;
    public rows = 24;
    public element: HTMLElement | undefined;
    public textarea: HTMLTextAreaElement | undefined;
    public options: Required<Pick<ITerminalOptions, 'fontFamily' | 'fontSize' | 'lineHeight' | 'letterSpacing' | 'scrollback' | 'cursorStyle' | 'cursorBlink'>> &
        ITerminalOptions = {
            fontFamily: 'JetBrainsMono Nerd Font, monospace',
            fontSize: 13,
            lineHeight: 1.15,
            letterSpacing: 0,
            scrollback: 10000,
            cursorStyle: 'block',
            cursorBlink: true,
            theme: {}
        };

    public modes: {
        mouseTrackingMode: 'none' | 'x10' | 'vt200' | 'drag' | 'any';
        bracketedPasteMode: boolean;
        applicationCursorKeysMode: boolean;
        insertMode: boolean;
        originMode: boolean;
        wraparoundMode: boolean;
        sendFocusMode: boolean;
    } = {
            mouseTrackingMode: 'none',
            bracketedPasteMode: false,
            applicationCursorKeysMode: false,
            insertMode: false,
            originMode: false,
            wraparoundMode: true,
            sendFocusMode: false
        };

    public readonly normalBuffer = new MockBuffer('normal');
    public readonly alternateBuffer = new MockBuffer('alternate');
    private activeBuffer: MockBuffer;

    public readonly buffer: IBufferNamespace;
    public readonly loadedAddons: ITerminalAddon[] = [];
    public readonly writtenChunks: Uint8Array[] = [];
    public readonly markers: MockMarker[] = [];
    public readonly decorations: MockDecoration[] = [];
    public autoInvokeWriteCallback = true;
    public pendingWriteCallbacks: Array<() => void> = [];
    public disposed = false;
    public openCount = 0;
    public refreshCalls: Array<{ start: number; end: number }> = [];
    public selectionText = '';

    private nextMarkerId = 1;
    private customKeyHandler: ((event: KeyboardEvent) => boolean) | null = null;
    private readonly dataListeners = new Set<(data: string) => void>();
    private readonly binaryListeners = new Set<(data: string) => void>();
    private readonly titleListeners = new Set<(title: string) => void>();
    private readonly renderListeners = new Set<(range: { start: number; end: number }) => void>();
    private readonly bufferChangeListeners = new Set<(buf: IBuffer) => void>();
    private readonly unicodeActiveBox = { activeVersion: '6' };

    public constructor(opts?: ITerminalOptions) {
        if (opts) {
            Object.assign(this.options, opts);
        }
        this.activeBuffer = this.normalBuffer;
        this.buffer = {
            get active(): IBuffer {
                return getActiveBuffer();
            },
            get normal(): IBuffer {
                return getNormalBuffer();
            },
            get alternate(): IBuffer {
                return getAlternateBuffer();
            },
            onBufferChange: (listener: (buffer: IBuffer) => void): IDisposable => {
                this.bufferChangeListeners.add(listener);
                return {
                    dispose: () => {
                        this.bufferChangeListeners.delete(listener);
                    }
                };
            }
        };
        const getActiveBuffer = (): IBuffer => this.activeBuffer;
        const getNormalBuffer = (): IBuffer => this.normalBuffer;
        const getAlternateBuffer = (): IBuffer => this.alternateBuffer;
    }

    public get unicode(): { activeVersion: string } {
        return this.unicodeActiveBox;
    }

    public open(parent: HTMLElement): void {
        this.openCount += 1;
        const el = document.createElement('div');
        el.className = 'xterm';
        const ta = document.createElement('textarea');
        ta.className = 'xterm-helper-textarea';
        el.appendChild(ta);
        parent.appendChild(el);
        this.element = el;
        this.textarea = ta;
    }

    public loadAddon(addon: ITerminalAddon): void {
        this.loadedAddons.push(addon);
        if (typeof addon.activate === 'function') {
            addon.activate(this as unknown as Terminal);
        }
    }

    public write(data: string | Uint8Array, callback?: () => void): void {
        const bytes = typeof data === 'string' ? new TextEncoder().encode(data) : data;
        this.writtenChunks.push(bytes);
        if (callback) {
            if (this.autoInvokeWriteCallback) {
                callback();
            } else {
                this.pendingWriteCallbacks.push(callback);
            }
        }
    }

    public flushPendingWriteCallbacks(): void {
        const cbs = this.pendingWriteCallbacks.splice(0, this.pendingWriteCallbacks.length);
        for (const cb of cbs) {
            cb();
        }
    }

    public resize(cols: number, rows: number): void {
        this.cols = cols;
        this.rows = rows;
    }

    public refresh(start: number, end: number): void {
        this.refreshCalls.push({ start, end });
        for (const cb of this.renderListeners) {
            cb({ start, end });
        }
    }

    public focus(): void {
        this.textarea?.focus();
    }

    public paste(data: string): void {
        const payload = this.modes.bracketedPasteMode ? `\x1b[200~${data}\x1b[201~` : data;
        this.emitData(payload);
    }

    public getSelection(): string {
        return this.selectionText;
    }

    public clearSelection(): void {
        this.selectionText = '';
    }

    public attachCustomKeyEventHandler(customKeyEventHandler: (event: KeyboardEvent) => boolean): void {
        this.customKeyHandler = customKeyEventHandler;
    }

    public triggerCustomKeyEvent(event: KeyboardEvent): boolean {
        return this.customKeyHandler ? this.customKeyHandler(event) : true;
    }

    public registerMarker(cursorYOffset = 0): IMarker {
        const line = Math.max(0, this.normalBuffer.baseY + this.normalBuffer.cursorY + cursorYOffset);
        const marker = new MockMarker(this.nextMarkerId++, line);
        this.markers.push(marker);
        return marker;
    }

    public registerDecoration(decorationOptions: IDecorationOptions): IDecoration | undefined {
        if (decorationOptions.marker.isDisposed) {
            return undefined;
        }
        const dec = new MockDecoration(decorationOptions.marker, decorationOptions);
        if (this.element && dec.element) {
            this.element.appendChild(dec.element);
        }
        this.decorations.push(dec);
        return dec;
    }

    public onData(listener: (data: string) => void): IDisposable {
        this.dataListeners.add(listener);
        return {
            dispose: () => {
                this.dataListeners.delete(listener);
            }
        };
    }

    public onBinary(listener: (data: string) => void): IDisposable {
        this.binaryListeners.add(listener);
        return {
            dispose: () => {
                this.binaryListeners.delete(listener);
            }
        };
    }

    public onTitleChange(listener: (title: string) => void): IDisposable {
        this.titleListeners.add(listener);
        return {
            dispose: () => {
                this.titleListeners.delete(listener);
            }
        };
    }

    public onRender(listener: (range: { start: number; end: number }) => void): IDisposable {
        this.renderListeners.add(listener);
        return {
            dispose: () => {
                this.renderListeners.delete(listener);
            }
        };
    }

    public emitData(data: string): void {
        for (const cb of this.dataListeners) {
            cb(data);
        }
    }

    public emitBinary(data: string): void {
        for (const cb of this.binaryListeners) {
            cb(data);
        }
    }

    public emitTitleChange(title: string): void {
        for (const cb of this.titleListeners) {
            cb(title);
        }
    }

    public switchBuffer(type: 'normal' | 'alternate'): void {
        this.activeBuffer = type === 'alternate' ? this.alternateBuffer : this.normalBuffer;
        for (const cb of this.bufferChangeListeners) {
            cb(this.activeBuffer);
        }
    }

    public dispose(): void {
        this.disposed = true;
        this.dataListeners.clear();
        this.binaryListeners.clear();
        this.titleListeners.clear();
        this.renderListeners.clear();
        this.bufferChangeListeners.clear();
        for (const dec of this.decorations) {
            dec.dispose();
        }
        for (const marker of this.markers) {
            marker.dispose();
        }
    }
}
