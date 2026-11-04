import { mockIPC } from '@tauri-apps/api/mocks';
import { mount } from '@vue/test-utils';
import { MaxApp } from '@maxvue/max-components-ui';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import App from '@/app/App.vue';
import AppEmptyState from '@/components/shell/AppEmptyState.vue';
import DbRecoveryPanel from '@/components/shell/DbRecoveryPanel.vue';
import { useSettingsStore } from '@/stores/settings';
import { useThemeStore } from '@/stores/theme';
import { FALLBACK_MAX_NAVY_TEAL_TOKENS } from '@/theme/bootstrap';
import type { AppInfo, DbStatusDto } from '@/types/bindings';
import type { ResolvedTheme } from '@/types/theme';

const mockAppInfo: AppInfo = {
    name: 'MaxTerminal',
    version: '0.1.0',
    buildProfile: 'debug',
    tauriVersion: '2.0.0',
    os: 'linux',
    arch: 'x86_64',
    paths: {
        configDir: '/tmp/config',
        dataDir: '/tmp/data',
        stateDir: '/tmp/state',
        cacheDir: '/tmp/cache',
        logDir: '/tmp/logs',
        dbPath: '/tmp/data/maxterminal.db'
    }
};

function createSampleResolved(id: string, name: string): ResolvedTheme {
    return {
        scheme: {
            id,
            name,
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
                black: '#000000',
                red: '#ff0000',
                green: '#00ff00',
                yellow: '#ffff00',
                blue: '#0000ff',
                magenta: '#ff00ff',
                cyan: '#00ffff',
                white: '#ffffff',
                brightBlack: '#808080',
                brightRed: '#ff8080',
                brightGreen: '#80ff80',
                brightYellow: '#ffff80',
                brightBlue: '#8080ff',
                brightMagenta: '#ff80ff',
                brightCyan: '#80ffff',
                brightWhite: '#ffffff'
            },
            indexedColors: {},
            metadata: {
                author: 'Max',
                sourceFormat: 'tabby-scheme-yaml',
                sourceFilename: `${id}.yaml`,
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
            red: '#ff0000',
            green: '#00ff00',
            yellow: '#ffff00',
            blue: '#0000ff',
            magenta: '#ff00ff',
            cyan: '#00ffff',
            white: '#ffffff',
            brightBlack: '#808080',
            brightRed: '#ff8080',
            brightGreen: '#80ff80',
            brightYellow: '#ffff80',
            brightBlue: '#8080ff',
            brightMagenta: '#ff80ff',
            brightCyan: '#80ffff',
            brightWhite: '#ffffff',
            extendedAnsi: null
        },
        uiTokens: FALLBACK_MAX_NAVY_TEAL_TOKENS
    };
}

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(async () => () => {})
}));

