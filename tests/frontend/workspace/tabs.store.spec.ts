import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useActiveTabSeen } from '@/composables/workspace/useActiveTabSeen';
import { usePaneLayout } from '@/composables/workspace/usePaneLayout';
import { MtIpcError } from '@/ipc/core';
import * as sessionsIpc from '@/ipc/sessions';
import * as tabsIpc from '@/ipc/tabs';
import * as workspaceIpc from '@/ipc/workspace';
import { useProjectsStore } from '@/stores/projects';
import { useTabsStore } from '@/stores/tabs';
import { useTabStatusStore } from '@/stores/tabStatus';
import type { Project_Serialize, TabView_Serialize } from '@/types/bindings';

vi.mock('@/ipc/sessions', () => ({
    sessionNew: vi.fn(),
    sessionList: vi.fn(),
    sessionGet: vi.fn(),
    sessionScrollbackGet: vi.fn(),
    sessionSearch: vi.fn(),
    onSessionStarted: vi.fn(),
    onSessionArchived: vi.fn()
}));

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
    onTabOpened: vi.fn(),
    onTabClosed: vi.fn(),
    onTabReordered: vi.fn(),
    onTabLayoutChanged: vi.fn(),
    onPanePtyReplaced: vi.fn(),
    onPaneExited: vi.fn()
}));

vi.mock('@/ipc/workspace', () => ({
    workspaceSnapshot: vi.fn(),
    workspaceInitialCommandRun: vi.fn(),
    onWorkspaceRestored: vi.fn()
}));

const PROJECT_ID = '01950000-0000-7000-8000-000000000010';

