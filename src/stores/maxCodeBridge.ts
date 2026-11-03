import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { useIpcError } from '@/composables/useIpcError';
import {
    bridgeIpc,
    onBridgeBugCycleProgress,
    onBridgeError,
    onBridgeProjectSynced,
    onBridgeStatusChanged,
    onBridgeTerminalOpened
} from '@/ipc/bridge';
import type {
    BridgeConnectionState,
    BridgeError,
    BridgeProjectSyncedEvent,
    BridgeSettingsPatch_Deserialize,
    BridgeSettings_Serialize,
    BridgeStatusSnapshot_Serialize,
    BridgeTerminalOpenedEvent,
    BugCycleProgressPayload_Serialize,
    BugsCycleAckPayload_Serialize,
    BugsCycleStartPayload_Deserialize,
    InspectDispatchResult_Serialize,
    IssueActionDispatchOutcome_Serialize,
    IssueCommandPayload_Deserialize,
    IssueProtocolSupport,
    OpenFilePayload_Deserialize
} from '@/types/bindings';

export interface BridgeIndicator {
    color: 'success' | 'info' | 'danger' | 'neutral';
    label: string;
}

const DEFAULT_SETTINGS: BridgeSettings_Serialize = {
    enabled: true,
    autoSyncActiveProject: true,
    autoRegisterUnknownProject: true,
    focusWindowOnIncomingCommand: true,
    inspectFallbackMode: 'uri_scheme',
    preferredPort: null
};

const DEFAULT_STATUS: BridgeStatusSnapshot_Serialize = {
    state: 'disconnected',
    serverPort: null,
    peerInstanceId: null,
    peerPid: null,
    peerVersion: null,
    peerCapabilities: [],
    lastRttMs: null,
    lastSeenMs: null,
    lastError: null
};

