import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { useIpcError } from '@/composables/useIpcError';
import {
    aiIpc,
    onAiCatalogUpdated,
    onAiSettingsChanged,
    onAiStatusChanged
} from '@/ipc/ai';
import type {
    AiContext_Serialize,
    AiHistoryFilter_Deserialize,
    AiHistoryItem_Serialize,
    AiRequestId,
    AiSelection_Serialize,
    AiSettings_Deserialize,
    AiSettings_Serialize,
    AiTrigger,
    CacheClearScope,
    CommandBlockId,
    CommandSuggestion_Serialize,
    DiagnosisSkip,
    GatewayHealth_Serialize,
    GatewayStatus,
    Harness_Serialize,
    HarnessCatalog_Serialize,
    InlineDiagnosis_Serialize,
    PaneExitedEvent_Serialize,
    PaneId,
    ProjectId,
    RagSearchResponse_Serialize,
    RagSlug_Serialize,
    RiskAssessment_Serialize
} from '@/types/bindings';

export type SuggestionStatus = 'idle' | 'streaming' | 'completed' | 'failed' | 'cancelled';

export interface PaneSuggestionState {
    requestId: AiRequestId | null;
    trigger: AiTrigger;
    prompt: string;
    selection: AiSelection_Serialize | null;
    status: SuggestionStatus;
    streamingText: string;
    suggestion: CommandSuggestion_Serialize | null;
    editedCommand: string | null;
    effectiveRisk: RiskAssessment_Serialize | null;
    contextPreview: AiContext_Serialize | null;
    error: string | null;
}

export interface BlockDiagnosisState {
    blockId: CommandBlockId;
    status: 'pending' | 'ready' | 'skipped' | 'dismissed' | 'failed';
    diagnosis: InlineDiagnosis_Serialize | null;
    skipReason: DiagnosisSkip | null;
    error: string | null;
}

export interface PaneAiPromptMode {
    kind: 'suggest' | 'rag';
    trigger: AiTrigger;
    selection: AiSelection_Serialize | null;
    initialPrompt?: string;
    ragSlug?: string | null;
}

export interface AiStatusBadgeInfo {
    color: 'success' | 'warning' | 'danger' | 'neutral';
    label: string;
}

export const CANONICAL_HARNESSES_FALLBACK: readonly Harness_Serialize[] = Object.freeze([
    {
        id: 'claude-code',
        label: 'Claude Code',
        icon: 'simple-icons:anthropic',
        models: []
    },
    {
        id: 'codex',
        label: 'Codex',
        icon: 'simple-icons:openai',
        models: []
    },
    {
        id: 'agy',
        label: 'Antigravity (agy)',
        icon: 'lucide:sparkles',
        models: []
    },
    {
        id: 'opencode',
        label: 'OpenCode',
        icon: 'lucide:terminal-square',
        models: []
    }
]);

export const DEFAULT_AI_SETTINGS: AiSettings_Serialize = {
    revision: 1,
    enabled: true,
    baseUrl: 'http://127.0.0.1:8787',
    allowRemote: false,
    defaultSelection: null,
    diagnosis: {
        enabled: true,
        debounceMs: 800,
        selection: null,
        ignoredExitCodes: [130],
        disabledProjects: [],
        maxPerMinute: 10
    },
    rag: {
        topK: 5,
        stackSlugs: {
            laravel: 'laravel',
            vue: 'vue',
            tauri: 'tauri',
            rust: 'rust'
        },
        lastSlug: null
    },
    context: {
        recentCommands: 10,
        includeOutputTail: true,
        outputTailBytes: 4096
    },
    timeouts: {
        connectMs: 2000,
        suggestMs: 60000,
        diagnosisMs: 20000,
        ragMs: 10000,
        structuredMs: 90000,
        catalogMs: 5000
    },
    saveHistory: true
};

