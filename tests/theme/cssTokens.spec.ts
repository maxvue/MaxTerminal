import { describe, expect, it } from 'vitest';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import {
    applyCssVarsRecordToElement,
    applyMaxUiTokensToRoot,
    buildCssVarsRecord
} from '@/theme/cssTokens';
import type { MaxUiTokens } from '@/types/theme';

const REAL_MAX_LIGHT_TOKENS: MaxUiTokens = {
    polarity: 'light',
    primary: {
        p50: '#ecfeff',
        p100: '#cffafe',
        p200: '#a5f3fc',
        p300: '#67e8f9',
        p400: '#22d3ee',
        p500: '#06b6d4',
        p600: '#0891b2',
        p700: '#0e7490',
        p800: '#155e75',
        p900: '#164e63',
        p950: '#083344',
        content: '#ffffff'
    },
    success: {
        p50: '#f0fdf4',
        p100: '#dcfce7',
        p200: '#bbf7d0',
        p300: '#86efac',
        p400: '#4ade80',
        p500: '#22c55e',
        p600: '#16a34a',
        p700: '#15803d',
        p800: '#166534',
        p900: '#14532d',
        p950: '#052e16',
        content: '#ffffff'
    },
    warning: {
        p50: '#fffbeb',
        p100: '#fef3c7',
        p200: '#fde68a',
        p300: '#fcd34d',
        p400: '#fbbf24',
        p500: '#f59e0b',
        p600: '#d97706',
        p700: '#b45309',
        p800: '#92400e',
        p900: '#78350f',
        p950: '#451a03',
        content: '#000000'
    },
    danger: {
        p50: '#fef2f2',
        p100: '#fee2e2',
        p200: '#fecaca',
        p300: '#fca5a5',
        p400: '#f87171',
        p500: '#ef4444',
        p600: '#dc2626',
        p700: '#b91c1c',
        p800: '#991b1b',
        p900: '#7f1d1d',
        p950: '#450a0a',
        content: '#ffffff'
    },
    info: {
        p50: '#f0f9ff',
        p100: '#e0f2fe',
        p200: '#bae6fd',
        p300: '#7dd3fc',
        p400: '#38bdf8',
        p500: '#0ea5e9',
        p600: '#0284c7',
        p700: '#0369a1',
        p800: '#075985',
        p900: '#0c4a6e',
        p950: '#082f49',
        content: '#ffffff'
    },
    background: {
        b0: '#ffffff',
        b25: '#fafbfc',
        b50: '#fbfcfd',
        b75: '#f9fbfc',
        b100: '#f8fafc',
        b125: '#f6f9fb',
        b150: '#f4f7fa',
        b175: '#f3f6fa',
        b200: '#f1f5f9',
        b225: '#eef2f7',
        b250: '#ebf0f5',
        b275: '#e8edf3',
        b300: '#e5eaf1',
        b325: '#e2e8f0',
        b350: '#dde4ec',
        b375: '#d7dfe8',
        b400: '#d1dae5',
        b425: '#cbd5e1',
        b450: '#c9d4e0',
        b475: '#bfcbd9',
        b500: '#b6c3d2',
        b525: '#acbacb',
        b550: '#a3b2c4',
        b575: '#99abc0',
        b600: '#8e9fb4',
        b625: '#8294a8',
        b650: '#76889c',
        b675: '#6a7c90',
        b700: '#5e7084',
        b725: '#526478',
        b750: '#47596d',
        b775: '#3b4d61',
        b800: '#2f4155',
        b825: '#233549',
        b850: '#17293d',
        b875: '#0c1e32',
        b900: '#001226',
        b925: '#000c1a',
        b950: '#00060e'
    },
    terminal: {
        bg: '#ffffff',
        fg: '#1e293b',
        cursor: '#0ea5e9',
        cursorAccent: '#ffffff',
        selectionBg: '#0ea5e933',
        selectionFg: null,
        tabActiveAccent: '#0ea5e9',
        scrollbarThumb: '#94a3b873',
        findMatchBg: '#fef08a',
        blockBg: '#f8fafc',
        blockBorder: '#e2e8f0',
        blockErrorBg: '#fee2e240'
    }
};

