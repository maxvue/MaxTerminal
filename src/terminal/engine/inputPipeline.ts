import type { PtyId, PtyInput } from '@/types/bindings';
import { MtTerminalError, type Disposable } from '@/types/terminal';
import { encodeBinary, ptyClient } from '@/terminal/stream/ptyClient';

/** Opções de inicialização do `InputPipeline`. */
export interface InputPipelineOptions {
    ptyId: PtyId;
    pasteToXterm: (text: string) => void;
    isBracketedPasteEnabled: () => boolean;
    onMultilineUnbracketedWarning?: (() => void) | undefined;
    onWriteError?: ((error: unknown, acceptedBytes: number) => void) | undefined;
    onProcessExited?: (() => void) | undefined;
}

interface QueuedInputItem {
    kind: 'text' | 'bytes';
    text?: string;
    bytes?: Uint8Array;
    byteLength: number;
    isPaste: boolean;
    openedBracketedPaste: boolean;
}

const BRACKETED_PASTE_START = '\x1b[200~';
const BRACKETED_PASTE_END = '\x1b[201~';

function extractErrorCode(err: unknown): string | null {
    if (typeof err === 'object' && err !== null && 'code' in err) {
        const code = (err as { code?: unknown }).code;
        if (typeof code === 'string') {
            return code;
        }
    }
    return null;
}

function sleepMs(ms: number): Promise<void> {
    return new Promise((resolve) => {
        setTimeout(resolve, ms);
    });
}

/**
 * Divide uma string em fatias cuja codificação UTF-8 nunca exceda `maxBytes`,
 * preservando fronteiras de code points Unicode (sem partir pares substitutos UTF-16).
 */
export function splitTextUtf8Safe(text: string, maxBytes: number): string[] {
    const encoder = new TextEncoder();
    const encoded = encoder.encode(text);
    if (encoded.byteLength <= maxBytes) {
        return [text];
    }

    const chunks: string[] = [];
    let startIdx = 0;
    let currentBytes = 0;

    for (let i = 0; i < text.length; ) {
        const codePoint = text.codePointAt(i);
        if (codePoint === undefined) {
            break;
        }
        const charLen = codePoint > 0xffff ? 2 : 1;
        let byteLen = 4;
        if (codePoint <= 0x7f) {
            byteLen = 1;
        } else if (codePoint <= 0x7ff) {
            byteLen = 2;
        } else if (codePoint <= 0xffff) {
            byteLen = 3;
        }

        if (currentBytes + byteLen > maxBytes && i > startIdx) {
            chunks.push(text.slice(startIdx, i));
            startIdx = i;
            currentBytes = 0;
        }

        currentBytes += byteLen;
        i += charLen;
    }

    if (startIdx < text.length) {
        chunks.push(text.slice(startIdx));
    }

    return chunks;
}

/**
 * Pipeline único de escrita no PTY com coalescência por `queueMicrotask`,
 * separação estrita texto vs. binário, fatiamento de 64 KiB e retry em `pty.write_queue_full`.
 */
export class InputPipeline implements Disposable {
    public static readonly MAX_CHUNK_BYTES = 64 * 1024;
    public static readonly MAX_QUEUE_BYTES = 1024 * 1024;
    public static readonly WRITE_FULL_RETRY_MS = 10;
    public static readonly WRITE_FULL_MAX_RETRIES = 50;

    private readonly opts: InputPipelineOptions;
    private readonly encoder = new TextEncoder();
    private readonly queue: QueuedInputItem[] = [];
    private queuedBytes = 0;
    private flushing = false;
    private microtaskScheduled = false;
    private exited = false;
    private disposed = false;
    private inPasteContext = false;
    private bracketedPasteOpenInFlight = false;
    private sendLineLock = false;
    private generation = 0;
    private idleResolvers: Array<() => void> = [];

    public constructor(opts: InputPipelineOptions) {
        this.opts = opts;
    }

    /** Recebe entrada textual UTF-8 (ex.: `xterm.onData`). */
    public onData(text: string): void {
        if (this.disposed || this.exited || text.length === 0) {
            return;
        }

        const byteLen = this.encoder.encode(text).byteLength;
        if (this.queuedBytes + byteLen > InputPipeline.MAX_QUEUE_BYTES) {
            throw new MtTerminalError(
                'input.queue_exceeded',
                'Limite da fila de entrada (1 MiB) excedido.'
            );
        }

        const isPaste = this.inPasteContext;
        const openedBracketed =
            text.includes(BRACKETED_PASTE_START) &&
            text.lastIndexOf(BRACKETED_PASTE_START) > text.lastIndexOf(BRACKETED_PASTE_END);

        const last = this.queue[this.queue.length - 1];
        if (last && last.kind === 'text') {
            last.text = (last.text ?? '') + text;
            last.byteLength += byteLen;
            last.isPaste = last.isPaste || isPaste;
            if (openedBracketed) {
                last.openedBracketedPaste = true;
            } else if (text.includes(BRACKETED_PASTE_END)) {
                last.openedBracketedPaste = false;
            }
        } else {
            this.queue.push({
                kind: 'text',
                text,
                byteLength: byteLen,
                isPaste,
                openedBracketedPaste: openedBracketed
            });
        }

        this.queuedBytes += byteLen;
        this.scheduleMicrotaskFlush();
    }

