import { listen } from '@tauri-apps/api/event';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useOsFocusTab } from '@/composables/useOsFocusTab';
import * as gitIpc from '@/ipc/git';
import { osIpc } from '@/ipc/os';
import * as portsIpc from '@/ipc/ports';
import * as sessionsIpc from '@/ipc/sessions';
import * as tabsIpc from '@/ipc/tabs';
import * as templatesIpc from '@/ipc/templates';
import { useOsSettingsStore } from '@/stores/osSettings';
import { useProjectsStore } from '@/stores/projects';
import { useTabsStore } from '@/stores/tabs';
import { useWindowStore } from '@/stores/window';
import {
    commands,
    type AutostartStatus_Serialize,
    type OsEnvironment_Serialize,
    type OsRuntimeSnapshot_Serialize,
    type OsSettings_Serialize,
    type ShortcutStatus_Serialize
} from '@/types/bindings';

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn()
}));

const eventListeners = new Map<string, (ev: { payload: unknown }) => void>();

function emitTauriEvent(name: string, payload: unknown): void {
    const cb = eventListeners.get(name);
    cb?.({ payload });
}

function makeDefaultSettings(): OsSettings_Serialize {
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

function makeDefaultEnvironment(): OsEnvironment_Serialize {
    return {
        displayServer: 'wayland',
        compositor: 'kde_plasma',
        trayAvailable: true,
        portalGlobalShortcuts: true,
        notificationServer: 'Plasma',
        notificationActions: true,
        editors: [{ kind: 'vs_code', available: true, binary: '/usr/bin/code' }],
        windowPositioning: false
    };
}

function makeDefaultShortcutStatus(): ShortcutStatus_Serialize {
    return {
        backend: 'portal',
        accelerator: 'Ctrl+Alt+Backquote',
        effectiveTrigger: 'Ctrl+Alt+`',
        registered: true,
        error: null,
        cliCommand: 'maxterminal --toggle',
        compositorHint: 'maxterminal --toggle'
    };
}

describe('useWindowStore, useOsSettingsStore, useOsFocusTab & IPC (Fases 07-09)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
        eventListeners.clear();

        vi.mocked(listen).mockImplementation(async (eventName, handler) => {
            eventListeners.set(
                String(eventName),
                handler as (ev: { payload: unknown }) => void
            );
            return () => {
                eventListeners.delete(String(eventName));
            };
        });
    });

    it('hidrata useWindowStore via uiReady e respeita ordenação monotônica por revision em todos os eventos os://*', async () => {
        const snapshot: OsRuntimeSnapshot_Serialize = {
            revision: 5,
            window: { visible: true, focused: true, minimized: false },
            environment: makeDefaultEnvironment(),
            shortcutStatus: makeDefaultShortcutStatus(),
            counters: {
                running: 1,
                continuous: 2,
                finishedUnseen: 0,
                errorUnseen: 1,
                idle: 3,
                awaitingInput: 0
            },
            shutdownPhase: 'idle',
            linkContexts: [
                {
                    tabId: 'tab-1',
                    paneId: 'pane-1',
                    ptyId: 'pty-1',
                    cwdEpoch: 1
                }
            ],
            pendingFocus: {
                tabId: 'tab-1',
                paneId: 'pane-1',
                projectId: 'proj-1',
                source: 'notification'
            }
        };
        vi.spyOn(commands, 'osUiReady').mockResolvedValueOnce({ status: 'ok', data: snapshot });

        const store = useWindowStore();
        await store.init();
        // Chamada repetida deve ser no-op
        await store.init();

        expect(store.isWayland).toBe(true);
        expect(store.trayAvailable).toBe(true);
        expect(store.shortcutHealthy).toBe(true);
        expect(store.needsAttention).toBe(true);
        expect(store.linkContextForPane('pane-1')?.cwdEpoch).toBe(1);
        expect(store.linkContextForPane('pane-inexistente')).toBeNull();
        expect(store.consumePendingFocus()?.tabId).toBe('tab-1');
        expect(store.consumePendingFocus()).toBeNull();

        // Evento com revisão antiga (<= 5) deve ser ignorado
        emitTauriEvent('os://window-visibility', {
            revision: 4,
            payload: { visible: false, focused: false, minimized: true }
        });
        expect(store.visible).toBe(true);

        // Evento com revisão mais nova (> 5) deve ser aplicado
        emitTauriEvent('os://window-visibility', {
            revision: 6,
            payload: { visible: false, focused: false, minimized: false }
        });
        expect(store.visible).toBe(false);
        expect(store.focused).toBe(false);

        // Demais eventos os://*
        emitTauriEvent('os://shortcut-status', {
            revision: 7,
            payload: { ...makeDefaultShortcutStatus(), backend: 'cli_only', registered: false }
        });
        expect(store.shortcutStatus?.backend).toBe('cli_only');
        expect(store.shortcutHealthy).toBe(true);

        emitTauriEvent('os://counters', {
            revision: 8,
            payload: {
                running: 0,
                continuous: 0,
                finishedUnseen: 0,
                errorUnseen: 0,
                idle: 1,
                awaitingInput: 0
            }
        });
        expect(store.needsAttention).toBe(false);

        emitTauriEvent('os://open-preferences', {
            revision: 9,
            payload: { section: 'system' }
        });
        expect(store.preferencesOpen).toBe(true);
        store.closePreferences();
        expect(store.preferencesOpen).toBe(false);
        store.openPreferences('system');
        expect(store.preferencesOpen).toBe(true);

        emitTauriEvent('os://shutdown-started', {
            revision: 10,
            payload: { source: 'tray' }
        });
        expect(store.shuttingDown).toBe(true);
        expect(store.shutdownPhase).toBe('stopping');

        emitTauriEvent('os://shutdown-failed', {
            revision: 11,
            payload: {
                phase: 'failed',
                error: { code: 'os.shutdown_failed', message: 'Falha ao encerrar' }
            }
        });
        expect(store.shuttingDown).toBe(false);
        expect(store.shutdownError?.code).toBe('os.shutdown_failed');

        emitTauriEvent('os://environment-changed', {
            revision: 12,
            payload: { ...makeDefaultEnvironment(), displayServer: 'x11' }
        });
        expect(store.isWayland).toBe(false);

        emitTauriEvent('os://link-context-changed', {
            revision: 13,
            payload: {
                tabId: 'tab-1',
                paneId: 'pane-1',
                ptyId: 'pty-1',
                cwdEpoch: 2
            }
        });
        expect(store.linkContextForPane('pane-1')?.cwdEpoch).toBe(2);

        // Ações diretas da store
        vi.spyOn(commands, 'osEnvironmentInfo').mockResolvedValueOnce({
            status: 'ok',
            data: makeDefaultEnvironment()
        });
        await store.refreshEnvironment();
        expect(store.isWayland).toBe(true);

        vi.spyOn(commands, 'osWindowToggle').mockResolvedValueOnce({
            status: 'ok',
            data: 'show'
        });
        expect(await store.toggle('ipc')).toBe('show');

        vi.spyOn(commands, 'osWindowShow').mockResolvedValueOnce({ status: 'ok', data: null });
        await store.show();

        vi.spyOn(commands, 'osWindowHide').mockResolvedValueOnce({ status: 'ok', data: null });
        await store.hide();

        vi.spyOn(commands, 'osQuitRequest').mockResolvedValueOnce({
            status: 'ok',
            data: 'quitting'
        });
        expect(await store.quit()).toBe('quitting');

        store.$reset();
        expect(store.initialized).toBe(false);
        expect(store.lastRevision).toBe(0);
    });

    it('realiza update otimista em useOsSettingsStore, suporta setAutostart/configurePortal e faz rollback em erro', async () => {
        const initial = makeDefaultSettings();
        const autostart: AutostartStatus_Serialize = { enabled: false, desktopFile: null };
        vi.spyOn(commands, 'osSettingsGet').mockResolvedValueOnce({
            status: 'ok',
            data: initial
        });
        vi.spyOn(commands, 'osGetAutostart').mockResolvedValueOnce({
            status: 'ok',
            data: autostart
        });

        const store = useOsSettingsStore();
        await store.load();
        expect(store.closeBehavior).toBe('auto');
        expect(store.editorKind).toBe('vs_code');
        expect(store.quake?.accelerator).toBe('Ctrl+Alt+Backquote');
        expect(store.notifications?.longCommandThresholdSecs).toBe(15);

        // Sucesso no update completo (testando todos os ramos do merge otimista)
        const updatedSettings: OsSettings_Serialize = {
            ...initial,
            closeBehavior: 'always_tray',
            quake: {
                enabled: false,
                accelerator: 'Ctrl+Alt+Space',
                mode: 'normal',
                dropDownHeightPct: 60,
                alwaysOnTop: false,
                hideOnBlur: true,
                preferredBackend: 'portal'
            },
            editor: {
                kind: 'custom',
                customArgv: ['nvim', '+{line}', '{file}']
            },
            notifications: {
                enabled: false,
                longCommandEnabled: false,
                longCommandThresholdSecs: 30,
                onlyWhenUnfocused: false
            },
            autostart: true,
            startMinimized: true
        };
        vi.spyOn(commands, 'osSettingsUpdate').mockResolvedValueOnce({
            status: 'ok',
            data: updatedSettings
        });

        await store.update({
            closeBehavior: 'always_tray',
            quake: updatedSettings.quake,
            editor: updatedSettings.editor,
            notifications: updatedSettings.notifications,
            autostart: true,
            startMinimized: true
        });
        expect(store.closeBehavior).toBe('always_tray');
        expect(store.autostart?.enabled).toBe(true);

        // setAccelerator
        vi.spyOn(commands, 'osSettingsUpdate').mockResolvedValueOnce({
            status: 'ok',
            data: {
                ...updatedSettings,
                quake: { ...updatedSettings.quake, accelerator: 'Ctrl+Shift+F12' }
            }
        });
        await store.setAccelerator('Ctrl+Shift+F12');
        expect(store.quake?.accelerator).toBe('Ctrl+Shift+F12');

        // setAutostart sucesso e falha com rollback
        vi.spyOn(commands, 'osSetAutostart').mockResolvedValueOnce({
            status: 'ok',
            data: { enabled: false, desktopFile: '/tmp/maxterminal.desktop' }
        });
        await store.setAutostart(false);
        expect(store.autostart?.enabled).toBe(false);

        vi.spyOn(commands, 'osSetAutostart').mockResolvedValueOnce({
            status: 'error',
            error: { code: 'os.autostart_io', message: 'Sem permissão em ~/.config/autostart' }
        });
        await expect(store.setAutostart(true)).rejects.toThrow(
            'Sem permissão em ~/.config/autostart'
        );
        expect(store.autostart?.enabled).toBe(false);

        // configurePortal
        vi.spyOn(commands, 'osShortcutConfigurePortal').mockResolvedValueOnce({
            status: 'ok',
            data: makeDefaultShortcutStatus()
        });
        const portalSt = await store.configurePortal();
        expect(portalSt.registered).toBe(true);

        // Evento os://settings-changed
        emitTauriEvent('os://settings-changed', {
            revision: 3,
            payload: initial
        });
        expect(store.closeBehavior).toBe('auto');

        // Simula falha no backend durante update()
        vi.spyOn(commands, 'osSettingsUpdate').mockResolvedValueOnce({
            status: 'error',
            error: { code: 'os.shortcut_conflict', message: 'Atalho em conflito' }
        });
        await expect(store.update({ closeBehavior: 'always_quit' })).rejects.toThrow(
            'Atalho em conflito'
        );
        expect(store.closeBehavior).toBe('auto');
        expect(store.error).toBe('Atalho em conflito');

        store.$reset();
        expect(store.settings).toBeNull();
    });

    it('conecta useOsFocusTab ao evento os://focus-tab e ao pendingFocus de useWindowStore', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        const windowStore = useWindowStore();

        const selectSpy = vi.spyOn(projectsStore, 'select').mockImplementation(() => {});
        const setActiveTabSpy = vi
            .spyOn(projectsStore, 'setActiveTab')
            .mockImplementation(() => {});
        const mockTabView = {
            id: 'tab-2',
            projectId: 'proj-2',
            templateId: null,
            title: 'Aba 2',
            icon: 'mdi:console',
            position: 0,
            origin: 'manual' as const,
            continuous: false,
            lifecycle: 'ready' as const,
            revision: 1,
            layoutRevision: 1,
            layout: { kind: 'leaf' as const, paneId: 'pane-2' },
            panes: [],
            activePaneId: 'pane-2',
            primaryPaneId: 'pane-2',
            initialDispatch: 'none' as const,
            tabSessionId: 'ts-2'
        };
        const focusTabSpy = vi.spyOn(tabsStore, 'focusTab').mockResolvedValue(mockTabView);

        windowStore.pendingFocus = {
            tabId: 'tab-pending',
            paneId: null,
            projectId: 'proj-1',
            source: 'cli'
        };

        const { applyFocusPayload, stop } = useOsFocusTab();
        expect(selectSpy).toHaveBeenCalledWith('proj-1');
        expect(setActiveTabSpy).toHaveBeenCalledWith('proj-1', 'tab-pending');

        tabsStore.byId['tab-2'] = mockTabView;

        await applyFocusPayload({
            tabId: 'tab-2',
            paneId: 'pane-2',
            projectId: 'proj-2',
            source: 'notification'
        });
        expect(focusTabSpy).toHaveBeenCalledWith('tab-2', 'pane-2');

        emitTauriEvent('os://focus-tab', {
            revision: 1,
            payload: {
                tabId: 'tab-2',
                paneId: 'pane-2',
                projectId: 'proj-2',
                source: 'tray'
            }
        });

        stop();
        stop();
    });

    it('exercita wrappers IPC de os, git, ports, templates, sessions e tabs', async () => {
        vi.spyOn(commands, 'osShortcutStatus').mockResolvedValueOnce({
            status: 'ok',
            data: makeDefaultShortcutStatus()
        });
        expect((await osIpc.shortcutStatus()).registered).toBe(true);

        vi.spyOn(commands, 'osNotify').mockResolvedValueOnce({
            status: 'ok',
            data: 'shown'
        });
        expect(
            await osIpc.notify({
                kind: 'info',
                title: 'Teste',
                body: 'Corpo'
            })
        ).toBe('shown');

        vi.spyOn(commands, 'osAttentionSet').mockResolvedValueOnce({
            status: 'ok',
            data: {
                running: 0,
                continuous: 0,
                finishedUnseen: 0,
                errorUnseen: 0,
                idle: 1,
                awaitingInput: 1
            }
        });
        expect((await osIpc.setAttention('tab-1', 'awaiting_input')).awaitingInput).toBe(1);

        vi.spyOn(commands, 'osRuntimeSnapshot').mockResolvedValueOnce({
            status: 'ok',
            data: {
                revision: 1,
                window: { visible: true, focused: true, minimized: false },
                environment: makeDefaultEnvironment(),
                shortcutStatus: makeDefaultShortcutStatus(),
                counters: {
                    running: 0,
                    continuous: 0,
                    finishedUnseen: 0,
                    errorUnseen: 0,
                    idle: 0,
                    awaitingInput: 0
                },
                shutdownPhase: 'idle',
                linkContexts: [],
                pendingFocus: null
            }
        });
        expect((await osIpc.runtimeSnapshot()).revision).toBe(1);

        // Git IPC
        vi.spyOn(commands, 'gitStatusForPath').mockResolvedValueOnce({
            status: 'ok',
            data: {
                kind: 'non_repository',
                path: '/tmp',
                revision: 1,
                refreshedAtMs: 0
            }
        });
        expect((await gitIpc.gitStatusForPath('/tmp')).kind).toBe('non_repository');

        vi.spyOn(commands, 'gitStatusRefresh').mockResolvedValueOnce({
            status: 'ok',
            data: {
                kind: 'non_repository',
                path: '/tmp',
                revision: 2,
                refreshedAtMs: 1
            }
        });
        expect((await gitIpc.gitStatusRefresh('/tmp')).revision).toBe(2);

        vi.spyOn(commands, 'gitWorktreeList').mockResolvedValueOnce({ status: 'ok', data: [] });
        expect(await gitIpc.gitWorktreeList('/tmp')).toEqual([]);

        vi.spyOn(commands, 'gitWorktreeRemove').mockResolvedValueOnce({
            status: 'ok',
            data: null
        });
        expect(await gitIpc.gitWorktreeRemove('/tmp/wt', false)).toBeNull();
        await gitIpc.onGitStatusChanged(() => {});

        // Ports IPC
        vi.spyOn(commands, 'portsListForTab').mockResolvedValueOnce({
            status: 'ok',
            data: { tabId: 'tab-1', ports: [], revision: 1 }
        });
        expect((await portsIpc.portsListForTab('tab-1')).ports).toEqual([]);

        vi.spyOn(commands, 'portsOpenBrowser').mockResolvedValueOnce({
            status: 'ok',
            data: null
        });
        await portsIpc.portsOpenBrowser('pane-1', 5173);

        vi.spyOn(commands, 'portsKill').mockResolvedValueOnce({
            status: 'ok',
            data: {
                paneId: 'pane-1',
                port: 5173,
                pid: 1234,
                outcome: 'graceful_term',
                durationMs: 10
            }
        });
        expect((await portsIpc.portsKill('pane-1', 5173)).outcome).toBe('graceful_term');

        vi.spyOn(commands, 'portsSetPollMode').mockResolvedValueOnce({
            status: 'ok',
            data: null
        });
        await portsIpc.portsSetPollMode('active');
        await portsIpc.onPortOpened(() => {});
        await portsIpc.onPortUpdated(() => {});
        await portsIpc.onPortClosed(() => {});

        // Templates, Sessions & Tabs IPC
        vi.spyOn(commands, 'templateList').mockResolvedValueOnce({ status: 'ok', data: [] });
        expect(await templatesIpc.templateList('proj-1')).toEqual([]);
        vi.spyOn(commands, 'templateDelete').mockResolvedValueOnce({ status: 'ok', data: null });
        await templatesIpc.templateDelete('tpl-1', 1);
        vi.spyOn(commands, 'templateSetDefault').mockResolvedValueOnce({
            status: 'ok',
            data: []
        });
        await templatesIpc.templateSetDefault('tpl-1');
        await templatesIpc.onTemplateChanged(() => {});

        vi.spyOn(commands, 'sessionList').mockResolvedValueOnce({
            status: 'ok',
            data: { items: [], nextCursor: null }
        });
        expect((await sessionsIpc.sessionList('tab-1', null, 20)).items).toEqual([]);
        vi.spyOn(commands, 'sessionSearch').mockResolvedValueOnce({ status: 'ok', data: [] });
        expect(await sessionsIpc.sessionSearch('tab-1', 'cargo', 10)).toEqual([]);
        await sessionsIpc.onSessionStarted(() => {});
        await sessionsIpc.onSessionArchived(() => {});

        vi.spyOn(commands, 'tabList').mockResolvedValueOnce({ status: 'ok', data: [] });
        expect(await tabsIpc.tabList('proj-1')).toEqual([]);
        vi.spyOn(commands, 'tabClose').mockResolvedValueOnce({ status: 'ok', data: null });
        await tabsIpc.tabClose('tab-1', 'req-1');
        vi.spyOn(commands, 'tabReorder').mockResolvedValueOnce({
            status: 'ok',
            data: ['tab-1']
        });
        expect(await tabsIpc.tabReorder('proj-1', ['tab-1'])).toEqual(['tab-1']);
        await tabsIpc.onTabOpened(() => {});
        await tabsIpc.onTabClosed(() => {});
        await tabsIpc.onTabReordered(() => {});
        await tabsIpc.onTabLayoutChanged(() => {});
        await tabsIpc.onPanePtyReplaced(() => {});
        await tabsIpc.onPaneExited(() => {});
    });
});
