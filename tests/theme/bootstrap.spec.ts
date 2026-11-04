import { beforeEach, describe, expect, it } from 'vitest';
import {
    applyThemeBootstrapSync,
    FALLBACK_MAX_NAVY_TEAL_TOKENS,
    loadThemeBootstrapSnapshot,
    saveThemeBootstrapSnapshot
} from '@/theme/bootstrap';
import { isValidFontFamilyStack, MONOSPACE_FONT_PRESETS } from '@/theme/fonts';
import { BOOTSTRAP_STORAGE_KEY } from '@/types/theme';

describe('theme bootstrap sem FOUC', () => {
    beforeEach(() => {
        window.localStorage.clear();
    });

    it('usa fallback max-navy-teal canônico v2 quando localStorage está vazio', () => {
        const el = document.createElement('div');
        const snap = applyThemeBootstrapSync(el);

        expect(snap.version).toBe(2);
        expect(snap.themeId).toBe('max-navy-teal');
        expect(snap.polarity).toBe('dark');
        expect(el.classList.contains('dark')).toBe(true);
        expect(el.dataset.themePolarity).toBe('dark');
        expect(el.style.getPropertyValue('--max-primary-500')).toBe('#00768e');
        // Rampa canônica do Rust: b0 é superfície escura
        expect(el.style.getPropertyValue('--background-0')).toBe('#17293d');
        expect(el.style.getPropertyValue('--mt-ui-surface')).toBe('#17293d');
    });

    it('persiste e restaura snapshot válido v2 sem FOUC nos dois sentidos (escuro e claro)', () => {
        const lightTokens = {
            ...FALLBACK_MAX_NAVY_TEAL_TOKENS,
            polarity: 'light' as const
        };
        saveThemeBootstrapSnapshot('max-light', lightTokens);

        const loaded = loadThemeBootstrapSnapshot();
        expect(loaded?.version).toBe(2);
        expect(loaded?.themeId).toBe('max-light');
        expect(loaded?.polarity).toBe('light');

        const el = document.createElement('div');
        const applied = applyThemeBootstrapSync(el);
        expect(applied.version).toBe(2);
        expect(applied.themeId).toBe('max-light');
        expect(el.classList.contains('dark')).toBe(false);
        expect(el.dataset.themePolarity).toBe('light');
        expect(el.style.colorScheme).toBe('light');

        // Agora salva novamente um tema escuro e confere restauração
        saveThemeBootstrapSnapshot('max-navy-teal', FALLBACK_MAX_NAVY_TEAL_TOKENS);
        const reloaded = loadThemeBootstrapSnapshot();
        expect(reloaded?.polarity).toBe('dark');

        const elDark = document.createElement('div');
        const appliedDark = applyThemeBootstrapSync(elDark);
        expect(appliedDark.polarity).toBe('dark');
        expect(elDark.classList.contains('dark')).toBe(true);
        expect(elDark.dataset.themePolarity).toBe('dark');
    });

    it('rejeita snapshot antigo v1 e faz fallback seguro', () => {
        window.localStorage.setItem(
            BOOTSTRAP_STORAGE_KEY,
            JSON.stringify({
                version: 1,
                themeId: 'max-navy-teal',
                polarity: 'dark',
                cssVars: { '--max-primary-500': '#00768e' }
            })
        );
        expect(loadThemeBootstrapSnapshot()).toBeNull();

        const el = document.createElement('div');
        const snap = applyThemeBootstrapSync(el);
        expect(snap.version).toBe(2);
        expect(snap.themeId).toBe('max-navy-teal');
    });

    it('ignora JSON corrompido no localStorage e cai no fallback v2', () => {
        window.localStorage.setItem(BOOTSTRAP_STORAGE_KEY, '{corrompido');
        expect(loadThemeBootstrapSnapshot()).toBeNull();

        const el = document.createElement('div');
        const snap = applyThemeBootstrapSync(el);
        expect(snap.version).toBe(2);
        expect(snap.themeId).toBe('max-navy-teal');
    });

    it('hidrata snapshot v2 legado completando novos papéis ausentes sem quebras', () => {
        // Simula snapshot v2 antigo gravado antes da introdução dos tokens de form/issues
        window.localStorage.setItem(
            BOOTSTRAP_STORAGE_KEY,
            JSON.stringify({
                version: 2,
                themeId: 'max-navy-teal',
                polarity: 'dark',
                cssVars: {
                    '--max-primary-500': '#00768e',
                    '--background-0': '#17293d'
                }
            })
        );

        const loaded = loadThemeBootstrapSnapshot();
        expect(loaded?.version).toBe(2);
        expect(loaded?.themeId).toBe('max-navy-teal');
        expect(loaded?.cssVars['--max-inputtext-border-color']).toBeDefined();
        expect(loaded?.cssVars['--max-purple-500']).toBe('#a855f7');
        expect(loaded?.cssVars['--max-primary-500']).toBe('#00768e');

        const el = document.createElement('div');
        const applied = applyThemeBootstrapSync(el);
        expect(applied.cssVars['--max-purple-500']).toBe('#a855f7');
        expect(el.style.getPropertyValue('--max-purple-500')).toBe('#a855f7');
    });

    it('valida pilhas de fontes monoespaçadas seguras e rejeita CSS injection', () => {
        expect(MONOSPACE_FONT_PRESETS.length).toBeGreaterThanOrEqual(4);
        expect(isValidFontFamilyStack('JetBrains Mono, monospace')).toBe(true);
        expect(isValidFontFamilyStack('')).toBe(false);
        expect(isValidFontFamilyStack('evil;color:red')).toBe(false);
    });
});
