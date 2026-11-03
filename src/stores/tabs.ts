import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { useIpcError } from '@/composables/useIpcError';
import { buildRequestId } from '@/composables/workspace/layoutTree';
import { isMtIpcError } from '@/ipc/core';
import * as sessionsIpc from '@/ipc/sessions';
import * as tabsIpc from '@/ipc/tabs';
import * as workspaceIpc from '@/ipc/workspace';
import { useProjectsStore } from '@/stores/projects';
import type {
    LayoutSizePatch,
    PaneCloseOutcome_Serialize,
    PaneExitedPayload_Serialize,
    PaneId,
    PanePtyReplacedPayload_Serialize,
    PaneView_Serialize,
    ProjectId,
    SplitDirection,
    TabClosedPayload,
    TabId,
    TabOpenOverrides_Deserialize,
    TabReorderedPayload,
    TabView_Serialize,
    TemplateId,
    WorkspaceRestoreReport_Serialize,
    WorkspaceSnapshot_Serialize
} from '@/types/bindings';

export const useTabsStore = defineStore('tabs', () => {
    const projectsStore = useProjectsStore();
    const ipcError = useIpcError();

    const bootId = ref<string | null>(null);
    const lastEventSeq = ref<number>(0);
    const restored = ref<boolean>(false);
    const restoreReport = ref<WorkspaceRestoreReport_Serialize | null>(null);
    const byId = ref<Record<TabId, TabView_Serialize>>({});
    const orderedIdsByProject = ref<Record<ProjectId, TabId[]>>({});
    const loadingByProject = ref<Record<ProjectId, boolean>>({});
    const error = ref<string | null>(null);

    const dispatchedInitialKeys = new Set<string>();

    function syncTabToProjectsStore(tab: TabView_Serialize, makeActive = false): void {
        try {
            projectsStore.attachTab(tab.id, tab.projectId);
            const currentActive = projectsStore.activeTabByProject[tab.projectId] ?? null;
            if (makeActive || currentActive === null) {
                projectsStore.setActiveTab(tab.projectId, tab.id);
            }
        } catch {
            // Se o catálogo de projetos ainda não hidratou este projectId, ignora até o próximo sync.
        }
    }

    function tabsForProject(projectId: ProjectId): TabView_Serialize[] {
        const ids = orderedIdsByProject.value[projectId] ?? [];
        const result: TabView_Serialize[] = [];
        for (const id of ids) {
            const tab = byId.value[id];
            if (tab) {
                result.push(tab);
            }
        }
        return result;
    }

    const activeTabForSelectedProject = computed<TabView_Serialize | null>(() => {
        const pid = projectsStore.selectedId;
        if (!pid) {
            return null;
        }
        const activeId = projectsStore.activeTabByProject[pid] ?? null;
        if (activeId && byId.value[activeId]) {
            return byId.value[activeId] ?? null;
        }
        const firstId = orderedIdsByProject.value[pid]?.[0] ?? null;
        return firstId ? (byId.value[firstId] ?? null) : null;
    });

    function paneInTab(tabId: TabId, paneId: PaneId): PaneView_Serialize | null {
        const tab = byId.value[tabId];
        if (!tab) {
            return null;
        }
        return tab.panes.find((p) => p.id === paneId) ?? null;
    }

    function applyTabUpsert(tab: TabView_Serialize, focusIfRequested = false): void {
        const prev = byId.value[tab.id];
        if (prev && tab.revision < prev.revision && tab.layoutRevision < prev.layoutRevision) {
            return;
        }

        byId.value = {
            ...byId.value,
            [tab.id]: tab
        };

        const currentList = [...(orderedIdsByProject.value[tab.projectId] ?? [])];
        if (!currentList.includes(tab.id)) {
            currentList.push(tab.id);
        }
        currentList.sort((a, b) => {
            const ta = byId.value[a];
            const tb = byId.value[b];
            return (ta?.position ?? 0) - (tb?.position ?? 0);
        });
        orderedIdsByProject.value = {
            ...orderedIdsByProject.value,
            [tab.projectId]: currentList
        };

        syncTabToProjectsStore(tab, focusIfRequested);
    }

    function applyTabClosed(payload: TabClosedPayload): void {
        const existing = byId.value[payload.tab_id];
        const projectId = existing?.projectId ?? payload.project_id;
        const currentList = orderedIdsByProject.value[projectId] ?? [];
        const removedIdx = currentList.indexOf(payload.tab_id);
        const nextList = currentList.filter((id) => id !== payload.tab_id);

        const nextById = { ...byId.value };
        delete nextById[payload.tab_id];
        byId.value = nextById;

        orderedIdsByProject.value = {
            ...orderedIdsByProject.value,
            [projectId]: nextList
        };

        const wasActive = projectsStore.activeTabByProject[projectId] === payload.tab_id;
        projectsStore.detachTab(payload.tab_id);

        if (wasActive && nextList.length > 0) {
            const fallbackIdx = Math.min(Math.max(removedIdx, 0), nextList.length - 1);
            const nextActiveId = nextList[fallbackIdx] ?? nextList[0] ?? null;
            if (nextActiveId) {
                try {
                    projectsStore.setActiveTab(projectId, nextActiveId);
                } catch {
                    // Ignora se projeto não estiver no catálogo.
                }
            }
        }
    }

    function applyTabReordered(payload: TabReorderedPayload): void {
        const nextIds = [...payload.ordered_tab_ids];
        const nextById = { ...byId.value };
        nextIds.forEach((tid, index) => {
            const current = nextById[tid];
            if (current) {
                nextById[tid] = {
                    ...current,
                    position: index
                };
            }
        });
        byId.value = nextById;
        orderedIdsByProject.value = {
            ...orderedIdsByProject.value,
            [payload.project_id]: nextIds
        };
    }

    function applyPanePtyReplaced(payload: PanePtyReplacedPayload_Serialize): void {
        const tab = byId.value[payload.tab_id];
        if (!tab) {
            return;
        }
        const nextPanes = tab.panes.map((p) => {
            if (p.id !== payload.pane_id) {
                return p;
            }
            return {
                ...p,
                ptyId: payload.new_pty_id,
                generation: payload.generation,
                state: 'live' as const,
                exit: null,
                error: null
            };
        });
        byId.value = {
            ...byId.value,
            [tab.id]: {
                ...tab,
                panes: nextPanes
            }
        };
    }

    function applyPaneExited(payload: PaneExitedPayload_Serialize): void {
        const tab = byId.value[payload.tab_id];
        if (!tab) {
            return;
        }
        const nextPanes = tab.panes.map((p) => {
            if (p.id !== payload.pane_id || p.generation > payload.generation) {
                return p;
            }
            return {
                ...p,
                ptyId: null,
                state: 'exited' as const,
                exit: payload.exit
            };
        });
        byId.value = {
            ...byId.value,
            [tab.id]: {
                ...tab,
                panes: nextPanes
            }
        };
    }

    function hydrateFromSnapshot(snapshot: WorkspaceSnapshot_Serialize): void {
        bootId.value = snapshot.bootId;
        lastEventSeq.value = snapshot.eventSeq;
        restored.value = snapshot.restored;
        restoreReport.value = snapshot.restoreReport;

        const nextById: Record<TabId, TabView_Serialize> = {};
        const nextOrdered: Record<ProjectId, TabId[]> = {};

        const sortedTabs = [...snapshot.tabs].sort((a, b) => a.position - b.position);
        for (const tab of sortedTabs) {
            nextById[tab.id] = tab;
            const list = nextOrdered[tab.projectId] ?? [];
            list.push(tab.id);
            nextOrdered[tab.projectId] = list;
        }

        // Remove abas antigas que não existem mais no snapshot
        for (const oldTabId of Object.keys(byId.value)) {
            if (!nextById[oldTabId]) {
                projectsStore.detachTab(oldTabId);
            }
        }

        byId.value = nextById;
        orderedIdsByProject.value = nextOrdered;

        for (const tab of sortedTabs) {
            syncTabToProjectsStore(tab, false);
        }
    }

    async function refreshSnapshot(): Promise<WorkspaceSnapshot_Serialize> {
        const snap = await workspaceIpc.workspaceSnapshot();
        hydrateFromSnapshot(snap);
        return snap;
    }

    async function loadForProject(projectId: ProjectId): Promise<TabView_Serialize[]> {
        loadingByProject.value = {
            ...loadingByProject.value,
            [projectId]: true
        };
        error.value = null;
        try {
            const list = await tabsIpc.tabList(projectId);
            const sorted = [...list].sort((a, b) => a.position - b.position);
            const nextById = { ...byId.value };
            for (const tab of sorted) {
                nextById[tab.id] = tab;
            }
            byId.value = nextById;
            orderedIdsByProject.value = {
                ...orderedIdsByProject.value,
                [projectId]: sorted.map((t) => t.id)
            };
            for (const tab of sorted) {
                syncTabToProjectsStore(tab, false);
            }
            return sorted;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loadingByProject.value = {
                ...loadingByProject.value,
                [projectId]: false
            };
        }
    }

    async function openDefaultTab(
        projectId: ProjectId,
        overrides: TabOpenOverrides_Deserialize | null = null
    ): Promise<TabView_Serialize> {
        error.value = null;
        try {
            const opened = await tabsIpc.tabOpen({
                requestId: buildRequestId('tab-open'),
                projectId,
                templateId: null,
                overrides,
                focus: true
            });
            applyTabUpsert(opened, true);
            return opened;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function openTemplateTab(
        projectId: ProjectId,
        templateId: TemplateId,
        overrides: TabOpenOverrides_Deserialize | null = null,
        focus = true
    ): Promise<TabView_Serialize> {
        error.value = null;
        try {
            const opened = await tabsIpc.tabOpen({
                requestId: buildRequestId('tab-open-tpl'),
                projectId,
                templateId,
                overrides,
                focus
            });
            applyTabUpsert(opened, focus);
            return opened;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function closeTab(tabId: TabId): Promise<void> {
        error.value = null;
        const tab = byId.value[tabId];
        try {
            await tabsIpc.tabClose(tabId, buildRequestId('tab-close'));
            if (tab) {
                applyTabClosed({
                    tab_id: tabId,
                    project_id: tab.projectId,
                    revision: tab.revision + 1
                });
            }
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function reorderTabs(projectId: ProjectId, orderedIds: TabId[]): Promise<TabId[]> {
        error.value = null;
        const previousIds = [...(orderedIdsByProject.value[projectId] ?? [])];
        applyTabReordered({
            project_id: projectId,
            ordered_tab_ids: orderedIds
        });
        try {
            const committed = await tabsIpc.tabReorder(projectId, orderedIds);
            applyTabReordered({
                project_id: projectId,
                ordered_tab_ids: committed
            });
            return committed;
        } catch (err: unknown) {
            applyTabReordered({
                project_id: projectId,
                ordered_tab_ids: previousIds
            });
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function focusTab(tabId: TabId, paneId: PaneId | null = null): Promise<TabView_Serialize> {
        const current = byId.value[tabId];
        if (current) {
            try {
                projectsStore.setActiveTab(current.projectId, tabId);
            } catch {
                // Ignora se projeto não estiver carregado.
            }
        }
        try {
            const updated = await tabsIpc.tabFocus(tabId, paneId);
            applyTabUpsert(updated, true);
            return updated;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function splitPane(
        paneId: PaneId,
        direction: SplitDirection
    ): Promise<TabView_Serialize> {
        error.value = null;
        const ownerTab = Object.values(byId.value).find((t) =>
            t.panes.some((p) => p.id === paneId)
        );
        const expectedLayoutRevision = ownerTab?.layoutRevision ?? 1;
        const reqId = buildRequestId('pane-split');

        try {
            const updated = await tabsIpc.paneSplit(
                paneId,
                direction,
                expectedLayoutRevision,
                reqId
            );
            applyTabUpsert(updated, true);
            return updated;
        } catch (err: unknown) {
            if (isMtIpcError(err) && err.code === 'workspace.layout_conflict' && ownerTab) {
                await loadForProject(ownerTab.projectId);
                const refreshed = byId.value[ownerTab.id];
                if (refreshed) {
                    const retried = await tabsIpc.paneSplit(
                        paneId,
                        direction,
                        refreshed.layoutRevision,
                        buildRequestId('pane-split-retry')
                    );
                    applyTabUpsert(retried, true);
                    return retried;
                }
            }
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function closePane(paneId: PaneId): Promise<PaneCloseOutcome_Serialize> {
        error.value = null;
        const ownerTab = Object.values(byId.value).find((t) =>
            t.panes.some((p) => p.id === paneId)
        );
        try {
            const outcome = await tabsIpc.paneClose(paneId, buildRequestId('pane-close'));
            if (outcome.kind === 'pane_closed') {
                applyTabUpsert(outcome.tab, false);
            } else if (ownerTab) {
                applyTabClosed({
                    tab_id: outcome.tabId,
                    project_id: ownerTab.projectId,
                    revision: ownerTab.revision + 1
                });
            }
            return outcome;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function restartPane(
        paneId: PaneId,
        cwdFallbackToProject = true
    ): Promise<TabView_Serialize> {
        error.value = null;
        try {
            const updated = await tabsIpc.paneRestart(
                paneId,
                cwdFallbackToProject,
                buildRequestId('pane-restart')
            );
            applyTabUpsert(updated, false);
            return updated;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function updateLayoutSizes(
        tabId: TabId,
        patches: LayoutSizePatch[]
    ): Promise<TabView_Serialize | null> {
        if (patches.length === 0) {
            return byId.value[tabId] ?? null;
        }
        const tab = byId.value[tabId];
        if (!tab) {
            return null;
        }

        try {
            const updated = await tabsIpc.paneLayoutUpdate(
                tabId,
                tab.layoutRevision,
                patches
            );
            applyTabUpsert(updated, false);
            return updated;
        } catch (err: unknown) {
            if (isMtIpcError(err) && err.code === 'workspace.layout_conflict') {
                await loadForProject(tab.projectId);
                const refreshed = byId.value[tabId];
                if (refreshed) {
                    const retried = await tabsIpc.paneLayoutUpdate(
                        tabId,
                        refreshed.layoutRevision,
                        patches
                    );
                    applyTabUpsert(retried, false);
                    return retried;
                }
            }
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function startNewTabSession(tabId: TabId): Promise<TabView_Serialize> {
        error.value = null;
        try {
            const updated = await sessionsIpc.sessionNew(tabId, buildRequestId('session-new'));
            applyTabUpsert(updated, true);
            return updated;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function dispatchInitialCommandIfPending(
        tabId: TabId,
        paneId: PaneId,
        generation: number
    ): Promise<void> {
        const tab = byId.value[tabId];
        if (!tab || tab.primaryPaneId !== paneId || tab.initialDispatch !== 'pending') {
            return;
        }
        const key = `${tabId}:${paneId}:${generation}`;
        if (dispatchedInitialKeys.has(key)) {
            return;
        }
        dispatchedInitialKeys.add(key);

        try {
            const updated = await workspaceIpc.workspaceInitialCommandRun(
                tabId,
                paneId,
                generation,
                buildRequestId('init-cmd')
            );
            applyTabUpsert(updated, false);
        } catch {
            // Se o comando já tiver sido despachado ou geração mudou, mantém silencioso.
        }
    }

    function resetForTests(): void {
        dispatchedInitialKeys.clear();
        bootId.value = null;
        lastEventSeq.value = 0;
        restored.value = false;
        restoreReport.value = null;
        byId.value = {};
        orderedIdsByProject.value = {};
        loadingByProject.value = {};
        error.value = null;
    }

    return {
        bootId,
        lastEventSeq,
        restored,
        restoreReport,
        byId,
        orderedIdsByProject,
        loadingByProject,
        error,
        activeTabForSelectedProject,
        tabsForProject,
        paneInTab,
        hydrateFromSnapshot,
        refreshSnapshot,
        loadForProject,
        openDefaultTab,
        openTemplateTab,
        closeTab,
        reorderTabs,
        focusTab,
        splitPane,
        closePane,
        restartPane,
        updateLayoutSizes,
        startNewTabSession,
        dispatchInitialCommandIfPending,
        applyTabUpsert,
        applyTabClosed,
        applyTabReordered,
        applyPanePtyReplaced,
        applyPaneExited,
        resetForTests
    };
});
