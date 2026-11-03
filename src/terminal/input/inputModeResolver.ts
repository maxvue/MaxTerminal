import type {
    Disposable,
    InputMode,
    InputModeHint,
    InputModeHintProvider,
    TerminalContext,
    TerminalModes
} from '@/types/terminal';

/** Parâmetros de resolução determinística do modo de entrada de um painel. */
export interface ResolveInputModeParams {
    context: TerminalContext;
    modes: TerminalModes;
    override: InputMode | null;
    richInputEnabled: boolean;
}

/** Resultado da resolução do modo de entrada com diagnóstico da fonte vencedora. */
export interface InputModeResolution {
    mode: InputMode;
    forcedByTui: boolean;
    source: 'tui' | 'override' | 'hint' | 'default';
    reason: string;
}

const hintProviders: InputModeHintProvider[] = [];

/**
 * Registra um provedor de dicas de modo de entrada (ex.: agentes CLI na Fase 13).
 */
export function registerInputModeHint(provider: InputModeHintProvider): Disposable {
    hintProviders.push(provider);
    return {
        dispose(): void {
            const idx = hintProviders.indexOf(provider);
            if (idx !== -1) {
                hintProviders.splice(idx, 1);
            }
        }
    };
}

/**
 * Verifica se a transição de modos VT deve limpar automaticamente o `inputModeOverride` manual.
 * Entrar ou sair de `altScreen` (`?1049h` / `?1049l`) sempre limpa o override.
 */
export function shouldClearOverrideOnModesChange(
    prevModes: TerminalModes,
    nextModes: TerminalModes
): boolean {
    return prevModes.altScreen !== nextModes.altScreen;
}

/**
 * Resolve o modo de entrada ativo (`'rich'` vs. `'passthrough'`) segundo a hierarquia canônica:
 * 1. `altScreen === true` ou `mouseTracking !== 'none'` => `'passthrough'` forçado.
 * 2. `override !== null` => modo escolhido manualmente pelo usuário.
 * 3. `InputModeHintProvider` de maior `priority` (em empate, o último registrado vence).
 * 4. Padrão das preferências (`richInputEnabled ? 'rich' : 'passthrough'`).
 */
export function resolveInputMode(params: ResolveInputModeParams): InputModeResolution {
    if (params.modes.altScreen || params.modes.mouseTracking !== 'none') {
        return {
            mode: 'passthrough',
            forcedByTui: true,
            source: 'tui',
            reason: 'Modo direto ativado automaticamente para aplicação interativa (TUI)'
        };
    }

    if (params.override !== null) {
        return {
            mode: params.override,
            forcedByTui: false,
            source: 'override',
            reason: 'Modo selecionado manualmente pelo usuário'
        };
    }

    let bestHint: InputModeHint | null = null;
    for (const provider of hintProviders) {
        try {
            const candidate = provider(params.context, params.modes);
            if (candidate) {
                if (bestHint === null || candidate.priority >= bestHint.priority) {
                    bestHint = candidate;
                }
            }
        } catch {
            // Provedor com falha é ignorado.
        }
    }

    if (bestHint !== null) {
        return {
            mode: bestHint.mode,
            forcedByTui: false,
            source: 'hint',
            reason: bestHint.reason
        };
    }

    return {
        mode: params.richInputEnabled ? 'rich' : 'passthrough',
        forcedByTui: false,
        source: 'default',
        reason: 'Configuração padrão do terminal'
    };
}

/** Limpa todos os provedores de dicas registrados (exclusivo para testes). */
export function clearInputModeHintsForTests(): void {
    hintProviders.length = 0;
}
