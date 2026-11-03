import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { commandGroupsIpc } from '@/ipc/commandGroups';
import { isMtIpcError } from '@/ipc/core';
import type { MtError } from '@/types/bindings';
import type {
    CommandGroup,
    CommandGroupCatalog,
    CommandGroupInput,
    DeleteGroupReceipt,
    GroupChangedEvent,
    GroupRun,
    GroupRunEvent,
    GroupRunItem,
    GroupRunItemEventDelta,
    GroupRunListPage,
    GroupRunRequest,
    GroupRunSummary,
    GroupScope,
    GroupSuggestRequest,
    GroupSuggestResponse,
    RecurringCommandsResponse,
    ReorderGroupEntry,
    RunStatus
} from '@/types/commandGroups';

const TERMINAL_RUN_STATUSES: ReadonlySet<RunStatus> = new Set<RunStatus>([
    'succeeded',
    'failed',
    'partial',
    'cancelled',
    'interrupted'
]);

export function isTerminalRunStatus(status: RunStatus): boolean {
    return TERMINAL_RUN_STATUSES.has(status);
}

function toMtError(err: unknown, fallbackMessage: string): MtError {
    if (isMtIpcError(err)) {
        return {
            code: err.code,
            message: err.message,
            details: (err.details as never) ?? null
        };
    }
    if (err instanceof Error) {
        return {
            code: 'core.internal',
            message: err.message,
            details: null
        };
    }
    return {
        code: 'core.internal',
        message: fallbackMessage,
        details: null
    };
}

function sortGroupsCanonical(groups: readonly CommandGroup[]): CommandGroup[] {
    return [...groups].sort((a, b) => {
        if (a.sortOrder !== b.sortOrder) {
            return a.sortOrder - b.sortOrder;
        }
        const nameCmp = a.name.localeCompare(b.name);
        if (nameCmp !== 0) {
            return nameCmp;
        }
        return a.id.localeCompare(b.id);
    });
}

function mergeGroupListWithCache(
    incoming: readonly CommandGroup[],
    existing: readonly CommandGroup[],
    tombstones: Record<string, number>
): CommandGroup[] {
    const byId = new Map<string, CommandGroup>();
    for (const prev of existing) {
        const deletedRev = tombstones[prev.id] ?? 0;
        if (prev.revision > deletedRev) {
            byId.set(prev.id, prev);
        }
    }

    const incomingIds = new Set<string>();
    const result: CommandGroup[] = [];

    for (const grp of incoming) {
        incomingIds.add(grp.id);
        const deletedRev = tombstones[grp.id] ?? 0;
        if (grp.revision <= deletedRev) {
            continue;
        }
        const cached = byId.get(grp.id);
        if (cached && cached.revision > grp.revision) {
            result.push(cached);
        } else {
            result.push(grp);
        }
    }

    return sortGroupsCanonical(result);
}

function applyItemDelta(item: GroupRunItem, delta: GroupRunItemEventDelta): GroupRunItem {
    if (delta.version < item.version) {
        return item;
    }
    return {
        ...item,
        status: delta.status,
        version: delta.version,
        ptyId: delta.ptyId ?? item.ptyId ?? null,
        tabId: delta.tabId ?? item.tabId ?? null,
        paneId: delta.paneId ?? item.paneId ?? null,
        sessionId: delta.sessionId ?? item.sessionId ?? null,
        commandBlockId: delta.commandBlockId ?? item.commandBlockId ?? null,
        exitCode: delta.exitCode !== undefined ? delta.exitCode : (item.exitCode ?? null),
        signal: delta.signal !== undefined ? delta.signal : (item.signal ?? null),
        errorCode: delta.errorCode !== undefined ? delta.errorCode : (item.errorCode ?? null),
        errorMessage:
            delta.errorMessage !== undefined ? delta.errorMessage : (item.errorMessage ?? null),
        startedAt: delta.startedAt ?? item.startedAt ?? null,
        finishedAt: delta.finishedAt ?? item.finishedAt ?? null,
        outputTruncated: delta.outputTruncated
    };
}

