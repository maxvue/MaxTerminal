import { describe, expect, it } from 'vitest';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import {
    applyCssVarsRecordToElement,
    applyMaxUiTokensToRoot,
    buildCssVarsRecord
} from '@/theme/cssTokens';

describe('cssTokens', () => {
    it('gera todas as variáveis CSS semânticas esperadas a partir de MaxUiTokens', () => {
        const vars = buildCssVarsRecord(FALLBACK_MAX_NAVY_TEAL_TOKENS);
        expect(vars['--max-primary-500']).toBe('#00768e');
        expect(vars['--max-primary-content']).toBe('#ffffff');
        expect(vars['--max-success-500']).toBe('#10b981');
        expect(vars['--max-warning-500']).toBe('#f59e0b');
        expect(vars['--max-danger-500']).toBe('#ef4444');
        expect(vars['--max-info-500']).toBe('#0ea5e9');
        expect(vars['--background-0']).toBe('#17293d');
        expect(vars['--background-950']).toBe('#ffffff');
        expect(vars['--mt-term-bg']).toBe('#0f1c2e');
        expect(vars['--mt-term-cursor']).toBe('#2ea4bc');
    });

    it('aplica variáveis CSS e alterna a classe dark no elemento alvo', () => {
        const el = document.createElement('div');
        const vars = applyMaxUiTokensToRoot(FALLBACK_MAX_NAVY_TEAL_TOKENS, el);

        expect(el.classList.contains('dark')).toBe(true);
        expect(el.dataset.themePolarity).toBe('dark');
        expect(el.style.colorScheme).toBe('dark');
        expect(el.style.getPropertyValue('--max-primary-500')).toBe(
            vars['--max-primary-500']
        );

        applyCssVarsRecordToElement(el, 'light', {
            '--max-primary-500': '#0284c7',
            '--mt-term-selection-fg': ''
        });
        expect(el.classList.contains('dark')).toBe(false);
        expect(el.dataset.themePolarity).toBe('light');
        expect(el.style.getPropertyValue('--max-primary-500')).toBe('#0284c7');
        expect(el.style.getPropertyValue('--mt-term-selection-fg')).toBe('');
    });
});
