import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { useIpcError } from '@/composables/useIpcError';
import * as projectsIpc from '@/ipc/projects';
import { useSettingsStore } from '@/stores/settings';
import { useTabStatusStore } from '@/stores/tabStatus';
import type {
    DiscoveredFolder,
    NewProject_Deserialize,
    NewProjectRoot,
    Project_Serialize,
    ProjectId,
    ProjectPatch_Deserialize,
    ProjectRoot,
    ProjectRootId,
    ProjectRootPatch_Deserialize,
    ProjectSnapshot_Serialize,
    RootImportReport_Serialize,
    TabId
} from '@/types/bindings';

/** Contadores semânticos de status de abas agregados por projeto (§6.1 e §6.4 da spec). */
export interface ProjectStatusCounters {
    running: number;
    continuous: number;
    finished: number;
    error: number;
    idle: number;
}

/** Objeto congelado compartilhado retornado por `countersFor` quando o projeto não possui abas ativas. */
export const ZERO_PROJECT_COUNTERS: Readonly<ProjectStatusCounters> = Object.freeze({
    running: 0,
    continuous: 0,
    finished: 0,
    error: 0,
    idle: 0
});

function createMutableCounters(): ProjectStatusCounters {
    return {
        running: 0,
        continuous: 0,
        finished: 0,
        error: 0,
        idle: 0
    };
}

function countersEqual(a: ProjectStatusCounters, b: ProjectStatusCounters): boolean {
    return (
        a.running === b.running &&
        a.continuous === b.continuous &&
        a.finished === b.finished &&
        a.error === b.error &&
        a.idle === b.idle
    );
}

const SELECT_PERSIST_DEBOUNCE_MS = 200;

