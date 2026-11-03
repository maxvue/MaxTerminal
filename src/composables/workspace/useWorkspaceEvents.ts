import type { UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentScope, onScopeDispose } from 'vue';
import * as sessionsIpc from '@/ipc/sessions';
import * as tabsIpc from '@/ipc/tabs';
import * as templatesIpc from '@/ipc/templates';
import * as workspaceIpc from '@/ipc/workspace';
import { useSessionsStore } from '@/stores/sessions';
import { useTabsStore } from '@/stores/tabs';
import { useTemplatesStore } from '@/stores/templates';
import type {
    PaneExitedPayload_Serialize,
    PanePtyReplacedPayload_Serialize,
    SessionSummary_Serialize,
    TabClosedPayload,
    TabReorderedPayload,
    TabView_Serialize,
    TemplateChangedPayload,
    WorkspaceEvent,
    WorkspaceRestoreReport_Serialize
} from '@/types/bindings';

type QueuedWorkspaceItem =
    | { kind: 'tab_opened'; event: WorkspaceEvent<TabView_Serialize> }
    | { kind: 'tab_closed'; event: WorkspaceEvent<TabClosedPayload> }
    | { kind: 'tab_reordered'; event: WorkspaceEvent<TabReorderedPayload> }
    | { kind: 'tab_layout'; event: WorkspaceEvent<TabView_Serialize> }
    | { kind: 'pane_pty_replaced'; event: WorkspaceEvent<PanePtyReplacedPayload_Serialize> }
    | { kind: 'pane_exited'; event: WorkspaceEvent<PaneExitedPayload_Serialize> }
    | { kind: 'session_started'; event: WorkspaceEvent<SessionSummary_Serialize> }
    | { kind: 'session_archived'; event: WorkspaceEvent<SessionSummary_Serialize> }
    | { kind: 'template_changed'; event: WorkspaceEvent<TemplateChangedPayload> }
    | { kind: 'workspace_restored'; event: WorkspaceEvent<WorkspaceRestoreReport_Serialize> };

export interface UseWorkspaceEventsReturn {
    start: () => Promise<void>;
    stop: () => void;
    resync: () => Promise<void>;
}

/**
 * Gerencia a inscrição nos eventos Tauri do subsistema de workspace (Fase 07) seguindo o protocolo:
 * 1. Inscreve os listeners e bufferiza eventos recebidos;
 * 2. Invoca `workspace_snapshot()` para hidratar a `useTabsStore`;
 * 3. Reproduz em ordem estrita de `event_seq` apenas eventos com `event_seq > snapshot.eventSeq`;
 * 4. Em caso de salto em `event_seq` ou mudança de `boot_id`, executa `resync()` automático.
 */
