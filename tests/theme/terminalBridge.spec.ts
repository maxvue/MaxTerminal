import { describe, expect, it } from 'vitest';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import { isValidFontFamilyStack, MONOSPACE_FONT_PRESETS } from '@/theme/fonts';
import {
    applyThemeAndTypographyToTerminals,
    resolveEffectiveCursorStyle,
    toXtermITheme
} from '@/theme/terminalBridge';
import type { AppearanceSettings, ResolvedTheme } from '@/types/theme';

function makeResolvedTheme(): ResolvedTheme {
    const palette = [
        '#0d1b2a',
        '#ef4444',
        '#10b981',
        '#f59e0b',
        '#0ea5e9',
        '#a78bfa',
        '#2ea4bc',
        '#d5dde6',
        '#74869a',
        '#f87171',
        '#34d399',
        '#fbbf24',
        '#38bdf8',
        '#c4b5fd',
        '#56c2d7',
        '#ffffff'
    ];
    return {
        scheme: {
            id: 'max-navy-teal',
            name: 'Max Navy/Teal',
            origin: 'builtin',
            polarity: 'dark',
            foreground: '#e2e8f0',
            background: '#0f1c2e',
            cursor: '#2ea4bc',
            cursorAccent: '#0f1c2e',
            cursorStyle: 'block',
            selectionBackground: '#00768e66',
            selectionForeground: '#ffffff',
            activeTabAccent: '#00768e',
            ansi: {
                black: palette[0]!,
                red: palette[1]!,
                green: palette[2]!,
                yellow: palette[3]!,
                blue: palette[4]!,
                magenta: palette[5]!,
                cyan: palette[6]!,
                white: palette[7]!,
                brightBlack: palette[8]!,
                brightRed: palette[9]!,
                brightGreen: palette[10]!,
                brightYellow: palette[11]!,
                brightBlue: palette[12]!,
                brightMagenta: palette[13]!,
                brightCyan: palette[14]!,
                brightWhite: palette[15]!
            },
            indexedColors: {},
            metadata: {
                author: 'Max',
                sourceFormat: 'tabby-scheme-yaml',
                sourceFilename: 'max-navy-teal.yaml',
                sha256: 'a'.repeat(64),
                createdAt: 0,
                updatedAt: 0
            }
        },
        xterm: {
            foreground: '#e2e8f0',
            background: '#0f1c2e',
            cursor: '#2ea4bc',
            cursorAccent: '#0f1c2e',
            selectionBackground: '#00768e66',
            selectionForeground: '#ffffff',
            selectionInactiveBackground: '#00768e33',
            black: palette[0]!,
            red: palette[1]!,
            green: palette[2]!,
            yellow: palette[3]!,
            blue: palette[4]!,
            magenta: palette[5]!,
            cyan: palette[6]!,
            white: palette[7]!,
            brightBlack: palette[8]!,
            brightRed: palette[9]!,
            brightGreen: palette[10]!,
            brightYellow: palette[11]!,
            brightBlue: palette[12]!,
            brightMagenta: palette[13]!,
            brightCyan: palette[14]!,
            brightWhite: palette[15]!,
            extendedAnsi: null
        },
        uiTokens: FALLBACK_MAX_NAVY_TEAL_TOKENS
    };
}

describe('terminalBridge & fonts', () => {
    it('converte XtermThemePayload para ITheme e resolve estilo do cursor', () => {
        const resolved = makeResolvedTheme();
        const iTheme = toXtermITheme(resolved.xterm);
        expect(iTheme.background).toBe('#0f1c2e');
        expect(iTheme.cyan).toBe('#2ea4bc');

        expect(resolveEffectiveCursorStyle(resolved, null)).toBe('block');

        const appearance: AppearanceSettings = {
            activeThemeId: 'max-navy-teal',
            followSystem: false,
            preferredDarkThemeId: 'max-navy-teal',
            preferredLightThemeId: 'max-light',
            fontFamily: 'JetBrains Mono, monospace',
            fontSize: 14,
            lineHeight: 1.25,
            fontLigatures: true,
            cursorBlink: true,
            cursorStyleOverride: 'bar',
            minimumContrastRatio: 1,
            projectOverrides: {}
        };
        expect(resolveEffectiveCursorStyle(resolved, appearance)).toBe('bar');
        applyThemeAndTypographyToTerminals(resolved, appearance);
    });

    it('valida pilhas de fontes monoespaçadas contra CSS injection', () => {
        expect(MONOSPACE_FONT_PRESETS.length).toBeGreaterThanOrEqual(5);
        expect(isValidFontFamilyStack('JetBrains Mono, monospace')).toBe(true);
        expect(isValidFontFamilyStack('bad;color:red')).toBe(false);
    });
});
