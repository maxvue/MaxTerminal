import { getCurrentScope, onScopeDispose } from 'vue';
import { useEventListener } from '@maxvue/max-use';

export interface UsePageReloadShortcutOptions {
    /** Janela alvo para escuta do evento. Padrão: `window` no ambiente de navegador. */
    target?: Window | null | undefined;
    /** Função disparada para recarregar o documento. Padrão: `window.location.reload()`. */
    reloadFn?: (() => void) | undefined;
}

export interface UsePageReloadShortcutReturn {
    /** Remove o listener manualmente, caso necessário fora do ciclo de vida Vue. */
    dispose: () => void;
}

/**
 * Registra listener global na fase de captura na raiz para a tecla F5 simples.
 *
 * Garante:
 * 1. Captura da tecla F5 simples (sem Ctrl, Alt, Shift ou Meta);
 * 2. Ignora eventos durante composição de caracteres (IME);
 * 3. Interrompe a propagação e previne comportamento default antes de xterm e outros handlers descendentes;
 * 4. Executa o reload apenas uma vez, bloqueando repetições contínuas (repeat) e disparos subsequentes;
 * 5. Descarte automático do listener no encerramento do escopo do composable.
 */
export function usePageReloadShortcut(
    options?: UsePageReloadShortcutOptions
): UsePageReloadShortcutReturn {
    const targetWindow =
        options?.target !== undefined
            ? options.target
            : typeof window !== 'undefined'
                ? window
                : null;

    let reloadInitiated = false;

    const performReload = (): void => {
        if (reloadInitiated) {
            return;
        }
        reloadInitiated = true;
        if (options?.reloadFn) {
            options.reloadFn();
        } else if (
            targetWindow &&
            targetWindow.location &&
            typeof targetWindow.location.reload === 'function'
        ) {
            targetWindow.location.reload();
        }
    };

    const handleKeyDown = (event: KeyboardEvent): void => {
        // Ignora teclas que não sejam F5
        if (event.key !== 'F5' && event.code !== 'F5') {
            return;
        }

        // Ignora F5 com modificadores (Ctrl+F5, Shift+F5, Alt+F5, Meta+F5)
        if (event.ctrlKey || event.altKey || event.shiftKey || event.metaKey) {
            return;
        }

        // Ignora durante composição de caracteres (IME)
        if (event.isComposing) {
            return;
        }

        // Consome a tecla na raiz impedindo propagação para xterm, agentes ou inputs
        event.preventDefault();
        event.stopPropagation();

        // Ignora repetição contínua (keydown repeat) para recarregar apenas uma vez
        if (event.repeat) {
            return;
        }

        performReload();
    };

    let cleanupListener: (() => void) | null = null;
    if (targetWindow && typeof targetWindow.addEventListener === 'function') {
        cleanupListener = useEventListener(targetWindow, 'keydown', handleKeyDown, {
            capture: true,
            passive: false
        });
    }

    const dispose = (): void => {
        if (cleanupListener) {
            cleanupListener();
            cleanupListener = null;
        }
    };

    if (getCurrentScope()) {
        onScopeDispose(() => {
            dispose();
        });
    }

    return {
        dispose
    };
}
