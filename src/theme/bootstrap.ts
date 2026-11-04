import {
    applyCssVarsRecordToElement,
    buildCssVarsRecord
} from '@/theme/cssTokens';
import {
    BOOTSTRAP_STORAGE_KEY,
    DEFAULT_DARK_THEME_ID,
    type CssVarsContext,
    type MaxUiTokens,
    type ThemeBootstrapSnapshot
} from '@/types/theme';

/** Tokens canônicos iniciais do tema `max-navy-teal` para fallback síncrono imediato sem FOUC. */
export const FALLBACK_MAX_NAVY_TEAL_TOKENS: MaxUiTokens = {
    polarity: 'dark',
    primary: {
        p50: '#67c8db',
        p100: '#56c2d7',
        p200: '#46bcd4',
        p300: '#2ea4bc',
        p400: '#178da5',
        p500: '#00768e',
        p600: '#005f77',
        p700: '#004860',
        p800: '#003048',
        p900: '#001931',
        p950: '#00152a',
        content: '#ffffff'
    },
    success: {
        p50: '#dcfce7',
        p100: '#bbf7d0',
        p200: '#86efac',
        p300: '#4ade80',
        p400: '#34d399',
        p500: '#10b981',
        p600: '#059669',
        p700: '#047857',
        p800: '#065f46',
        p900: '#064e3b',
        p950: '#022c22',
        content: '#0a1220'
    },
    warning: {
        p50: '#fef3c7',
        p100: '#fde68a',
        p200: '#fcd34d',
        p300: '#fbbf24',
        p400: '#f59e0b',
        p500: '#f59e0b',
        p600: '#d97706',
        p700: '#b45309',
        p800: '#92400e',
        p900: '#78350f',
        p950: '#451a03',
        content: '#0a1220'
    },
    danger: {
        p50: '#fee2e2',
        p100: '#fecaca',
        p200: '#fca5a5',
        p300: '#f87171',
        p400: '#ef4444',
        p500: '#ef4444',
        p600: '#dc2626',
        p700: '#b91c1c',
        p800: '#991b1b',
        p900: '#7f1d1d',
        p950: '#450a0a',
        content: '#ffffff'
    },
    info: {
        p50: '#e0f2fe',
        p100: '#bae6fd',
        p200: '#7dd3fc',
        p300: '#38bdf8',
        p400: '#0ea5e9',
        p500: '#0ea5e9',
        p600: '#0284c7',
        p700: '#0369a1',
        p800: '#075985',
        p900: '#0c4a6e',
        p950: '#082f49',
        content: '#0a1220'
    },
    background: {
        b0: '#17293d',
        b25: '#233549',
        b50: '#2f4155',
        b75: '#3b4d61',
        b100: '#47596d',
        b125: '#526478',
        b150: '#5e7084',
        b175: '#6a7c90',
        b200: '#76889c',
        b225: '#8294a8',
        b250: '#8e9fb4',
        b275: '#99abc0',
        b300: '#a3b2c4',
        b325: '#acbacb',
        b350: '#b6c3d2',
        b375: '#bfcbd9',
        b400: '#c9d4e0',
        b425: '#cbd5e1',
        b450: '#d1dae5',
        b475: '#d7dfe8',
        b500: '#dde4ec',
        b525: '#e2e8f0',
        b550: '#e5eaf1',
        b575: '#e8edf3',
        b600: '#ebf0f5',
        b625: '#eef2f7',
        b650: '#f1f5f9',
        b675: '#f3f6fa',
        b700: '#f4f7fa',
        b725: '#f6f9fb',
        b750: '#f8fafc',
        b775: '#f9fbfc',
        b800: '#fbfcfd',
        b825: '#fafbfc',
        b850: '#ffffff',
        b875: '#ffffff',
        b900: '#ffffff',
        b925: '#ffffff',
        b950: '#ffffff'
    },
    terminal: {
        bg: '#0f1c2e',
        fg: '#e2e8f0',
        cursor: '#2ea4bc',
        cursorAccent: '#0f1c2e',
        selectionBg: '#00768e66',
        selectionFg: '#ffffff',
        tabActiveAccent: '#00768e',
        scrollbarThumb: '#76889c73',
        findMatchBg: '#f59e0b52',
        blockBg: '#233549',
        blockBorder: '#3b4d61',
        blockErrorBg: '#ef444424'
    }
};

/**
 * Salva o snapshot do tema atual no `localStorage` para hidratação síncrona no próximo boot.
 */
export function saveThemeBootstrapSnapshot(
    themeId: string,
    tokens: MaxUiTokens,
    context?: CssVarsContext
): void {
    if (typeof window === 'undefined' || !window.localStorage) {
        return;
    }
    try {
        const snapshot: ThemeBootstrapSnapshot = {
            version: 2,
            themeId,
            polarity: tokens.polarity,
            cssVars: buildCssVarsRecord(tokens, context)
        };
        window.localStorage.setItem(BOOTSTRAP_STORAGE_KEY, JSON.stringify(snapshot));
    } catch {
        // Ignora falhas de quota do localStorage
    }
}

/**
 * Lê o snapshot salvo no `localStorage` (retorna `null` se ausente ou corrompido).
 * Para snapshots v2 legados com papéis ausentes, mescla defaults seguros sem quebras.
 */
export function loadThemeBootstrapSnapshot(): ThemeBootstrapSnapshot | null {
    if (typeof window === 'undefined' || !window.localStorage) {
        return null;
    }
    try {
        const raw = window.localStorage.getItem(BOOTSTRAP_STORAGE_KEY);
        if (!raw) {
            return null;
        }
        const parsed = JSON.parse(raw) as Partial<ThemeBootstrapSnapshot>;
        if (
            parsed.version !== 2 ||
            typeof parsed.themeId !== 'string' ||
            (parsed.polarity !== 'dark' && parsed.polarity !== 'light') ||
            typeof parsed.cssVars !== 'object' ||
            parsed.cssVars === null
        ) {
            return null;
        }

        const defaults = buildCssVarsRecord({
            ...FALLBACK_MAX_NAVY_TEAL_TOKENS,
            polarity: parsed.polarity
        });
        const completeCssVars: Record<string, string> = {
            ...defaults,
            ...(parsed.cssVars as Record<string, string>)
        };

        return {
            version: 2,
            themeId: parsed.themeId,
            polarity: parsed.polarity,
            cssVars: completeCssVars
        };
    } catch {
        return null;
    }
}

/**
 * Aplica sincronicamente o último tema conhecido (ou fallback `max-navy-teal`) antes de `app.mount('#app')`,
 * eliminando completamente qualquer FOUC (Flash of Unstyled Content).
 */
export function applyThemeBootstrapSync(rootEl?: HTMLElement | null): ThemeBootstrapSnapshot {
    const target =
        rootEl ?? (typeof document !== 'undefined' ? document.documentElement : null);
    const cached = loadThemeBootstrapSnapshot();

    if (cached) {
        if (target) {
            applyCssVarsRecordToElement(target, cached.polarity, cached.cssVars);
        }
        return cached;
    }

    const fallbackVars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS);
    if (target) {
        applyCssVarsRecordToElement(target, 'dark', fallbackVars);
    }
    return {
        version: 2,
        themeId: DEFAULT_DARK_THEME_ID,
        polarity: 'dark',
        cssVars: fallbackVars
    };
}
