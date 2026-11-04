import { mount } from '@vue/test-utils';
import { MaxInputSelect } from '@maxvue/max-components-ui';
import { createMaxPinia } from '@maxvue/max-pinia';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AppearancePreferences from '@/components/preferences/appearance/AppearancePreferences.vue';
import ProjectThemeOverrides from '@/components/preferences/appearance/ProjectThemeOverrides.vue';
import TerminalFontSettings from '@/components/preferences/appearance/TerminalFontSettings.vue';
import ThemeImportReport from '@/components/preferences/appearance/ThemeImportReport.vue';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import type { AppearanceSettings, ImportReport, ThemeSummary } from '@/types/theme';

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
    beforeEach(() => {
        const pinia = createPinia();
        pinia.use(createMaxPinia() as unknown as PiniaPlugin);
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
});
