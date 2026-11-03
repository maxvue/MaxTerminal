import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import PreferencesDialog from '@/components/preferences/PreferencesDialog.vue';
import PreferencesSystemSection from '@/components/preferences/system/PreferencesSystemSection.vue';
import { osIpc } from '@/ipc';
import { useOsSettingsStore } from '@/stores/osSettings';
import { useWindowStore } from '@/stores/window';
import type { OsSettings_Serialize } from '@/types/bindings';

vi.mock('@/ipc', async (importOriginal) => {
    const actual = await importOriginal<Record<string, unknown>>();
    return {
        ...actual,
        osIpc: {
            getSettings: vi.fn(),
            updateSettings: vi.fn(),
            environment: vi.fn(),
            shortcutStatus: vi.fn(),
            configurePortal: vi.fn(),
            toggleWindow: vi.fn(),
            showWindow: vi.fn(),
            hideWindow: vi.fn(),
            requestQuit: vi.fn(),
            resolveLinks: vi.fn(),
            openLink: vi.fn(),
            notify: vi.fn(),
            setAttention: vi.fn(),
            getAutostart: vi.fn(),
            setAutostart: vi.fn(),
            runtimeSnapshot: vi.fn(),
            uiReady: vi.fn(),
            onFocusTab: vi.fn().mockResolvedValue(() => {}),
            onWindowVisibility: vi.fn().mockResolvedValue(() => {}),
            onSettingsChanged: vi.fn().mockResolvedValue(() => {}),
            onShortcutStatus: vi.fn().mockResolvedValue(() => {}),
            onCounters: vi.fn().mockResolvedValue(() => {}),
            onOpenPreferences: vi.fn().mockResolvedValue(() => {}),
            onShutdownStarted: vi.fn().mockResolvedValue(() => {}),
            onShutdownFailed: vi.fn().mockResolvedValue(() => {}),
            onEnvironmentChanged: vi.fn().mockResolvedValue(() => {}),
            onLinkContextChanged: vi.fn().mockResolvedValue(() => {})
        }
    };
});

function createDefaultSettings(): OsSettings_Serialize {
    return {
        schemaVersion: 1,
        closeBehavior: 'auto',
        quake: {
            enabled: true,
            accelerator: 'Ctrl+Alt+Backquote',
            mode: 'drop_down',
            dropDownHeightPct: 50,
            alwaysOnTop: true,
            hideOnBlur: false,
            preferredBackend: 'auto'
        },
        editor: {
            kind: 'vs_code',
            customArgv: []
        },
        notifications: {
            enabled: true,
            longCommandEnabled: true,
            longCommandThresholdSecs: 15,
            onlyWhenUnfocused: true
        },
        autostart: false,
        startMinimized: false
    };
}

describe('PreferencesSystemSection & PreferencesDialog (Fase 09)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('renderiza as 4 seções de preferências do sistema e dispara update ao alterar política de fechamento', async () => {
        const osSettingsStore = useOsSettingsStore();
        const windowStore = useWindowStore();
        const initial = createDefaultSettings();
        osSettingsStore.settings = initial;
        windowStore.environment = {
            displayServer: 'wayland',
            compositor: 'hyprland',
            trayAvailable: true,
            portalGlobalShortcuts: true,
            notificationServer: 'mako',
            notificationActions: true,
            editors: [{ kind: 'vs_code', available: true, binary: '/usr/bin/code' }],
            windowPositioning: false
        };

        vi.mocked(osIpc.updateSettings).mockResolvedValue({
            ...initial,
            closeBehavior: 'always_quit'
        });

        const wrapper = mount(PreferencesSystemSection, {
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        expect(wrapper.text()).toContain('Janela, Bandeja e Inicialização');
        expect(wrapper.text()).toContain('Atalho Global e Modo Quake');
        expect(wrapper.text()).toContain('Links Clicáveis para o Editor');
        expect(wrapper.text()).toContain('Notificações Nativas');

        const radios = wrapper.findAll('input[type="radio"]');
        expect(radios).toHaveLength(3);
        await radios[2]?.setValue(true);

        expect(osIpc.updateSettings).toHaveBeenCalledWith({
            closeBehavior: 'always_quit'
        });
    });

    it('abre e fecha o PreferencesDialog controlado pela useWindowStore', async () => {
        const windowStore = useWindowStore();
        const osSettingsStore = useOsSettingsStore();
        osSettingsStore.settings = createDefaultSettings();

        windowStore.openPreferences('system');

        const wrapper = mount(PreferencesDialog, {
            attachTo: document.body,
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        await nextTick();

        expect(document.body.querySelector('[role="dialog"]')).not.toBeNull();
        expect(document.body.textContent).toContain('Configurações gerais');

        windowStore.closePreferences();
        await nextTick();
        expect(windowStore.preferencesOpen).toBe(false);
        wrapper.unmount();
        document.body.innerHTML = '';
    });
});
