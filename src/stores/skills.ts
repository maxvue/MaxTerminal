import type { UnlistenFn } from '@tauri-apps/api/event';
import { defineStore } from 'pinia';
import { ref } from 'vue';
import { isMtIpcError } from '@/ipc/core';
import { memoryIpc } from '@/ipc/memory';
import { skillsIpc } from '@/ipc/skills';
import type {
    ConflictResolution,
    ErrorCode,
    MemoryEntry_Serialize,
    MemoryQuery_Deserialize,
    MtError_Serialize,
    PaneId,
    ProjectId,
    SkillCatalog,
    SkillSyncTarget_Deserialize,
    SkillSyncTarget_Serialize,
    SyncPlan,
    SyncResult
} from '@/types/bindings';

const GLOBAL_CATALOG_KEY = '__global__';

function catalogKey(projectId: ProjectId | null): string {
    return projectId ?? GLOBAL_CATALOG_KEY;
}

function toMtError(err: unknown): MtError_Serialize {
    if (isMtIpcError(err)) {
        return {
            code: err.code as ErrorCode,
            message: err.message,
            details: (err.details as MtError_Serialize['details']) ?? null
        };
    }
    if (err instanceof Error) {
        return {
            code: 'io.unexpected',
            message: err.message,
            details: null
        };
    }
    return {
        code: 'io.unexpected',
        message: 'Falha inesperada na operação de MaxSkills ou Memória Contextual.',
        details: null
    };
}

