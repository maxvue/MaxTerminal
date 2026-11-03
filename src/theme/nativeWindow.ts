import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

let lastAppliedPolarity: 'dark' | 'light' | null = null;
let inFlightPromise: Promise<boolean> | null = null;
let pendingPolarity: 'dark' | 'light' | null = null;

/**
 * Verifica se a aplicação está executando dentro do runtime nativo Tauri.
 */
export function isNativeWindowAvailable(): boolean {
    if (typeof window === 'undefined') {
        return false;
    }
    try {
        return (
            isTauri() ||
            Boolean((window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
        );
    } catch {
        return false;
    }
}

/**
 * Aplica a polaridade informada ('dark' | 'light') à janela nativa do Tauri.
 *
 * Garante:
 * 1. Execução segura apenas em ambiente Tauri;
 * 2. Deduplicação de polaridades já aplicadas com sucesso;
 * 3. Serialização e reconciliação sob concorrência (último tema sempre vence);
 * 4. Tratamento resiliente de erro sem rejeições não tratadas.
 */
export async function applyNativeWindowTheme(polarity: 'dark' | 'light'): Promise<boolean> {
    if (!isNativeWindowAvailable()) {
        return false;
    }

    if (polarity === lastAppliedPolarity && !inFlightPromise) {
        return true;
    }

    if (inFlightPromise) {
        pendingPolarity = polarity;
        return inFlightPromise;
    }

    inFlightPromise = (async () => {
        let currentTarget: 'dark' | 'light' = polarity;
        while (currentTarget) {
            try {
                const currentWin = typeof getCurrentWindow === 'function' ? getCurrentWindow() : null;
                if (currentWin && typeof currentWin.setTheme === 'function') {
                    await currentWin.setTheme(currentTarget);
                    lastAppliedPolarity = currentTarget;
                }
            } catch (err) {
                console.warn('[nativeWindow] Falha ao sincronizar tema da janela nativa:', err);
            }

            if (pendingPolarity && pendingPolarity !== lastAppliedPolarity) {
                currentTarget = pendingPolarity;
                pendingPolarity = null;
            } else {
                pendingPolarity = null;
                break;
            }
        }
        return lastAppliedPolarity === polarity;
    })().finally(() => {
        inFlightPromise = null;
    });

    return inFlightPromise;
}

/**
 * Redefine o estado interno do adaptador (exclusivo para isolamento em testes unitários).
 */
export function _resetNativeWindowStateForTests(): void {
    lastAppliedPolarity = null;
    inFlightPromise = null;
    pendingPolarity = null;
}
