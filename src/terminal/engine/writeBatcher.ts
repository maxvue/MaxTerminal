import { MtTerminalError, type Disposable, type FlowConfig } from '@/types/terminal';

/** Frame individual enfileirado no `WriteBatcher`. */
export interface BatchFrameInput {
    seq: bigint;
    byteOffset: bigint;
    bytes: Uint8Array;
    onParsed?: () => void;
}

/** Alvo de escrita binária compatível com `Terminal.write` do `@xterm/xterm`. */
export interface XtermWriteTarget {
    write(data: Uint8Array, callback?: () => void): void;
}

/** Opções de construção do `WriteBatcher`. */
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

/** Limite máximo de bytes somando fila + lote em voo (1 MiB). */
const MAX_TOTAL_BUFFERED_BYTES = 1024 * 1024;

/**
 * Acumulador de frames de saída do PTY com agendamento por `requestAnimationFrame` (visível)
 * ou `setTimeout(16)` (oculto), orçamento por frame e no máximo 1 `xterm.write` em voo.
 */
export class WriteBatcher implements Disposable {
    private readonly target: XtermWriteTarget;
    private flowConfig: FlowConfig;
    private readonly onChunkEnqueued: ((byteLength: number) => void) | undefined;
    private readonly onBatchParsed:
        | ((info: { byteLength: number; confirmedCursor: { nextSeq: string; byteOffset: string } }) => void)
        | undefined;
    private readonly onQueueOverflow: ((error: Error) => void) | undefined;

    private readonly queue: BatchFrameInput[] = [];
    private queuedBytes = 0;
    private inFlight = false;
    private inFlightBytes = 0;
    private visible = true;
    private rafId: number | null = null;
    private timerId: ReturnType<typeof setTimeout> | null = null;
    private confirmedCursor: { nextSeq: string; byteOffset: string } | null = null;
    private generation = 0;
    private disposed = false;
    private idleResolvers: Array<() => void> = [];

    public constructor(opts: WriteBatcherOptions) {
        this.target = opts.target;
        this.flowConfig = { ...opts.flowConfig };
        this.onChunkEnqueued = opts.onChunkEnqueued;
        this.onBatchParsed = opts.onBatchParsed;
        this.onQueueOverflow = opts.onQueueOverflow;
    }

    /** Enfileira um frame binário preservando fronteiras para confirmação de cursor pós-parse. */
    public push(frame: BatchFrameInput): void {
        if (this.disposed) {
            return;
        }
        const byteLen = frame.bytes.byteLength;
        if (byteLen === 0) {
            frame.onParsed?.();
            return;
        }

        if (this.queuedBytes + this.inFlightBytes + byteLen > MAX_TOTAL_BUFFERED_BYTES) {
            const overflowErr = new MtTerminalError(
                'terminal.queue_overflow',
                'Limite de 1 MiB da fila de renderização do terminal foi excedido.'
            );
            this.onQueueOverflow?.(overflowErr);
            throw overflowErr;
        }

        this.queue.push(frame);
        this.queuedBytes += byteLen;
        this.onChunkEnqueued?.(byteLen);
        this.scheduleFlush();
    }

