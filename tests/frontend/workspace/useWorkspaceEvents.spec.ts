import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useWorkspaceEvents } from '@/composables/workspace/useWorkspaceEvents';
import * as sessionsIpc from '@/ipc/sessions';
import * as tabsIpc from '@/ipc/tabs';
import * as templatesIpc from '@/ipc/templates';
import * as workspaceIpc from '@/ipc/workspace';
import { useTabsStore } from '@/stores/tabs';
import type { TabView_Serialize, WorkspaceEvent } from '@/types/bindings';

let tabOpenedCb: ((ev: WorkspaceEvent<TabView_Serialize>) => void) | null = null;

vi.mock('@/ipc/tabs', () => ({
    tabList: vi.fn(),
    tabOpen: vi.fn(),
    tabClose: vi.fn(),
    tabReorder: vi.fn(),
    tabFocus: vi.fn(),
    paneSplit: vi.fn(),
    paneClose: vi.fn(),
    paneRestart: vi.fn(),
    paneLayoutUpdate: vi.fn(),
    onTabOpened: vi.fn(async (cb: (ev: WorkspaceEvent<TabView_Serialize>) => void) => {
        tabOpenedCb = cb;
        return () => {};
    }),
    onTabClosed: vi.fn(async () => () => {}),
    onTabReordered: vi.fn(async () => () => {}),
    onTabLayoutChanged: vi.fn(async () => () => {}),
    onPanePtyReplaced: vi.fn(async () => () => {}),
    onPaneExited: vi.fn(async () => () => {})
}));

vi.mock('@/ipc/sessions', () => ({
    sessionNew: vi.fn(),
    sessionList: vi.fn(),
    sessionGet: vi.fn(),
    sessionScrollbackGet: vi.fn(),
    sessionSearch: vi.fn(),
    onSessionStarted: vi.fn(async () => () => {}),
    onSessionArchived: vi.fn(async () => () => {})
}));

vi.mock('@/ipc/templates', () => ({
    templateList: vi.fn(),
    templateGet: vi.fn(),
    templateCreate: vi.fn(),
    templateUpdate: vi.fn(),
    templateDelete: vi.fn(),
    templateSetDefault: vi.fn(),
    onTemplateChanged: vi.fn(async () => () => {})
}));

vi.mock('@/ipc/workspace', () => ({
    workspaceSnapshot: vi.fn(),
    workspaceInitialCommandRun: vi.fn(),
    onWorkspaceRestored: vi.fn(async () => () => {})
}));

function makeTab(id: string, position: number): TabView_Serialize {
    return {
        id,
        projectId: 'proj-1',
        templateId: 'tpl-1',
        title: `Aba ${id}`,
        icon: 'lucide:terminal',
        position,
        origin: 'manual',
        continuous: false,
        lifecycle: 'ready',
        revision: 1,
        layoutRevision: 1,
        layout: { kind: 'leaf', paneId: `pane-${id}` },
        panes: [
            {
                id: `pane-${id}`,
                ptyId: `pty-${id}`,
                generation: 1,
                cwd: '/tmp',
                state: 'live',
                exit: null,
                error: null
            }
        ],
        activePaneId: `pane-${id}`,
        primaryPaneId: `pane-${id}`,
        initialDispatch: 'none',
        tabSessionId: `tsess-${id}`
    };
}

describe('useWorkspaceEvents (Fase 07)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        tabOpenedCb = null;
    });

    it('bufferiza eventos durante workspace_snapshot e aplica apenas event_seq > snapshot.eventSeq', async () => {
        const tabsStore = useTabsStore();
        const t1 = makeTab('t1', 0);
        const t2 = makeTab('t2', 1);

        vi.mocked(workspaceIpc.workspaceSnapshot).mockImplementationOnce(async () => {
            // Simula chegada de 2 eventos durante a chamada do snapshot:
            // seq 5 (já contido no snapshot) e seq 6 (posterior ao snapshot)
            tabOpenedCb?.({
                boot_id: 'boot-1',
                event_seq: 5,
                payload: makeTab('t1-stale', 0)
            });
            tabOpenedCb?.({
                boot_id: 'boot-1',
                event_seq: 6,
                payload: t2
            });
            return {
                bootId: 'boot-1',
                eventSeq: 5,
                restored: true,
                tabs: [t1],
                restoreReport: null
            };
        });

        const wsEvents = useWorkspaceEvents();
        await wsEvents.start();

        expect(tabsStore.bootId).toBe('boot-1');
        expect(tabsStore.lastEventSeq).toBe(6);
        expect(tabsStore.byId.t1?.title).toBe('Aba t1');
        expect(tabsStore.byId['t1-stale']).toBeUndefined();
        expect(tabsStore.byId.t2?.title).toBe('Aba t2');

        wsEvents.stop();
        expect(sessionsIpc.onSessionStarted).toHaveBeenCalledTimes(1);
        expect(templatesIpc.onTemplateChanged).toHaveBeenCalledTimes(1);
        expect(tabsIpc.onTabOpened).toHaveBeenCalledTimes(1);
    });
});
