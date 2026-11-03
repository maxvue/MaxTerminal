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
    props: {
        fallback: {
            type: Boolean,
            default: false
        }
    },
    setup(props) {
        const { themeStore } = useThemeApplier(
            props.fallback ? { nativeIntegrationAvailable: false } : undefined
        );
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

    describe('caminho principal (prioridade do sistema)', () => {
        it('sincroniza janela nativa no boot com null para preservar as cores do SO', () => {
            mount(TestHost);
            expect(applyNativeWindowThemeSpy).toHaveBeenCalledWith(null);
        });

        it('mantém null (sistema) mesmo quando a polaridade do tema resolvido muda', async () => {
            const wrapper = mount(TestHost);
            const themeStore = useThemeStore();

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
            expect(applyNativeWindowThemeSpy).toHaveBeenCalledWith(null);

            wrapper.unmount();
        });
    });

    describe('modo fallback escuro (quando integração nativa for indisponível)', () => {
        it('sincroniza janela nativa no boot com "dark" se o conteúdo for escuro', () => {
            mount(TestHost, { props: { fallback: true } });
            expect(applyNativeWindowThemeSpy).toHaveBeenCalledWith('dark');
        });

        it('alterna para null ao transicionar para tema claro e volta para "dark" ao retornar ao escuro', async () => {
            const wrapper = mount(TestHost, { props: { fallback: true } });
            const themeStore = useThemeStore();

            // Transiciona para tema claro: fallback desativa, retornando para null
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
            expect(applyNativeWindowThemeSpy).toHaveBeenCalledWith(null);

            // Transiciona de volta para tema escuro: fallback escuro ativa com 'dark'
            themeStore.activeResolved = {
                scheme: {
                    id: 'max-navy-teal',
                    name: 'Max Navy Teal',
                    origin: 'builtin',
                    polarity: 'dark'
                },
                uiTokens: {
                    polarity: 'dark'
                }
            } as unknown as ResolvedTheme;

            await nextTick();
            expect(applyNativeWindowThemeSpy).toHaveBeenCalledWith('dark');

            wrapper.unmount();
        });
    });
});
