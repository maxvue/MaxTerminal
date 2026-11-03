import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import { agentsIpc } from '@/ipc/agents';
import { isMtIpcError } from '@/ipc/core';
import type {
    AckReason,
    AgentCloseInspection_Serialize,
    AgentInfo_Serialize,
    AgentKind,
    AgentsSettings_Deserialize,
    AgentsSettings_Serialize,
    AgentTabLink_Serialize,
    AgentTabOpened_Serialize,
    AgentTabOpenRequest_Deserialize,
    AgentTelemetry_Serialize,
    AgentTemplateOptions_Deserialize,
    AgentTemplateOptions_Serialize,
    AttentionSnapshot_Serialize,
    CapabilityReport_Serialize,
    ErrorCode,
    MtError_Serialize,
    ProjectId,
    TabClosedEvent,
    TabId,
    TemplateId,
    WorktreeProposal_Serialize,
    WorktreeResolution
} from '@/types/bindings';

export interface AgentOpenDialogState {
    open: boolean;
    projectId: ProjectId | null;
    agent: AgentKind;
    forceIsolation: boolean;
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
        message: 'Falha inesperada na operação de agentes CLI.',
        details: null
    };
}

function extractClosedTabId(payload: unknown): TabId | null {
    if (typeof payload !== 'object' || payload === null) {
        return null;
    }
    const obj = payload as Record<string, unknown>;
    if (typeof obj.tabId === 'string') {
        return obj.tabId;
    }
    if (typeof obj.tab_id === 'string') {
        return obj.tab_id;
    }
    if (typeof obj.payload === 'object' && obj.payload !== null) {
        const inner = obj.payload as Record<string, unknown>;
        if (typeof inner.tab_id === 'string') {
            return inner.tab_id;
        }
        if (typeof inner.tabId === 'string') {
            return inner.tabId;
        }
    }
    return null;
}