export const useProjectsStore = defineStore('projects', () => {
    const settingsStore = useSettingsStore();
    const tabStatusStore = useTabStatusStore();
    const ipcError = useIpcError();

    const projects = ref<Project_Serialize[]>([]);
    const roots = ref<ProjectRoot[]>([]);
    const discovered = ref<DiscoveredFolder[]>([]);
    const selectedId = ref<ProjectId | null>(null);
    const tabOwner = ref<Record<TabId, ProjectId>>({});
    const activeTabByProject = ref<Record<ProjectId, TabId | null>>({});
    const loading = ref<boolean>(false);
    const reordering = ref<boolean>(false);
    const error = ref<string | null>(null);
    const epoch = ref<string | null>(null);
    const revision = ref<number>(0);
    const initialized = ref<boolean>(false);

    let lifecycleToken = 0;
    let initPromise: Promise<void> | null = null;
    let resyncPromise: Promise<void> | null = null;
    let dirtyResync = false;
    let targetRevision = 0;
    let targetEpoch: string | null = null;
    let selectPersistTimer: ReturnType<typeof setTimeout> | null = null;
    let previousCountersMap = new Map<ProjectId, Readonly<ProjectStatusCounters>>();

    const projectsByIdMap = computed<Map<ProjectId, Project_Serialize>>(() => {
        const map = new Map<ProjectId, Project_Serialize>();
        for (const proj of projects.value) {
            map.set(proj.id, proj);
        }
        return map;
    });

    function projectById(id: ProjectId): Project_Serialize | undefined {
        return projectsByIdMap.value.get(id);
    }

    const selectedProject = computed<Project_Serialize | null>(() => {
        if (!selectedId.value) {
            return null;
        }
        return projectsByIdMap.value.get(selectedId.value) ?? null;
    });

    const discoveredByRoot = computed<Map<ProjectRootId, DiscoveredFolder[]>>(() => {
        const map = new Map<ProjectRootId, DiscoveredFolder[]>();
        for (const item of discovered.value) {
            const list = map.get(item.rootId);
            if (list) {
                list.push(item);
            } else {
                map.set(item.rootId, [item]);
            }
        }
        return map;
    });

    /**
     * Agregação O(abas) de contadores por projeto com estabilidade referencial por projeto:
     * reutiliza o objeto congelado anterior quando os cinco contadores permanecem idênticos.
     */
    const countersByProject = computed<Map<ProjectId, Readonly<ProjectStatusCounters>>>(() => {
        const rawCounts = new Map<ProjectId, ProjectStatusCounters>();
        const byTab = tabStatusStore.byTab;
        const knownProjects = projectsByIdMap.value;

        for (const [tabId, projectId] of Object.entries(tabOwner.value)) {
            const entry = byTab[tabId];
            if (!entry || !knownProjects.has(projectId)) {
                continue;
            }
            let acc = rawCounts.get(projectId);
            if (!acc) {
                acc = createMutableCounters();
                rawCounts.set(projectId, acc);
            }
            acc[entry.badgeBucket] += 1;
        }

        const stabilized = new Map<ProjectId, Readonly<ProjectStatusCounters>>();
        for (const [projectId, nextCounts] of rawCounts.entries()) {
            const prev = previousCountersMap.get(projectId);
            if (prev && countersEqual(prev, nextCounts)) {
                stabilized.set(projectId, prev);
            } else {
                stabilized.set(projectId, Object.freeze({ ...nextCounts }));
            }
        }
        previousCountersMap = stabilized;
        return stabilized;
    });

    function countersFor(id: ProjectId): Readonly<ProjectStatusCounters> {
        return countersByProject.value.get(id) ?? ZERO_PROJECT_COUNTERS;
    }

    function schedulePersistSelectedId(id: ProjectId | null): void {
        if (selectPersistTimer !== null) {
            clearTimeout(selectPersistTimer);
        }
        selectPersistTimer = setTimeout(() => {
            selectPersistTimer = null;
            void settingsStore.set('projects.selected_id', id);
        }, SELECT_PERSIST_DEBOUNCE_MS);
    }

    function applySnapshot(snapshot: ProjectSnapshot_Serialize): void {
        if (epoch.value !== null && epoch.value === snapshot.epoch && snapshot.revision < revision.value) {
            return;
        }

        const prevSelectedId = selectedId.value;
        const prevIndex = prevSelectedId
            ? projects.value.findIndex((p) => p.id === prevSelectedId)
            : -1;

        epoch.value = snapshot.epoch;
        revision.value = snapshot.revision;
        projects.value = [...snapshot.projects].sort((a, b) => a.sortOrder - b.sortOrder);
        roots.value = [...snapshot.roots];
        discovered.value = [...snapshot.discovered];

        const validIds = new Set(projects.value.map((p) => p.id));

        // Limpa vínculos locais de abas cujos projetos foram removidos do catálogo
        const nextOwners: Record<TabId, ProjectId> = {};
        let ownersChanged = false;
        for (const [tabId, projId] of Object.entries(tabOwner.value)) {
            if (validIds.has(projId)) {
                nextOwners[tabId] = projId;
            } else {
                ownersChanged = true;
            }
        }
        if (ownersChanged) {
            tabOwner.value = nextOwners;
        }

        const nextActiveTabs: Record<ProjectId, TabId | null> = {};
        let activeChanged = false;
        for (const [projId, activeTab] of Object.entries(activeTabByProject.value)) {
            if (!validIds.has(projId)) {
                activeChanged = true;
                continue;
            }
            if (activeTab !== null && nextOwners[activeTab] !== projId) {
                nextActiveTabs[projId] = null;
                activeChanged = true;
            } else {
                nextActiveTabs[projId] = activeTab;
            }
        }
        if (activeChanged) {
            activeTabByProject.value = nextActiveTabs;
        }

        // Se o projeto selecionado foi removido, seleciona o vizinho imediato (ou null se lista vazia)
        if (prevSelectedId !== null && !validIds.has(prevSelectedId)) {
            if (projects.value.length === 0) {
                selectedId.value = null;
            } else {
                const fallbackIdx = Math.min(
                    Math.max(prevIndex, 0),
                    projects.value.length - 1
                );
                selectedId.value = projects.value[fallbackIdx]?.id ?? null;
            }
            schedulePersistSelectedId(selectedId.value);
        }
    }

    async function resync(): Promise<void> {
        if (resyncPromise !== null) {
            dirtyResync = true;
            return resyncPromise;
        }

        const token = lifecycleToken;
        const runLoop = async (): Promise<void> => {
            do {
                dirtyResync = false;
                const snap = await projectsIpc.projectSnapshot();
                if (token !== lifecycleToken) {
                    return;
                }
                applySnapshot(snap);
                if (
                    targetEpoch !== null &&
                    snap.epoch === targetEpoch &&
                    snap.revision < targetRevision
                ) {
                    dirtyResync = true;
                } else {
                    targetEpoch = null;
                    targetRevision = 0;
                }
            } while (dirtyResync && token === lifecycleToken);
        };

        resyncPromise = runLoop().finally(() => {
            if (token === lifecycleToken) {
                resyncPromise = null;
            }
        });
        return resyncPromise;
    }

    function invalidate(event: { epoch: string; revision: number }): void {
        if (epoch.value !== null && event.epoch === epoch.value && event.revision <= revision.value) {
            return;
        }
        targetEpoch = event.epoch;
        targetRevision = Math.max(targetRevision, event.revision);
        void resync().catch((err: unknown) => {
            error.value = ipcError.message(err);
        });
    }

    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        if (initPromise !== null) {
            return initPromise;
        }

        const token = lifecycleToken;
        loading.value = true;
        error.value = null;

        initPromise = (async () => {
            try {
                const persistedSelected = settingsStore.get('projects.selected_id');
                await resync();
                if (token !== lifecycleToken) {
                    return;
                }
                if (
                    typeof persistedSelected === 'string' &&
                    persistedSelected.length > 0 &&
                    projectsByIdMap.value.has(persistedSelected)
                ) {
                    selectedId.value = persistedSelected;
                } else if (selectedId.value !== null && !projectsByIdMap.value.has(selectedId.value)) {
                    selectedId.value = null;
                }
                initialized.value = true;
            } catch (err: unknown) {
                if (token === lifecycleToken) {
                    error.value = ipcError.message(err);
                }
                throw err;
            } finally {
                if (token === lifecycleToken) {
                    loading.value = false;
                    initPromise = null;
                }
            }
        })();

        return initPromise;
    }

    function select(id: ProjectId | null): void {
        if (id !== null && !projectsByIdMap.value.has(id)) {
            return;
        }
        if (selectedId.value === id) {
            return;
        }
        selectedId.value = id;
        schedulePersistSelectedId(id);
    }

    async function runMutationWithResync<T>(action: () => Promise<T>): Promise<T> {
        error.value = null;
        let result: T;
        try {
            result = await action();
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }

        try {
            await resync();
        } catch {
            error.value = 'Alteração salva; atualização da lista pendente';
        }
        return result;
    }

    async function addProject(input: NewProject_Deserialize): Promise<Project_Serialize> {
        const created = await runMutationWithResync(() => projectsIpc.projectAdd(input));
        if (selectedId.value === null && projectsByIdMap.value.has(created.id)) {
            select(created.id);
        }
        return projectsByIdMap.value.get(created.id) ?? created;
    }

    async function updateProject(
        id: ProjectId,
        patch: ProjectPatch_Deserialize
    ): Promise<Project_Serialize> {
        const updated = await runMutationWithResync(() => projectsIpc.projectUpdate(id, patch));
        return projectsByIdMap.value.get(id) ?? updated;
    }

    async function removeProject(id: ProjectId): Promise<void> {
        const currentIdx = projects.value.findIndex((p) => p.id === id);
        let neighborId: ProjectId | null = null;
        if (currentIdx !== -1 && projects.value.length > 1) {
            const nextCandidate = projects.value[currentIdx + 1] ?? projects.value[currentIdx - 1];
            neighborId = nextCandidate?.id ?? null;
        }

        const wasSelected = selectedId.value === id;
        await runMutationWithResync(() => projectsIpc.projectRemove(id));

        if (wasSelected) {
            const nextId =
                neighborId && projectsByIdMap.value.has(neighborId)
                    ? neighborId
                    : (projects.value[0]?.id ?? null);
            selectedId.value = nextId;
            schedulePersistSelectedId(nextId);
        }
    }

    async function reorder(ids: ProjectId[]): Promise<void> {
        if (reordering.value) {
            return;
        }
        reordering.value = true;
        error.value = null;
        try {
            await projectsIpc.projectReorder(ids);
            await resync();
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            try {
                await resync();
            } catch {
                // Ignora erro secundário de recarga após mismatch.
            }
            throw err;
        } finally {
            reordering.value = false;
        }
    }

    async function redetect(id: ProjectId): Promise<Project_Serialize> {
        const updated = await runMutationWithResync(() => projectsIpc.projectRedetect(id));
        return projectsByIdMap.value.get(id) ?? updated;
    }

    async function refreshMissing(): Promise<Project_Serialize[]> {
        error.value = null;
        try {
            const changed = await projectsIpc.projectRefreshMissing();
            if (changed.length > 0) {
                await resync();
            }
            return changed;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function addRoot(input: NewProjectRoot): Promise<RootImportReport_Serialize> {
        return runMutationWithResync(() => projectsIpc.projectRootAdd(input));
    }

    async function updateRoot(
        id: ProjectRootId,
        patch: ProjectRootPatch_Deserialize
    ): Promise<ProjectRoot> {
        return runMutationWithResync(() => projectsIpc.projectRootUpdate(id, patch));
    }

    async function removeRoot(id: ProjectRootId): Promise<void> {
        await runMutationWithResync(() => projectsIpc.projectRootRemove(id));
    }

    async function rescanRoot(id: ProjectRootId): Promise<RootImportReport_Serialize> {
        return runMutationWithResync(() => projectsIpc.projectRootRescan(id));
    }

    async function acceptDiscovered(paths: string[]): Promise<Project_Serialize[]> {
        return runMutationWithResync(() => projectsIpc.projectDiscoveryAccept(paths));
    }

    async function dismissDiscovered(paths: string[]): Promise<void> {
        await runMutationWithResync(() => projectsIpc.projectDiscoveryDismiss(paths));
    }

    function attachTab(tabId: TabId, projectId: ProjectId): void {
        if (!projectsByIdMap.value.has(projectId)) {
            throw new Error(`Projeto '${projectId}' não encontrado para vincular a aba '${tabId}'.`);
        }
        const existingOwner = tabOwner.value[tabId];
        if (existingOwner !== undefined) {
            if (existingOwner === projectId) {
                return;
            }
            throw new Error(
                `A aba '${tabId}' já pertence ao projeto '${existingOwner}'. Chame detachTab antes de reatribuir.`
            );
        }
        tabOwner.value = {
            ...tabOwner.value,
            [tabId]: projectId
        };
    }

    function detachTab(tabId: TabId): void {
        const owner = tabOwner.value[tabId];
        if (owner === undefined) {
            return;
        }
        const nextOwners = { ...tabOwner.value };
        delete nextOwners[tabId];
        tabOwner.value = nextOwners;

        if (activeTabByProject.value[owner] === tabId) {
            activeTabByProject.value = {
                ...activeTabByProject.value,
                [owner]: null
            };
        }
    }

    function setActiveTab(projectId: ProjectId, tabId: TabId | null): void {
        if (!projectsByIdMap.value.has(projectId)) {
            throw new Error(`Projeto '${projectId}' não encontrado ao definir aba ativa.`);
        }
        if (tabId !== null && tabOwner.value[tabId] !== projectId) {
            throw new Error(
                `A aba '${tabId}' não pertence ao projeto '${projectId}'.`
            );
        }
        activeTabByProject.value = {
            ...activeTabByProject.value,
            [projectId]: tabId
        };
    }

    function dispose(): void {
        lifecycleToken += 1;
        initPromise = null;
        resyncPromise = null;
        dirtyResync = false;
        if (selectPersistTimer !== null) {
            clearTimeout(selectPersistTimer);
            selectPersistTimer = null;
        }
        initialized.value = false;
    }

    function resetForTests(): void {
        dispose();
        projects.value = [];
        roots.value = [];
        discovered.value = [];
        selectedId.value = null;
        tabOwner.value = {};
        activeTabByProject.value = {};
        loading.value = false;
        reordering.value = false;
        error.value = null;
        epoch.value = null;
        revision.value = 0;
        targetRevision = 0;
        targetEpoch = null;
        previousCountersMap = new Map();
    }

    return {
        // state
        projects,
        roots,
        discovered,
        selectedId,
        tabOwner,
        activeTabByProject,
        loading,
        reordering,
        error,
        epoch,
        revision,
        initialized,
        // getters
        selectedProject,
        projectById,
        discoveredByRoot,
        countersByProject,
        countersFor,
        // actions
        init,
        resync,
        select,
        addProject,
        updateProject,
        removeProject,
        reorder,
        redetect,
        refreshMissing,
        addRoot,
        updateRoot,
        removeRoot,
        rescanRoot,
        acceptDiscovered,
        dismissDiscovered,
        attachTab,
        detachTab,
        setActiveTab,
        invalidate,
        applySnapshot,
        dispose,
        resetForTests
    };
});
