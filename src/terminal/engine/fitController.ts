import type { FitAddon } from '@xterm/addon-fit';
import type { Terminal } from '@xterm/xterm';
import type { PtyId } from '@/types/bindings';
import type { Disposable } from '@/types/terminal';
import { ptyClient } from '@/terminal/stream/ptyClient';

/** Opções de inicialização do `FitController`. */
export interface FitControllerOptions {
    ptyId: PtyId;
    xterm: Terminal;
    fitAddon: Pick<FitAddon, 'proposeDimensions'>;
    onDimensionsChanged?: ((cols: number, rows: number) => void) | undefined;
    debounceMs?: number | undefined;
    maxWaitMs?: number | undefined;
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
 * Controlador de redimensionamento da grade do terminal (`ResizeObserver` + debounce 50ms / maxWait 100ms),
 * deduplicação de dimensões e serialização de `pty_resize`.
 */
export class FitController implements Disposable {
    public static readonly MIN_COLS = 2;
    public static readonly MIN_ROWS = 1;
    public static readonly MAX_DIM = 1000;

    private readonly ptyId: PtyId;
    private readonly xterm: Terminal;
    private readonly fitAddon: Pick<FitAddon, 'proposeDimensions'>;
    private readonly onDimensionsChanged: ((cols: number, rows: number) => void) | undefined;
    private readonly debounceMs: number;
    private readonly maxWaitMs: number;

    private host: HTMLElement | null = null;
    private observer: ResizeObserver | null = null;
    private visible = true;
    private ptyExited = false;
    private disposed = false;

    private debounceTimer: ReturnType<typeof setTimeout> | null = null;
    private maxWaitTimer: ReturnType<typeof setTimeout> | null = null;

    private confirmedCols: number | null = null;
    private confirmedRows: number | null = null;
    private desiredSize: { cols: number; rows: number } | null = null;
    private resizeInFlight = false;

    public constructor(opts: FitControllerOptions) {
        this.ptyId = opts.ptyId;
        this.xterm = opts.xterm;
        this.fitAddon = opts.fitAddon;
        this.onDimensionsChanged = opts.onDimensionsChanged;
        this.debounceMs = opts.debounceMs ?? 50;
        this.maxWaitMs = opts.maxWaitMs ?? 100;
    }

    /** Inicia a observação de dimensões do elemento host do terminal. */
    public observe(host: HTMLElement): void {
        if (this.disposed) {
            return;
        }
        this.unobserve();
        this.host = host;

        if (typeof ResizeObserver !== 'undefined') {
            this.observer = new ResizeObserver(() => {
                this.scheduleDebouncedFit();
            });
            this.observer.observe(host);
        }

        if (typeof document !== 'undefined' && 'fonts' in document && document.fonts?.ready) {
            void document.fonts.ready
                .then(() => {
                    if (!this.disposed && this.host === host && this.visible) {
                        void this.requestFit(false);
                    }
                })
                .catch(() => {});
        }
    }

    /** Encerra a observação do `ResizeObserver` atual. */
    public unobserve(): void {
        this.clearTimers();
        if (this.observer) {
            this.observer.disconnect();
            this.observer = null;
        }
        this.host = null;
    }

    /** Atualiza visibilidade; ao tornar-se visível, recalcula o ajuste da grade. */
    public setVisible(visible: boolean): void {
        if (this.disposed || this.visible === visible) {
            return;
        }
        this.visible = visible;
        if (!visible) {
            this.clearTimers();
            return;
        }
        if (this.host) {
            void this.requestFit(false);
        }
    }

    /** Solicita cálculo de ajuste da grade (`immediate = true` ignora o debounce). */
    public async requestFit(immediate = false): Promise<void> {
        if (this.disposed) {
            return;
        }
        if (!immediate) {
            this.scheduleDebouncedFit();
            return;
        }
        this.clearTimers();
        await this.performFit();
    }

