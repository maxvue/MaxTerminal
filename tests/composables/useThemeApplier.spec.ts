import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { defineComponent, h, nextTick } from 'vue';
import { mount } from '@vue/test-utils';
import { useThemeApplier } from '@/composables/useThemeApplier';
import { useThemeStore } from '@/stores/theme';
import type { ResolvedTheme } from '@/types/theme';
import * as nativeWindow from '@/theme/nativeWindow';

vi.mock('@/ipc/themes', () => ({
    themeList: vi.fn(async () => []),
    themeGet: vi.fn(async () => null),
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
    themeAppearanceUpdate: vi.fn(async () => ({})),
    themeSetActive: vi.fn(async () => null),
    themeImportFile: vi.fn(async () => ({})),
    themeImportDir: vi.fn(async () => ({})),
    themeDelete: vi.fn(async () => null),
    themeExport: vi.fn(async () => ''),
    onThemeChanged: vi.fn(async () => () => {}),
    onThemeLibraryChanged: vi.fn(async () => () => {})
}));

const TestHost = defineComponent({
    setup() {
        const { themeStore } = useThemeApplier();
        return { themeStore };
    },
    render() {
        return h('div', 'host');
    }
});

describe('useThemeApplier integration', () => {
    let applyNativeWindowThemeSpy: ReturnType<typeof vi.spyOn>;

    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        document.documentElement.dataset.themePolarity = 'dark';
        applyNativeWindowThemeSpy = vi.spyOn(nativeWindow, 'applyNativeWindowTheme').mockResolvedValue(true);
    });

    it('sincroniza janela nativa no boot com a polaridade inicial do DOM', () => {
        mount(TestHost);
        expect(applyNativeWindowThemeSpy).toHaveBeenCalledWith('dark');
    });

    it('reage a mudanças na polaridade do tema resolvido', async () => {
        const wrapper = mount(TestHost);
        const themeStore = useThemeStore();

        // Simula tema resolvido claro
        themeStore.activeResolved = {
            scheme: {
                id: 'max-light',
                name: 'Max Light',
                origin: 'builtin',
                polarity: 'light'
            },
            uiTokens: {
                polarity: 'light'
            }
        } as unknown as ResolvedTheme;

        await nextTick();
        expect(applyNativeWindowThemeSpy).toHaveBeenCalledWith('light');

        wrapper.unmount();
    });
});