export function useWorkspaceEvents(): UseWorkspaceEventsReturn {
    const tabsStore = useTabsStore();
    const templatesStore = useTemplatesStore();
    const sessionsStore = useSessionsStore();

    let unlisteners: UnlistenFn[] = [];
    let startPromise: Promise<void> | null = null;
    let resyncPromise: Promise<void> | null = null;
    let hydrating = false;
    let started = false;
    let disposed = false;
    let generation = 0;
    const preHydrationBuffer: QueuedWorkspaceItem[] = [];

    function dispatchItem(item: QueuedWorkspaceItem): void {
        switch (item.kind) {
            case 'tab_opened':
                tabsStore.applyTabUpsert(item.event.payload, false);
                break;
            case 'tab_closed':
                tabsStore.applyTabClosed(item.event.payload);
                break;
            case 'tab_reordered':
                tabsStore.applyTabReordered(item.event.payload);
                break;
            case 'tab_layout':
                tabsStore.applyTabUpsert(item.event.payload, false);
                break;
            case 'pane_pty_replaced':
                tabsStore.applyPanePtyReplaced(item.event.payload);
                break;
            case 'pane_exited':
                tabsStore.applyPaneExited(item.event.payload);
                break;
            case 'session_started':
                sessionsStore.handleSessionStarted(item.event.payload);
                break;
            case 'session_archived':
                sessionsStore.handleSessionArchived(item.event.payload);
                break;
            case 'template_changed':
                templatesStore.handleTemplateChangedEvent(item.event.payload);
                break;
            case 'workspace_restored':
                tabsStore.restored = true;
                tabsStore.restoreReport = item.event.payload;
                break;
        }
    }

    function handleIncoming(item: QueuedWorkspaceItem): void {
        if (disposed) {
            return;
        }
        if (hydrating) {
            preHydrationBuffer.push(item);
            return;
        }

        const ev = item.event;
        if (tabsStore.bootId !== null && ev.boot_id !== tabsStore.bootId) {
            void resync();
            return;
        }

        if (ev.event_seq <= tabsStore.lastEventSeq) {
            return;
        }

        if (tabsStore.lastEventSeq > 0 && ev.event_seq > tabsStore.lastEventSeq + 1) {
            // Detectado salto de sequência: aplica o evento e dispara resync de reconciliação
            tabsStore.lastEventSeq = ev.event_seq;
            dispatchItem(item);
            void resync();
            return;
        }

        tabsStore.lastEventSeq = ev.event_seq;
        dispatchItem(item);
    }

    async function resync(): Promise<void> {
        if (disposed) {
            return;
        }
        if (resyncPromise !== null) {
            return resyncPromise;
        }

        hydrating = true;
        resyncPromise = (async () => {
            try {
                const snap = await tabsStore.refreshSnapshot();
                if (disposed) {
                    return;
                }
                const drained = preHydrationBuffer.splice(0, preHydrationBuffer.length);
                drained.sort((a, b) => a.event.event_seq - b.event.event_seq);
                for (const item of drained) {
                    if (
                        item.event.boot_id === snap.bootId &&
                        item.event.event_seq > tabsStore.lastEventSeq
                    ) {
                        tabsStore.lastEventSeq = item.event.event_seq;
                        dispatchItem(item);
                    }
                }
            } catch {
                // Em ambientes de teste sem mock de workspace_snapshot, mantém estado seguro.
                preHydrationBuffer.splice(0, preHydrationBuffer.length);
            } finally {
                hydrating = false;
                resyncPromise = null;
            }
        })();

        return resyncPromise;
    }

    function safeUnlistenAll(list: UnlistenFn[]): void {
        for (const fn of list) {
            try {
                fn();
            } catch {
                // Ignora erro ao liberar listener.
            }
        }
    }

    async function start(): Promise<void> {
        if (disposed || started) {
            return;
        }
        if (startPromise !== null) {
            return startPromise;
        }

        const myGen = ++generation;
        const localUnlisteners: UnlistenFn[] = [];
        hydrating = true;

        startPromise = (async () => {
            try {
                const regs = await Promise.all([
                    tabsIpc.onTabOpened((event) =>
                        handleIncoming({ kind: 'tab_opened', event })
                    ),
                    tabsIpc.onTabClosed((event) =>
                        handleIncoming({ kind: 'tab_closed', event })
                    ),
                    tabsIpc.onTabReordered((event) =>
                        handleIncoming({ kind: 'tab_reordered', event })
                    ),
                    tabsIpc.onTabLayoutChanged((event) =>
                        handleIncoming({ kind: 'tab_layout', event })
                    ),
                    tabsIpc.onPanePtyReplaced((event) =>
                        handleIncoming({ kind: 'pane_pty_replaced', event })
                    ),
                    tabsIpc.onPaneExited((event) =>
                        handleIncoming({ kind: 'pane_exited', event })
                    ),
                    sessionsIpc.onSessionStarted((event) =>
                        handleIncoming({ kind: 'session_started', event })
                    ),
                    sessionsIpc.onSessionArchived((event) =>
                        handleIncoming({ kind: 'session_archived', event })
                    ),
                    templatesIpc.onTemplateChanged((event) =>
                        handleIncoming({ kind: 'template_changed', event })
                    ),
                    workspaceIpc.onWorkspaceRestored((event) =>
                        handleIncoming({ kind: 'workspace_restored', event })
                    )
                ]);

                localUnlisteners.push(...regs);

                if (disposed || myGen !== generation) {
                    safeUnlistenAll(localUnlisteners);
                    return;
                }

                unlisteners = localUnlisteners;
                await resync();
                if (!disposed && myGen === generation) {
                    started = true;
                }
            } catch (err: unknown) {
                hydrating = false;
                safeUnlistenAll(localUnlisteners);
                unlisteners = [];
                throw err;
            } finally {
                if (myGen === generation) {
                    startPromise = null;
                }
            }
        })();

        return startPromise;
    }

    function stop(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        started = false;
        hydrating = false;
        generation += 1;
        startPromise = null;
        preHydrationBuffer.splice(0, preHydrationBuffer.length);
        const toClean = unlisteners.splice(0, unlisteners.length);
        safeUnlistenAll(toClean);
    }

    if (getCurrentScope()) {
        onScopeDispose(stop);
    }

    return {
        start,
        stop,
        resync
    };
}
