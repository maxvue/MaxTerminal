import axios from 'axios';
import { createApp, defineComponent } from 'vue';
import { flushPromises, mount } from '@vue/test-utils';
import { MaxInputSelect } from '@maxvue/max-components-ui';
import { createMaxPinia } from '@maxvue/max-pinia';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AppearancePreferences from '@/components/preferences/appearance/AppearancePreferences.vue';
import ProjectThemeOverrides from '@/components/preferences/appearance/ProjectThemeOverrides.vue';
import TerminalFontSettings from '@/components/preferences/appearance/TerminalFontSettings.vue';
import ThemeImportReport from '@/components/preferences/appearance/ThemeImportReport.vue';
import { MtIpcError } from '@/ipc/core';
import { installThemeMaxPiniaAdapter } from '@/ipc/themeMaxPiniaAdapter';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import type {
    AppearanceSettings,
    ImportReport,
    ResolvedTheme,
    ThemeSummary
} from '@/types/theme';

type MaxPiniaAxios = NonNullable<NonNullable<Parameters<typeof createMaxPinia>[0]>['axios']>;

const sampleSummary: ThemeSummary = {
    id: 'max-navy-teal',
    name: 'Max Navy/Teal',
    origin: 'builtin',
    polarity: 'dark',
    background: '#0f1c2e',
    foreground: '#e2e8f0',
    cursor: '#2ea4bc',
    accentSwatches: [
        '#0d1b2a',
        '#ef4444',
        '#10b981',
        '#f59e0b',
        '#0ea5e9',
        '#a78bfa',
        '#2ea4bc',
        '#d5dde6'
    ],
    author: 'Max',
    sourceFormat: 'tabby-scheme-yaml'
};

const sampleAppearance: AppearanceSettings = {
    activeThemeId: 'max-navy-teal',
    followSystem: false,
    preferredDarkThemeId: 'max-navy-teal',
    preferredLightThemeId: 'max-light',
    fontFamily: 'JetBrains Mono, Fira Code, Cascadia Code, monospace',
    fontSize: 13,
    lineHeight: 1.2,
    fontLigatures: true,
    cursorBlink: true,
    cursorStyleOverride: null,
    minimumContrastRatio: 1,
    projectOverrides: {}
};