export const useSkillsStore = defineStore('skills', () => {
    const catalogByProject = ref<Record<string, SkillCatalog>>({});
    const targets = ref<SkillSyncTarget_Serialize[]>([]);
    const plansByTarget = ref<Record<number, SyncPlan>>({});
    const syncResults = ref<Record<number, SyncResult>>({});
    const memoryByProject = ref<Record<ProjectId, MemoryEntry_Serialize[]>>({});
    const suggestionsByPane = ref<Record<PaneId, MemoryEntry_Serialize[]>>({});
    const loadingByKey = ref<Record<string, boolean>>({});
    const errorsByKey = ref<Record<string, MtError_Serialize | null>>({});
    const initialized = ref<boolean>(false);

    const unlistenFns: UnlistenFn[] = [];
    let initPromise: Promise<void> | null = null;

    function setLoading(key: string, value: boolean): void {
        loadingByKey.value = {
            ...loadingByKey.value,
            [key]: value
        };
    }

    function setError(key: string, err: MtError_Serialize | null): void {
        errorsByKey.value = {
            ...errorsByKey.value,
            [key]: err
        };
    }

    function catalogFor(projectId: ProjectId | null = null): SkillCatalog | null {
        return catalogByProject.value[catalogKey(projectId)] ?? null;
    }

    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        if (initPromise) {
            return initPromise;
        }

        initPromise = (async () => {
            const subs = await Promise.all([
                skillsIpc.onChanged(() => {
                    // Invalida planos em memória quando a fonte muda no disco.
                    plansByTarget.value = {};
                    const loadedKeys = Object.keys(catalogByProject.value);
                    for (const key of loadedKeys) {
                        const pid = key === GLOBAL_CATALOG_KEY ? null : key;
                        void scan(pid).catch(() => {
                            // Mantém catálogo anterior se re-scan automático falhar.
                        });
                    }
                }),
                skillsIpc.onSyncCompleted((result) => {
                    syncResults.value = {
                        ...syncResults.value,
                        [result.targetId]: result
                    };
                    const nextPlans = { ...plansByTarget.value };
                    delete nextPlans[result.targetId];
                    plansByTarget.value = nextPlans;
                }),
                memoryIpc.onUpdated(({ projectId }) => {
                    if (projectId in memoryByProject.value) {
                        void loadMemory(projectId).catch(() => {
                            // Ignora falha silenciosa de reload em background.
                        });
                    }
                })
            ]);

            unlistenFns.push(...subs);
            initialized.value = true;
            initPromise = null;
        })();

        return initPromise;
    }

    async function scan(projectId: ProjectId | null = null): Promise<SkillCatalog> {
        const key = `scan:${catalogKey(projectId)}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const catalog = await skillsIpc.scan(projectId);
            catalogByProject.value = {
                ...catalogByProject.value,
                [catalogKey(projectId)]: catalog
            };
            return catalog;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    async function loadTargets(
        projectId: ProjectId | null = null
    ): Promise<SkillSyncTarget_Serialize[]> {
        const key = `targets:${catalogKey(projectId)}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const list = await skillsIpc.targetsList(projectId);
            targets.value = list;
            return list;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    async function upsertTarget(
        target: SkillSyncTarget_Deserialize
    ): Promise<SkillSyncTarget_Serialize> {
        const key = 'target:upsert';
        setLoading(key, true);
        setError(key, null);
        try {
            const saved = await skillsIpc.targetUpsert(target);
            const idx = targets.value.findIndex((t) => t.id === saved.id);
            if (idx >= 0) {
                const copy = [...targets.value];
                copy[idx] = saved;
                targets.value = copy;
            } else {
                targets.value = [...targets.value, saved];
            }
            if (saved.id !== null) {
                const nextPlans = { ...plansByTarget.value };
                delete nextPlans[saved.id];
                plansByTarget.value = nextPlans;
            }
            return saved;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    async function removeTarget(targetId: number, removeManaged = false): Promise<number> {
        const key = `target:remove:${targetId}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const removedCount = await skillsIpc.targetRemove(targetId, removeManaged);
            targets.value = targets.value.filter((t) => t.id !== targetId);
            const nextPlans = { ...plansByTarget.value };
            delete nextPlans[targetId];
            plansByTarget.value = nextPlans;
            return removedCount;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    async function planSync(targetId: number): Promise<SyncPlan> {
        const key = `plan:${targetId}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const plan = await skillsIpc.plan(targetId);
            plansByTarget.value = {
                ...plansByTarget.value,
                [targetId]: plan
            };
            return plan;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    async function applySync(
        planId: string,
        resolutions: Partial<Record<string, ConflictResolution>> = {},
        targetId?: number
    ): Promise<SyncResult> {
        const key = targetId !== undefined ? `apply:${targetId}` : `apply:${planId}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const result = await skillsIpc.apply(planId, resolutions);
            syncResults.value = {
                ...syncResults.value,
                [result.targetId]: result
            };
            const nextPlans = { ...plansByTarget.value };
            delete nextPlans[result.targetId];
            plansByTarget.value = nextPlans;
            return result;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    async function loadMemory(
        projectId: ProjectId,
        includeHidden = false,
        limit = 50
    ): Promise<MemoryEntry_Serialize[]> {
        const key = `memory:${projectId}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const entries = await memoryIpc.list(projectId, includeHidden, limit);
            memoryByProject.value = {
                ...memoryByProject.value,
                [projectId]: entries
            };
            return entries;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    async function suggest(
        paneId: PaneId,
        query: MemoryQuery_Deserialize
    ): Promise<MemoryEntry_Serialize[]> {
        const key = `suggest:${paneId}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const entries = await memoryIpc.suggestions(query);
            suggestionsByPane.value = {
                ...suggestionsByPane.value,
                [paneId]: entries
            };
            return entries;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    function updateMemoryEntryInState(
        projectId: ProjectId,
        updated: MemoryEntry_Serialize
    ): void {
        const current = memoryByProject.value[projectId] ?? [];
        const idx = current.findIndex((item) => item.command === updated.command);
        const next = [...current];
        if (idx >= 0) {
            next[idx] = updated;
        } else {
            next.unshift(updated);
        }
        memoryByProject.value = {
            ...memoryByProject.value,
            [projectId]: next
        };
    }

    async function pin(
        projectId: ProjectId,
        command: string,
        pinned: boolean
    ): Promise<MemoryEntry_Serialize> {
        const key = `memory:pin:${projectId}`;
        setError(key, null);
        try {
            const updated = await memoryIpc.pin(projectId, command, pinned);
            updateMemoryEntryInState(projectId, updated);
            return updated;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        }
    }

    async function hide(
        projectId: ProjectId,
        command: string,
        hidden: boolean
    ): Promise<MemoryEntry_Serialize> {
        const key = `memory:hide:${projectId}`;
        setError(key, null);
        try {
            const updated = await memoryIpc.hide(projectId, command, hidden);
            updateMemoryEntryInState(projectId, updated);
            if (hidden) {
                const nextSuggestions: Record<PaneId, MemoryEntry_Serialize[]> = {};
                for (const [paneId, list] of Object.entries(suggestionsByPane.value)) {
                    nextSuggestions[paneId] = list.filter((e) => e.command !== command);
                }
                suggestionsByPane.value = nextSuggestions;
            }
            return updated;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        }
    }

    async function forget(projectId: ProjectId, command: string): Promise<boolean> {
        const key = `memory:forget:${projectId}`;
        setError(key, null);
        try {
            const removed = await memoryIpc.forget(projectId, command);
            const current = memoryByProject.value[projectId] ?? [];
            memoryByProject.value = {
                ...memoryByProject.value,
                [projectId]: current.filter((item) => item.command !== command)
            };
            const nextSuggestions: Record<PaneId, MemoryEntry_Serialize[]> = {};
            for (const [paneId, list] of Object.entries(suggestionsByPane.value)) {
                nextSuggestions[paneId] = list.filter((e) => e.command !== command);
            }
            suggestionsByPane.value = nextSuggestions;
            return removed;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        }
    }

    async function rebuild(projectId: ProjectId | null = null): Promise<number> {
        const key = `memory:rebuild:${catalogKey(projectId)}`;
        setLoading(key, true);
        setError(key, null);
        try {
            const count = await memoryIpc.rebuild(projectId);
            if (projectId !== null) {
                await loadMemory(projectId);
            }
            return count;
        } catch (err: unknown) {
            setError(key, toMtError(err));
            throw err;
        } finally {
            setLoading(key, false);
        }
    }

    function dispose(): void {
        while (unlistenFns.length > 0) {
            const fn = unlistenFns.pop();
            try {
                fn?.();
            } catch {
                // Ignora erro de unlisten durante teardown.
            }
        }
        initialized.value = false;
        initPromise = null;
    }

    return {
        catalogByProject,
        targets,
        plansByTarget,
        syncResults,
        memoryByProject,
        suggestionsByPane,
        loadingByKey,
        errorsByKey,
        initialized,
        catalogFor,
        init,
        scan,
        loadTargets,
        upsertTarget,
        removeTarget,
        planSync,
        applySync,
        loadMemory,
        suggest,
        pin,
        hide,
        forget,
        rebuild,
        dispose
    };
});
