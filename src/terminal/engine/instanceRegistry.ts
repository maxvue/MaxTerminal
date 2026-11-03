import type { PaneId } from '@/types/bindings';
import { MtTerminalError } from '@/types/terminal';
import { TerminalInstance, type TerminalInstanceOptions } from './terminalInstance';

/**
 * Registro não-reativo de instâncias `TerminalInstance` indexadas por `PaneId`.
 * Preserva o emulador entre trocas de abas/rotas e impede reutilização de `PaneId` com `PtyId` divergente.
 */
export class TerminalInstanceRegistry {
    private readonly instances = new Map<PaneId, TerminalInstance>();

    /**
     * Retorna a instância existente para `opts.paneId` (validando `ptyId`) ou cria uma nova.
     */
    public getOrCreate(opts: TerminalInstanceOptions): TerminalInstance {
        const existing = this.instances.get(opts.paneId);
        if (existing && !existing.isDisposed()) {
            if (existing.ptyId !== opts.ptyId) {
                throw new MtTerminalError(
                    'terminal.pane_conflict',
                    'Painel já possui uma instância vinculada a outro PtyId. Descarte a instância anterior antes de recriar.'
                );
            }
            if (opts.callbacks) {
                existing.setCallbacks(opts.callbacks);
            }
            return existing;
        }

        const created = new TerminalInstance(opts);
        this.instances.set(opts.paneId, created);
        return created;
    }

    public get(paneId: PaneId): TerminalInstance | undefined {
        const inst = this.instances.get(paneId);
        if (inst && inst.isDisposed()) {
            this.instances.delete(paneId);
            return undefined;
        }
        return inst;
    }

    public has(paneId: PaneId): boolean {
        return this.get(paneId) !== undefined;
    }

    public dispose(paneId: PaneId): boolean {
        const inst = this.instances.get(paneId);
        if (!inst) {
            return false;
        }
        this.instances.delete(paneId);
        inst.dispose();
        return true;
    }

    public forEach(cb: (instance: TerminalInstance, paneId: PaneId) => void): void {
        for (const [paneId, inst] of this.instances.entries()) {
            if (!inst.isDisposed()) {
                cb(inst, paneId);
            }
        }
    }

    public size(): number {
        return this.instances.size;
    }

    public disposeAll(): void {
        const all = Array.from(this.instances.values());
        this.instances.clear();
        for (const inst of all) {
            inst.dispose();
        }
    }
}

/** Cria uma nova instância isolada do `TerminalInstanceRegistry`. */
export function createTerminalInstanceRegistry(): TerminalInstanceRegistry {
    return new TerminalInstanceRegistry();
}

/** Instância singleton do registro de instâncias de terminal. */
export const terminalInstanceRegistry: TerminalInstanceRegistry = createTerminalInstanceRegistry();