    /** Marca o PTY como finalizado para suprimir novos `pty_resize`. */
    public markPtyExited(): void {
        this.ptyExited = true;
        this.desiredSize = null;
    }

    public getConfirmedSize(): { cols: number; rows: number } | null {
        if (this.confirmedCols === null || this.confirmedRows === null) {
            return null;
        }
        return {
            cols: this.confirmedCols,
            rows: this.confirmedRows
        };
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;
        this.unobserve();
    }

    private scheduleDebouncedFit(): void {
        if (this.disposed || !this.visible) {
            return;
        }
        if (this.debounceTimer !== null) {
            clearTimeout(this.debounceTimer);
        }
        this.debounceTimer = setTimeout(() => {
            this.clearTimers();
            void this.performFit();
        }, this.debounceMs);

        if (this.maxWaitTimer === null) {
            this.maxWaitTimer = setTimeout(() => {
                this.clearTimers();
                void this.performFit();
            }, this.maxWaitMs);
        }
    }

    private clearTimers(): void {
        if (this.debounceTimer !== null) {
            clearTimeout(this.debounceTimer);
            this.debounceTimer = null;
        }
        if (this.maxWaitTimer !== null) {
            clearTimeout(this.maxWaitTimer);
            this.maxWaitTimer = null;
        }
    }

    private async performFit(): Promise<void> {
        if (this.disposed || !this.visible || !this.host) {
            return;
        }
        if (!this.host.isConnected) {
            return;
        }

        // Em ambientes DOM reais (ou mocks que definem clientWidth/Height > 0), ignora 0x0.
        const hasZeroSize =
            this.host.clientWidth === 0 &&
            this.host.clientHeight === 0 &&
            this.host.getBoundingClientRect().width === 0 &&
            this.host.getBoundingClientRect().height === 0;
        if (hasZeroSize) {
            return;
        }

        let proposed: { cols: number; rows: number } | undefined;
        try {
            proposed = this.fitAddon.proposeDimensions();
        } catch {
            return;
        }

        if (
            !proposed ||
            !Number.isFinite(proposed.cols) ||
            !Number.isFinite(proposed.rows) ||
            proposed.cols < FitController.MIN_COLS ||
            proposed.rows < FitController.MIN_ROWS
        ) {
            return;
        }

        const cols = Math.min(FitController.MAX_DIM, Math.floor(proposed.cols));
        const rows = Math.min(FitController.MAX_DIM, Math.floor(proposed.rows));

        if (this.xterm.cols !== cols || this.xterm.rows !== rows) {
            this.xterm.resize(cols, rows);
            this.onDimensionsChanged?.(cols, rows);
        }

        if (this.ptyExited) {
            return;
        }

        if (this.confirmedCols === cols && this.confirmedRows === rows && !this.resizeInFlight) {
            return;
        }

        this.desiredSize = { cols, rows };
        await this.drainResizeQueue();
    }

    private async drainResizeQueue(): Promise<void> {
        if (this.resizeInFlight || this.disposed || this.ptyExited) {
            return;
        }
        this.resizeInFlight = true;

        try {
            while (this.desiredSize && !this.disposed && !this.ptyExited) {
                const target = this.desiredSize;
                this.desiredSize = null;

                if (this.confirmedCols === target.cols && this.confirmedRows === target.rows) {
                    continue;
                }

                try {
                    await ptyClient.resize(this.ptyId, {
                        rows: target.rows,
                        cols: target.cols,
                        pixelWidth: this.host?.clientWidth ?? 0,
                        pixelHeight: this.host?.clientHeight ?? 0
                    });
                    this.confirmedCols = target.cols;
                    this.confirmedRows = target.rows;
                } catch (err: unknown) {
                    const code = extractErrorCode(err);
                    if (code === 'pty.already_exited' || code === 'pty.not_found') {
                        this.markPtyExited();
                    }
                    // Mantém confirmedCols/confirmedRows anteriores para que o próximo requestFit tente novamente.
                    break;
                }
            }
        } finally {
            this.resizeInFlight = false;
        }
    }
}
