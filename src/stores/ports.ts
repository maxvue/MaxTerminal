import { defineStore } from 'pinia';
import { ref } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import * as portsIpc from '@/ipc/ports';
import type {
    ListeningPort_Serialize,
    PaneId,
    PollMode,
    PortClosed,
    PortId,
    PortKillResult,
    PortOpened_Serialize,
    PortUpdated_Serialize,
    TabId
} from '@/types/bindings';

export const usePortsStore = defineStore('ports', () => {
    const portsByTab = ref<Record<TabId, ListeningPort_Serialize[]>>({});
    const revisionByTab = ref<Record<TabId, number>>({});
    const killingByPortId = ref<Record<PortId, boolean>>({});
    const pollMode = ref<PollMode>('active');
    const error = ref<string | null>(null);

    let unlistenOpened: UnlistenFn | null = null;
    let unlistenUpdated: UnlistenFn | null = null;
    let unlistenClosed: UnlistenFn | null = null;

    function sortPorts(list: ListeningPort_Serialize[]): ListeningPort_Serialize[] {
        return [...list].sort((a, b) => {
            if (a.port !== b.port) {
                return a.port - b.port;
            }
            return a.paneId.localeCompare(b.paneId);
        });
    }

    function portsForTab(tabId: TabId): ListeningPort_Serialize[] {
        return portsByTab.value[tabId] ?? [];
    }

    function portsForPane(tabId: TabId, paneId: PaneId): ListeningPort_Serialize[] {
        return portsForTab(tabId).filter((p) => p.paneId === paneId);
    }

    function applyOpened(payload: PortOpened_Serialize): void {
        const currentRev = revisionByTab.value[payload.tabId] ?? 0;
        if (payload.revision < currentRev) {
            return;
        }
        revisionByTab.value[payload.tabId] = payload.revision;
        const list = portsByTab.value[payload.tabId] ?? [];
        const filtered = list.filter((p) => p.id !== payload.port.id);
        filtered.push(payload.port);
        portsByTab.value[payload.tabId] = sortPorts(filtered);
    }

    function applyUpdated(payload: PortUpdated_Serialize): void {
        const currentRev = revisionByTab.value[payload.tabId] ?? 0;
        if (payload.revision < currentRev) {
            return;
        }
        revisionByTab.value[payload.tabId] = payload.revision;
        const list = portsByTab.value[payload.tabId] ?? [];
        const filtered = list.filter((p) => p.id !== payload.port.id);
        filtered.push(payload.port);
        portsByTab.value[payload.tabId] = sortPorts(filtered);
    }

    function applyClosed(payload: PortClosed): void {
        const currentRev = revisionByTab.value[payload.tabId] ?? 0;
        if (payload.revision < currentRev) {
            return;
        }
        revisionByTab.value[payload.tabId] = payload.revision;
        const list = portsByTab.value[payload.tabId] ?? [];
        portsByTab.value[payload.tabId] = list.filter((p) => p.id !== payload.id);
        delete killingByPortId.value[payload.id];
    }

    async function loadForTab(tabId: TabId): Promise<ListeningPort_Serialize[]> {
        try {
            const snap = await portsIpc.portsListForTab(tabId);
            const currentRev = revisionByTab.value[tabId] ?? 0;
            if (snap.revision >= currentRev) {
                revisionByTab.value[tabId] = snap.revision;
                portsByTab.value[tabId] = sortPorts(snap.ports);
            }
            return portsForTab(tabId);
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            return [];
        }
    }

    async function openInBrowser(paneId: PaneId, port: number): Promise<void> {
        error.value = null;
        try {
            await portsIpc.portsOpenBrowser(paneId, port);
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            throw err;
        }
    }

    async function killPort(
        tabId: TabId,
        paneId: PaneId,
        port: number,
        expectedPid: number | null = null
    ): Promise<PortKillResult> {
        const id: PortId = `${paneId}:tcp:${port}`;
        killingByPortId.value[id] = true;
        error.value = null;
        try {
            const res = await portsIpc.portsKill(paneId, port, expectedPid);
            const list = portsByTab.value[tabId] ?? [];
            portsByTab.value[tabId] = list.filter((p) => p.id !== id);
            return res;
        } catch (err) {
            error.value = err instanceof Error ? err.message : String(err);
            throw err;
        } finally {
            delete killingByPortId.value[id];
        }
    }

    async function setPollMode(mode: PollMode): Promise<void> {
        pollMode.value = mode;
        try {
            await portsIpc.portsSetPollMode(mode);
        } catch {
            // Ignora quando backend de teste não provê comando.
        }
    }

    function removeTab(tabId: TabId): void {
        delete portsByTab.value[tabId];
        delete revisionByTab.value[tabId];
    }

    async function startListening(): Promise<void> {
        if (!unlistenOpened) {
            unlistenOpened = await portsIpc.onPortOpened((p) => applyOpened(p));
        }
        if (!unlistenUpdated) {
            unlistenUpdated = await portsIpc.onPortUpdated((p) => applyUpdated(p));
        }
        if (!unlistenClosed) {
            unlistenClosed = await portsIpc.onPortClosed((p) => applyClosed(p));
        }
    }

    function stopListening(): void {
        if (unlistenOpened) {
            unlistenOpened();
            unlistenOpened = null;
        }
        if (unlistenUpdated) {
            unlistenUpdated();
            unlistenUpdated = null;
        }
        if (unlistenClosed) {
            unlistenClosed();
            unlistenClosed = null;
        }
    }

    function resetForTests(): void {
        stopListening();
        portsByTab.value = {};
        revisionByTab.value = {};
        killingByPortId.value = {};
        pollMode.value = 'active';
        error.value = null;
    }

    return {
        portsByTab,
        revisionByTab,
        killingByPortId,
        pollMode,
        error,
        portsForTab,
        portsForPane,
        applyOpened,
        applyUpdated,
        applyClosed,
        loadForTab,
        openInBrowser,
        killPort,
        setPollMode,
        removeTab,
        startListening,
        stopListening,
        resetForTests
    };
});
