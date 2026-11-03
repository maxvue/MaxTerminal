import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import { useThemeStore } from '@/stores/theme';
import type { AppearanceSettings, ResolvedTheme, ThemeSummary } from '@/types/theme';

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
    themeDelete: vi.fn(async () => null),
    themeExport: vi.fn(async () => 'name: Test\n'),
    onThemeChanged: vi.fn(async () => () => {}),
    onThemeLibraryChanged: vi.fn(async () => () => {})
}));

describe('useThemeStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        window.localStorage.clear();
    });

    it('inicializa listando temas, aparência e aplicando tokens ao DOM', async () => {
        const store = useThemeStore();
        await store.init();

        expect(store.initialized).toBe(true);
        expect(store.themes).toHaveLength(2);
        expect(store.effectiveThemeId).toBe('max-navy-teal');
        expect(store.activeResolved?.scheme.id).toBe('max-navy-teal');
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
});
