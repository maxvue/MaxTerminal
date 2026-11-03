import type { Disposable, FlowConfig } from '@/types/terminal';

/** Destino de envio de confirmações cumulativas (`pty_ack`) ao backend. */
export interface AckSink {
    ack(consumedBytes: string): Promise<void>;
}

/** Fotografia interna do estado do controlador de fluxo. */
export interface FlowControllerStateSnapshot {
    paused: boolean;
    pendingBytes: number;
    consumedTotal: bigint;
    unflushedAckBytes: bigint;
}

function extractErrorCode(err: unknown): string | null {
    if (typeof err === 'object' && err !== null && 'code' in err) {
        const code = (err as { code?: unknown }).code;
        if (typeof code === 'string') {
            return code;
        }
    }
    return null;
}

/**
 * Controlador de crédito e backpressure do stream do PTY.
 * Contabiliza bytes cumulativos em `bigint` e envia `pty_ack` somente após o parse no xterm.
 */
export class FlowController implements Disposable {
    private cfg: FlowConfig;
    private readonly sink: AckSink;
    private readonly onState: (state: { paused: boolean; pendingBytes: number }) => void;

    private pendingBytes = 0;
    private consumedTotal = 0n;
    private lastFlushedTotal = 0n;
    private paused = false;
    private attachmentReady = false;
    private ackInFlight = false;
    private timer: ReturnType<typeof setTimeout> | null = null;
    private generation = 0;
    private disposed = false;

    public constructor(
        cfg: FlowConfig,
        sink: AckSink,
        onState: (state: { paused: boolean; pendingBytes: number }) => void
    ) {
        this.cfg = { ...cfg };
        this.sink = sink;
        this.onState = onState;
    }

    /** Registra bytes enfileirados no `WriteBatcher` antes do parse pelo xterm. */
    public onEnqueued(bytesLength: number): void {
        if (this.disposed || bytesLength <= 0) {
            return;
        }
        this.pendingBytes += bytesLength;
        this.updatePausedAndNotify();
    }

    /**
     * Registra bytes efetivamente processados pelo parser do `xterm.write` (frames `Data` e `Replay`).
     */
    public onWritten(bytesLength: number): void {
        if (this.disposed || bytesLength <= 0) {
            return;
        }
        this.pendingBytes = Math.max(0, this.pendingBytes - bytesLength);
        this.consumedTotal += BigInt(bytesLength);
        this.updatePausedAndNotify();

        const unflushed = this.consumedTotal - this.lastFlushedTotal;
        if (this.attachmentReady && unflushed >= BigInt(this.cfg.ackThresholdBytes)) {
            this.clearTimer();
            void this.flushAck();
            return;
        }

        if (unflushed > 0n && this.timer === null) {
            this.timer = setTimeout(() => {
                this.timer = null;
                this.tick();
            }, this.cfg.ackIntervalMs);
        }
    }

    /** Dispara o envio periódico de ACK quando o timer de intervalo expira. */
    public tick(): void {
        this.clearTimer();
        if (this.disposed) {
            return;
        }
        void this.flushAck();
    }

    /** Envia imediatamente o total cumulativo (`consumedTotal`) ao backend, serializando 1 ACK por vez. */
    public async flushAck(): Promise<void> {
        if (this.disposed || !this.attachmentReady) {
            return;
        }
        const unflushed = this.consumedTotal - this.lastFlushedTotal;
        if (unflushed <= 0n || this.ackInFlight) {
            return;
        }

        this.clearTimer();
        this.ackInFlight = true;
        const targetTotal = this.consumedTotal;
        const gen = this.generation;
        this.lastFlushedTotal = targetTotal;
        this.updatePausedAndNotify();

        try {
            await this.sink.ack(targetTotal.toString(10));
        } catch (err: unknown) {
            if (gen !== this.generation || this.disposed) {
                return;
            }
            const code = extractErrorCode(err);
            if (code === 'pty.attachment_stale') {
                // Geração antiga substituída no backend; ignorar silenciosamente.
                return;
            }
            if (code === 'pty.ack_invalid') {
                // Não fazer retry cego em erro de protocolo.
                return;
            }
        } finally {
            if (gen === this.generation && !this.disposed) {
                this.ackInFlight = false;
                this.updatePausedAndNotify();
                if (this.consumedTotal > this.lastFlushedTotal && this.attachmentReady) {
                    void this.flushAck();
                }
            }
        }
    }

    /** Define se o `attachmentId` já foi recebido do `pty_attach` e libera ACKs retidos. */
    public setAttachmentReady(ready: boolean): void {
        if (this.disposed) {
            return;
        }
        this.attachmentReady = ready;
        if (ready && this.consumedTotal > this.lastFlushedTotal) {
            this.clearTimer();
            void this.flushAck();
        }
    }

    /** Reinicia os contadores exclusivamente ao iniciar uma nova geração de `pty_attach`. */
    public reset(): void {
        this.generation += 1;
        this.clearTimer();
        this.pendingBytes = 0;
        this.consumedTotal = 0n;
        this.lastFlushedTotal = 0n;
        this.paused = false;
        this.attachmentReady = false;
        this.ackInFlight = false;
        this.onState({ paused: false, pendingBytes: 0 });
    }

    /** Atualiza parâmetros de limiar/intervalo de ACK em tempo de execução. */
    public updateConfig(cfg: Partial<FlowConfig>): void {
        this.cfg = {
            ...this.cfg,
            ...cfg
        };
        this.updatePausedAndNotify();
    }

    /** Retorna fotografia imutável do estado atual para testes e telemetria. */
    public getSnapshot(): FlowControllerStateSnapshot {
        return {
            paused: this.paused,
            pendingBytes: this.pendingBytes,
            consumedTotal: this.consumedTotal,
            unflushedAckBytes: this.consumedTotal - this.lastFlushedTotal
        };
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;
        this.generation += 1;
        this.clearTimer();
    }

    private clearTimer(): void {
        if (this.timer !== null) {
            clearTimeout(this.timer);
            this.timer = null;
        }
    }

    private updatePausedAndNotify(): void {
        const unflushedNumber = Number(this.consumedTotal - this.lastFlushedTotal);
        const inFlightWindow = this.pendingBytes + unflushedNumber;
        this.paused = inFlightWindow >= this.cfg.pausedHintBytes;
        this.onState({
            paused: this.paused,
            pendingBytes: this.pendingBytes
        });
    }
}