function runToSummary(run: GroupRun): GroupRunSummary {
    return {
        id: run.id,
        requestId: run.requestId,
        groupId: run.groupId ?? null,
        projectId: run.projectId,
        originSessionId: run.originSessionId ?? null,
        snapshot: run.snapshot,
        mode: run.mode,
        status: run.status,
        counts: { ...run.counts },
        version: run.version,
        lastExitCode: run.lastExitCode ?? null,
        createdAt: run.createdAt,
        startedAt: run.startedAt ?? null,
        cancelRequestedAt: run.cancelRequestedAt ?? null,
        finishedAt: run.finishedAt ?? null
    };
}

export interface EditorModalState {
    open: boolean;
    group: CommandGroup | null;
    scope: GroupScope;
    projectId: string | null;
    initialName: string;
}

export interface BusyPaneFallbackState {
    groupId: string;
    projectId: string;
    reason: string;
}

export const useCommandGroupsStore = defineStore('commandGroups', () => {
    const globalGroups = ref<CommandGroup[]>([]);
    const globalLoaded = ref<boolean>(false);
    const globalLoading = ref<boolean>(false);
    const globalError = ref<MtError | null>(null);

    const localGroupsByProject = ref<Record<string, CommandGroup[]>>({});
    const localLoadedByProject = ref<Record<string, boolean>>({});
    const localLoadingByProject = ref<Record<string, boolean>>({});
    const localErrorByProject = ref<Record<string, MtError | null>>({});

    const deletedRevisionByGroupId = ref<Record<string, number>>({});

    const runsById = ref<Record<string, GroupRun>>({});
    const runSummariesByProject = ref<Record<string, GroupRunSummary[]>>({});
    const nextCursorByProject = ref<Record<string, string | null>>({});
    const pendingEventsByRunId = ref<Record<string, GroupRunEvent[]>>({});
    const recurringByProject = ref<Record<string, RecurringCommandsResponse>>({});

    const editorModalState = ref<EditorModalState>({
        open: false,
        group: null,
        scope: 'local',
        projectId: null,
        initialName: ''
    });

    const activeRunPanelId = ref<string | null>(null);
    const busyPaneFallback = ref<BusyPaneFallbackState | null>(null);

    const inFlightLoads = new Map<string, Promise<CommandGroupCatalog>>();
    const inFlightRunLoads = new Map<string, Promise<GroupRun>>();
    const unlistenFns: UnlistenFn[] = [];
    let listenersInitPromise: Promise<void> | null = null;
    let listenersActive = false;

    const activeRunForPanel = computed<GroupRun | null>(() => {
        if (!activeRunPanelId.value) {
            return null;
        }
        return runsById.value[activeRunPanelId.value] ?? null;
    });

    function localGroupsFor(projectId: string | null): CommandGroup[] {
        if (!projectId) {
            return [];
        }
        return localGroupsByProject.value[projectId] ?? [];
    }

    function groupsForProject(projectId: string | null): CommandGroupCatalog {
        return {
            projectId,
            localGroups: projectId ? [...(localGroupsByProject.value[projectId] ?? [])] : [],
            globalGroups: [...globalGroups.value]
        };
    }

    function runsForProject(projectId: string): GroupRun[] {
        const list = Object.values(runsById.value).filter((r) => r.projectId === projectId);
        return list.sort((a, b) => b.createdAt - a.createdAt);
    }

    function activeRunsForProject(projectId: string): GroupRun[] {
        return runsForProject(projectId).filter((r) => !isTerminalRunStatus(r.status));
    }

    function upsertSummary(summary: GroupRunSummary): void {
        const list = [...(runSummariesByProject.value[summary.projectId] ?? [])];
        const idx = list.findIndex((item) => item.id === summary.id);
        if (idx === -1) {
            list.unshift(summary);
        } else {
            const current = list[idx]!;
            if (summary.version >= current.version) {
                list[idx] = summary;
            }
        }
        list.sort((a, b) => b.createdAt - a.createdAt);
        runSummariesByProject.value = {
            ...runSummariesByProject.value,
            [summary.projectId]: list
        };
    }

    function upsertGroupInCache(group: CommandGroup): void {
        const tombstoneRev = deletedRevisionByGroupId.value[group.id] ?? 0;
        if (group.revision <= tombstoneRev) {
            return;
        }

        if (group.scope === 'global') {
            const currentList = [...globalGroups.value];
            const idx = currentList.findIndex((g) => g.id === group.id);
            if (idx === -1) {
                currentList.push(group);
            } else if (group.revision >= currentList[idx]!.revision) {
                currentList[idx] = group;
            }
            globalGroups.value = sortGroupsCanonical(currentList);
            return;
        }

        const pid = group.projectId;
        if (!pid) {
            return;
        }
        const currentLocal = [...(localGroupsByProject.value[pid] ?? [])];
        const idx = currentLocal.findIndex((g) => g.id === group.id);
        if (idx === -1) {
            currentLocal.push(group);
        } else if (group.revision >= currentLocal[idx]!.revision) {
            currentLocal[idx] = group;
        }
        localGroupsByProject.value = {
            ...localGroupsByProject.value,
            [pid]: sortGroupsCanonical(currentLocal)
        };
    }

    function removeGroupFromCache(
        groupId: string,
        revision: number,
        scope?: GroupScope,
        projectId?: string | null
    ): void {
        const prevRev = deletedRevisionByGroupId.value[groupId] ?? 0;
        deletedRevisionByGroupId.value = {
            ...deletedRevisionByGroupId.value,
            [groupId]: Math.max(prevRev, revision)
        };

        if (!scope || scope === 'global') {
            globalGroups.value = globalGroups.value.filter(
                (g) => g.id !== groupId || g.revision > revision
            );
        }

        if (projectId && localGroupsByProject.value[projectId]) {
            localGroupsByProject.value = {
                ...localGroupsByProject.value,
                [projectId]: localGroupsByProject.value[projectId]!.filter(
                    (g) => g.id !== groupId || g.revision > revision
                )
            };
            return;
        }

        if (!scope || scope === 'local') {
            const nextMap: Record<string, CommandGroup[]> = {};
            for (const [pid, list] of Object.entries(localGroupsByProject.value)) {
                nextMap[pid] = list.filter((g) => g.id !== groupId || g.revision > revision);
            }
            localGroupsByProject.value = nextMap;
        }
    }

    async function load(projectId: string | null, force = false): Promise<CommandGroupCatalog> {
        if (!force) {
            if (projectId === null && globalLoaded.value) {
                return {
                    projectId: null,
                    localGroups: [],
                    globalGroups: [...globalGroups.value]
                };
            }
            if (
                projectId !== null &&
                localLoadedByProject.value[projectId] &&
                globalLoaded.value
            ) {
                return {
                    projectId,
                    localGroups: [...(localGroupsByProject.value[projectId] ?? [])],
                    globalGroups: [...globalGroups.value]
                };
            }
        }

        const key = projectId ?? '__global__';
        const existingPromise = inFlightLoads.get(key);
        if (existingPromise) {
            return existingPromise;
        }

        if (projectId === null) {
            globalLoading.value = true;
            globalError.value = null;
        } else {
            localLoadingByProject.value = {
                ...localLoadingByProject.value,
                [projectId]: true
            };
            localErrorByProject.value = {
                ...localErrorByProject.value,
                [projectId]: null
            };
        }

        const promise = (async (): Promise<CommandGroupCatalog> => {
            try {
                const catalog = await commandGroupsIpc.list(projectId);
                globalGroups.value = mergeGroupListWithCache(
                    catalog.globalGroups,
                    globalGroups.value,
                    deletedRevisionByGroupId.value
                );
                globalLoaded.value = true;
                globalError.value = null;

                if (projectId !== null) {
                    const mergedLocal = mergeGroupListWithCache(
                        catalog.localGroups,
                        localGroupsByProject.value[projectId] ?? [],
                        deletedRevisionByGroupId.value
                    );
                    localGroupsByProject.value = {
                        ...localGroupsByProject.value,
                        [projectId]: mergedLocal
                    };
                    localLoadedByProject.value = {
                        ...localLoadedByProject.value,
                        [projectId]: true
                    };
                    localErrorByProject.value = {
                        ...localErrorByProject.value,
                        [projectId]: null
                    };
                    return {
                        projectId,
                        localGroups: [...mergedLocal],
                        globalGroups: [...globalGroups.value]
                    };
                }

                return {
                    projectId: null,
                    localGroups: [],
                    globalGroups: [...globalGroups.value]
                };
            } catch (err: unknown) {
                const mtErr = toMtError(err, 'Falha ao carregar grupos de comandos.');
                if (projectId === null) {
                    globalError.value = mtErr;
                } else {
                    localErrorByProject.value = {
                        ...localErrorByProject.value,
                        [projectId]: mtErr
                    };
                }
                throw err;
            } finally {
                inFlightLoads.delete(key);
                if (projectId === null) {
                    globalLoading.value = false;
                } else {
                    localLoadingByProject.value = {
                        ...localLoadingByProject.value,
                        [projectId]: false
                    };
                }
            }
        })();

        inFlightLoads.set(key, promise);
        return promise;
    }

    async function get(groupId: string): Promise<CommandGroup> {
        const group = await commandGroupsIpc.get(groupId);
        upsertGroupInCache(group);
        return group;
    }

    async function create(input: CommandGroupInput): Promise<CommandGroup> {
        const created = await commandGroupsIpc.create(input);
        upsertGroupInCache(created);
        return created;
    }

    async function update(
        groupId: string,
        expectedRevision: number,
        input: CommandGroupInput
    ): Promise<CommandGroup> {
        const updated = await commandGroupsIpc.update(groupId, expectedRevision, input);
        upsertGroupInCache(updated);
        return updated;
    }

    async function remove(groupId: string, expectedRevision: number): Promise<DeleteGroupReceipt> {
        const receipt = await commandGroupsIpc.remove(groupId, expectedRevision);
        removeGroupFromCache(receipt.id, receipt.revision);
        return receipt;
    }

    async function reorder(
        scope: GroupScope,
        projectId: string | null,
        entries: ReorderGroupEntry[]
    ): Promise<CommandGroupCatalog> {
        const catalog = await commandGroupsIpc.reorder(scope, projectId, entries);
        globalGroups.value = mergeGroupListWithCache(
            catalog.globalGroups,
            globalGroups.value,
            deletedRevisionByGroupId.value
        );
        globalLoaded.value = true;
        if (projectId !== null) {
            const mergedLocal = mergeGroupListWithCache(
                catalog.localGroups,
                localGroupsByProject.value[projectId] ?? [],
                deletedRevisionByGroupId.value
            );
            localGroupsByProject.value = {
                ...localGroupsByProject.value,
                [projectId]: mergedLocal
            };
            localLoadedByProject.value = {
                ...localLoadedByProject.value,
                [projectId]: true
            };
        }
        return groupsForProject(projectId);
    }

    function applySingleRunEventToRun(current: GroupRun, event: GroupRunEvent): GroupRun {
        const nextItems = current.items.map((item) => {
            if (event.item && (event.item.id === item.id || event.item.position === item.position)) {
                return applyItemDelta(item, event.item);
            }
            return item;
        });

        return {
            ...current,
            status: event.status,
            counts: { ...event.counts },
            version: event.seq,
            lastExitCode:
                event.lastExitCode !== undefined
                    ? event.lastExitCode
                    : (current.lastExitCode ?? null),
            startedAt: event.startedAt ?? current.startedAt ?? null,
            cancelRequestedAt: event.cancelRequestedAt ?? current.cancelRequestedAt ?? null,
            finishedAt: event.finishedAt ?? current.finishedAt ?? null,
            items: nextItems
        };
    }

    function applyRunSnapshot(snapshot: GroupRun): GroupRun {
        const existing = runsById.value[snapshot.id];
        let base =
            existing && existing.version > snapshot.version ? existing : snapshot;

        const buffered = pendingEventsByRunId.value[snapshot.id] ?? [];
        if (buffered.length > 0) {
            const nextPending = { ...pendingEventsByRunId.value };
            delete nextPending[snapshot.id];
            pendingEventsByRunId.value = nextPending;

            const sorted = [...buffered].sort((a, b) => a.seq - b.seq);
            let gapDetected = false;
            for (const ev of sorted) {
                if (ev.seq <= base.version) {
                    continue;
                }
                if (ev.seq === base.version + 1) {
                    base = applySingleRunEventToRun(base, ev);
                } else {
                    gapDetected = true;
                    break;
                }
            }
            if (gapDetected) {
                void loadRun(snapshot.id).catch(() => {});
            }
        }

        runsById.value = {
            ...runsById.value,
            [base.id]: base
        };
        upsertSummary(runToSummary(base));
        return base;
    }

    async function loadRun(runId: string): Promise<GroupRun> {
        const existingFlight = inFlightRunLoads.get(runId);
        if (existingFlight) {
            return existingFlight;
        }

        const promise = (async (): Promise<GroupRun> => {
            try {
                const run = await commandGroupsIpc.getRun(runId);
                return applyRunSnapshot(run);
            } finally {
                inFlightRunLoads.delete(runId);
            }
        })();

        inFlightRunLoads.set(runId, promise);
        return promise;
    }

    async function startRun(request: GroupRunRequest): Promise<GroupRun> {
        busyPaneFallback.value = null;
        try {
            const run = await commandGroupsIpc.run(request);
            const applied = applyRunSnapshot(run);
            activeRunPanelId.value = applied.id;
            return applied;
        } catch (err: unknown) {
            if (
                isMtIpcError(err) &&
                (err.code === 'commands.group.pane_busy' ||
                    err.code === 'commands.group.tracking_unavailable')
            ) {
                busyPaneFallback.value = {
                    groupId: request.groupId,
                    projectId: request.projectId,
                    reason: err.message
                };
            }
            throw err;
        }
    }

    async function cancelRun(runId: string, itemId: string | null = null): Promise<GroupRun> {
        const run = await commandGroupsIpc.cancelRun(runId, itemId);
        return applyRunSnapshot(run);
    }

    async function loadHistory(
        projectId: string,
        cursor: string | null = null,
        limit = 25,
        activeOnly: boolean | null = null
    ): Promise<GroupRunListPage> {
        const page = await commandGroupsIpc.listRuns(projectId, cursor, limit, activeOnly);
        const currentList = cursor ? [...(runSummariesByProject.value[projectId] ?? [])] : [];
        const byId = new Map<string, GroupRunSummary>();
        for (const item of currentList) {
            byId.set(item.id, item);
        }
        for (const incoming of page.items) {
            const prev = byId.get(incoming.id);
            if (!prev || incoming.version >= prev.version) {
                byId.set(incoming.id, incoming);
            }
        }
        const merged = [...byId.values()].sort((a, b) => b.createdAt - a.createdAt);
        runSummariesByProject.value = {
            ...runSummariesByProject.value,
            [projectId]: merged
        };
        nextCursorByProject.value = {
            ...nextCursorByProject.value,
            [projectId]: page.nextCursor ?? null
        };
        return page;
    }

    async function suggest(request: GroupSuggestRequest): Promise<GroupSuggestResponse> {
        return commandGroupsIpc.suggest(request);
    }

    async function loadRecurring(
        projectId: string,
        limit = 15
    ): Promise<RecurringCommandsResponse> {
        const response = await commandGroupsIpc.recurringCommands(projectId, limit);
        recurringByProject.value = {
            ...recurringByProject.value,
            [projectId]: response
        };
        return response;
    }

    function handleGroupChanged(event: GroupChangedEvent): void {
        if (event.operation === 'deleted' && event.groupId) {
            removeGroupFromCache(
                event.groupId,
                event.revision,
                event.scope,
                event.projectId ?? null
            );
            return;
        }

        if (event.scope === 'local' && event.projectId) {
            const pid = event.projectId;
            const wasLoaded = Boolean(localLoadedByProject.value[pid]);
            localLoadedByProject.value = {
                ...localLoadedByProject.value,
                [pid]: false
            };
            if (wasLoaded) {
                void load(pid, true).catch(() => {});
            }
            return;
        }

        if (event.scope === 'global') {
            const wasGlobalLoaded = globalLoaded.value;
            globalLoaded.value = false;
            if (wasGlobalLoaded) {
                void load(null, true).catch(() => {});
            }
        }
    }

    function handleRunEvent(event: GroupRunEvent): void {
        const current = runsById.value[event.runId];
        if (!current) {
            const list = [...(pendingEventsByRunId.value[event.runId] ?? []), event];
            pendingEventsByRunId.value = {
                ...pendingEventsByRunId.value,
                [event.runId]: list
            };
            void loadRun(event.runId).catch(() => {});
            return;
        }

        if (event.seq <= current.version) {
            return;
        }

        if (event.seq > current.version + 1) {
            const list = [...(pendingEventsByRunId.value[event.runId] ?? []), event];
            pendingEventsByRunId.value = {
                ...pendingEventsByRunId.value,
                [event.runId]: list
            };
            void loadRun(event.runId).catch(() => {});
            return;
        }

        const updated = applySingleRunEventToRun(current, event);
        runsById.value = {
            ...runsById.value,
            [updated.id]: updated
        };
        upsertSummary(runToSummary(updated));
    }

    async function initListeners(): Promise<void> {
        if (listenersActive) {
            return;
        }
        if (listenersInitPromise) {
            return listenersInitPromise;
        }

        listenersInitPromise = (async () => {
            try {
                const unlistenChanged = await commandGroupsIpc.onGroupChanged(handleGroupChanged);
                const unlistenStarted = await commandGroupsIpc.onRunStarted(handleRunEvent);
                const unlistenItem = await commandGroupsIpc.onRunItemChanged(handleRunEvent);
                const unlistenFinished = await commandGroupsIpc.onRunFinished(handleRunEvent);
                unlistenFns.push(unlistenChanged, unlistenStarted, unlistenItem, unlistenFinished);
                listenersActive = true;
            } finally {
                listenersInitPromise = null;
            }
        })();

        return listenersInitPromise;
    }

    function disposeListeners(): void {
        while (unlistenFns.length > 0) {
            const fn = unlistenFns.pop();
            fn?.();
        }
        listenersActive = false;
        listenersInitPromise = null;
    }

    function openCreateModal(
        scope: GroupScope,
        projectId: string | null,
        initialName = ''
    ): void {
        editorModalState.value = {
            open: true,
            group: null,
            scope,
            projectId: scope === 'local' ? projectId : null,
            initialName: initialName.trim()
        };
    }

    function openEditModal(group: CommandGroup): void {
        editorModalState.value = {
            open: true,
            group,
            scope: group.scope,
            projectId: group.projectId ?? null,
            initialName: group.name
        };
    }

    function closeEditorModal(): void {
        editorModalState.value = {
            ...editorModalState.value,
            open: false
        };
    }

    return {
        globalGroups,
        globalLoaded,
        globalLoading,
        globalError,
        localGroupsByProject,
        localLoadedByProject,
        localLoadingByProject,
        localErrorByProject,
        deletedRevisionByGroupId,
        runsById,
        runSummariesByProject,
        nextCursorByProject,
        pendingEventsByRunId,
        recurringByProject,
        editorModalState,
        activeRunPanelId,
        activeRunForPanel,
        busyPaneFallback,
        localGroupsFor,
        groupsForProject,
        runsForProject,
        activeRunsForProject,
        initListeners,
        disposeListeners,
        load,
        get,
        create,
        update,
        remove,
        reorder,
        startRun,
        cancelRun,
        loadRun,
        loadHistory,
        suggest,
        loadRecurring,
        handleGroupChanged,
        handleRunEvent,
        applyRunSnapshot,
        openCreateModal,
        openEditModal,
        closeEditorModal
    };
});