    /** Recebe sequência binária 0..255 (ex.: `xterm.onBinary` / relatórios de mouse legados). */
    public onBinary(binaryString: string): void {
        if (this.disposed || this.exited || binaryString.length === 0) {
            return;
        }

        let bytes: Uint8Array;
        try {
            bytes = encodeBinary(binaryString);
        } catch (err: unknown) {
            throw new MtTerminalError(
                'input.invalid_binary',
                err instanceof Error ? err.message : 'Sequência binária inválida.',
                err
            );
        }

        if (this.queuedBytes + bytes.byteLength > InputPipeline.MAX_QUEUE_BYTES) {
            throw new MtTerminalError(
                'input.queue_exceeded',
                'Limite da fila de entrada (1 MiB) excedido.'
            );
        }

        this.queue.push({
            kind: 'bytes',
            bytes,
            byteLength: bytes.byteLength,
            isPaste: false,
            openedBracketedPaste: false
        });
        this.queuedBytes += bytes.byteLength;
        this.scheduleMicrotaskFlush();
    }

    /** Delega colagem ao `xterm.paste(text)` preservando bracketed paste nativo. */
    public paste(text: string): void {
        if (this.disposed || this.exited || text.length === 0) {
            return;
        }
        const byteLen = this.encoder.encode(text).byteLength;
        if (this.queuedBytes + byteLen > InputPipeline.MAX_QUEUE_BYTES) {
            throw new MtTerminalError(
                'input.queue_exceeded',
                'Limite da fila de entrada (1 MiB) excedido.'
            );
        }

        this.inPasteContext = true;
        try {
            this.opts.pasteToXterm(text);
        } finally {
            this.inPasteContext = false;
        }
    }

    /**
     * Submete uma linha completa a partir do `RichInput`, respeitando bloqueio de IME (`isComposing`),
     * deduplicação de duplo `Enter` no mesmo ciclo e aviso de multilinha sem bracketed paste.
     */
    public async sendLine(text: string, opts?: { isComposing?: boolean }): Promise<void> {
        if (this.disposed || this.exited) {
            return;
        }
        if (opts?.isComposing === true) {
            return;
        }
        if (this.sendLineLock) {
            return;
        }

        const totalByteLen = this.encoder.encode(text).byteLength + 1;
        if (this.queuedBytes + totalByteLen > InputPipeline.MAX_QUEUE_BYTES) {
            throw new MtTerminalError(
                'input.queue_exceeded',
                'Limite da fila de entrada (1 MiB) excedido.'
            );
        }

        this.sendLineLock = true;
        try {
            if ((text.includes('\n') || text.includes('\r')) && !this.opts.isBracketedPasteEnabled()) {
                this.opts.onMultilineUnbracketedWarning?.();
            }

            if (text.length > 0) {
                this.paste(text);
            }
            this.onData('\r');
        } finally {
            queueMicrotask(() => {
                this.sendLineLock = false;
            });
        }

        await this.flushNow();
    }

    /**
     * Cancela fatias de colagem ainda pendentes na fila, fecha bracketed paste aberto se necessário
     * e envia `\x03` (`Ctrl+C`) imediatamente.
     */
    public async cancelPendingPasteAndInterrupt(): Promise<void> {
        if (this.disposed || this.exited) {
            return;
        }

        this.generation += 1;
        let hadOpenBracket = this.bracketedPasteOpenInFlight;

        for (let i = this.queue.length - 1; i >= 0; i--) {
            const item = this.queue[i]!;
            if (item.isPaste || item.openedBracketedPaste) {
                if (item.openedBracketedPaste) {
                    hadOpenBracket = true;
                }
                this.queuedBytes = Math.max(0, this.queuedBytes - item.byteLength);
                this.queue.splice(i, 1);
            }
        }

        this.bracketedPasteOpenInFlight = false;
        if (hadOpenBracket || this.opts.isBracketedPasteEnabled()) {
            if (hadOpenBracket) {
                this.onData(BRACKETED_PASTE_END);
            }
        }
        this.onData('\x03');
        await this.flushNow();
    }

    /** Marca o PTY como encerrado, bloqueando novas escritas. */
    public markExited(): void {
        this.exited = true;
        this.queue.length = 0;
        this.queuedBytes = 0;
        this.notifyIdle();
    }

