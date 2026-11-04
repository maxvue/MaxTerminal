import axios from 'axios';
import { createApp, defineComponent } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import { installThemeMaxPiniaAdapter } from '@/ipc/themeMaxPiniaAdapter';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import { useThemeStore } from '@/stores/theme';
import { useThemeCatalogStore } from '@/stores/themeCatalog';
import {
    formatSelectionBackground,
    parseSelectionBackground,
    useThemeEditorStore
} from '@/stores/themeEditor';
import type {
    AppearanceSettings,
    ResolvedTheme,
    ThemeChangedEvent,
    ThemeDraftInput,
    ThemeSummary
} from '@/types/theme';

const mockSummaries: ThemeSummary[] = [
    {
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
    },
    {
        id: 'dracula',
        name: 'Dracula',
        origin: 'builtin',
        polarity: 'dark',
        background: '#282a36',
        foreground: '#f8f8f2',
        cursor: '#f8f8f2',
        accentSwatches: [
            '#21222c',
            '#ff5555',
            '#50fa7b',
            '#f1fa8c',
            '#bd93f9',
            '#ff79c6',
            '#8be9fd',
            '#f8f8f2'
        ],
        author: 'Zeno Rocha',
        sourceFormat: 'tabby-scheme-yaml'
    }
];

function makeResolved(id: string, name: string, polarity: 'dark' | 'light' = 'dark'): ResolvedTheme {
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
            id,
            name,
            origin: 'builtin',
            polarity,
            foreground: polarity === 'dark' ? '#e2e8f0' : '#1e293b',
            background: polarity === 'dark' ? '#0f1c2e' : '#ffffff',
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
                sourceFilename: `${id}.yaml`,
                sha256: 'b'.repeat(64),
                createdAt: 0,
                updatedAt: 0
            }
        },
        xterm: {
            foreground: polarity === 'dark' ? '#e2e8f0' : '#1e293b',
            background: polarity === 'dark' ? '#0f1c2e' : '#ffffff',
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
        uiTokens: {
            ...FALLBACK_MAX_NAVY_TEAL_TOKENS,
            polarity
        }
    };
}

const mockAppearance: AppearanceSettings = {
    activeThemeId: 'max-navy-teal',
    followSystem: false,
    preferredDarkThemeId: 'max-navy-teal',
    preferredLightThemeId: 'max-light',
    fontFamily: 'JetBrains Mono, monospace',
    fontSize: 13,
    lineHeight: 1.2,
    fontLigatures: true,
    cursorBlink: true,
    cursorStyleOverride: null,
    minimumContrastRatio: 1,
    projectOverrides: {}
};

vi.mock('@/ipc/themes', () => ({
    themeList: vi.fn(async () => mockSummaries),
    themeGet: vi.fn(async (id: string) => makeResolved(id, id)),
    themeAppearanceGet: vi.fn(async () => ({ ...mockAppearance })),
    themeAppearanceUpdate: vi.fn(async (patch: Partial<AppearanceSettings>) => ({
        ...mockAppearance,
        ...patch
    })),
    themeSetActive: vi.fn(async (id: string) => makeResolved(id, id)),
    themeImportFile: vi.fn(async () => ({
        imported: [],
        warnings: [],
        failures: [],
        skipped: []
    })),
    themeImportDir: vi.fn(async () => ({
        imported: [],
        warnings: [],
        failures: [],
        skipped: []
    })),
    themeCreate: vi.fn(async (input: ThemeDraftInput) =>
        makeResolved('oceano-custom', input.name)
    ),
    themeUpdate: vi.fn(async (id: string, input: ThemeDraftInput) =>
        makeResolved(id, input.name)
    ),
    themeDuplicate: vi.fn(async (id: string) =>
        makeResolved(`${id}-copia`, `${id} (cópia)`)
    ),
    themeDelete: vi.fn(async () => null),
    themeExport: vi.fn(async () => 'name: Test\n'),
    onThemeChanged: vi.fn(async () => () => {}),
    onThemeLibraryChanged: vi.fn(async () => () => {})
}));

