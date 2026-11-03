import { getCurrentInstance, onBeforeUnmount } from 'vue';
import type { PaneId } from '@/types/bindings';
import type { Disposable } from '@/types/terminal';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';

export interface ViewportTriggersOptions {
    paneId: PaneId;
    onTrigger: (trigger: string) => void;
}

/**
 * Intercepta teclas de atalho rápido (`>`, `>>`, `@`, `?`, `/`) diretamente do viewport do terminal
 * quando a aba estiver ociosa em prompt (sem modo altScreen ativo), transferindo imediatamente o foco
 * e o caractere digitado para a barra estruturada `RichInput` sem vazamento para o PTY nativo.
 */
export function useViewportTriggers(options: ViewportTriggersOptions) {
    const terminalStore = useTerminalStore();
    let unregister: (() => void) | null = null;
    let pendingChevron = false;
    let chevronTimer: ReturnType<typeof setTimeout> | null = null;

    function attach(): Disposable | null {
        const instance = terminalInstanceRegistry.get(options.paneId);
        if (!instance) {
            return null;
        }

        unregister?.();

        const sub = instance.registerKeyHandler((event: KeyboardEvent) => {
            // Ignora atalhos de controle de sistema (Ctrl, Alt, Meta)
            if (event.ctrlKey || event.altKey || event.metaKey) {
                pendingChevron = false;
                return true;
            }

            // Ignora se estiver no modo de tela cheia/TUI interativa (ex.: vim, htop, claude code)
            const state = terminalStore.instance(options.paneId);
            if (!state || state.modes.altScreen || state.inputMode !== 'rich') {
                pendingChevron = false;
                return true;
            }

            const key = event.key;

            if (key === '>') {
                event.preventDefault();
                event.stopPropagation();

                if (pendingChevron) {
                    pendingChevron = false;
                    if (chevronTimer) {
                        clearTimeout(chevronTimer);
                        chevronTimer = null;
                    }
                    options.onTrigger('>>');
                    return false;
                }

                pendingChevron = true;
                chevronTimer = setTimeout(() => {
                    if (pendingChevron) {
                        pendingChevron = false;
                        options.onTrigger('>');
                    }
                }, 200);
                return false;
            }

            if (pendingChevron) {
                pendingChevron = false;
                if (chevronTimer) {
                    clearTimeout(chevronTimer);
                    chevronTimer = null;
                }
                options.onTrigger('>');
            }

            if (key === '@' || key === '?' || key === '/') {
                event.preventDefault();
                event.stopPropagation();
                options.onTrigger(key);
                return false;
            }

            return true;
        });

        unregister = () => {
            sub.dispose();
            if (chevronTimer) {
                clearTimeout(chevronTimer);
                chevronTimer = null;
            }
        };

        return {
            dispose: unregister
        };
    }

    if (getCurrentInstance()) {
        onBeforeUnmount(() => {
            unregister?.();
        });
    }

    return {
        attach
    };
}