    public isBlocked(): boolean {
        return this.exited || this.disposed;
    }

    public getQueuedBytes(): number {
        return this.queuedBytes;
    }

    /** Força o envio imediato de todos os itens atualmente enfileirados. */
    public async flushNow(): Promise<void> {
        if (this.disposed || this.exited) {
            return;
        }
        if (this.flushing) {
            return new Promise<void>((resolve) => {
                this.idleResolvers.push(resolve);
            });
        }
        await this.drainQueue();
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;
        this.generation += 1;
        this.queue.length = 0;
        this.queuedBytes = 0;
        this.notifyIdle();
    }

    private scheduleMicrotaskFlush(): void {
        if (this.microtaskScheduled || this.flushing || this.disposed) {
            return;
        }
        this.microtaskScheduled = true;
        queueMicrotask(() => {
            this.microtaskScheduled = false;
            if (!this.disposed && !this.flushing && this.queue.length > 0) {
                void this.drainQueue();
            }
        });
    }

    private async drainQueue(): Promise<void> {
        if (this.flushing || this.disposed || this.exited) {
            return;
        }
        this.flushing = true;

        try {
            while (this.queue.length > 0 && !this.disposed && !this.exited) {
                const item = this.queue.shift()!;
                this.queuedBytes = Math.max(0, this.queuedBytes - item.byteLength);
                const genAtItemStart = this.generation;
                let acceptedBytes = 0;

                if (item.kind === 'text' && item.text !== undefined) {
                    const chunks = splitTextUtf8Safe(item.text, InputPipeline.MAX_CHUNK_BYTES);
                    for (const chunk of chunks) {
                        if (this.disposed || this.exited || genAtItemStart !== this.generation) {
                            break;
                        }
                        if (chunk.includes(BRACKETED_PASTE_START)) {
                            this.bracketedPasteOpenInFlight = true;
                        }
                        const ok = await this.writeChunkWithRetry(
                            { kind: 'text', text: chunk },
                            acceptedBytes,
                            genAtItemStart
                        );
                        if (!ok) {
                            break;
                        }
                        if (chunk.includes(BRACKETED_PASTE_END)) {
                            this.bracketedPasteOpenInFlight = false;
                        }
                        acceptedBytes += this.encoder.encode(chunk).byteLength;
                    }
                } else if (item.kind === 'bytes' && item.bytes !== undefined) {
                    const raw = item.bytes;
                    for (let offset = 0; offset < raw.byteLength; offset += InputPipeline.MAX_CHUNK_BYTES) {
                        if (this.disposed || this.exited || genAtItemStart !== this.generation) {
                            break;
                        }
                        const slice = raw.slice(offset, offset + InputPipeline.MAX_CHUNK_BYTES);
                        const ok = await this.writeChunkWithRetry(
                            { kind: 'bytes', bytes: Array.from(slice) },
                            acceptedBytes,
                            genAtItemStart
                        );
                        if (!ok) {
                            break;
                        }
                        acceptedBytes += slice.byteLength;
                    }
                }
            }
        } finally {
            this.flushing = false;
            this.notifyIdle();
        }
    }

    private async writeChunkWithRetry(
        input: PtyInput,
        acceptedBytes: number,
        expectedGen: number
    ): Promise<boolean> {
        let retries = 0;

        while (!this.disposed && !this.exited && expectedGen === this.generation) {
            try {
                await ptyClient.write(this.opts.ptyId, input);
                return true;
            } catch (err: unknown) {
                if (this.disposed || expectedGen !== this.generation) {
                    return false;
                }
                const code = extractErrorCode(err);
                if (code === 'pty.already_exited' || code === 'pty.not_found') {
                    this.markExited();
                    this.opts.onProcessExited?.();
                    return false;
                }
                if (code === 'pty.write_queue_full') {
                    retries += 1;
                    if (retries > InputPipeline.WRITE_FULL_MAX_RETRIES) {
                        this.queue.length = 0;
                        this.queuedBytes = 0;
                        const timeoutErr = new MtTerminalError(
                            'input.write_timeout',
                            'Fila do PTY permaneceu cheia após 50 tentativas de escrita.',
                            err
                        );
                        this.opts.onWriteError?.(timeoutErr, acceptedBytes);
                        return false;
                    }
                    await sleepMs(InputPipeline.WRITE_FULL_RETRY_MS);
                    continue;
                }

                // Outros erros não são retentados para evitar duplicar comandos.
                this.queue.length = 0;
                this.queuedBytes = 0;
                this.opts.onWriteError?.(err, acceptedBytes);
                return false;
            }
        }

        return false;
    }

    private notifyIdle(): void {
        if (this.idleResolvers.length === 0) {
            return;
        }
        const resolvers = this.idleResolvers.splice(0, this.idleResolvers.length);
        for (const resolve of resolvers) {
            resolve();
        }
    }
}