    /** Drena imediatamente até `frameBudgetBytes` em uma única chamada `target.write`. */
    public flush(): void {
        this.cancelScheduled();
        if (this.disposed || this.inFlight || this.queue.length === 0) {
            return;
        }

        const budget = Math.max(1, this.flowConfig.frameBudgetBytes);
        const batchFrames: BatchFrameInput[] = [];
        let batchBytes = 0;

        while (this.queue.length > 0) {
            const next = this.queue[0]!;
            if (batchFrames.length > 0 && batchBytes + next.bytes.byteLength > budget) {
                break;
            }
            this.queue.shift();
            batchFrames.push(next);
            batchBytes += next.bytes.byteLength;
        }

        this.queuedBytes = Math.max(0, this.queuedBytes - batchBytes);

        let payload: Uint8Array;
        if (batchFrames.length === 1) {
            payload = batchFrames[0]!.bytes;
        } else {
            payload = new Uint8Array(batchBytes);
            let offset = 0;
            for (const item of batchFrames) {
                payload.set(item.bytes, offset);
                offset += item.bytes.byteLength;
            }
        }

        const lastFrame = batchFrames[batchFrames.length - 1]!;
        const nextCursor = {
            nextSeq: (lastFrame.seq + 1n).toString(10),
            byteOffset: (lastFrame.byteOffset + BigInt(lastFrame.bytes.byteLength)).toString(10)
        };

        this.inFlight = true;
        this.inFlightBytes = batchBytes;
        const gen = this.generation;

        this.target.write(payload, () => {
            if (gen !== this.generation || this.disposed) {
                return;
            }
            this.inFlight = false;
            this.inFlightBytes = 0;
            this.confirmedCursor = nextCursor;

            this.onBatchParsed?.({
                byteLength: batchBytes,
                confirmedCursor: nextCursor
            });

            for (const item of batchFrames) {
                item.onParsed?.();
            }

            if (this.queue.length > 0) {
                this.scheduleFlush();
            } else {
                this.notifyIdle();
            }
        });
    }

    /** Alterna entre agendamento por `requestAnimationFrame` (visível) e `setTimeout(16)` (oculto). */
    public setVisible(visible: boolean): void {
        if (this.disposed || this.visible === visible) {
            return;
        }
        this.visible = visible;
        if (!visible && this.rafId !== null) {
            if (typeof cancelAnimationFrame === 'function') {
                cancelAnimationFrame(this.rafId);
            }
            this.rafId = null;
            if (!this.inFlight && this.queue.length > 0) {
                this.scheduleFlush();
            }
        } else if (visible && this.timerId !== null) {
            clearTimeout(this.timerId);
            this.timerId = null;
            if (!this.inFlight && this.queue.length > 0) {
                this.scheduleFlush();
            }
        }
    }

    /** Atualiza parâmetros de orçamento por frame. */
    public updateFlowConfig(cfg: Partial<FlowConfig>): void {
        this.flowConfig = {
            ...this.flowConfig,
            ...cfg
        };
    }

    /** Retorna uma Promise resolvida quando a fila esvaziar e nenhum write estiver em voo. */
    public whenIdle(): Promise<void> {
        if (this.disposed || (!this.inFlight && this.queue.length === 0)) {
            return Promise.resolve();
        }
        return new Promise<void>((resolve) => {
            this.idleResolvers.push(resolve);
        });
    }

    /** Descarta frames ainda não confirmados sem avançar `confirmedCursor`. */
    public clearUnparsed(): void {
        this.generation += 1;
        this.cancelScheduled();
        this.queue.length = 0;
        this.queuedBytes = 0;
        this.inFlight = false;
        this.inFlightBytes = 0;
        this.notifyIdle();
    }

    public getQueuedBytes(): number {
        return this.queuedBytes + this.inFlightBytes;
    }

    public getConfirmedCursor(): { nextSeq: string; byteOffset: string } | null {
        return this.confirmedCursor ? { ...this.confirmedCursor } : null;
    }

    public isWriteInFlight(): boolean {
        return this.inFlight;
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;
        this.clearUnparsed();
    }

    private scheduleFlush(): void {
        if (this.disposed || this.inFlight || this.queue.length === 0) {
            return;
        }
        if (this.rafId !== null || this.timerId !== null) {
            return;
        }

        const isDocHidden = typeof document !== 'undefined' && document.hidden === true;
        if (this.visible && !isDocHidden && typeof requestAnimationFrame === 'function') {
            this.rafId = requestAnimationFrame(() => {
                this.rafId = null;
                this.flush();
            });
        } else {
            this.timerId = setTimeout(() => {
                this.timerId = null;
                this.flush();
            }, 16);
        }
    }

    private cancelScheduled(): void {
        if (this.rafId !== null) {
            if (typeof cancelAnimationFrame === 'function') {
                cancelAnimationFrame(this.rafId);
            }
            this.rafId = null;
        }
        if (this.timerId !== null) {
            clearTimeout(this.timerId);
            this.timerId = null;
        }
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