export const useAgentsStore = defineStore('agents', () => {
    const agents = ref<AgentInfo_Serialize[]>([]);
    const settings = ref<AgentsSettings_Serialize | null>(null);
    const telemetryByTab = ref<Record<TabId, AgentTelemetry_Serialize>>({});
    const attentionByTab = ref<Record<TabId, AttentionSnapshot_Serialize>>({});
    const linksByTab = ref<Record<TabId, AgentTabLink_Serialize>>({});
    const pendingWorktrees = ref<AgentCloseInspection_Serialize[]>([]);
    const activeCloseDialogInspection = ref<AgentCloseInspection_Serialize | null>(null);
    const openDialogState = ref<AgentOpenDialogState>({
        open: false,
        projectId: null,
        agent: 'claude-code',
        forceIsolation: false
    });
    const loading = ref<boolean>(false);
    const error = ref<MtError_Serialize | null>(null);
    const initialized = ref<boolean>(false);

    const closedTabTombstones = new Set<TabId>();
    const unlistenFns: UnlistenFn[] = [];
    const revalidatingTelemetryRuns = new Set<string>();
    const revalidatingAttentionRuns = new Set<string>();

    let initPromise: Promise<void> | null = null;
    let hydrating = false;
    let telemetryBuffer: AgentTelemetry_Serialize[] = [];
    let attentionBuffer: AttentionSnapshot_Serialize[] = [];

    const capabilitiesByAgent = computed<Record<AgentKind, CapabilityReport_Serialize | undefined>>(
        () => {
            const map: Record<AgentKind, CapabilityReport_Serialize | undefined> = {
                'claude-code': undefined,
                codex: undefined,
                agy: undefined,
                opencode: undefined
            };
            for (const info of agents.value) {
                map[info.descriptor.kind] = info.capabilities;
            }
            return map;
        }
    );

    function agentByKind(kind: AgentKind): AgentInfo_Serialize | undefined {
        return agents.value.find((a) => a.descriptor.kind === kind);
    }

    function telemetryForTab(tabId: TabId): AgentTelemetry_Serialize | null {
        return telemetryByTab.value[tabId] ?? null;
    }

    function attentionForTab(tabId: TabId): AttentionSnapshot_Serialize | null {
        return attentionByTab.value[tabId] ?? null;
    }

    function linkForTab(tabId: TabId): AgentTabLink_Serialize | null {
        return linksByTab.value[tabId] ?? null;
    }

    function isTabClosed(tabId: TabId): boolean {
        return closedTabTombstones.has(tabId);
    }

    function markTabClosed(tabId: TabId): void {
        closedTabTombstones.add(tabId);
        const nextTelemetry = { ...telemetryByTab.value };
        delete nextTelemetry[tabId];
        telemetryByTab.value = nextTelemetry;

        const nextAttention = { ...attentionByTab.value };
        delete nextAttention[tabId];
        attentionByTab.value = nextAttention;
    }

    function upsertPendingInspection(inspection: AgentCloseInspection_Serialize): void {
        const idx = pendingWorktrees.value.findIndex((item) => item.link.id === inspection.link.id);
        if (idx >= 0) {
            const copy = [...pendingWorktrees.value];
            copy[idx] = inspection;
            pendingWorktrees.value = copy;
        } else {
            pendingWorktrees.value = [inspection, ...pendingWorktrees.value];
        }
        if (inspection.requiresConfirmation) {
            activeCloseDialogInspection.value = inspection;
        }
    }

    /**
     * Aplica um snapshot de telemetria respeitando tombstones, revisão monotônica por `runId`
     * e reconsulta ao backend caso chegue um `runId` diferente do atual (`spec.md` §6.2).
     */
    async function applyTelemetry(incoming: AgentTelemetry_Serialize): Promise<void> {
        if (closedTabTombstones.has(incoming.tabId)) {
            return;
        }
        if (hydrating) {
            telemetryBuffer.push(incoming);
            return;
        }

        const current = telemetryByTab.value[incoming.tabId];
        if (!current) {
            telemetryByTab.value = {
                ...telemetryByTab.value,
                [incoming.tabId]: incoming
            };
            return;
        }

        if (incoming.runId === current.runId) {
            if (incoming.revision > current.revision) {
                telemetryByTab.value = {
                    ...telemetryByTab.value,
                    [incoming.tabId]: incoming
                };
            }
            return;
        }

        const guardKey = `${incoming.tabId}:${incoming.runId}`;
        if (revalidatingTelemetryRuns.has(guardKey)) {
            return;
        }
        revalidatingTelemetryRuns.add(guardKey);
        try {
            const confirmed = await agentsIpc.telemetryGet(incoming.tabId);
            if (!confirmed || closedTabTombstones.has(incoming.tabId)) {
                return;
            }
            const latest = telemetryByTab.value[incoming.tabId];
            if (
                !latest ||
                confirmed.runId !== latest.runId ||
                confirmed.revision > latest.revision
            ) {
                telemetryByTab.value = {
                    ...telemetryByTab.value,
                    [incoming.tabId]: confirmed
                };
            }
        } catch {
            // Mantém snapshot atual se a reconsulta falhar.
        } finally {
            revalidatingTelemetryRuns.delete(guardKey);
        }
    }

    /**
     * Aplica um snapshot de atenção respeitando tombstones, revisão monotônica por `runId`
     * e reconsulta ao backend quando `runId` diverge (`spec.md` §6.2).
     */
    async function applyAttention(incoming: AttentionSnapshot_Serialize): Promise<void> {
        if (closedTabTombstones.has(incoming.tabId)) {
            return;
        }
        if (hydrating) {
            attentionBuffer.push(incoming);
            return;
        }

        const current = attentionByTab.value[incoming.tabId];
        if (!current) {
            attentionByTab.value = {
                ...attentionByTab.value,
                [incoming.tabId]: incoming
            };
            return;
        }

        if (incoming.runId === current.runId) {
            if (incoming.revision > current.revision) {
                attentionByTab.value = {
                    ...attentionByTab.value,
                    [incoming.tabId]: incoming
                };
            }
            return;
        }

        const guardKey = `${incoming.tabId}:${incoming.runId}`;
        if (revalidatingAttentionRuns.has(guardKey)) {
            return;
        }
        revalidatingAttentionRuns.add(guardKey);
        try {
            const confirmed = await agentsIpc.attentionGet(incoming.tabId);
            if (!confirmed || closedTabTombstones.has(incoming.tabId)) {
                return;
            }
            const latest = attentionByTab.value[incoming.tabId];
            if (
                !latest ||
                confirmed.runId !== latest.runId ||
                confirmed.revision > latest.revision
            ) {
                attentionByTab.value = {
                    ...attentionByTab.value,
                    [incoming.tabId]: confirmed
                };
            }
        } catch {
            // Mantém estado atual se a reconsulta falhar.
        } finally {
            revalidatingAttentionRuns.delete(guardKey);
        }
    }

    /**
     * Inicializa a store registrando listeners IPC antes de buscar os snapshots iniciais
     * e drenando eventos bufferizados sem condição de corrida (`spec.md` §6.2).
     */
    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        if (initPromise) {
            return initPromise;
        }

        initPromise = (async () => {
            loading.value = true;
            error.value = null;
            hydrating = true;
            telemetryBuffer = [];
            attentionBuffer = [];

            try {
                const subs = await Promise.all([
                    agentsIpc.onInstallationsChanged((nextAgents) => {
                        agents.value = nextAgents;
                    }),
                    agentsIpc.onSettingsChanged((nextSettings) => {
                        settings.value = nextSettings;
                    }),
                    agentsIpc.onTelemetry((snap) => {
                        void applyTelemetry(snap);
                    }),
                    agentsIpc.onAttention((snap) => {
                        void applyAttention(snap);
                    }),
                    agentsIpc.onLinkChanged((link) => {
                        linksByTab.value = {
                            ...linksByTab.value,
                            [link.tabId]: link
                        };
                        if (link.resolution !== null) {
                            pendingWorktrees.value = pendingWorktrees.value.filter(
                                (item) => item.link.id !== link.id
                            );
                        }
                    }),
                    agentsIpc.onWorktreeClosePending((inspection) => {
                        upsertPendingInspection(inspection);
                    }),
                    listen<TabClosedEvent>('tab://closed', (ev) => {
                        const closedId = extractClosedTabId(ev.payload);
                        if (closedId) {
                            markTabClosed(closedId);
                        }
                    })
                ]);

                unlistenFns.push(...subs);

                const [agentListRes, settingsRes, telemetryListRes, pendingRes] = await Promise.all([
                    agentsIpc.list(),
                    agentsIpc.settingsGet(),
                    agentsIpc.telemetryList(),
                    agentsIpc.worktreePendingList(null)
                ]);

                agents.value = agentListRes;
                settings.value = settingsRes;
                pendingWorktrees.value = pendingRes;

                const nextTelemetryMap: Record<TabId, AgentTelemetry_Serialize> = {
                    ...telemetryByTab.value
                };
                for (const item of telemetryListRes) {
                    if (!closedTabTombstones.has(item.tabId)) {
                        nextTelemetryMap[item.tabId] = item;
                    }
                }
                telemetryByTab.value = nextTelemetryMap;

                hydrating = false;

                const queuedTelemetry = telemetryBuffer.splice(0, telemetryBuffer.length);
                for (const item of queuedTelemetry) {
                    await applyTelemetry(item);
                }

                const queuedAttention = attentionBuffer.splice(0, attentionBuffer.length);
                for (const item of queuedAttention) {
                    await applyAttention(item);
                }

                initialized.value = true;
            } catch (err: unknown) {
                hydrating = false;
                telemetryBuffer = [];
                attentionBuffer = [];
                error.value = toMtError(err);
                throw err;
            } finally {
                loading.value = false;
                initPromise = null;
            }
        })();

        return initPromise;
    }

    async function refreshInstallations(): Promise<AgentInfo_Serialize[]> {
        error.value = null;
        try {
            const list = await agentsIpc.detectRefresh();
            agents.value = list;
            return list;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function reloadDescriptors(): Promise<AgentInfo_Serialize[]> {
        error.value = null;
        try {
            const list = await agentsIpc.descriptorsReload();
            agents.value = list;
            return list;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function updateSettings(
        nextSettings: AgentsSettings_Deserialize
    ): Promise<AgentsSettings_Serialize> {
        error.value = null;
        try {
            const saved = await agentsIpc.settingsUpdate(nextSettings);
            settings.value = saved;
            return saved;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function ensureTemplates(projectId: ProjectId): Promise<number> {
        error.value = null;
        try {
            return await agentsIpc.templatesEnsure(projectId);
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function getTemplateOptions(
        templateId: TemplateId
    ): Promise<AgentTemplateOptions_Serialize> {
        error.value = null;
        try {
            return await agentsIpc.templateOptionsGet(templateId);
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function setTemplateOptions(
        templateId: TemplateId,
        options: AgentTemplateOptions_Deserialize
    ): Promise<AgentTemplateOptions_Serialize> {
        error.value = null;
        try {
            return await agentsIpc.templateOptionsSet(templateId, options);
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function proposeWorktree(
        projectId: ProjectId,
        agent: AgentKind,
        hint: string | null = null
    ): Promise<WorktreeProposal_Serialize> {
        error.value = null;
        try {
            return await agentsIpc.worktreePropose(projectId, agent, hint);
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function openAgent(
        request: AgentTabOpenRequest_Deserialize
    ): Promise<AgentTabOpened_Serialize> {
        error.value = null;
        try {
            const opened = await agentsIpc.tabOpen(request);
            closedTabTombstones.delete(opened.tab.id);
            if (opened.link) {
                linksByTab.value = {
                    ...linksByTab.value,
                    [opened.tab.id]: opened.link
                };
            }
            return opened;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    function openAgentDialog(
        projectId: ProjectId,
        agent: AgentKind,
        forceIsolation = false
    ): void {
        openDialogState.value = {
            open: true,
            projectId,
            agent,
            forceIsolation
        };
    }

    function closeAgentDialog(): void {
        openDialogState.value = {
            ...openDialogState.value,
            open: false
        };
    }

    async function ackAttention(
        tabId: TabId,
        runId: string,
        episode: number,
        reason: AckReason
    ): Promise<AttentionSnapshot_Serialize | null> {
        error.value = null;
        try {
            const updated = await agentsIpc.attentionAck(tabId, runId, episode, reason);
            if (updated && !closedTabTombstones.has(tabId)) {
                attentionByTab.value = {
                    ...attentionByTab.value,
                    [tabId]: updated
                };
            }
            return updated;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function inspectClose(tabId: TabId): Promise<AgentCloseInspection_Serialize | null> {
        error.value = null;
        try {
            const inspection = await agentsIpc.tabCloseCheck(tabId);
            if (inspection) {
                upsertPendingInspection(inspection);
            }
            return inspection;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function resolveWorktree(
        linkId: number,
        action: WorktreeResolution,
        inspectionToken: string
    ): Promise<AgentTabLink_Serialize> {
        error.value = null;
        try {
            const link = await agentsIpc.worktreeResolve(linkId, action, inspectionToken);
            linksByTab.value = {
                ...linksByTab.value,
                [link.tabId]: link
            };
            pendingWorktrees.value = pendingWorktrees.value.filter(
                (item) => item.link.id !== linkId
            );
            if (activeCloseDialogInspection.value?.link.id === linkId) {
                activeCloseDialogInspection.value = null;
            }
            return link;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function loadPendingWorktrees(
        projectId: ProjectId | null = null
    ): Promise<AgentCloseInspection_Serialize[]> {
        error.value = null;
        try {
            const list = await agentsIpc.worktreePendingList(projectId);
            pendingWorktrees.value = list;
            return list;
        } catch (err: unknown) {
            error.value = toMtError(err);
            throw err;
        }
    }

    async function fetchTabTelemetry(tabId: TabId): Promise<AgentTelemetry_Serialize | null> {
        if (closedTabTombstones.has(tabId)) {
            return null;
        }
        const snap = await agentsIpc.telemetryGet(tabId);
        if (snap && !closedTabTombstones.has(tabId)) {
            telemetryByTab.value = {
                ...telemetryByTab.value,
                [tabId]: snap
            };
        }
        return snap;
    }

    async function fetchTabAttention(tabId: TabId): Promise<AttentionSnapshot_Serialize | null> {
        if (closedTabTombstones.has(tabId)) {
            return null;
        }
        const snap = await agentsIpc.attentionGet(tabId);
        if (snap && !closedTabTombstones.has(tabId)) {
            attentionByTab.value = {
                ...attentionByTab.value,
                [tabId]: snap
            };
        }
        return snap;
    }

    async function fetchTabLink(tabId: TabId): Promise<AgentTabLink_Serialize | null> {
        const link = await agentsIpc.tabLinkGet(tabId);
        if (link) {
            linksByTab.value = {
                ...linksByTab.value,
                [tabId]: link
            };
        }
        return link;
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
        hydrating = false;
        telemetryBuffer = [];
        attentionBuffer = [];
        revalidatingTelemetryRuns.clear();
        revalidatingAttentionRuns.clear();
    }

    return {
        agents,
        settings,
        capabilitiesByAgent,
        telemetryByTab,
        attentionByTab,
        linksByTab,
        pendingWorktrees,
        activeCloseDialogInspection,
        openDialogState,
        loading,
        error,
        initialized,
        agentByKind,
        telemetryForTab,
        attentionForTab,
        linkForTab,
        isTabClosed,
        markTabClosed,
        applyTelemetry,
        applyAttention,
        init,
        refreshInstallations,
        reloadDescriptors,
        updateSettings,
        ensureTemplates,
        getTemplateOptions,
        setTemplateOptions,
        proposeWorktree,
        openAgent,
        openAgentDialog,
        closeAgentDialog,
        ackAttention,
        inspectClose,
        resolveWorktree,
        loadPendingWorktrees,
        fetchTabTelemetry,
        fetchTabAttention,
        fetchTabLink,
        dispose
    };
});
