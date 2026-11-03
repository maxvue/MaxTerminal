import { isTauri } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** Alvo de tema da janela nativa: 'dark', 'light' ou null para seguir as cores do sistema operacional. */
export type NativeWindowThemeTarget = 'dark' | 'light' | null;

/** Opções para resolução da política de tema da janela nativa. */
export interface WindowThemePolicyOptions {
    contentPolarity: 'dark' | 'light';
    /** Indica se a integração de cores do sistema está disponível. Se omitido ou true, prioriza o sistema. */
    nativeIntegrationAvailable?: boolean | undefined;
}

/** Sentinela interna para distinguir 'não aplicado' e 'sem pedido pendente' de null (sistema). */
const UNSET = Symbol('UNSET') as symbol;

type WindowThemeTargetOrUnset = NativeWindowThemeTarget | typeof UNSET;

let lastAppliedTarget: WindowThemeTargetOrUnset = UNSET;
let inFlightPromise: Promise<boolean> | null = null;
let pendingTarget: WindowThemeTargetOrUnset = UNSET;

/**
 * Deriva o alvo de tema da janela nativa conforme a política canônica:
 * 1. Caminho principal: 'null' (preserva as cores nativas e decorações do sistema operacional);
 * 2. Fallback escuro: 'dark' apenas quando a integração nativa for comprovadamente indisponível E o conteúdo for escuro;
 * 3. Retorno ao sistema: 'null' quando o conteúdo for claro (não impõe barra clara artificial).
 */
export function resolveNativeWindowThemeTarget(
    options: WindowThemePolicyOptions
): NativeWindowThemeTarget {
    if (options.nativeIntegrationAvailable === false) {
        return options.contentPolarity === 'dark' ? 'dark' : null;
    }
    return null;
}

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
 * Aplica o alvo informado ('dark' | 'light' | null) à janela nativa do Tauri via IPC.
 *
 * O valor `null` instrui o Tauri/SO a utilizar o tema/cores nativas do sistema.
 *
 * Garante:
 * 1. Execução segura apenas em ambiente Tauri;
 * 2. Deduplicação de alvos já aplicados com sucesso (inclusive null);
 * 3. Serialização e reconciliação sob concorrência (último alvo solicitado sempre prevalece);
 * 4. Tratamento resiliente de erro sem rejeições não tratadas.
 *
 * @returns `true` se o alvo foi despachado via IPC ou se já estava aplicado;
 * note que isso atesta o transporte IPC e não a correspondência de renderização pelo gerenciador de janelas.
 */
export async function applyNativeWindowTheme(
    target: NativeWindowThemeTarget
): Promise<boolean> {
    if (!isNativeWindowAvailable()) {
        return false;
    }

    if (target === lastAppliedTarget && !inFlightPromise) {
        return true;
    }

    if (inFlightPromise) {
        pendingTarget = target;
        return inFlightPromise;
    }

    inFlightPromise = (async () => {
        let currentTarget: WindowThemeTargetOrUnset = target;
        while (currentTarget !== UNSET) {
            const themeToApply = currentTarget as NativeWindowThemeTarget;
            try {
                let currentWin: ReturnType<typeof getCurrentWindow> | null = null;
                try {
                    currentWin = typeof getCurrentWindow === 'function' ? getCurrentWindow() : null;
                } catch {
                    currentWin = null;
                }
                if (currentWin && typeof currentWin.setTheme === 'function') {
                    await currentWin.setTheme(themeToApply);
                    lastAppliedTarget = themeToApply;
                }
            } catch (err) {
                console.warn('[nativeWindow] Falha ao sincronizar tema da janela nativa:', err);
            }

            if (pendingTarget !== UNSET && pendingTarget !== lastAppliedTarget) {
                currentTarget = pendingTarget;
                pendingTarget = UNSET;
            } else {
                pendingTarget = UNSET;
                break;
            }
        }
        return lastAppliedTarget === target;
    })().finally(() => {
        inFlightPromise = null;
    });

    return inFlightPromise;
}

/**
 * Redefine o estado interno do adaptador (exclusivo para isolamento em testes unitários).
 */
export function _resetNativeWindowStateForTests(): void {
    lastAppliedTarget = UNSET;
    inFlightPromise = null;
    pendingTarget = UNSET;
}
