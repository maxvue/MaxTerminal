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

    it('usa fallback max-navy-teal quando localStorage está vazio', () => {
        const el = document.createElement('div');
        const snap = applyThemeBootstrapSync(el);

        expect(snap.themeId).toBe('max-navy-teal');
        expect(snap.polarity).toBe('dark');
        expect(el.classList.contains('dark')).toBe(true);
        expect(el.style.getPropertyValue('--max-primary-500')).toBe('#00768e');
    });

    it('persiste e restaura snapshot válido sem FOUC', () => {
        const lightTokens = {
            ...FALLBACK_MAX_NAVY_TEAL_TOKENS,
            polarity: 'light' as const
        };
        saveThemeBootstrapSnapshot('max-light', lightTokens);

        const loaded = loadThemeBootstrapSnapshot();
        expect(loaded?.themeId).toBe('max-light');
        expect(loaded?.polarity).toBe('light');

        const el = document.createElement('div');
        const applied = applyThemeBootstrapSync(el);
        expect(applied.themeId).toBe('max-light');
        expect(el.classList.contains('dark')).toBe(false);
        expect(el.dataset.themePolarity).toBe('light');
    });

    it('ignora JSON corrompido no localStorage e cai no fallback', () => {
        window.localStorage.setItem(BOOTSTRAP_STORAGE_KEY, '{corrompido');
        expect(loadThemeBootstrapSnapshot()).toBeNull();

        const el = document.createElement('div');
        const snap = applyThemeBootstrapSync(el);
        expect(snap.themeId).toBe('max-navy-teal');
    });

    it('valida pilhas de fontes monoespaçadas seguras e rejeita CSS injection', () => {
        expect(MONOSPACE_FONT_PRESETS.length).toBeGreaterThanOrEqual(4);
        expect(isValidFontFamilyStack('JetBrains Mono, monospace')).toBe(true);
        expect(isValidFontFamilyStack('')).toBe(false);
        expect(isValidFontFamilyStack('evil;color:red')).toBe(false);
    });
});
