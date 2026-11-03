import {
    applyCssVarsRecordToElement,
    buildCssVarsRecord
} from '@/theme/cssTokens';
import {
    BOOTSTRAP_STORAGE_KEY,
    DEFAULT_DARK_THEME_ID,
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
        b0: '#ffffff',
        b25: '#fbfcfd',
        b50: '#f8fafc',
        b75: '#f3f6fa',
        b100: '#e8eef8',
        b125: '#dfe7f4',
        b150: '#d5e0ef',
        b175: '#cdd9eb',
        b200: '#c5d1e6',
        b225: '#b5c4dc',
        b250: '#a5b7d1',
        b275: '#9caecb',
        b300: '#94a6c4',
        b325: '#879bb9',
        b350: '#7a90af',
        b375: '#6f84a4',
        b400: '#64799e',
        b425: '#596e92',
        b450: '#4e6387',
        b475: '#45597e',
        b500: '#3d5278',
        b525: '#36496e',
        b550: '#304164',
        b575: '#2c3e60',
        b600: '#283b5c',
        b625: '#243452',
        b650: '#202e48',
        b675: '#1d2a42',
        b700: '#1b2a44',
        b725: '#18243c',
        b750: '#152036',
        b775: '#131e33',
        b800: '#111c30',
        b825: '#0f182b',
        b850: '#0d1527',
        b875: '#0c1424',
        b900: '#0b1322',
        b925: '#080f1b',
        b950: '#060b14'
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
    tokens: MaxUiTokens
): void {
    if (typeof window === 'undefined' || !window.localStorage) {
        return;
    }
    try {
        const snapshot: ThemeBootstrapSnapshot = {
            version: 1,
            themeId,
            polarity: tokens.polarity,
            cssVars: buildCssVarsRecord(tokens)
        };
        window.localStorage.setItem(BOOTSTRAP_STORAGE_KEY, JSON.stringify(snapshot));
    } catch {
        // Ignora falhas de quota do localStorage
    }
}

/**
 * Lê o snapshot salvo no `localStorage` (retorna `null` se ausente ou corrompido).
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
            parsed.version !== 1 ||
            typeof parsed.themeId !== 'string' ||
            (parsed.polarity !== 'dark' && parsed.polarity !== 'light') ||
            typeof parsed.cssVars !== 'object' ||
            parsed.cssVars === null
        ) {
            return null;
        }
        return parsed as ThemeBootstrapSnapshot;
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
        version: 1,
        themeId: DEFAULT_DARK_THEME_ID,
        polarity: 'dark',
        cssVars: fallbackVars
    };
}