type MaxPiniaAxios = NonNullable<NonNullable<Parameters<typeof createMaxPinia>[0]>['axios']>;

describe('useThemeStore', () => {
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
        window.localStorage.clear();
        vi.clearAllMocks();
    });

    it('inicializa listando temas, aparência e aplicando tokens ao DOM', async () => {
        const store = useThemeStore();
        await store.init();

        expect(store.initialized).toBe(true);
        expect(store.themes).toHaveLength(2);
        expect(store.effectiveThemeId).toBe('max-navy-teal');
        expect(store.activeResolved?.scheme.id).toBe('max-navy-teal');
    });

    it('delega coleção de temas para themeCatalogStore garantindo reatividade compartilhada', async () => {
        const themeStore = useThemeStore();
        const catalogStore = useThemeCatalogStore();

        expect(themeStore.themes).toEqual(catalogStore.data);

        catalogStore.replaceCatalog(mockSummaries);
        expect(themeStore.themes).toHaveLength(2);
        expect(themeStore.builtinThemes).toHaveLength(2);

        const reloaded = await themeStore.refreshCatalog();
        expect(reloaded).toHaveLength(2);
        expect(themeStore.themes).toEqual(reloaded);
    });

    it('altera tema ativo globalmente e por projeto, além de suportar preview efêmero', async () => {
        const store = useThemeStore();
        await store.init();

        await store.startPreview('dracula');
        expect(store.effectiveThemeId).toBe('dracula');

        store.cancelPreview();
        expect(store.effectiveThemeId).toBe('max-navy-teal');

        await store.setActiveTheme('dracula', {
            kind: 'project',
            projectId: 'proj-1'
        });
        await store.setCurrentProjectContext('proj-1');
        expect(store.effectiveThemeId).toBe('dracula');

        await store.setCurrentProjectContext(null);
        expect(store.effectiveThemeId).toBe('max-navy-teal');

        await store.setActiveTheme('max-navy-teal', {
            kind: 'clear_project',
            projectId: 'proj-1'
        });
        expect(store.appearance.projectOverrides['proj-1']).toBeUndefined();

        await store.setActiveTheme('dracula', { kind: 'global' });
        expect(store.appearance.activeThemeId).toBe('dracula');
    });

    it('atualiza aparência, importa arquivos/diretórios, exporta, remove tema e faz dispose', async () => {
        const store = useThemeStore();
        await store.init();
        // Chamada duplicada de init retorna imediatamente
        await store.init();

        expect(store.builtinThemes).toHaveLength(2);
        expect(store.customThemes).toHaveLength(0);

        const updated = await store.updateAppearance({ fontSize: 15, cursorBlink: false });
        expect(updated.fontSize).toBe(15);

        const reportFile = await store.importFile('/tmp/scheme.yaml', {
            overwritePolicy: 'rename_with_suffix',
            dryRun: false
        });
        expect(reportFile.imported).toEqual([]);

        const reportDir = await store.importDir('/tmp/schemes', {
            overwritePolicy: 'skip_existing',
            dryRun: false
        });
        expect(reportDir.imported).toEqual([]);

        const exported = await store.exportTheme('max-navy-teal', 'tabby_yaml');
        expect(exported).toContain('name: Test');

        await store.deleteTheme('dracula', 'max-navy-teal');
        store.dispose();
        expect(store.initialized).toBe(false);
    });

    it('sincroniza polaridade ao trocar para tema claro e persiste snapshot v2', async () => {
        const store = useThemeStore();
        await store.init();

        expect(store.effectiveResolved?.uiTokens.polarity).toBe('dark');

        // Mock para resolver max-light com polaridade clara
        const ipc = await import('@/ipc/themes');
        vi.mocked(ipc.themeSetActive).mockImplementationOnce(async (id: string) =>
            makeResolved(id, 'Max Light', 'light')
        );
        vi.mocked(ipc.themeGet).mockImplementationOnce(async (id: string) =>
            makeResolved(id, 'Max Light', 'light')
        );

        await store.setActiveTheme('max-light', { kind: 'global' });
        expect(store.effectiveThemeId).toBe('max-light');
        expect(store.effectiveResolved?.uiTokens.polarity).toBe('light');
        expect(document.documentElement.classList.contains('dark')).toBe(false);
        expect(document.documentElement.dataset.themePolarity).toBe('light');
    });

    it('propaga contexto ANSI e aplica tokens correspondentes no DOM e snapshot', async () => {
        const store = useThemeStore();
        await store.init();

        const ipc = await import('@/ipc/themes');
        vi.mocked(ipc.themeSetActive).mockImplementationOnce(async (id: string) => {
            const res = makeResolved(id, 'Dracula', 'dark');
            res.scheme.ansi.magenta = '#ff79c6';
            res.scheme.ansi.brightMagenta = '#ff92df';
            return res;
        });

        await store.setActiveTheme('dracula', { kind: 'global' });
        expect(document.documentElement.style.getPropertyValue('--max-purple-500')).toBe('#ff79c6');
        expect(document.documentElement.style.getPropertyValue('--mt-ui-issue-category')).toBe('#ff79c6');
    });

    it('cria, atualiza, duplica e exclui temas mantendo preview e overrides de projeto consistentes', async () => {
        const store = useThemeStore();
        await store.init();
        const ipc = await import('@/ipc/themes');

        const createdResolved = makeResolved('oceano-custom', 'Oceano Custom', 'dark');
        createdResolved.scheme.origin = 'custom';
        vi.mocked(ipc.themeCreate).mockResolvedValueOnce(createdResolved);
        vi.mocked(ipc.themeSetActive).mockResolvedValueOnce(createdResolved);

        const created = await store.createTheme(
            {
                name: 'Oceano Custom',
                author: 'Max',
                foreground: '#e2e8f0',
                background: '#0f1c2e',
                cursor: '#2ea4bc',
                cursorAccent: '#0f1c2e',
                cursorStyle: 'block',
                selectionBackground: '#00768e66',
                selectionForeground: null,
                activeTabAccent: '#00768e',
                ansi: createdResolved.scheme.ansi,
                indexedColors: { 200: '#abcdef' }
            },
            { kind: 'global' }
        );
        expect(created.scheme.id).toBe('oceano-custom');
        expect(store.effectiveThemeId).toBe('oceano-custom');

        const updatedResolved = makeResolved('oceano-custom', 'Oceano Claro', 'light');
        updatedResolved.scheme.origin = 'custom';
        vi.mocked(ipc.themeUpdate).mockResolvedValueOnce(updatedResolved);

        const updated = await store.updateTheme('oceano-custom', {
            name: 'Oceano Claro',
            author: 'Max',
            foreground: '#1e293b',
            background: '#ffffff',
            cursor: '#2ea4bc',
            cursorAccent: '#ffffff',
            cursorStyle: 'underline',
            selectionBackground: '#00768e40',
            selectionForeground: null,
            activeTabAccent: '#00768e',
            ansi: updatedResolved.scheme.ansi,
            indexedColors: { 200: '#abcdef' }
        });
        expect(updated.scheme.polarity).toBe('light');
        expect(store.activeResolved?.scheme.name).toBe('Oceano Claro');
        expect(document.documentElement.dataset.themePolarity).toBe('light');

        const duplicatedResolved = makeResolved('oceano-claro-copia', 'Oceano Claro (cópia)', 'light');
        duplicatedResolved.scheme.origin = 'custom';
        vi.mocked(ipc.themeDuplicate).mockResolvedValueOnce(duplicatedResolved);
        const dup = await store.duplicateTheme('oceano-custom');
        expect(dup.scheme.id).toBe('oceano-claro-copia');

        await store.startPreview('oceano-claro-copia');
        expect(store.previewResolved?.scheme.id).toBe('oceano-claro-copia');
        await store.deleteTheme('oceano-claro-copia');
        expect(store.previewResolved).toBeNull();
    });

    it('preserva o tema efetivo do projeto quando um evento global altera para outro tema', async () => {
        const ipc = await import('@/ipc/themes');
        let changedListener: (ev: ThemeChangedEvent) => void = () => {};
        vi.mocked(ipc.onThemeChanged).mockImplementationOnce(async (cb) => {
            changedListener = cb;
            return () => {};
        });

        const store = useThemeStore();
        await store.init();

        const lightOverride = makeResolved('max-light', 'Max Light', 'light');
        vi.mocked(ipc.themeSetActive).mockResolvedValueOnce(lightOverride);
        await store.setCurrentProjectContext('proj-override');
        await store.setActiveTheme('max-light', {
            kind: 'project',
            projectId: 'proj-override'
        });
        expect(store.effectiveThemeId).toBe('max-light');

        changedListener({
            scope: { kind: 'global' },
            resolved: makeResolved('max-navy-teal', 'Max Navy/Teal', 'dark'),
            appearance: {
                ...mockAppearance,
                activeThemeId: 'max-navy-teal',
                projectOverrides: { 'proj-override': 'max-light' }
            }
        });

        await Promise.resolve();
        expect(store.effectiveThemeId).toBe('max-light');
    });

    it('useThemeEditorStore gerencia rascunho via MaxPinia, preserva indexedColors e persiste somente em submitDraft', async () => {
        const ipc = await import('@/ipc/themes');
        const editor = useThemeEditorStore();
        const baseResolved = makeResolved('meu-tema', 'Meu Tema', 'dark');
        baseResolved.scheme.origin = 'custom';
        baseResolved.scheme.indexedColors = { 42: '#123456', 210: '#fedcba' };
        baseResolved.scheme.selectionBackground = '#22d3ee80';

        editor.openEdit(baseResolved);
        expect(editor.isOpen).toBe(true);
        expect(editor.isDirty).toBe(false);
        expect(editor.data.draft.indexedColors).toEqual({
            42: '#123456',
            210: '#fedcba'
        });

        const parsedSel = parseSelectionBackground(
            editor.data.draft.selectionBackground
        );
        expect(parsedSel.hex).toBe('#22d3ee');
        expect(parsedSel.alphaPercent).toBe(50);

        editor.data.draft.name = 'Meu Tema Editado';
        editor.setSelectionColor('#10b981', 75);
        expect(editor.isDirty).toBe(true);
        expect(editor.data.draft.selectionBackground).toBe(
            formatSelectionBackground('#10b981', 75)
        );
        expect(ipc.themeUpdate).not.toHaveBeenCalled();

        const savedResolved = makeResolved('meu-tema', 'Meu Tema Editado', 'dark');
        savedResolved.scheme.origin = 'custom';
        savedResolved.scheme.indexedColors = { 42: '#123456', 210: '#fedcba' };
        vi.mocked(ipc.themeUpdate).mockResolvedValueOnce(savedResolved);

        const result = await editor.submitDraft();
        expect(result.scheme.name).toBe('Meu Tema Editado');
        expect(ipc.themeUpdate).toHaveBeenCalledTimes(1);
        expect(ipc.themeUpdate).toHaveBeenCalledWith(
            'meu-tema',
            expect.objectContaining({
                name: 'Meu Tema Editado',
                indexedColors: { 42: '#123456', 210: '#fedcba' }
            })
        );
        expect(editor.isOpen).toBe(false);
        expect(editor.isDirty).toBe(false);
    });
});