export const useMaxCodeBridgeStore = defineStore('maxCodeBridge', () => {
    const ipcError = useIpcError();

    const status = ref<BridgeStatusSnapshot_Serialize>({ ...DEFAULT_STATUS });
    const settings = ref<BridgeSettings_Serialize>({ ...DEFAULT_SETTINGS });
    const issueSupport = ref<IssueProtocolSupport>({
        issuesProtocolV1: false,
        bugsCycleV1: false
    });
    const pendingBlockIds = ref<Set<string>>(new Set());
    const lastInspectResult = ref<InspectDispatchResult_Serialize | null>(null);
    const lastSyncedProject = ref<BridgeProjectSyncedEvent | null>(null);
    const lastOpenedTerminal = ref<BridgeTerminalOpenedEvent | null>(null);
    const activeBugCycles = ref<Record<string, BugCycleProgressPayload_Serialize>>({});
    const lastError = ref<string | null>(null);
    const initialized = ref<boolean>(false);

    let generation = 0;
    let initPromise: Promise<void> | null = null;
    const unlisteners: UnlistenFn[] = [];
    const inFlightInspects = new Map<string, Promise<InspectDispatchResult_Serialize>>();

    const state = computed<BridgeConnectionState>(() => status.value?.state ?? 'disconnected');
    const isConnected = computed<boolean>(() => status.value?.state === 'connected');
    const serverPort = computed<number | null>(() => status.value?.serverPort ?? null);
    const peerCapabilities = computed<string[]>(() => status.value?.peerCapabilities ?? []);
    const syncEnabled = computed<boolean>(
        () => Boolean(settings.value?.enabled && settings.value?.autoSyncActiveProject)
    );

    const indicator = computed<BridgeIndicator>(() => {
        if (!settings.value?.enabled || status.value?.state === 'disabled') {
            return { color: 'neutral', label: 'Ponte desativada' };
        }
        if (status.value?.state === 'connected') {
            return { color: 'success', label: 'MaxCode conectado' };
        }
        if (status.value?.lastError) {
            return { color: 'danger', label: 'Ponte indisponível' };
        }
        return { color: 'info', label: 'Aguardando MaxCode' };
    });

    function isInspecting(blockId: string): boolean {
        return pendingBlockIds.value.has(blockId);
    }

    function applyStatus(incoming: BridgeStatusSnapshot_Serialize | null | undefined): void {
        if (!incoming || typeof incoming !== 'object') {
            return;
        }
        const caps = Array.isArray(incoming.peerCapabilities)
            ? [...incoming.peerCapabilities]
            : [];
        status.value = {
            ...DEFAULT_STATUS,
            ...incoming,
            peerCapabilities: caps
        };
        issueSupport.value = {
            issuesProtocolV1: caps.includes('issues_protocol_v1'),
            bugsCycleV1: caps.includes('bugs_cycle_v1')
        };
    }

    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        if (initPromise !== null) {
            return initPromise;
        }

        const currentGen = ++generation;
        initPromise = (async () => {
            try {
                const listeners = await Promise.all([
                    onBridgeStatusChanged((nextStatus) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        applyStatus(nextStatus);
                    }),
                    onBridgeProjectSynced((ev) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        lastSyncedProject.value = ev;
                    }),
                    onBridgeTerminalOpened((ev) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        lastOpenedTerminal.value = ev;
                    }),
                    onBridgeBugCycleProgress((ev) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        activeBugCycles.value = {
                            ...activeBugCycles.value,
                            [ev.cycleId]: ev
                        };
                    }),
                    onBridgeError((err: BridgeError) => {
                        if (currentGen !== generation) {
                            return;
                        }
                        lastError.value = `${err.code}: ${err.message}`;
                    })
                ]);

                if (currentGen !== generation) {
                    for (const unlisten of listeners) {
                        unlisten();
                    }
                    return;
                }

                unlisteners.push(...listeners);

                const [fetchedStatus, fetchedSettings, fetchedSupport] = await Promise.all([
                    bridgeIpc.status(),
                    bridgeIpc.getSettings(),
                    bridgeIpc.issueProtocolSupport().catch(() => ({
                        issuesProtocolV1: false,
                        bugsCycleV1: false
                    }))
                ]);

                if (currentGen !== generation) {
                    return;
                }

                if (fetchedSettings && typeof fetchedSettings === 'object') {
                    settings.value = {
                        ...DEFAULT_SETTINGS,
                        ...fetchedSettings
                    };
                }
                applyStatus(fetchedStatus);
                const caps = status.value?.peerCapabilities ?? [];
                issueSupport.value = {
                    issuesProtocolV1:
                        Boolean(fetchedSupport?.issuesProtocolV1) ||
                        caps.includes('issues_protocol_v1'),
                    bugsCycleV1:
                        Boolean(fetchedSupport?.bugsCycleV1) ||
                        caps.includes('bugs_cycle_v1')
                };
                initialized.value = true;
            } catch (err: unknown) {
                if (currentGen === generation) {
                    lastError.value = ipcError.message(err);
                }
                throw err;
            } finally {
                if (currentGen === generation) {
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
            const unlisten = unlisteners.pop();
            if (unlisten) {
                unlisten();
            }
        }
        inFlightInspects.clear();
        pendingBlockIds.value = new Set();
        initialized.value = false;
    }

    async function refreshStatus(): Promise<BridgeStatusSnapshot_Serialize> {
        const snap = await bridgeIpc.status();
        applyStatus(snap);
        return snap;
    }

    async function updateSettings(
        patch: BridgeSettingsPatch_Deserialize
    ): Promise<BridgeSettings_Serialize> {
        const previous = { ...settings.value };
        lastError.value = null;
        try {
            const updated = await bridgeIpc.updateSettings(patch);
            settings.value = updated;
            const snap = await bridgeIpc.status();
            applyStatus(snap);
            return updated;
        } catch (err: unknown) {
            settings.value = previous;
            lastError.value = ipcError.message(err);
            throw err;
        }
    }

    async function reconnectNow(): Promise<BridgeStatusSnapshot_Serialize> {
        lastError.value = null;
        try {
            const snap = await bridgeIpc.reconnectNow();
            applyStatus(snap);
            return snap;
        } catch (err: unknown) {
            lastError.value = ipcError.message(err);
            throw err;
        }
    }

    async function inspectCommandError(blockId: string): Promise<InspectDispatchResult_Serialize> {
        const existing = inFlightInspects.get(blockId);
        if (existing) {
            return existing;
        }

        const nextPending = new Set(pendingBlockIds.value);
        nextPending.add(blockId);
        pendingBlockIds.value = nextPending;
        lastError.value = null;

        const promise = (async (): Promise<InspectDispatchResult_Serialize> => {
            try {
                const res = await bridgeIpc.inspectCommandError(blockId);
                if (
                    res.transport === 'clipboard_only' &&
                    typeof navigator !== 'undefined' &&
                    navigator.clipboard?.writeText
                ) {
                    await navigator.clipboard.writeText(res.formattedPrompt).catch(() => {});
                }
                lastInspectResult.value = res;
                return res;
            } catch (err: unknown) {
                lastError.value = ipcError.message(err);
                throw err;
            } finally {
                inFlightInspects.delete(blockId);
                const updatedPending = new Set(pendingBlockIds.value);
                updatedPending.delete(blockId);
                pendingBlockIds.value = updatedPending;
            }
        })();

        inFlightInspects.set(blockId, promise);
        return promise;
    }

    async function notifyProjectSelected(
        projectPath: string,
        projectName: string | null = null,
        gitBranch: string | null = null
    ): Promise<boolean> {
        if (!syncEnabled.value) {
            return false;
        }
        try {
            return await bridgeIpc.notifyProjectSelected(projectPath, projectName, gitBranch);
        } catch (err: unknown) {
            lastError.value = ipcError.message(err);
            return false;
        }
    }

    async function openFileInMaxCode(payload: OpenFilePayload_Deserialize): Promise<boolean> {
        lastError.value = null;
        try {
            return await bridgeIpc.openFileInMaxCode(payload);
        } catch (err: unknown) {
            lastError.value = ipcError.message(err);
            throw err;
        }
    }

    async function sendIssueCommand(
        payload: IssueCommandPayload_Deserialize
    ): Promise<IssueActionDispatchOutcome_Serialize> {
        lastError.value = null;
        try {
            return await bridgeIpc.sendIssueCommand(payload);
        } catch (err: unknown) {
            lastError.value = ipcError.message(err);
            throw err;
        }
    }

    async function previewBugsCycle(
        payload: BugsCycleStartPayload_Deserialize
    ): Promise<BugsCycleAckPayload_Serialize> {
        lastError.value = null;
        try {
            return await bridgeIpc.bugsCyclePreview(payload);
        } catch (err: unknown) {
            lastError.value = ipcError.message(err);
            throw err;
        }
    }

    async function startBugsCycle(
        payload: BugsCycleStartPayload_Deserialize
    ): Promise<BugsCycleAckPayload_Serialize> {
        lastError.value = null;
        try {
            return await bridgeIpc.bugsCycleStart(payload);
        } catch (err: unknown) {
            lastError.value = ipcError.message(err);
            throw err;
        }
    }

    async function openPlan(
        prompt: string,
        projectPath: string,
        _source = 'artisan_create'
    ): Promise<void> {
        lastError.value = null;
        if (
            typeof navigator !== 'undefined' &&
            navigator.clipboard?.writeText
        ) {
            await navigator.clipboard.writeText(prompt).catch(() => {});
        }
        if (syncEnabled.value && projectPath) {
            await bridgeIpc.notifyProjectSelected(projectPath, null, null).catch(() => false);
        }
    }

    return {
        status,
        settings,
        issueSupport,
        pendingBlockIds,
        lastInspectResult,
        lastSyncedProject,
        lastOpenedTerminal,
        activeBugCycles,
        lastError,
        initialized,
        state,
        isConnected,
        serverPort,
        peerCapabilities,
        syncEnabled,
        indicator,
        isInspecting,
        init,
        dispose,
        refreshStatus,
        updateSettings,
        reconnectNow,
        inspectCommandError,
        inspect: inspectCommandError,
        notifyProjectSelected,
        openFileInMaxCode,
        sendIssueCommand,
        previewBugsCycle,
        startBugsCycle,
        openPlan
    };
});
