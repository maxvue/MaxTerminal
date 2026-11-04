import { describe, expect, it } from 'vitest';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import {
    buildMaxAppPaletteFromBootstrap,
    buildMaxAppPaletteFromUiTokens,
    resolveMaxAppColors
} from '@/theme/maxAppPalette';
import type { ColorScheme, ResolvedTheme, ThemeBootstrapSnapshot } from '@/types/theme';

describe('maxAppPalette', () => {
    it('mapeia MaxUiTokens e ColorScheme para PartialColorPalette de cores-base e ANSI', () => {
        const mockScheme: ColorScheme = {
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
                black: '#0d1b2a',
                red: '#ef4444',
                green: '#10b981',
                yellow: '#f59e0b',
                blue: '#0ea5e9',
                magenta: '#a78bfa',
                cyan: '#2ea4bc',
                white: '#d5dde6',
                brightBlack: '#74869a',
                brightRed: '#f87171',
                brightGreen: '#34d399',
                brightYellow: '#fbbf24',
                brightBlue: '#38bdf8',
                brightMagenta: '#c4b5fd',
                brightCyan: '#56c2d7',
                brightWhite: '#ffffff'
            },
            indexedColors: {},
            metadata: {
                author: 'Max',
                sourceFormat: null,
                sourceFilename: null,
                sha256: 'a'.repeat(64),
                createdAt: 0,
                updatedAt: 0
            }
        };

        const palette = buildMaxAppPaletteFromUiTokens(
            FALLBACK_MAX_NAVY_TEAL_TOKENS,
            mockScheme
        );

        expect(palette.primary).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.primary.p500);
        expect(palette.secondary).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.info.p500);
        expect(palette.success).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.success.p500);
        expect(palette.info).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.info.p500);
        expect(palette.warn).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.warning.p500);
        expect(palette.danger).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.danger.p500);
        expect(palette.error).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.danger.p500);
        expect(palette.background).toBe('#0f1c2e');
        expect(palette.red).toBe('#ef4444');
        expect(palette.green).toBe('#10b981');
        expect(palette.yellow).toBe('#f59e0b');
        expect(palette.blue).toBe('#0ea5e9');
        expect(palette.violet).toBe('#a78bfa');
        expect(palette.black).toBe('#0d1b2a');
        expect(palette.white).toBe('#d5dde6');
    });

    it('reconstrói PartialColorPalette a partir de ThemeBootstrapSnapshot e prioriza ResolvedTheme', () => {
        const snapshot: ThemeBootstrapSnapshot = {
            version: 2,
            themeId: 'custom-theme',
            polarity: 'dark',
            cssVars: {
                '--max-primary-500': '#22d3ee',
                '--max-warning-500': '#f59e0b',
                '--max-danger-500': '#ef4444',
                '--mt-term-bg': '#0a0f18',
                '--max-purple-500': '#ff79c6'
            }
        };

        const fromBoot = buildMaxAppPaletteFromBootstrap(snapshot);
        expect(fromBoot?.primary).toBe('#22d3ee');
        expect(fromBoot?.warn).toBe('#f59e0b');
        expect(fromBoot?.danger).toBe('#ef4444');
        expect(fromBoot?.error).toBe('#ef4444');
        expect(fromBoot?.background).toBe('#0a0f18');
        expect(fromBoot?.violet).toBe('#ff79c6');

        const fakeResolved = {
            uiTokens: {
                ...FALLBACK_MAX_NAVY_TEAL_TOKENS,
                primary: {
                    ...FALLBACK_MAX_NAVY_TEAL_TOKENS.primary,
                    p500: '#10b981'
                }
            }
        } as unknown as ResolvedTheme;

        const resolvedColors = resolveMaxAppColors(fakeResolved, snapshot);
        expect(resolvedColors.primary).toBe('#10b981');

        const fallbackColors = resolveMaxAppColors(null, null);
        expect(fallbackColors.primary).toBe(
            FALLBACK_MAX_NAVY_TEAL_TOKENS.primary.p500
        );
    });
});
