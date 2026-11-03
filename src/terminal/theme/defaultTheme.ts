import type { TerminalTheme } from '@/types/terminal';

const HEX_OR_RGB_COLOR_RE = /^(#[0-9a-fA-F]{3,8}|rgba?\([^)]+\))$/;

/**
 * Resolve o valor computado de uma variável CSS (`--max-*` ou `--background-*`) na raiz do documento,
 * retornando `fallbackHex` se ausente ou em ambiente de teste sem estilos carregados.
 */
export function resolveCssToken(tokenName: string, fallbackHex: string): string {
    if (typeof document === 'undefined' || typeof getComputedStyle !== 'function') {
        return fallbackHex;
    }
    try {
        const raw = getComputedStyle(document.documentElement).getPropertyValue(tokenName).trim();
        if (raw.length > 0 && HEX_OR_RGB_COLOR_RE.test(raw)) {
            return raw;
        }
    } catch {
        // Ignora falhas de ambiente DOM sintético e usa fallback seguro.
    }
    return fallbackHex;
}

/**
 * Constrói o tema padrão `Max Navy/Teal` integrado aos tokens semânticos do `MaxComponentsUi`.
 */
export function createMaxNavyTealTheme(): TerminalTheme {
    const background = resolveCssToken('--background-950', '#0b1220');
    const foreground = resolveCssToken('--background-50', '#e2e8f0');
    const cursor = resolveCssToken('--max-primary-400', '#2dd4bf');
    const cursorAccent = resolveCssToken('--background-950', '#0b1220');
    const selectionBackground = resolveCssToken('--max-primary-800', 'rgba(20, 184, 166, 0.28)');

    const ansi: TerminalTheme['colors']['ansi'] = [
        resolveCssToken('--background-900', '#0f172a'),
        resolveCssToken('--max-danger-500', '#ef4444'),
        resolveCssToken('--max-success-500', '#22c55e'),
        resolveCssToken('--max-warning-500', '#f59e0b'),
        resolveCssToken('--max-info-500', '#0ea5e9'),
        '#a855f7',
        resolveCssToken('--max-primary-500', '#14b8a6'),
        resolveCssToken('--background-200', '#cbd5e1'),
        resolveCssToken('--background-600', '#475569'),
        resolveCssToken('--max-danger-400', '#f87171'),
        resolveCssToken('--max-success-400', '#4ade80'),
        resolveCssToken('--max-warning-400', '#fbbf24'),
        resolveCssToken('--max-info-400', '#38bdf8'),
        '#c084fc',
        resolveCssToken('--max-primary-400', '#2dd4bf'),
        resolveCssToken('--background-50', '#f8fafc')
    ];

    return {
        id: 'max-navy-teal',
        name: 'Max Navy/Teal',
        source: 'builtin',
        colors: {
            foreground,
            background,
            cursor,
            cursorAccent,
            selectionBackground,
            selectionInactiveBackground: resolveCssToken('--background-800', 'rgba(51, 65, 85, 0.35)'),
            ansi
        },
        ui: {
            blockBackground: resolveCssToken('--background-900', '#0f172a'),
            blockBorder: resolveCssToken('--background-700', '#334155'),
            blockHeaderBackground: resolveCssToken('--background-800', '#1e293b'),
            richInputBackground: resolveCssToken('--background-900', '#0f172a'),
            richInputBorder: resolveCssToken('--background-700', '#334155')
        }
    };
}
