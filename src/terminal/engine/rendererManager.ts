import { WebglAddon } from '@xterm/addon-webgl';
import type { ITerminalAddon, Terminal } from '@xterm/xterm';
import type { Disposable, RendererKind } from '@/types/terminal';

/** Interface mínima do `WebglAddon` necessária para gerenciamento e testes. */
export interface WebglAddonLike extends Disposable {
    onContextLoss(listener: () => void): Disposable;
    clearTextureAtlas?(): void;
}

/** Opções de construção do `RendererManager`. */
export interface RendererManagerOptions {
    xterm: Terminal;
    webglEnabled: boolean;
    visible: boolean;
    onRendererChange?: ((kind: RendererKind) => void) | undefined;
    createWebglAddon?: (() => WebglAddonLike) | undefined;
    createCanvasAddon?: (() => Disposable) | null | undefined;
    now?: (() => number) | undefined;
}

/**
 * Gerenciador do renderizador do Xterm (`webgl` -> `canvas` -> `dom`),
 * com suspensão automática em abas ocultas e trava em `dom` após 3 perdas de contexto em 60s.
 */
export class RendererManager implements Disposable {
    public static readonly LOSS_WINDOW_MS = 60_000;
    public static readonly MAX_LOSSES_IN_WINDOW = 3;
    public static readonly RETRY_COOLDOWN_MS = 30_000;

    private readonly xterm: Terminal;
    private webglEnabled: boolean;
    private visible: boolean;
    private readonly onRendererChange: ((kind: RendererKind) => void) | undefined;
    private readonly createWebglAddon: () => WebglAddonLike;
    private readonly createCanvasAddon: (() => Disposable) | null;
    private readonly now: () => number;

    private kind: RendererKind = 'dom';
    private webglAddon: WebglAddonLike | null = null;
    private contextLossSub: Disposable | null = null;
    private canvasAddon: Disposable | null = null;
    private retryTimer: ReturnType<typeof setTimeout> | null = null;
    private lossTimestamps: number[] = [];
    private lockedToDom = false;
    private activated = false;
    private disposed = false;

    public constructor(opts: RendererManagerOptions) {
        this.xterm = opts.xterm;
        this.webglEnabled = opts.webglEnabled;
        this.visible = opts.visible;
        this.onRendererChange = opts.onRendererChange;
        this.createWebglAddon =
            opts.createWebglAddon ??
            (() => {
                return new WebglAddon();
            });
        this.createCanvasAddon = opts.createCanvasAddon ?? null;
        this.now = opts.now ?? (() => Date.now());
    }

    /** Ativa o renderizador adequado ao estado atual (`webgl`, `canvas` ou `dom`). */
    public activate(): RendererKind {
        if (this.disposed) {
            return 'dom';
        }
        this.activated = true;
        if (!this.visible || !this.webglEnabled || this.lockedToDom || !this.xterm.element) {
            this.disposeWebgl();
            this.disposeCanvas();
            this.setKind('dom');
            return this.kind;
        }

        if (this.tryActivateWebgl()) {
            return this.kind;
        }

        return this.activateFallbackRenderer();
    }

    /** Atualiza visibilidade da instância; quando oculta, libera o contexto WebGL imediatamente. */
    public setVisible(visible: boolean): void {
        if (this.disposed || this.visible === visible) {
            return;
        }
        this.visible = visible;
        if (!visible) {
            this.suspend();
            return;
        }
        if (this.activated) {
            this.activate();
            this.safeRefresh();
        }
    }

    /** Habilita ou desabilita aceleração WebGL em tempo de execução. */
    public setWebglEnabled(enabled: boolean): void {
        if (this.disposed || this.webglEnabled === enabled) {
            return;
        }
        this.webglEnabled = enabled;
        if (!enabled) {
            this.clearRetryTimer();
            this.disposeWebgl();
            this.disposeCanvas();
            this.setKind('dom');
            this.safeRefresh();
            return;
        }
        if (this.activated && this.visible && !this.lockedToDom) {
            this.activate();
            this.safeRefresh();
        }
    }

    /** Suspende o contexto GPU enquanto o painel está oculto. */
    public suspend(): void {
        this.clearRetryTimer();
        this.disposeWebgl();
        this.disposeCanvas();
        this.setKind('dom');
    }

    /** Força perda de contexto WebGL para validação e diagnóstico no Playground. */
    public forceContextLossForDebug(): void {
        if (this.disposed) {
            return;
        }
        this.handleContextLoss();
    }