vi.mock('@/ipc/themes', () => ({
    themeList: vi.fn(async () => [sampleSummary]),
    themeGet: vi.fn(async () => ({
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
            ansi: new Array(16).fill('#000000'),
            indexedColors: {},
            metadata: {
                author: 'Max',
                sourceFormat: 'tabby-scheme-yaml',
                sourceFilename: 'max-navy-teal.yaml',
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
    themeAppearanceGet: vi.fn(async () => ({ ...sampleAppearance })),
    themeAppearanceUpdate: vi.fn(async (p: Record<string, unknown>) => ({
        ...sampleAppearance,
        ...p
    })),
    themeSetActive: vi.fn(async () => ({})),
    themeImportFile: vi.fn(async () => ({
        imported: [],
        warnings: [],
        errors: [],
        skipped: []
    })),
    themeImportDir: vi.fn(async () => ({
        imported: [],
        warnings: [],
        errors: [],
        skipped: []
    })),
    themeCreate: vi.fn(async () => ({})),
    themeUpdate: vi.fn(async () => ({})),
    themeDuplicate: vi.fn(async () => ({
        scheme: { id: 'max-navy-teal-copia', name: 'Max Navy/Teal (cópia)' }
    })),
    themeDelete: vi.fn(async () => null),
    themeExport: vi.fn(async () => 'name: Exported\n'),
    onThemeChanged: vi.fn(async () => () => {}),
    onThemeLibraryChanged: vi.fn(async () => () => {})
}));

describe('AppearancePreferences e subcomponentes', () => {
    let pinia: ReturnType<typeof createPinia>;

    beforeEach(() => {
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);
        pinia = createPinia();
        pinia.use(
            createMaxPinia({
                axios: client as unknown as MaxPiniaAxios
            }) as unknown as PiniaPlugin
        );
        const app = createApp(defineComponent({ render: () => null }));
        app.use(pinia);
        setActivePinia(pinia);
    });

    it('renderiza as seções de pré-visualização, catálogo, botão Novo Tema, tipografia e overrides', () => {
        const wrapper = mount(AppearancePreferences);
        expect(wrapper.text()).toContain('Catálogo de Temas e Importador Tabby');
        expect(wrapper.text()).toContain('Novo Tema');
        expect(wrapper.text()).toContain('Tipografia e Cursor do Terminal');
        expect(wrapper.text()).toContain('Temas por Projeto');
    });

    it('TerminalFontSettings emite atualizações de tipografia e valida pilha de fontes', async () => {
        const wrapper = mount(TerminalFontSettings, {
            props: {
                appearance: { ...sampleAppearance }
            }
        });

        const selects = wrapper.findAllComponents(MaxInputSelect);
        await selects[0]!.vm.$emit('change', 'fira-code');
        expect(wrapper.emitted('update')?.[0]?.[0]).toHaveProperty('fontFamily');

        await wrapper.find('#mt-font-family').setValue('bad;font{color:red}');
        expect(wrapper.text()).toContain('Pilha de fontes contém caracteres inválidos.');

        await wrapper.find('#mt-font-family').setValue('Iosevka, monospace');
        await wrapper.find('#mt-font-size').setValue('16');
        await wrapper.find('#mt-line-height').setValue('1.35');
        await selects[1]!.vm.$emit('change', 'bar');
        await selects[1]!.vm.$emit('change', 'theme');
        await wrapper.find('#mt-contrast-ratio').setValue('4.5');

        const checkboxes = wrapper.findAll('input[type="checkbox"]');
        await checkboxes[0]!.setValue(false);
        await checkboxes[1]!.setValue(false);

        expect(wrapper.emitted('update')!.length).toBeGreaterThanOrEqual(7);
    });

    it('TerminalFontSettings normaliza números localizados em pt-BR e respeita limites e passos', async () => {
        const wrapper = mount(TerminalFontSettings, {
            props: {
                appearance: { ...sampleAppearance }
            }
        });

        const inputs = wrapper.findAllComponents({ name: 'MaxInputNumber' });
        expect(inputs.length).toBe(3);

        // 1. fontSize: clamp de 5 para 8
        inputs[0]!.vm.$emit('update:modelValue', 5);
        // 2. fontSize: clamp de 45 para 36
        inputs[0]!.vm.$emit('update:modelValue', 45);
        // 3. lineHeight com vírgula pt-BR: '1,35' -> 1.35
        inputs[1]!.vm.$emit('update:modelValue', '1,35');
        // 4. contrastRatio com vírgula pt-BR: '4,5' -> 4.5
        inputs[2]!.vm.$emit('update:modelValue', '4,5');

        const updates = wrapper.emitted('update') as Array<[Partial<AppearanceSettings>]>;
        expect(updates).toBeDefined();

        expect(updates[0]![0]).toEqual({ fontSize: 8 });
        expect(updates[1]![0]).toEqual({ fontSize: 36 });
        expect(updates[2]![0]).toEqual({ lineHeight: 1.35 });
        expect(updates[3]![0]).toEqual({ minimumContrastRatio: 4.5 });
    });

    it('ProjectThemeOverrides emite escopo por projeto e limpeza de override', async () => {
        const wrapper = mount(ProjectThemeOverrides, {
            props: {
                projects: [
                    {
                        id: 'proj-1',
                        title: 'MaxTerminal',
                        icon: 'lucide:terminal',
                        color: '#00768e',
                        folder: '/tmp/maxterminal',
                        parentFolderId: null,
                        detectedStack: 'tauri',
                        sortOrder: 0,
                        githubOwner: null,
                        githubRepo: null,
                        openIssuesCount: 0,
                        issuesUpdatedAt: null,
                        createdAt: 0,
                        updatedAt: 0
                    }
                ],
                themes: [sampleSummary],
                overrides: {}
            }
        });

        const select = wrapper.findComponent({ name: 'MaxInputSelect' });
        select.vm.$emit('change', 'max-navy-teal');
        expect(wrapper.emitted('change')?.[0]).toEqual([
            'max-navy-teal',
            { kind: 'project', projectId: 'proj-1' }
        ]);

        select.vm.$emit('change', '');
        expect(wrapper.emitted('change')?.[1]).toEqual([
            'max-navy-teal',
            { kind: 'clear_project', projectId: 'proj-1' }
        ]);
    });

    it('ProjectThemeOverrides exibe MaxEmptyDiv com label quando a lista de projetos estiver vazia', () => {
        const wrapper = mount(ProjectThemeOverrides, {
            props: {
                projects: [],
                themes: [sampleSummary],
                overrides: {}
            }
        });

        const emptyDiv = wrapper.findComponent({ name: 'MaxEmptyDiv' });
        expect(emptyDiv.exists()).toBe(true);
        expect(emptyDiv.props('label')).toBe('Nenhum projeto cadastrado até o momento.');
        expect(emptyDiv.text()).toContain('Nenhum projeto cadastrado até o momento.');
    });

    it('ThemeImportReport exibe contadores, avisos, erros e emite close', async () => {
        const report: ImportReport = {
            imported: [sampleSummary],
            warnings: [
                {
                    schemeName: 'Dracula',
                    code: 'low_fg_bg_contrast',
                    message: 'Contraste baixo'
                }
            ],
            errors: [
                {
                    source: 'broken.yaml',
                    error: {
                        kind: 'unsupported_format',
                        detail: { reason: 'erro de sintaxe' }
                    }
                }
            ],
            skipped: [
                {
                    name: 'dup.yaml',
                    existingId: 'dracula',
                    reason: 'identical_already_exists'
                }
            ]
        };
        const wrapper = mount(ThemeImportReport, {
            props: { report }
        });

        expect(wrapper.text()).toContain('1 importado(s)');
        expect(wrapper.text()).toContain('Contraste baixo');
        expect(wrapper.text()).toContain('broken.yaml');

        await wrapper.find('.import-report__close').trigger('click');
        expect(wrapper.emitted('close')).toHaveLength(1);
    });

    it('exibe impacto de exclusão informando que projetos passarão a seguir o tema global', async () => {
        const customSummary: ThemeSummary = {
            ...sampleSummary,
            id: 'custom-solar',
            name: 'Custom Solar',
            origin: 'custom'
        };

        const ipc = await import('@/ipc/themes');
        vi.mocked(ipc.themeList).mockResolvedValue([sampleSummary, customSummary]);
        vi.mocked(ipc.themeAppearanceGet).mockResolvedValue({
            ...sampleAppearance,
            projectOverrides: { 'proj-1': 'custom-solar' }
        });

        const wrapper = mount(AppearancePreferences, {
            global: {
                plugins: [pinia]
            }
        });
        const { useThemeStore } = await import('@/stores/theme');
        const themeStore = useThemeStore();
        await themeStore.init();
        await flushPromises();

        // Dispara exclusão do tema customizado
        const picker = wrapper.findComponent({ name: 'ThemePicker' });
        picker.vm.$emit('delete', 'custom-solar');
        await wrapper.vm.$nextTick();

        expect(wrapper.text()).toContain('Confirmar Exclusão de Tema');
        expect(wrapper.text()).toContain('1 projeto(s) com override próprio passarão a seguir o tema global');
    });

    it('mantém o modal aberto e exibe erro inline se a exclusão falhar', async () => {
        const customSummary: ThemeSummary = {
            ...sampleSummary,
            id: 'custom-solar',
            name: 'Custom Solar',
            origin: 'custom'
        };

        const ipc = await import('@/ipc/themes');
        vi.mocked(ipc.themeList).mockResolvedValue([sampleSummary, customSummary]);
        vi.mocked(ipc.themeDelete).mockRejectedValueOnce(
            new MtIpcError(
                {
                    code: 'theme.delete_failed',
                    message: 'Falha no banco ao remover tema customizado'
                },
                'theme_delete'
            )
        );

        const wrapper = mount(AppearancePreferences, {
            global: {
                plugins: [pinia]
            }
        });
        const { useThemeStore } = await import('@/stores/theme');
        const themeStore = useThemeStore();
        await themeStore.init();
        await flushPromises();

        const picker = wrapper.findComponent({ name: 'ThemePicker' });
        picker.vm.$emit('delete', 'custom-solar');
        await wrapper.vm.$nextTick();

        const deleteModal = wrapper
            .findAllComponents({ name: 'MaxModal' })
            .find((m) => m.props('title') === 'Confirmar Exclusão de Tema')!;
        expect(deleteModal?.exists()).toBe(true);
        expect(deleteModal.props('visible')).toBe(true);

        const confirmBtn = wrapper
            .findAll('button')
            .find((b) => b.text().includes('Confirmar exclusão'));
        expect(confirmBtn?.exists()).toBe(true);

        await confirmBtn!.trigger('click');
        await flushPromises();

        // Modal deve continuar aberto para permitir nova tentativa
        expect(deleteModal.props('visible')).toBe(true);
        expect(wrapper.text()).toContain('Falha no banco ao remover tema customizado');
    });

    it('fecha o modal após exclusão bem-sucedida', async () => {
        const customSummary: ThemeSummary = {
            ...sampleSummary,
            id: 'custom-solar',
            name: 'Custom Solar',
            origin: 'custom'
        };

        const ipc = await import('@/ipc/themes');
        vi.mocked(ipc.themeList).mockResolvedValue([sampleSummary, customSummary]);
        vi.mocked(ipc.themeDelete).mockResolvedValueOnce(null);

        const wrapper = mount(AppearancePreferences, {
            global: {
                plugins: [pinia]
            }
        });
        const { useThemeStore } = await import('@/stores/theme');
        const themeStore = useThemeStore();
        await themeStore.init();
        await flushPromises();

        const picker = wrapper.findComponent({ name: 'ThemePicker' });
        picker.vm.$emit('delete', 'custom-solar');
        await wrapper.vm.$nextTick();

        const deleteModal = wrapper
            .findAllComponents({ name: 'MaxModal' })
            .find((m) => m.props('title') === 'Confirmar Exclusão de Tema')!;
        expect(deleteModal?.exists()).toBe(true);
        expect(deleteModal.props('visible')).toBe(true);

        const confirmBtn = wrapper
            .findAll('button')
            .find((b) => b.text().includes('Confirmar exclusão'));
        await confirmBtn!.trigger('click');
        await flushPromises();

        expect(deleteModal.props('visible')).toBe(false);
    });

    it('onThemeSaved atualiza o catálogo via themeStore.refreshCatalog e não quebra se refresh falhar', async () => {
        const wrapper = mount(AppearancePreferences, {
            global: {
                plugins: [pinia]
            }
        });
        const { useThemeStore } = await import('@/stores/theme');
        const themeStore = useThemeStore();
        const refreshSpy = vi.spyOn(themeStore, 'refreshCatalog').mockRejectedValueOnce(
            new Error('Timeout de rede virtual')
        );

        const editorDialog = wrapper.findComponent({ name: 'ThemeEditorDialog' });
        const fakeResolved = {
            scheme: { id: 'novo', name: 'Novo', origin: 'custom' }
        } as unknown as ResolvedTheme;

        editorDialog.vm.$emit('saved', fakeResolved);
        await flushPromises();

        expect(refreshSpy).toHaveBeenCalledTimes(1);
        expect(wrapper.text()).toContain(
            'Tema salvo com sucesso, mas ocorreu uma falha ao atualizar o catálogo'
        );
    });
});