export const useAiStore = defineStore('ai', () => {
    const ipcError = useIpcError();

    const health = ref<GatewayHealth_Serialize | null>(null);
    const settings = ref<AiSettings_Serialize | null>(null);
    const catalog = ref<HarnessCatalog_Serialize | null>(null);
    const ragSlugs = ref<RagSlug_Serialize[]>([]);
    const paneSuggestions = ref<Record<PaneId, PaneSuggestionState>>({});
    const paneRagResults = ref<Record<PaneId, RagSearchResponse_Serialize | null>>({});
    const paneActiveMode = ref<Record<PaneId, PaneAiPromptMode | null>>({});
    const blockDiagnoses = ref<Record<CommandBlockId, BlockDiagnosisState>>({});
    const history = ref<AiHistoryItem_Serialize[]>([]);
    const lastSelectionByPane = ref<Record<PaneId, AiSelection_Serialize>>({});
    const loading = ref<boolean>(false);
    const error = ref<string | null>(null);
    const initialized = ref<boolean>(false);

    let generation = 0;
    let initPromise: Promise<void> | null = null;
    const unlisteners: UnlistenFn[] = [];

    const status = computed<GatewayStatus>(() => {
        if (settings.value && !settings.value.enabled) {
            return 'disabled';
        }
        if (settings.value && (!settings.value.baseUrl || settings.value.baseUrl.trim() === '')) {
            return 'unconfigured';
        }
        return health.value?.status ?? 'offline';
    });

    const isEnabled = computed<boolean>(() => settings.value?.enabled ?? true);

    const isOnline = computed<boolean>(
        () => status.value === 'online' || status.value === 'degraded'
    );

    const defaultSelection = computed<AiSelection_Serialize | null>(
        () => settings.value?.defaultSelection ?? null
    );

    const harnesses = computed<Harness_Serialize[]>(() => {
        const list = catalog.value?.harnesses;
        if (list && list.length > 0) {
            return list;
        }
        return [...CANONICAL_HARNESSES_FALLBACK];
    });

    const indicator = computed<AiStatusBadgeInfo>(() => {
        switch (status.value) {
            case 'online':
                return { color: 'success', label: 'MaxAiManager Online' };
            case 'degraded':
                return { color: 'warning', label: 'MaxAiManager Parcial' };
            case 'disabled':
                return { color: 'neutral', label: 'IA Desativada' };
            case 'unconfigured':
                return { color: 'warning', label: 'IA Não Configurada' };
            case 'offline':
            default:
                return { color: 'danger', label: 'MaxAiManager Offline' };
        }
    });

    function suggestionForPane(paneId: PaneId): PaneSuggestionState | null {
        return paneSuggestions.value[paneId] ?? null;
    }

    function ragResultsForPane(paneId: PaneId): RagSearchResponse_Serialize | null {
        return paneRagResults.value[paneId] ?? null;
    }

    function activeModeForPane(paneId: PaneId): PaneAiPromptMode | null {
        return paneActiveMode.value[paneId] ?? null;
    }

    function diagnosisForBlock(blockId: CommandBlockId): BlockDiagnosisState | null {
        return blockDiagnoses.value[blockId] ?? null;
    }

    function setPaneSelection(paneId: PaneId, selection: AiSelection_Serialize): void {
        lastSelectionByPane.value = {
            ...lastSelectionByPane.value,
            [paneId]: selection
        };
    }

    function openPanePrompt(paneId: PaneId, mode: PaneAiPromptMode): void {
        paneActiveMode.value = {
            ...paneActiveMode.value,
            [paneId]: mode
        };
        if (mode.selection) {
            setPaneSelection(paneId, mode.selection);
        }
    }

    function closePanePrompt(paneId: PaneId): void {
        if (!(paneId in paneActiveMode.value)) {
            return;
        }
        const next = { ...paneActiveMode.value };
        delete next[paneId];
        paneActiveMode.value = next;
    }

    function setPaneSuggestion(paneId: PaneId, state: PaneSuggestionState): void {
        paneSuggestions.value = {
            ...paneSuggestions.value,
            [paneId]: state
        };
    }

    function patchPaneSuggestion(paneId: PaneId, patch: Partial<PaneSuggestionState>): void {
        const current = paneSuggestions.value[paneId];
        if (!current) {
            return;
        }
        paneSuggestions.value = {
            ...paneSuggestions.value,
            [paneId]: {
                ...current,
                ...patch
            }
        };
    }

    function clearPaneSuggestion(paneId: PaneId): void {
        if (!(paneId in paneSuggestions.value)) {
            return;
        }
        const next = { ...paneSuggestions.value };
        delete next[paneId];
        paneSuggestions.value = next;
    }

    function setPaneRagResults(
        paneId: PaneId,
        response: RagSearchResponse_Serialize | null
    ): void {
        if (response === null) {
            const next = { ...paneRagResults.value };
            delete next[paneId];
            paneRagResults.value = next;
            return;
        }
        paneRagResults.value = {
            ...paneRagResults.value,
            [paneId]: response
        };
    }

    function setBlockDiagnosis(blockId: CommandBlockId, state: BlockDiagnosisState): void {
        blockDiagnoses.value = {
            ...blockDiagnoses.value,
            [blockId]: state
        };
    }

    function clearPaneState(paneId: PaneId): void {
        const nextSug = { ...paneSuggestions.value };
        delete nextSug[paneId];
        paneSuggestions.value = nextSug;

        const nextRag = { ...paneRagResults.value };
        delete nextRag[paneId];
        paneRagResults.value = nextRag;

        const nextMode = { ...paneActiveMode.value };
        delete nextMode[paneId];
        paneActiveMode.value = nextMode;

        const nextSel = { ...lastSelectionByPane.value };
        delete nextSel[paneId];
        lastSelectionByPane.value = nextSel;
    }

    async function refreshHealth(): Promise<GatewayHealth_Serialize> {
        error.value = null;
        try {
            const nextHealth = await aiIpc.health();
            health.value = nextHealth;
            return nextHealth;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function loadSettings(): Promise<AiSettings_Serialize> {
        error.value = null;
        try {
            const fetched = await aiIpc.settingsGet();
            settings.value = fetched;
            return fetched;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function saveSettings(next: AiSettings_Deserialize): Promise<AiSettings_Serialize> {
        error.value = null;
        try {
            const saved = await aiIpc.settingsUpdate(next);
            settings.value = saved;
            return saved;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function loadCatalog(force = false): Promise<HarnessCatalog_Serialize> {
        error.value = null;
        try {
            const fetched = force ? await aiIpc.catalogRefresh() : await aiIpc.catalogGet();
            catalog.value = fetched;
            return fetched;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function loadRagSlugs(): Promise<RagSlug_Serialize[]> {
        error.value = null;
        try {
            const list = await aiIpc.ragListSlugs();
            ragSlugs.value = list;
            return list;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function loadHistory(
        filter: Partial<AiHistoryFilter_Deserialize> = {}
    ): Promise<AiHistoryItem_Serialize[]> {
        error.value = null;
        try {
            const page = await aiIpc.historyList({
                limit: filter.limit ?? 50,
                kind: filter.kind ?? null,
                projectId: filter.projectId ?? null,
                beforeId: filter.beforeId ?? null
            });
            history.value = page.items;
            return page.items;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function clearHistory(projectId: ProjectId | null = null): Promise<number> {
        error.value = null;
        try {
            const removed = await aiIpc.historyClear(projectId);
            if (projectId === null) {
                history.value = [];
            } else {
                history.value = history.value.filter((item) => item.projectId !== projectId);
            }
            return removed;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function clearCache(scope: CacheClearScope = 'all'): Promise<number> {
        error.value = null;
        try {
            const removed = await aiIpc.cacheClear(scope);
            if (scope === 'diagnosis' || scope === 'all') {
                blockDiagnoses.value = {};
            }
            return removed;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        }
    }

    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        if (initPromise !== null) {
            return initPromise;
        }

        const currentGen = ++generation;
        loading.value = true;
        error.value = null;

        initPromise = (async () => {
            try {
                const listeners = await Promise.all([
                    onAiStatusChanged((nextHealth) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        if (
                            health.value &&
                            nextHealth.configRevision < health.value.configRevision
                        ) {
                            return;
                        }
                        health.value = nextHealth;
                    }),
                    onAiCatalogUpdated((nextCatalog) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        if (
                            catalog.value &&
                            nextCatalog.configRevision < catalog.value.configRevision
                        ) {
                            return;
                        }
                        catalog.value = nextCatalog;
                    }),
                    onAiSettingsChanged((nextSettings) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        if (
                            settings.value &&
                            nextSettings.revision < settings.value.revision
                        ) {
                            return;
                        }
                        settings.value = nextSettings;
                    }),
                    listen<PaneExitedEvent_Serialize>('pane://exited', (ev) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        const paneId = ev.payload?.payload?.pane_id;
                        if (paneId) {
                            clearPaneState(paneId);
                        }
                    })
                ]);

                if (currentGen !== generation) {
                    for (const unlisten of listeners) {
                        unlisten();
                    }
                    return;
                }

                unlisteners.push(...listeners);

                const [fetchedSettings, fetchedHealth, fetchedCatalog] = await Promise.all([
                    aiIpc.settingsGet().catch(() => ({ ...DEFAULT_AI_SETTINGS })),
                    aiIpc.health().catch(() => null),
                    aiIpc.catalogGet().catch(() => null)
                ]);

                if (currentGen !== generation) {
                    return;
                }

                settings.value = fetchedSettings;
                if (fetchedHealth) {
                    health.value = fetchedHealth;
                }
                if (fetchedCatalog) {
                    catalog.value = fetchedCatalog;
                }

                if (fetchedHealth?.status === 'online' || fetchedHealth?.status === 'degraded') {
                    void aiIpc
                        .ragListSlugs()
                        .then((slugs) => {
                            if (currentGen === generation) {
                                ragSlugs.value = slugs;
                            }
                        })
                        .catch(() => {});
                }

                initialized.value = true;
            } catch (err: unknown) {
                if (currentGen === generation) {
                    error.value = ipcError.message(err);
                }
                throw err;
            } finally {
                if (currentGen === generation) {
                    loading.value = false;
                    initPromise = null;
                }
            }
        })();

        return initPromise;
    }

    function dispose(): void {
        generation += 1;
        initPromise = null;
        while (unlisteners.length > 0) {
            const fn = unlisteners.pop();
            fn?.();
        }
        loading.value = false;
        initialized.value = false;
    }

    function resetForTests(): void {
        dispose();
        health.value = null;
        settings.value = null;
        catalog.value = null;
        ragSlugs.value = [];
        paneSuggestions.value = {};
        paneRagResults.value = {};
        paneActiveMode.value = {};
        blockDiagnoses.value = {};
        history.value = [];
        lastSelectionByPane.value = {};
        error.value = null;
    }

    return {
        // state
        health,
        settings,
        catalog,
        ragSlugs,
        paneSuggestions,
        paneRagResults,
        paneActiveMode,
        blockDiagnoses,
        history,
        lastSelectionByPane,
        loading,
        error,
        initialized,
        // getters
        status,
        isEnabled,
        isOnline,
        defaultSelection,
        harnesses,
        indicator,
        // selectors
        suggestionForPane,
        ragResultsForPane,
        activeModeForPane,
        diagnosisForBlock,
        // mutations & actions
        setPaneSelection,
        openPanePrompt,
        closePanePrompt,
        setPaneSuggestion,
        patchPaneSuggestion,
        clearPaneSuggestion,
        setPaneRagResults,
        setBlockDiagnosis,
        clearPaneState,
        refreshHealth,
        loadSettings,
        saveSettings,
        loadCatalog,
        loadRagSlugs,
        loadHistory,
        clearHistory,
        clearCache,
        init,
        dispose,
        resetForTests,
        $reset: resetForTests
    };
});