    public getKind(): RendererKind {
        return this.kind;
    }

    public isLockedToDom(): boolean {
        return this.lockedToDom;
    }

    public getContextLossCountInWindow(): number {
        this.pruneLossTimestamps(this.now());
        return this.lossTimestamps.length;
    }

    public dispose(): void {
        if (this.disposed) {
            return;
        }
        this.disposed = true;
        this.clearRetryTimer();
        this.disposeWebgl();
        this.disposeCanvas();
    }

    private tryActivateWebgl(): boolean {
        this.clearRetryTimer();
        this.disposeCanvas();
        this.disposeWebgl();

        try {
            const addon = this.createWebglAddon();
            this.contextLossSub = addon.onContextLoss(() => {
                this.handleContextLoss();
            });
            this.xterm.loadAddon(addon as unknown as ITerminalAddon);
            this.webglAddon = addon;
            this.setKind('webgl');
            return true;
        } catch {
            this.disposeWebgl();
            this.recordLossAndEvaluateLock();
            return false;
        }
    }

    private handleContextLoss(): void {
        if (this.disposed) {
            return;
        }
        this.clearRetryTimer();
        this.disposeWebgl();
        const locked = this.recordLossAndEvaluateLock();

        if (locked) {
            this.disposeCanvas();
            this.setKind('dom');
            this.safeRefresh();
            return;
        }

        this.activateFallbackRenderer();
        this.safeRefresh();

        if (this.visible && this.webglEnabled && !this.lockedToDom) {
            this.retryTimer = setTimeout(() => {
                this.retryTimer = null;
                if (!this.disposed && this.visible && this.webglEnabled && !this.lockedToDom) {
                    if (this.tryActivateWebgl()) {
                        this.safeRefresh();
                    }
                }
            }, RendererManager.RETRY_COOLDOWN_MS);
        }
    }

    private recordLossAndEvaluateLock(): boolean {
        const currentNow = this.now();
        this.pruneLossTimestamps(currentNow);
        this.lossTimestamps.push(currentNow);
        if (this.lossTimestamps.length >= RendererManager.MAX_LOSSES_IN_WINDOW) {
            this.lockedToDom = true;
        }
        return this.lockedToDom;
    }

    private pruneLossTimestamps(currentNow: number): void {
        const cutoff = currentNow - RendererManager.LOSS_WINDOW_MS;
        while (this.lossTimestamps.length > 0 && this.lossTimestamps[0]! < cutoff) {
            this.lossTimestamps.shift();
        }
    }

    private activateFallbackRenderer(): RendererKind {
        if (this.lockedToDom || !this.createCanvasAddon) {
            this.disposeCanvas();
            this.setKind('dom');
            return 'dom';
        }

        try {
            this.disposeCanvas();
            const canvas = this.createCanvasAddon();
            this.xterm.loadAddon(canvas as unknown as ITerminalAddon);
            this.canvasAddon = canvas;
            this.setKind('canvas');
            return 'canvas';
        } catch {
            this.disposeCanvas();
            this.setKind('dom');
            return 'dom';
        }
    }

    private setKind(next: RendererKind): void {
        if (this.kind === next) {
            return;
        }
        this.kind = next;
        this.onRendererChange?.(next);
    }

    private safeRefresh(): void {
        try {
            if (this.xterm.rows > 0) {
                this.xterm.refresh(0, this.xterm.rows - 1);
            }
        } catch {
            // Ignora falhas de refresh caso o elemento já tenha sido desmontado.
        }
    }

    private disposeWebgl(): void {
        if (this.contextLossSub) {
            try {
                this.contextLossSub.dispose();
            } catch {
                // Ignora erros no descarte do listener.
            }
            this.contextLossSub = null;
        }
        if (this.webglAddon) {
            try {
                this.webglAddon.dispose();
            } catch {
                // Ignora erros no descarte do WebglAddon.
            }
            this.webglAddon = null;
        }
    }

    private disposeCanvas(): void {
        if (this.canvasAddon) {
            try {
                this.canvasAddon.dispose();
            } catch {
                // Ignora erros no descarte do CanvasAddon.
            }
            this.canvasAddon = null;
        }
    }

    private clearRetryTimer(): void {
        if (this.retryTimer !== null) {
            clearTimeout(this.retryTimer);
            this.retryTimer = null;
        }
    }
}