describe('cssTokens', () => {
    it('gera todas as variáveis CSS semânticas a partir de MaxUiTokens escuro canônico', () => {
        const vars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS);
        expect(vars['--max-primary-500']).toBe('#00768e');
        expect(vars['--max-primary-content']).toBe('#ffffff');
        expect(vars['--max-success-500']).toBe('#10b981');
        expect(vars['--max-warning-500']).toBe('#f59e0b');
        expect(vars['--max-danger-500']).toBe('#ef4444');
        expect(vars['--max-info-500']).toBe('#0ea5e9');

        // Contrato canônico escuro: b0 é superfície escura, b950 é contraste máximo
        expect(vars['--background-0']).toBe('#17293d');
        expect(vars['--background-950']).toBe('#ffffff');

        // Aliases semânticos de UI
        expect(vars['--mt-ui-surface']).toBe('#17293d');
        expect(vars['--mt-ui-surface-panel']).toBe('#233549');
        expect(vars['--mt-ui-border']).toBe('#47596d');
        expect(vars['--mt-ui-text-strong']).toBe('#ffffff');

        expect(vars['--mt-term-bg']).toBe('#0f1c2e');
        expect(vars['--mt-term-cursor']).toBe('#2ea4bc');
    });

    it('gera todas as variáveis CSS semânticas a partir de MaxUiTokens claro real', () => {
        const vars = buildCssVarsRecord(REAL_MAX_LIGHT_TOKENS);
        expect(vars['--max-primary-500']).toBe('#06b6d4');

        // Contrato canônico claro: b0 é superfície clara, b950 é texto de alto contraste escuro
        expect(vars['--background-0']).toBe('#ffffff');
        expect(vars['--background-950']).toBe('#00060e');

        // Aliases semânticos de UI no tema claro revisados para contraste
        expect(vars['--mt-ui-surface']).toBe('#ffffff');
        expect(vars['--mt-ui-surface-panel']).toBe('#fafbfc');
        expect(vars['--mt-ui-border']).toBe('#e5eaf1');
        expect(vars['--mt-ui-text-strong']).toBe('#00060e');
    });

    it('aplica variáveis CSS e alterna polaridade no elemento alvo nos dois sentidos', () => {
        const el = document.createElement('div');

        // 1. Aplica tema escuro
        const darkVars = applyMaxUiTokensToRoot(FALLBACK_MAX_NAVY_TEAL_TOKENS, el);
        expect(el.classList.contains('dark')).toBe(true);
        expect(el.dataset.themePolarity).toBe('dark');
        expect(el.style.colorScheme).toBe('dark');
        expect(el.style.getPropertyValue('--mt-ui-surface')).toBe(darkVars['--mt-ui-surface']);
        expect(el.style.getPropertyValue('--background-0')).toBe('#17293d');

        // 2. Transição para tema claro
        const lightVars = applyMaxUiTokensToRoot(REAL_MAX_LIGHT_TOKENS, el);
        expect(el.classList.contains('dark')).toBe(false);
        expect(el.dataset.themePolarity).toBe('light');
        expect(el.style.colorScheme).toBe('light');
        expect(el.style.getPropertyValue('--mt-ui-surface')).toBe(lightVars['--mt-ui-surface']);
        expect(el.style.getPropertyValue('--background-0')).toBe('#ffffff');
        expect(el.style.getPropertyValue('--background-950')).toBe('#00060e');

        // 3. Retorno para tema escuro
        applyMaxUiTokensToRoot(FALLBACK_MAX_NAVY_TEAL_TOKENS, el);
        expect(el.classList.contains('dark')).toBe(true);
        expect(el.dataset.themePolarity).toBe('dark');
        expect(el.style.colorScheme).toBe('dark');
        expect(el.style.getPropertyValue('--background-0')).toBe('#17293d');
    });

    it('remove propriedades vazias ao aplicar CSS vars record', () => {
        const el = document.createElement('div');
        applyCssVarsRecordToElement(el, 'light', {
            '--max-primary-500': '#0284c7',
            '--mt-term-selection-fg': ''
        });
        expect(el.classList.contains('dark')).toBe(false);
        expect(el.dataset.themePolarity).toBe('light');
        expect(el.style.getPropertyValue('--max-primary-500')).toBe('#0284c7');
        expect(el.style.getPropertyValue('--mt-term-selection-fg')).toBe('');
    });

    it('gera tokens do ecossistema MaxComponentsUi e controles de formulário/foco', () => {
        const vars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS);

        expect(vars['--max-inputtext-border-color']).toBe('#47596d');
        expect(vars['--max-inputtext-focus-border-color']).toBe('#00768e');
        expect(vars['--max-form-field-disabled-background']).toBe('#3b4d61');
        expect(vars['--max-floatlabel-on-active-background']).toBe('#17293d');
        expect(vars['--max-focus-ring-color']).toBe('#178da5');
        expect(vars['--max-focus-ring']).toContain('0 0 0 2px');
        expect(vars['--max-button-primary-border-color']).toBe('#00768e');
        expect(vars['--max-button-help-border-color']).toBe('#a855f7');
        expect(vars['--max-table-header-bg']).toBe('#2f4155');
        expect(vars['--max-selection-background']).toBe('#00768e66');
    });

    it('deriva categoria de issues a partir do contexto ANSI (ou fallback canônico)', () => {
        // 1. Fallback sem contexto
        const fallbackVars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS);
        expect(fallbackVars['--max-purple-500']).toBe('#a855f7');
        expect(fallbackVars['--max-purple-400']).toBe('#c084fc');
        expect(fallbackVars['--mt-ui-issue-category']).toBe('#a855f7');

        // 2. Com paleta ANSI explícita (ex.: Dracula)
        const draculaVars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS, {
            scheme: {
                ansi: {
                    black: '#21222c',
                    red: '#ff5555',
                    green: '#50fa7b',
                    yellow: '#f1fa8c',
                    blue: '#bd93f9',
                    magenta: '#ff79c6',
                    cyan: '#8be9fd',
                    white: '#f8f8f2',
                    brightBlack: '#6272a4',
                    brightRed: '#ff6e6e',
                    brightGreen: '#69ff94',
                    brightYellow: '#ffffa5',
                    brightBlue: '#d6acff',
                    brightMagenta: '#ff92df',
                    brightCyan: '#a4ffff',
                    brightWhite: '#ffffff'
                }
            }
        });
        expect(draculaVars['--max-purple-500']).toBe('#ff79c6');
        expect(draculaVars['--max-purple-400']).toBe('#ff92df');
        expect(draculaVars['--mt-ui-issue-category']).toBe('#ff79c6');
    });

    it('assegura contraste WCAG em pares essenciais (texto normal >= 4.5:1, texto mudo >= 3:1)', async () => {
        const { getColorFromVar } = await import('@maxvue/max-use');
        const darkVars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS);
        const darkSurface = getColorFromVar(darkVars['--mt-ui-surface']!);
        const darkText = getColorFromVar(darkVars['--mt-ui-text']!);
        const darkMuted = getColorFromVar(darkVars['--mt-ui-text-muted']!);

        expect(darkText.contrast(darkSurface)).toBeGreaterThanOrEqual(4.5);
        expect(darkMuted.contrast(darkSurface)).toBeGreaterThanOrEqual(3.0);

        const lightVars = buildCssVarsRecord(REAL_MAX_LIGHT_TOKENS);
        const lightSurface = getColorFromVar(lightVars['--mt-ui-surface']!);
        const lightText = getColorFromVar(lightVars['--mt-ui-text']!);
        const lightMuted = getColorFromVar(lightVars['--mt-ui-text-muted']!);

        expect(lightText.contrast(lightSurface)).toBeGreaterThanOrEqual(4.5);
        expect(lightMuted.contrast(lightSurface)).toBeGreaterThanOrEqual(3.0);
    });
});
