import { defineStore } from 'pinia';
import { ref } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import * as gitIpc from '@/ipc/git';
import { useTabsStore } from '@/stores/tabs';
import type {
    GitContext_Serialize,
    GitStatusChanged_Serialize,
    PaneId,
    TabId,
    WorktreeCreateRequest_Deserialize,
    WorktreeEntry_Serialize
} from '@/types/bindings';

function contextKey(ctx: GitContext_Serialize): string {
    if (ctx.kind === 'repository') {
        return ctx.repoKey;
    }
    return ctx.path;
}

export const useGitStore = defineStore('git', () => {
    const contextByKey = ref<Record<string, GitContext_Serialize>>({});
    const keyByPane = ref<Record<PaneId, string>>({});
    const worktreesByRoot = ref<Record<string, WorktreeEntry_Serialize[]>>({});
    const loadingByKey = ref<Record<string, boolean>>({});
    const error = ref<string | null>(null);

    let unlistenStatus: UnlistenFn | null = null;

    function applyContext(ctx: GitContext_Serialize, paneIds: PaneId[] = []): void {
        const key = contextKey(ctx);
        const existing = contextByKey.value[key];
        if (existing && ctx.revision < existing.revision) {
            return;
        }
        contextByKey.value[key] = ctx;
        for (const pid of paneIds) {
            keyByPane.value[pid] = key;
        }
    }

    function applyStatusChangedEvent(payload: GitStatusChanged_Serialize): void {
        applyContext(payload.context, payload.affectedPaneIds);
    }

    function contextForPane(paneId: PaneId | null | undefined): GitContext_Serialize | null {
        if (!paneId) {
            return null;
        }
        const key = keyByPane.value[paneId];
        if (!key) {
            return null;
        }
        return contextByKey.value[key] ?? null;
    }

    function contextForTab(tabId: TabId): GitContext_Serialize | null {
        const tabsStore = useTabsStore();
        const tab = tabsStore.byId[tabId];
        if (!tab) {
            return null;
        }
        const activeCtx = contextForPane(tab.activePaneId);
        if (activeCtx) {
            return activeCtx;
        }
        return contextForPane(tab.primaryPaneId);
    }

    async function ensurePaneStatus(paneId: PaneId, cwd: string): Promise<GitContext_Serialize | null> {
        const trimmed = cwd.trim();
        if (!trimmed) {
            return null;
        }
        loadingByKey.value[trimmed] = true;
        try {
            const ctx = await gitIpc.gitStatusForPath(trimmed);
            applyContext(ctx, [paneId]);
            return ctx;
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            return null;
        } finally {
            loadingByKey.value[trimmed] = false;
        }
    }

    async function refreshForPath(path: string, paneId?: PaneId): Promise<GitContext_Serialize> {
        loadingByKey.value[path] = true;
        error.value = null;
        try {
            const ctx = await gitIpc.gitStatusRefresh(path);
            applyContext(ctx, paneId ? [paneId] : []);
            return ctx;
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            throw err;
        } finally {
            loadingByKey.value[path] = false;
        }
    }

    async function loadWorktrees(projectRoot: string): Promise<WorktreeEntry_Serialize[]> {
        error.value = null;
        try {
            const list = await gitIpc.gitWorktreeList(projectRoot);
            worktreesByRoot.value[projectRoot] = list;
            return list;
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            throw err;
        }
    }

    async function createWorktree(
        req: WorktreeCreateRequest_Deserialize
    ): Promise<WorktreeEntry_Serialize> {
        error.value = null;
        try {
            const created = await gitIpc.gitWorktreeCreate(req);
            await loadWorktrees(req.projectRoot);
            return created;
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            throw err;
        }
    }

    async function removeWorktree(
        projectRoot: string,
        worktreePath: string,
        force = false
    ): Promise<void> {
        error.value = null;
        try {
            await gitIpc.gitWorktreeRemove(worktreePath, force);
            await loadWorktrees(projectRoot);
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            throw err;
        }
    }

    function removePane(paneId: PaneId): void {
        delete keyByPane.value[paneId];
    }

    async function startListening(): Promise<void> {
        if (unlistenStatus) {
            return;
        }
        unlistenStatus = await gitIpc.onGitStatusChanged((payload) => {
            applyStatusChangedEvent(payload);
        });
    }

    function stopListening(): void {
        if (unlistenStatus) {
            unlistenStatus();
            unlistenStatus = null;
        }
    }

    function resetForTests(): void {
        stopListening();
        contextByKey.value = {};
        keyByPane.value = {};
        worktreesByRoot.value = {};
        loadingByKey.value = {};
        error.value = null;
    }

    return {
        contextByKey,
        keyByPane,
        worktreesByRoot,
        loadingByKey,
        error,
        applyContext,
        applyStatusChangedEvent,
        contextForPane,
        contextForTab,
        ensurePaneStatus,
        refreshForPath,
        loadWorktrees,
        createWorktree,
        removeWorktree,
        removePane,
        startListening,
        stopListening,
        resetForTests
    };
});