describe('MaxAppIntegration — Composição sem AppShell e reatividade de paleta', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        window.localStorage.clear();

        mockIPC((cmd) => {
            if (cmd === 'plugin:event|listen') {
                return 1;
            }
            if (cmd === 'plugin:event|unlisten') {
                return null;
            }
            if (cmd === 'app_info') {
                return mockAppInfo;
            }
            if (cmd === 'app_log_frontend') {
                return null;
            }
            if (cmd === 'theme_list') {
                return [];
            }
            if (cmd === 'theme_appearance_get') {
                return {
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
                };
            }
            if (cmd === 'theme_get') {
                return createSampleResolved('max-navy-teal', 'Max Navy/Teal');
            }
            return null;
        });
    });

    it('renderiza MaxApp em blank-mode sem AppShell e posiciona workspace em .app-workspace', async () => {
        const pinia = createPinia();
        const wrapper = mount(App, {
            global: {
                plugins: [pinia],
                stubs: { MaxIcon: true }
            }
        });

        await new Promise((resolve) => setTimeout(resolve, 35));
        await nextTick();

        const maxApp = wrapper.findComponent(MaxApp);
        expect(maxApp.exists()).toBe(true);
        expect(maxApp.props('blankMode')).toBe(true);
        expect(maxApp.props('noLogin')).toBe(true);
        expect(maxApp.props('controlledTheme')).toBe(true);

        // AppShell não deve mais existir no DOM ou componentes
        expect(wrapper.find('.app-shell').exists()).toBe(false);
        expect(wrapper.find('.app-shell__workspace').exists()).toBe(false);

        // O workspace direto deve estar no container .app-workspace
        const workspace = wrapper.find('.app-workspace');
        expect(workspace.exists()).toBe(true);
        expect(workspace.attributes('data-loading')).toBeDefined();
    });

    it('propaga colors reativamente para o MaxApp conforme o tema efetivo da store', async () => {
        const pinia = createPinia();
        const wrapper = mount(App, {
            global: {
                plugins: [pinia],
                stubs: { MaxIcon: true }
            }
        });

        await new Promise((resolve) => setTimeout(resolve, 35));
        await nextTick();

        const themeStore = useThemeStore();
        const maxApp = wrapper.findComponent(MaxApp);

        // Paleta inicial refletindo bootstrap / tema inicial
        const initialColors = maxApp.props('colors');
        expect(initialColors).toBeDefined();
        expect(initialColors?.primary).toBe(FALLBACK_MAX_NAVY_TEAL_TOKENS.primary.p500);

        // Atualiza tema ativo e verifica reatividade da prop colors no MaxApp
        const customResolved = createSampleResolved('custom-roxo', 'Custom Roxo');
        customResolved.uiTokens = {
            ...FALLBACK_MAX_NAVY_TEAL_TOKENS,
            primary: {
                ...FALLBACK_MAX_NAVY_TEAL_TOKENS.primary,
                p500: '#a855f7'
            }
        };
        themeStore.activeResolved = customResolved;
        await nextTick();

        const updatedColors = maxApp.props('colors');
        expect(updatedColors?.primary).toBe('#a855f7');

        // Pré-visualização tem precedência na paleta de cores
        const previewResolved = createSampleResolved('preview-verde', 'Preview Verde');
        previewResolved.uiTokens = {
            ...FALLBACK_MAX_NAVY_TEAL_TOKENS,
            primary: {
                ...FALLBACK_MAX_NAVY_TEAL_TOKENS.primary,
                p500: '#10b981'
            }
        };
        themeStore.previewResolved = previewResolved;
        await nextTick();

        expect(maxApp.props('colors')?.primary).toBe('#10b981');

        // Cancelamento do preview restaura o tema ativo anterior
        themeStore.cancelPreview();
        await nextTick();
        expect(maxApp.props('colors')?.primary).toBe('#a855f7');
    });

    it('renderiza AppEmptyState com retry no estado de erro e recupera para workspace nominal', async () => {
        let shouldFail = true;

        mockIPC((cmd) => {
            if (cmd === 'plugin:event|listen') {
                return 1;
            }
            if (cmd === 'plugin:event|unlisten') {
                return null;
            }
            if (cmd === 'app_info') {
                if (shouldFail) {
                    throw {
                        code: 'core.paths_unresolved',
                        message: 'Falha durante o boot inicial do MaxTerminal.'
                    };
                }
                return mockAppInfo;
            }
            if (cmd === 'app_log_frontend') {
                return null;
            }
            return null;
        });

        const pinia = createPinia();
        const wrapper = mount(App, {
            global: {
                plugins: [pinia],
                stubs: { MaxIcon: true }
            }
        });

        await new Promise((resolve) => setTimeout(resolve, 35));
        await nextTick();

        const emptyState = wrapper.findComponent(AppEmptyState);
        expect(emptyState.exists()).toBe(true);
        expect(wrapper.text()).toContain('Falha ao inicializar o MaxTerminal');
        expect(wrapper.text()).toContain('Não foi possível localizar os diretórios de configuração do usuário.');
        expect(wrapper.find('.app-error-code').text()).toBe('core.paths_unresolved');

        const retryBtn = wrapper.find('.app-retry-btn');
        expect(retryBtn.exists()).toBe(true);

        // Ao resolver erro e clicar em retry, recupera o estado nominal
        shouldFail = false;
        await retryBtn.trigger('click');

        await new Promise((resolve) => setTimeout(resolve, 50));
        await nextTick();

        expect(wrapper.find('.app-retry-btn').exists()).toBe(false);
        expect(wrapper.text()).toContain('Nenhum projeto selecionado');
        expect(wrapper.find('.app-workspace').exists()).toBe(true);
    });

    it('renderiza DbRecoveryPanel quando a persistência exige recuperação de banco', async () => {
        const pinia = createPinia();
        const settingsStore = useSettingsStore(pinia);
        const recoveryStatus: DbStatusDto = {
            health: {
                state: 'recovery_required',
                code: 'db.corrupted',
                reason: 'Corrupção detectada',
                db_path: '/tmp/data/maxterminal.db',
                available_backups: []
            },
            pendingBytes: 0,
            pendingRecords: 0,
            writerLagEvents: 0
        };
        settingsStore.dbStatus = recoveryStatus;

        const wrapper = mount(App, {
            global: {
                plugins: [pinia],
                stubs: { MaxIcon: true }
            }
        });

        await nextTick();
        expect(wrapper.findComponent(DbRecoveryPanel).exists()).toBe(true);

        const readyStatus: DbStatusDto = {
            health: {
                state: 'ready',
                schema_version: 1,
                sqlite_version: '3.51.3'
            },
            pendingBytes: 0,
            pendingRecords: 0,
            writerLagEvents: 0
        };
        settingsStore.dbStatus = readyStatus;
        await nextTick();
        expect(wrapper.findComponent(DbRecoveryPanel).exists()).toBe(false);
    });
});