function seedProject(projectsStore: ReturnType<typeof useProjectsStore>): void {
    const project: Project_Serialize = {
        id: PROJECT_ID,
        title: 'MaxTerminal',
        icon: 'logos:tauri',
        color: '#24C8DB',
        path: '/tmp/maxterminal',
        rootId: null,
        stack: 'tauri',
        identitySource: 'auto',
        sortOrder: 0,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
    projectsStore.applySnapshot({
        epoch: 'epoch-1',
        revision: 1,
        projects: [project],
        roots: [],
        discovered: []
    });
    projectsStore.select(PROJECT_ID);
}

function makeTabView(overrides: Partial<TabView_Serialize> = {}): TabView_Serialize {
    return {
        id: 'tab-1',
        projectId: PROJECT_ID,
        templateId: 'tpl-1',
        title: 'Terminal',
        icon: 'lucide:terminal',
        position: 0,
        origin: 'manual',
        continuous: false,
        lifecycle: 'ready',
        revision: 1,
        layoutRevision: 1,
        layout: { kind: 'leaf', paneId: 'pane-1' },
        panes: [
            {
                id: 'pane-1',
                ptyId: 'pty-1',
                generation: 1,
                cwd: '/tmp/maxterminal',
                state: 'live',
                exit: null,
                error: null
            }
        ],
        activePaneId: 'pane-1',
        primaryPaneId: 'pane-1',
        initialDispatch: 'none',
        tabSessionId: 'tsess-1',
        ...overrides
    };
}

describe('useTabsStore (Fase 07)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('abre aba padrão, vincula ao projeto e seleciona aba vizinha ao fechar', async () => {
        const projectsStore = useProjectsStore();
        seedProject(projectsStore);
        const tabsStore = useTabsStore();

        const t1 = makeTabView({ id: 'tab-1', position: 0 });
        const t2 = makeTabView({ id: 'tab-2', position: 1, title: 'Build' });

        vi.mocked(tabsIpc.tabOpen).mockResolvedValueOnce(t1).mockResolvedValueOnce(t2);
        vi.mocked(tabsIpc.tabClose).mockResolvedValueOnce(null);

        await tabsStore.openDefaultTab(PROJECT_ID);
        await tabsStore.openDefaultTab(PROJECT_ID);

        expect(tabsStore.tabsForProject(PROJECT_ID).map((t) => t.id)).toEqual(['tab-1', 'tab-2']);
        expect(projectsStore.activeTabByProject[PROJECT_ID]).toBe('tab-2');

        await tabsStore.closeTab('tab-2');
        expect(tabsStore.tabsForProject(PROJECT_ID).map((t) => t.id)).toEqual(['tab-1']);
        expect(projectsStore.activeTabByProject[PROJECT_ID]).toBe('tab-1');
    });

    it('faz retry automático em pane_split quando ocorre workspace.layout_conflict', async () => {
        const projectsStore = useProjectsStore();
        seedProject(projectsStore);
        const tabsStore = useTabsStore();

        const initial = makeTabView({ id: 'tab-1', layoutRevision: 1 });
        tabsStore.applyTabUpsert(initial, true);

        const refreshed = makeTabView({ id: 'tab-1', layoutRevision: 2 });
        const splitResult = makeTabView({
            id: 'tab-1',
            layoutRevision: 3,
            layout: {
                kind: 'split',
                nodeId: 's-1',
                direction: 'vertical',
                sizes: [50, 50],
                children: [
                    { kind: 'leaf', paneId: 'pane-1' },
                    { kind: 'leaf', paneId: 'pane-2' }
                ]
            }
        });

        vi.mocked(tabsIpc.paneSplit)
            .mockRejectedValueOnce(
                new MtIpcError(
                    {
                        code: 'workspace.layout_conflict',
                        message: 'Conflito de revisão de layout.',
                        details: null
                    },
                    'pane_split'
                )
            )
            .mockResolvedValueOnce(splitResult);
        vi.mocked(tabsIpc.tabList).mockResolvedValueOnce([refreshed]);

        const out = await tabsStore.splitPane('pane-1', 'vertical');
        expect(out.layoutRevision).toBe(3);
        expect(tabsIpc.paneSplit).toHaveBeenCalledTimes(2);
    });

    it('despacha comando inicial de forma idempotente apenas uma vez por geração do painel primário', async () => {
        const projectsStore = useProjectsStore();
        seedProject(projectsStore);
        const tabsStore = useTabsStore();

        const pendingTab = makeTabView({
            id: 'tab-1',
            primaryPaneId: 'pane-1',
            initialDispatch: 'pending'
        });
        tabsStore.applyTabUpsert(pendingTab, true);

        vi.mocked(workspaceIpc.workspaceInitialCommandRun).mockResolvedValue({
            ...pendingTab,
            initialDispatch: 'sent'
        });

        await tabsStore.dispatchInitialCommandIfPending('tab-1', 'pane-1', 1);
        await tabsStore.dispatchInitialCommandIfPending('tab-1', 'pane-1', 1);

        expect(workspaceIpc.workspaceInitialCommandRun).toHaveBeenCalledTimes(1);
    });

    it('cobre openTemplateTab, reorderTabs, focusTab, closePane, restartPane, updateLayoutSizes, startNewTabSession e eventos de painel', async () => {
        const projectsStore = useProjectsStore();
        seedProject(projectsStore);
        const tabsStore = useTabsStore();

        const t1 = makeTabView({ id: 'tab-1', position: 0, layoutRevision: 1 });
        const t2 = makeTabView({ id: 'tab-2', position: 1, title: 'API' });

        vi.mocked(tabsIpc.tabOpen).mockResolvedValueOnce(t1);
        const opened = await tabsStore.openTemplateTab(PROJECT_ID, 'tpl-1', null, true);
        expect(opened.id).toBe('tab-1');
        expect(tabsStore.paneInTab('tab-1', 'pane-1')?.cwd).toBe('/tmp/maxterminal');
        expect(tabsStore.paneInTab('tab-999', 'pane-1')).toBeNull();

        tabsStore.applyTabUpsert(t2, false);
        // Versão antiga ignorada
        tabsStore.applyTabUpsert({ ...t1, revision: 0, layoutRevision: 0 }, false);

        vi.mocked(tabsIpc.tabReorder).mockResolvedValueOnce(['tab-2', 'tab-1']);
        const reordered = await tabsStore.reorderTabs(PROJECT_ID, ['tab-2', 'tab-1']);
        expect(reordered).toEqual(['tab-2', 'tab-1']);

        // Reorder com erro faz rollback
        vi.mocked(tabsIpc.tabReorder).mockRejectedValueOnce(new Error('reorder falhou'));
        await expect(tabsStore.reorderTabs(PROJECT_ID, ['tab-1', 'tab-2'])).rejects.toThrow();
        expect(tabsStore.orderedIdsByProject[PROJECT_ID]).toEqual(['tab-2', 'tab-1']);

        // focusTab
        vi.mocked(tabsIpc.tabFocus).mockResolvedValueOnce({ ...t1, activePaneId: 'pane-1' });
        await tabsStore.focusTab('tab-1', 'pane-1');

        // applyPanePtyReplaced & applyPaneExited
        tabsStore.applyPanePtyReplaced({
            tab_id: 'tab-1',
            pane_id: 'pane-1',
            old_pty_id: 'pty-1',
            new_pty_id: 'pty-2',
            session_id: 'psess-2',
            generation: 2
        });
        expect(tabsStore.paneInTab('tab-1', 'pane-1')?.ptyId).toBe('pty-2');

        tabsStore.applyPaneExited({
            tab_id: 'tab-1',
            pane_id: 'pane-1',
            session_id: 'psess-2',
            pty_id: 'pty-2',
            generation: 2,
            exit: {
                code: 1,
                signal: null,
                signalName: null,
                reason: 'exited',
                atMs: 1000
            }
        });
        expect(tabsStore.paneInTab('tab-1', 'pane-1')?.state).toBe('exited');

        // restartPane
        vi.mocked(tabsIpc.paneRestart).mockResolvedValueOnce(t1);
        await tabsStore.restartPane('pane-1', true);

        // closePane (pane_closed e tab_closed)
        vi.mocked(tabsIpc.paneClose).mockResolvedValueOnce({
            kind: 'pane_closed',
            tab: t1
        });
        await tabsStore.closePane('pane-1');

        vi.mocked(tabsIpc.paneClose).mockResolvedValueOnce({
            kind: 'tab_closed',
            tabId: 'tab-2'
        });
        await tabsStore.closePane('pane-1');

        // updateLayoutSizes com retry em conflito
        vi.mocked(tabsIpc.paneLayoutUpdate)
            .mockRejectedValueOnce(
                new MtIpcError(
                    {
                        code: 'workspace.layout_conflict',
                        message: 'Conflito',
                        details: null
                    },
                    'pane_layout_update'
                )
            )
            .mockResolvedValueOnce({ ...t1, layoutRevision: 3 });
        vi.mocked(tabsIpc.tabList).mockResolvedValueOnce([{ ...t1, layoutRevision: 2 }]);
        const layoutUpdated = await tabsStore.updateLayoutSizes('tab-1', [
            { nodeId: 's-1', sizes: [60, 40] }
        ]);
        expect(layoutUpdated?.layoutRevision).toBe(3);

        // startNewTabSession
        vi.mocked(sessionsIpc.sessionNew).mockResolvedValueOnce({
            ...t1,
            tabSessionId: 'tsess-2'
        });
        const newSessTab = await tabsStore.startNewTabSession('tab-1');
        expect(newSessTab.tabSessionId).toBe('tsess-2');

        // hydrateFromSnapshot & resetForTests
        vi.mocked(workspaceIpc.workspaceSnapshot).mockResolvedValueOnce({
            bootId: 'boot-snap',
            eventSeq: 10,
            restored: true,
            tabs: [t1],
            restoreReport: null
        });
        await tabsStore.refreshSnapshot();
        expect(tabsStore.bootId).toBe('boot-snap');
        tabsStore.resetForTests();
        expect(tabsStore.bootId).toBeNull();
    });

    it('cobre usePaneLayout e useActiveTabSeen', async () => {
        const projectsStore = useProjectsStore();
        seedProject(projectsStore);
        const tabsStore = useTabsStore();
        const tabStatusStore = useTabStatusStore();

        const t1 = makeTabView({ id: 'tab-1', position: 0 });
        tabsStore.applyTabUpsert(t1, true);

        vi.mocked(tabsIpc.paneLayoutUpdate).mockResolvedValue(t1);
        const paneLayout = usePaneLayout({ debounceMs: 0 });
        paneLayout.scheduleNodeResize('tab-1', 'node-1', [70, 30]);
        await paneLayout.flushPendingResizes('tab-1');
        expect(tabsIpc.paneLayoutUpdate).toHaveBeenCalled();
        paneLayout.stop();

        // useActiveTabSeen
        const markSeenSpy = vi.spyOn(tabStatusStore, 'markSeen').mockResolvedValue(null);
        tabStatusStore.applySnapshot({
            tabId: 'tab-1',
            revision: 5,
            status: 'completed_unseen',
            focused: true,
            panes: [],
            lastCompletion: {
                blockId: 'blk-1',
                ptyId: 'pty-1',
                paneId: 'pane-1',
                outcome: 'success',
                exitCode: 0,
                signal: null,
                endReason: 'exit_zero',
                endedAt: 1000,
                durationMs: 100,
                seen: false
            },
            attention: null,
            updatedAt: 1000
        });

        const seenComposable = useActiveTabSeen();
        expect(seenComposable.activeTabId.value).toBe('tab-1');
        expect(seenComposable.activePaneId.value).toBe('pane-1');
        expect(seenComposable.visiblePaneIds.value).toEqual(['pane-1']);
        window.dispatchEvent(new Event('focus'));
        window.dispatchEvent(new Event('blur'));
        document.dispatchEvent(new Event('visibilitychange'));
        expect(markSeenSpy).toHaveBeenCalled();
        seenComposable.stop();
    });
});
