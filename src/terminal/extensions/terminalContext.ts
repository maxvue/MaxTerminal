import type { PaneId, PtyId } from '@/types/bindings';
import type { Disposable, TerminalContext, TerminalContextProvider } from '@/types/terminal';

const contextProviders = new Set<TerminalContextProvider>();

/**
 * Registra um provedor de contexto para enriquecer o `TerminalContext` de cada painel.
 */
export function registerContextProvider(provider: TerminalContextProvider): Disposable {
    contextProviders.add(provider);
    return {
        dispose(): void {
            contextProviders.delete(provider);
        }
    };
}

/**
 * Resolve o `TerminalContext` completo de um painel combinando os provedores registrados na ordem de registro.
 */
export function resolveTerminalContext(base: { paneId: PaneId; ptyId: PtyId }): TerminalContext {
    let projectId: string | null = null;
    let cwd: string | null = null;
    const extraMerged: Record<string, unknown> = {};

    for (const provider of contextProviders) {
        try {
            const partial = provider({ paneId: base.paneId, ptyId: base.ptyId });
            if (partial.projectId !== undefined) {
                projectId = partial.projectId;
            }
            if (partial.cwd !== undefined) {
                cwd = partial.cwd;
            }
            if (partial.extra) {
                Object.assign(extraMerged, partial.extra);
            }
        } catch {
            // Provedor com falha é ignorado para não comprometer o painel.
        }
    }

    return {
        paneId: base.paneId,
        ptyId: base.ptyId,
        projectId,
        cwd,
        extra: Object.freeze(extraMerged)
    };
}

/** Limpa todos os provedores de contexto registrados (exclusivo para testes). */
export function clearContextProvidersForTests(): void {
    contextProviders.clear();
}
