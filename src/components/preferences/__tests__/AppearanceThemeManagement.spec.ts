import axios from 'axios';
import { createApp, defineComponent } from 'vue';
import { flushPromises, mount } from '@vue/test-utils';
import { createMaxPinia } from '@maxvue/max-pinia';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AppearancePreferences from '@/components/preferences/appearance/AppearancePreferences.vue';
import ThemeEditorDialog from '@/components/preferences/appearance/ThemeEditorDialog.vue';
import ThemeSwatch from '@/components/preferences/appearance/ThemeSwatch.vue';
import { installThemeMaxPiniaAdapter } from '@/ipc/themeMaxPiniaAdapter';
import { useThemeStore } from '@/stores/theme';
import { isThemeDraftValid, useThemeEditorStore } from '@/stores/themeEditor';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import {
    DEFAULT_DARK_THEME_ID,
    type AppearanceSettings,
    type ResolvedTheme,
    type ThemeDraftInput,
    type ThemeSummary
} from '@/types/theme';

type MaxPiniaAxios = NonNullable<NonNullable<Parameters<typeof createMaxPinia>[0]>['axios']>;

const sampleBuiltinSummary: ThemeSummary = {
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

const sampleCustomSummary: ThemeSummary = {
    id: 'oceano-custom',
    name: 'Oceano Custom',
    origin: 'custom',
    polarity: 'dark',
    background: '#0b1420',
    foreground: '#e2e8f0',
    cursor: '#14b8a6',
    accentSwatches: [
        '#0b1420',
        '#f43f5e',
        '#10b981',
        '#f59e0b',
        '#3b82f6',
        '#a855f7',
        '#06b6d4',
        '#e2e8f0'
    ],
    author: 'Equipe Max',
    sourceFormat: null
};

const sampleAppearance: AppearanceSettings = {
    activeThemeId: 'max-navy-teal',
    followSystem: false,
    preferredDarkThemeId: 'max-navy-teal',
    preferredLightThemeId: 'max-light',
    fontFamily: 'JetBrains Mono, Fira Code, monospace',
    fontSize: 13,
    lineHeight: 1.2,
    fontLigatures: true,
    cursorBlink: true,
    cursorStyleOverride: null,
    minimumContrastRatio: 1,
    projectOverrides: {
        'projeto-alfa': 'oceano-custom'
    }
};

function createResolved(
    id: string,
    name: string,
    origin: 'builtin' | 'custom' = 'custom'
): ResolvedTheme {
    const ansi = {
        black: '#0b1420',
        red: '#f43f5e',
        green: '#10b981',
        yellow: '#f59e0b',
        blue: '#3b82f6',
        magenta: '#a855f7',
        cyan: '#06b6d4',
        white: '#e2e8f0',
        brightBlack: '#475569',
        brightRed: '#fb7185',
        brightGreen: '#34d399',
        brightYellow: '#fbbf24',
        brightBlue: '#60a5fa',
        brightMagenta: '#c084fc',
        brightCyan: '#22d3ee',
        brightWhite: '#f8fafc'
    };
    return {
        scheme: {
            id,
            name,
            origin,
            polarity: 'dark',
            foreground: '#e2e8f0',
            background: '#0b1420',
            cursor: '#14b8a6',
            cursorAccent: '#0b1420',
            cursorStyle: 'block',
            selectionBackground: '#14b8a652',
            selectionForeground: null,
            activeTabAccent: '#14b8a6',
            ansi,
            indexedColors: {},
            metadata: {
                author: 'Equipe Max',
                sourceFormat: null,
                sourceFilename: null,
                sha256: '0'.repeat(64),
                createdAt: 100,
                updatedAt: 200
            }
        },
        xterm: {
            foreground: '#e2e8f0',
            background: '#0b1420',
            cursor: '#14b8a6',
            cursorAccent: '#0b1420',
            selectionBackground: '#14b8a652',
            selectionForeground: null,
            selectionInactiveBackground: '#14b8a626',
            ...ansi,
            extendedAnsi: null
        },
        uiTokens: FALLBACK_MAX_NAVY_TEAL_TOKENS
    };
}

const ipcMocks = vi.hoisted(() => ({
    themeList: vi.fn(),
    themeGet: vi.fn(),
    themeAppearanceGet: vi.fn(),
    themeAppearanceUpdate: vi.fn(),
    themeSetActive: vi.fn(),
    themeCreate: vi.fn(),
    themeUpdate: vi.fn(),
    themeDuplicate: vi.fn(),
    themeDelete: vi.fn(),
    themeExport: vi.fn(),
    themeImportFile: vi.fn(),
    themeImportDir: vi.fn(),
    onThemeChanged: vi.fn(),
    onThemeLibraryChanged: vi.fn()
}));

vi.mock('@/ipc/themes', () => ({
    themeList: ipcMocks.themeList,
    themeGet: ipcMocks.themeGet,
    themeAppearanceGet: ipcMocks.themeAppearanceGet,
    themeAppearanceUpdate: ipcMocks.themeAppearanceUpdate,
    themeSetActive: ipcMocks.themeSetActive,
    themeCreate: ipcMocks.themeCreate,
    themeUpdate: ipcMocks.themeUpdate,
    themeDuplicate: ipcMocks.themeDuplicate,
    themeDelete: ipcMocks.themeDelete,
    themeExport: ipcMocks.themeExport,
    themeImportFile: ipcMocks.themeImportFile,
    themeImportDir: ipcMocks.themeImportDir,
    onThemeChanged: ipcMocks.onThemeChanged,
    onThemeLibraryChanged: ipcMocks.onThemeLibraryChanged
}));

describe('Gestão de Temas — AppearanceThemeManagement', () => {
    let catalog: ThemeSummary[] = [];

    beforeEach(() => {
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);
        const pinia = createPinia();
        pinia.use(
            createMaxPinia({
                axios: client as unknown as MaxPiniaAxios
            }) as unknown as PiniaPlugin
        );
        const app = createApp(defineComponent({ render: () => null }));
        app.use(pinia);
        setActivePinia(pinia);
        vi.clearAllMocks();

        catalog = [{ ...sampleBuiltinSummary }, { ...sampleCustomSummary }];

        ipcMocks.themeList.mockImplementation(async () => [...catalog]);
        ipcMocks.themeGet.mockImplementation(async (id: string) => {
            const found = catalog.find((t) => t.id === id);
            return createResolved(id, found?.name ?? id, found?.origin === 'builtin' ? 'builtin' : 'custom');
        });
        ipcMocks.themeAppearanceGet.mockImplementation(async () => ({ ...sampleAppearance }));
        ipcMocks.themeAppearanceUpdate.mockImplementation(async (p: Partial<AppearanceSettings>) => ({
            ...sampleAppearance,
            ...p
        }));
        ipcMocks.themeSetActive.mockImplementation(async (id: string) =>
            createResolved(id, id)
        );
        ipcMocks.themeCreate.mockImplementation(async (input: ThemeDraftInput) => {
            const newTheme: ThemeSummary = {
                id: 'tema-criado',
                name: input.name,
                origin: 'custom',
                polarity: 'dark',
                background: input.background,
                foreground: input.foreground,
                cursor: input.cursor,
                accentSwatches: sampleCustomSummary.accentSwatches,
                author: input.author ?? null,
                sourceFormat: null
            };
            catalog.push(newTheme);
            return createResolved('tema-criado', input.name, 'custom');
        });
        ipcMocks.themeUpdate.mockImplementation(async (id: string, input: ThemeDraftInput) => {
            const idx = catalog.findIndex((t) => t.id === id);
            if (idx >= 0) {
                catalog[idx] = {
                    ...catalog[idx]!,
                    name: input.name,
                    background: input.background,
                    foreground: input.foreground
                };
            }
            return createResolved(id, input.name, 'custom');
        });
        ipcMocks.themeDuplicate.mockImplementation(async (id: string) => {
            const target = catalog.find((t) => t.id === id) ?? sampleBuiltinSummary;
            const duplicated: ThemeSummary = {
                ...target,
                id: `${id}-copia`,
                name: `${target.name} (cópia)`,
                origin: 'custom'
            };
            catalog.push(duplicated);
            return createResolved(duplicated.id, duplicated.name, 'custom');
        });
        ipcMocks.themeDelete.mockImplementation(async (id: string) => {
            catalog = catalog.filter((t) => t.id !== id);
            return null;
        });
        ipcMocks.themeExport.mockResolvedValue('name: Exportado\n');
        ipcMocks.onThemeChanged.mockResolvedValue(() => {});
        ipcMocks.onThemeLibraryChanged.mockResolvedValue(() => {});
    });

    it('valida rascunho de tema e rejeita nomes vazios ou cores inválidas', () => {
        const validDraft: ThemeDraftInput = {
            name: 'Tema Teste',
            author: 'Autor',
            foreground: '#e2e8f0',
            background: '#0b1420',
            cursor: '#14b8a6',
            cursorAccent: '#0b1420',
            cursorStyle: 'block',
            selectionBackground: '#14b8a652',
            selectionForeground: null,
            activeTabAccent: null,
            ansi: {
                black: '#0b1420',
                red: '#f43f5e',
                green: '#10b981',
                yellow: '#f59e0b',
                blue: '#3b82f6',
                magenta: '#a855f7',
                cyan: '#06b6d4',
                white: '#e2e8f0',
                brightBlack: '#475569',
                brightRed: '#fb7185',
                brightGreen: '#34d399',
                brightYellow: '#fbbf24',
                brightBlue: '#60a5fa',
                brightMagenta: '#c084fc',
                brightCyan: '#22d3ee',
                brightWhite: '#f8fafc'
            },
            indexedColors: {}
        };

        expect(isThemeDraftValid(validDraft).valid).toBe(true);

        const emptyName = { ...validDraft, name: '   ' };
        expect(isThemeDraftValid(emptyName).valid).toBe(false);

        const invalidBg = { ...validDraft, background: 'cor-invalida' };
        expect(isThemeDraftValid(invalidBg).valid).toBe(false);

        const invalidAnsi = {
            ...validDraft,
            ansi: { ...validDraft.ansi, red: 'invalido' }
        };
        expect(isThemeDraftValid(invalidAnsi).valid).toBe(false);
    });

    it('abre modal pelo botão Novo Tema, valida formulário e salva tema customizado atualizando catálogo', async () => {
        const themeStore = useThemeStore();
        await themeStore.init();

        const wrapper = mount(AppearancePreferences);
        await flushPromises();

        const newThemeBtn = wrapper.findAll('button').find((b) => b.text().includes('Novo Tema'));
        expect(newThemeBtn?.exists()).toBe(true);
        await newThemeBtn!.trigger('click');

        const editorStore = useThemeEditorStore();
        expect(editorStore.isOpen).toBe(true);
        expect(editorStore.data.mode).toBe('create');

        editorStore.data.draft.name = 'Tema Galáxia';
        await flushPromises();

        const dialog = wrapper.findComponent(ThemeEditorDialog);
        expect(dialog.exists()).toBe(true);

        const submitBtn = dialog.findAll('button').find((b) => b.text().includes('Criar tema'));
        expect(submitBtn?.exists()).toBe(true);
        await submitBtn!.trigger('click');
        await flushPromises();

        expect(ipcMocks.themeCreate).toHaveBeenCalledTimes(1);
        expect(ipcMocks.themeCreate).toHaveBeenCalledWith(
            expect.objectContaining({ name: 'Tema Galáxia' })
        );
        expect(editorStore.isOpen).toBe(false);
        expect(themeStore.themes.some((t) => t.name === 'Tema Galáxia')).toBe(true);
    });

    it('protege contra submissão repetida enquanto uma gravação está em andamento', async () => {
        const editorStore = useThemeEditorStore();
        editorStore.openCreate(null);
        editorStore.data.draft.name = 'Tema Rápido';

        let resolveCreate: (value: ResolvedTheme) => void = () => {};
        ipcMocks.themeCreate.mockImplementationOnce(
            () => new Promise<ResolvedTheme>((resolve) => {
                resolveCreate = resolve;
            })
        );

        const savePromise1 = editorStore.submitDraft();
        expect(editorStore.saving).toBe(true);

        await expect(editorStore.submitDraft()).rejects.toThrow('Salvamento já em andamento.');

        resolveCreate(createResolved('tema-rapido', 'Tema Rápido'));
        await savePromise1;
        expect(editorStore.saving).toBe(false);
    });

    it('exibe alerta de erro mantendo o diálogo aberto caso o backend rejeite a criação', async () => {
        const editorStore = useThemeEditorStore();
        editorStore.openCreate(null);
        editorStore.data.draft.name = 'Tema Conflitante';

        ipcMocks.themeCreate.mockRejectedValueOnce(
            new Error('Já existe um tema com este nome.')
        );

        const wrapper = mount(ThemeEditorDialog);
        const submitBtn = wrapper.findAll('button').find((b) => b.text().includes('Criar tema'));
        await submitBtn!.trigger('click');
        await flushPromises();

        expect(editorStore.isOpen).toBe(true);
        expect(wrapper.text()).toContain('Já existe um tema com este nome.');
    });

    it('bloqueia edição de temas nativos, orientando duplicação via swatch e store', async () => {
        const themeStore = useThemeStore();
        await themeStore.init();

        const swatchWrapper = mount(ThemeSwatch, {
            props: {
                summary: sampleBuiltinSummary,
                active: true
            }
        });

        const editBtn = swatchWrapper.find('button[aria-label*="somente leitura"]');
        expect(editBtn.exists()).toBe(true);
        expect(editBtn.attributes('disabled')).toBeDefined();

        await editBtn.trigger('click');
        const editorStore = useThemeEditorStore();
        expect(editorStore.isOpen).toBe(false);

        const prefsWrapper = mount(AppearancePreferences);
        const picker = prefsWrapper.findComponent({ name: 'ThemePicker' });
        await picker.vm.$emit('edit', 'max-navy-teal');
        expect(editorStore.isOpen).toBe(false);
        expect(prefsWrapper.text()).toContain('Temas nativos são somente leitura.');
    });

    it('duplica tema nativo preservando o original e disponibilizando a cópia customizada no catálogo', async () => {
        const themeStore = useThemeStore();
        await themeStore.init();

        const initialBuiltin = themeStore.themes.find((t) => t.id === 'max-navy-teal');
        expect(initialBuiltin).toBeDefined();

        await themeStore.duplicateTheme('max-navy-teal');
        await flushPromises();

        expect(ipcMocks.themeDuplicate).toHaveBeenCalledWith('max-navy-teal');
        expect(themeStore.themes.some((t) => t.id === 'max-navy-teal')).toBe(true);
        expect(themeStore.themes.some((t) => t.id === 'max-navy-teal-copia')).toBe(true);
    });

    it('exclui tema customizado mediante modal de confirmação com resumo dos impactos e fallback padrão', async () => {
        const themeStore = useThemeStore();
        await themeStore.init();

        const wrapper = mount(AppearancePreferences);
        await flushPromises();

        // Aciona exclusão do tema customizado (oceano-custom) que está em override de projeto
        const targetTheme = themeStore.themes.find((t) => t.id === 'oceano-custom')!;
        const picker = wrapper.findComponent({ name: 'ThemePicker' });
        await picker.vm.$emit('delete', targetTheme.id);
        await flushPromises();

        expect(wrapper.text()).toContain('Confirmar Exclusão de Tema');
        expect(wrapper.text()).toContain('projeto(s) com override próprio passarão a seguir o tema global');

        // Cancelar mantém o tema
        const cancelBtn = wrapper.findAll('button').find((b) => b.text() === 'Cancelar');
        await cancelBtn!.trigger('click');
        expect(themeStore.themes.some((t) => t.id === 'oceano-custom')).toBe(true);

        // Reabre e confirma
        await picker.vm.$emit('delete', targetTheme.id);
        await flushPromises();
        const confirmBtn = wrapper.findAll('button').find((b) => b.text().includes('Confirmar exclusão'));
        await confirmBtn!.trigger('click');
        await flushPromises();

        expect(ipcMocks.themeDelete).toHaveBeenCalledWith('oceano-custom', DEFAULT_DARK_THEME_ID);
        expect(themeStore.themes.some((t) => t.id === 'oceano-custom')).toBe(false);
    });

    it('suporta pré-visualização efêmera e restaura tema ativo ao cancelar sem persistir alterações', async () => {
        const themeStore = useThemeStore();
        await themeStore.init();

        expect(themeStore.effectiveThemeId).toBe('max-navy-teal');

        await themeStore.startPreview('oceano-custom');
        expect(themeStore.effectiveThemeId).toBe('oceano-custom');
        expect(ipcMocks.themeSetActive).not.toHaveBeenCalled();

        themeStore.cancelPreview();
        expect(themeStore.effectiveThemeId).toBe('max-navy-teal');
        expect(ipcMocks.themeSetActive).not.toHaveBeenCalled();
    });
});
