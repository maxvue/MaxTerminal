import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { defineComponent } from 'vue';
import { useThemeApplier } from '@/composables/useThemeApplier';
import { useTerminalAppearance } from '@/composables/useTerminalAppearance';
import { useThemePreview } from '@/composables/useThemePreview';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';

vi.mock('@/ipc/themes', () => ({
    themeList: vi.fn(async () => []),
    themeGet: vi.fn(async (id: string) => ({
        scheme: {
            id,
            name: id,
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
            ansi: new Array(16).fill('#000000'),
            indexedColors: {},
            metadata: {
                author: null,
                sourceFormat: null,
                sourceFilename: null,
                sha256: '0'.repeat(64),
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
            black: '#000000',
            red: '#000000',
            green: '#000000',
            yellow: '#000000',
            blue: '#000000',
            magenta: '#000000',
            cyan: '#000000',
            white: '#000000',
            brightBlack: '#000000',
            brightRed: '#000000',
            brightGreen: '#000000',
            brightYellow: '#000000',
            brightBlue: '#000000',
            brightMagenta: '#000000',
            brightCyan: '#000000',
            brightWhite: '#000000',
            extendedAnsi: null
        },
        uiTokens: FALLBACK_MAX_NAVY_TEAL_TOKENS
    })),
    themeAppearanceGet: vi.fn(async () => ({
        activeThemeId: 'max-navy-teal',
        followSystem: false,
        preferredDarkThemeId: 'max-navy-teal',
        preferredLightThemeId: 'max-light',
        fontFamily: 'monospace',
        fontSize: 13,
        lineHeight: 1.2,
        fontLigatures: true,
        cursorBlink: true,
        cursorStyleOverride: null,
        minimumContrastRatio: 1,
        projectOverrides: {}
    })),
    onThemeChanged: vi.fn(async () => () => {}),
    onThemeLibraryChanged: vi.fn(async () => () => {})
}));

describe('useThemeApplier', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('inicializa a store de temas e expõe useTerminalAppearance e useThemePreview reativamente', () => {
        vi.useFakeTimers();
        const Host = defineComponent({
            setup() {
                const { themeStore } = useThemeApplier();
                const { xtermTheme, fontOptions } = useTerminalAppearance();
                const { previewingId, previewOnHover, stopPreview } = useThemePreview();
                return {
                    themeStore,
                    xtermTheme,
                    fontOptions,
                    previewingId,
                    previewOnHover,
                    stopPreview
                };
            },
            template: '<div>{{ fontOptions.fontSize }}</div>'
        });

        const wrapper = mount(Host);
        expect(wrapper.text()).toBe('13');
        wrapper.vm.previewOnHover('dracula', 10);
        vi.advanceTimersByTime(20);
        expect(wrapper.vm.previewingId).toBe('dracula');
        wrapper.vm.stopPreview();
        expect(wrapper.vm.previewingId).toBeNull();
        wrapper.unmount();
        vi.useRealTimers();
    });
});
