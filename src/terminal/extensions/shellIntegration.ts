import { useShellHistoryStore } from '@/stores/shellHistory';
import { useTabStatusStore } from '@/stores/tabStatus';
import { registerContextProvider } from '@/terminal/extensions/terminalContext';
import { registerTerminalExtension } from '@/terminal/extensions/terminalExtensions';
import { registerInputModeHint } from '@/terminal/input/inputModeResolver';
import type {
    Disposable,
    InputModeHint,
    TerminalContext,
    TerminalExtension,
    TerminalInstanceHandle
} from '@/types/terminal';

/**
 * Cria a extensão `shell.integration` do Motor de Terminal Híbrido:
 * - Enriquece o `TerminalContext` com o `cwd` sanitizado apenas quando reportado por `osc133`.
 * - Fornece dica de modo de entrada (`'rich'` quando ocioso em prompt OSC 133; `'passthrough'`
 *   quando comando está em execução ou em modo `fallback_proc`).
 */
export function createShellIntegrationExtension(): TerminalExtension {
    const activeInstances = new Map<string, TerminalInstanceHandle>();

    return {
        id: 'shell.integration',
        activate(instance: TerminalInstanceHandle): Disposable {
            activeInstances.set(instance.paneId, instance);
            return {
                dispose(): void {
                    if (activeInstances.get(instance.paneId) === instance) {
                        activeInstances.delete(instance.paneId);
                    }
                }
            };
        }
    };
}

/**
 * Registra globalmente a extensão `shell.integration`, o `TerminalContextProvider` de `cwd`
 * e o `InputModeHintProvider` baseado no estado do painel na `useTabStatusStore` / `useShellHistoryStore`.
 */
export function installShellIntegrationExtension(): Disposable {
    const extSub = registerTerminalExtension(createShellIntegrationExtension());

    const ctxSub = registerContextProvider(({ paneId, ptyId }): Partial<TerminalContext> => {
        const historyStore = useShellHistoryStore();
        const tabStatusStore = useTabStatusStore();

        const integration = historyStore.integrationOf(ptyId);
        let mode = integration?.mode ?? null;
        let cwd = historyStore.cwdOf(ptyId);

        if (!mode || !cwd) {
            for (const snap of Object.values(tabStatusStore.tabs)) {
                const pane = snap.panes.find(
                    (p) => p.ptyId === ptyId || (p.paneId !== null && p.paneId === paneId)
                );
                if (pane) {
                    mode = mode ?? pane.integrationMode;
                    cwd = cwd ?? pane.cwd;
                    break;
                }
            }
        }

        // Cwd não confiável (fallback_proc) não determina contexto de execução de ações
        const trustedCwd = mode === 'osc133' ? cwd : null;

        return {
            ...(trustedCwd ? { cwd: trustedCwd } : {}),
            extra: {
                integrationMode: mode ?? 'pending'
            }
        };
    });

    const hintSub = registerInputModeHint((ctx): InputModeHint | null => {
        const tabStatusStore = useTabStatusStore();
        const historyStore = useShellHistoryStore();

        let paneStatus: string | null = null;
        let hasActiveExec = false;
        let mode = historyStore.integrationOf(ctx.ptyId)?.mode ?? null;

        for (const snap of Object.values(tabStatusStore.tabs)) {
            const pane = snap.panes.find(
                (p) => p.ptyId === ctx.ptyId || (p.paneId !== null && p.paneId === ctx.paneId)
            );
            if (pane) {
                paneStatus = pane.status;
                hasActiveExec = pane.activeExecution !== null;
                mode = mode ?? pane.integrationMode;
                break;
            }
        }

        if (
            hasActiveExec ||
            paneStatus === 'running' ||
            paneStatus === 'continuous' ||
            mode === 'fallback_proc'
        ) {
            return {
                mode: 'passthrough',
                priority: 20,
                reason: 'Comando em execução ou integração em modo fallback /proc'
            };
        }

        if (mode === 'osc133') {
            return {
                mode: 'rich',
                priority: 10,
                reason: 'Prompt OSC 133 correlacionado e ocioso'
            };
        }

        return null;
    });

    return {
        dispose(): void {
            hintSub.dispose();
            ctxSub.dispose();
            extSub.dispose();
        }
    };
}
