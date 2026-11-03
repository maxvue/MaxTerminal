import { defineStore } from 'pinia';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    onTabStateChanged,
    onTabStateRemoved,
    tabMarkSeen,
    tabSetContinuous,
    tabSetFocus,
    tabStateGet,
    tabStateList
} from '@/ipc/tabState';
import type {
    CommandBlockId,
    ContinuousPolicy,
    FocusSnapshot_Deserialize,
    PaneId,
    PaneStateDto_Serialize,
    SeenRequest_Deserialize,
    TabId,
    TabStateRemovedEvent,
    TabStateSnapshot_Serialize,
    TabStatus
} from '@/types/bindings';

/** Limite máximo de eventos retidos no buffer temporário durante o bootstrap `init()`. */
const MAX_BUFFERED_TAB_EVENTS = 512;

type BufferedTabEvent =
    | { type: 'changed'; payload: TabStateSnapshot_Serialize }
    | { type: 'removed'; payload: TabStateRemovedEvent };

/** Contadores agregados por status semântico de abas (usado na coluna de projetos da Fase 06). */
export interface TabStatusCounts {
    running: number;
    continuous: number;
    completed_unseen: number;
    error_unseen: number;
    idle: number;
}

/** Bucket semântico de badge de projeto derivado de `TabStatus` (Fase 06). */
export type TabBadgeBucket = 'running' | 'continuous' | 'finished' | 'error' | 'idle';

/** Entrada de snapshot de aba enriquecida com `badgeBucket` para agregação na coluna de projetos. */
export type TabStatusEntry = TabStateSnapshot_Serialize & {
    badgeBucket: TabBadgeBucket;
};

/** Converte `TabStatus` da Fase 05 para o `TabBadgeBucket` da coluna de projetos (Fase 06). */
export function statusToBadgeBucket(status: TabStatus): TabBadgeBucket {
    switch (status) {
        case 'running':
            return 'running';
        case 'continuous':
            return 'continuous';
        case 'completed_unseen':
            return 'finished';
        case 'error_unseen':
            return 'error';
        case 'idle':
        default:
            return 'idle';
    }
}

/** Estado pendente de confirmação de visualização (`tab_mark_seen`) por aba. */
export interface PendingSeenEntry {
    blockId: CommandBlockId | null;
    upToRevision: number | null;
}

/** Estado reativo da store `useTabStatusStore`. */
export interface TabStatusStoreState {
    tabs: Record<TabId, TabStateSnapshot_Serialize>;
    tombstones: Record<TabId, number>;
    focus: FocusSnapshot_Deserialize;
    pendingSeen: Record<TabId, PendingSeenEntry>;
    initialized: boolean;
}

let initInFlight: Promise<void> | null = null;
let initGeneration = 0;
let activeUnlisteners: UnlistenFn[] = [];
let focusSeq = 0;

function clearActiveUnlisteners(): void {
    const list = activeUnlisteners.splice(0, activeUnlisteners.length);
    for (const unlisten of list) {
        try {
            unlisten();
        } catch {
            // Ignora falha de remoção de listener.
        }
    }
}

export const useTabStatusStore = defineStore('tabStatus', {
    state: (): TabStatusStoreState => ({
        tabs: {},
        tombstones: {},
        focus: {
            windowFocused: true,
            activeTabId: null,
            activePaneId: null
        },
        pendingSeen: {},
        initialized: false
    }),

    getters: {
        /**
         * Mapa reativo de abas enriquecidas com `badgeBucket` (`running`, `continuous`, `finished`, `error`, `idle`)
         * consumido pela agregação de contadores por projeto na Fase 06.
         */
        byTab(state): Record<TabId, TabStatusEntry> {
            const out: Record<TabId, TabStatusEntry> = {};
            for (const [tabId, snap] of Object.entries(state.tabs)) {
                const explicitBucket = (snap as Partial<TabStatusEntry>).badgeBucket;
                out[tabId] = {
                    ...snap,
                    badgeBucket: explicitBucket ?? statusToBadgeBucket(snap.status)
                };
            }
            return out;
        },

        /** Retorna o status semântico atual de uma aba (`idle` quando não registrada). */
        statusOf(state): (tabId: TabId) => TabStatus {
            return (tabId: TabId): TabStatus => state.tabs[tabId]?.status ?? 'idle';
        },

        /** Retorna o snapshot completo de uma aba ou `null` se não existir. */
        snapshotOf(state): (tabId: TabId) => TabStateSnapshot_Serialize | null {
            return (tabId: TabId): TabStateSnapshot_Serialize | null => state.tabs[tabId] ?? null;
        },

        /** Retorna o estado de um painel específico dentro de uma aba. */
        paneOf(state): (tabId: TabId, paneId?: PaneId | null) => PaneStateDto_Serialize | null {
            return (tabId: TabId, paneId?: PaneId | null): PaneStateDto_Serialize | null => {
                const snap = state.tabs[tabId];
                if (!snap) {
                    return null;
                }
                if (paneId === undefined) {
                    return snap.panes[0] ?? null;
                }
                return snap.panes.find((p) => (p.paneId ?? null) === (paneId ?? null)) ?? null;
            };
        },

        /**
         * Calcula a contagem determinística de abas por bucket semântico, deduplicando `tabIds`
         * e ignorando abas removidas/inexistentes.
         */
        countsFor(state): (tabIds: readonly TabId[]) => TabStatusCounts {
            return (tabIds: readonly TabId[]): TabStatusCounts => {
                const counts: TabStatusCounts = {
                    running: 0,
                    continuous: 0,
                    completed_unseen: 0,
                    error_unseen: 0,
                    idle: 0
                };
                const seenIds = new Set<TabId>();
                for (const tabId of tabIds) {
                    if (seenIds.has(tabId)) {
                        continue;
                    }
                    seenIds.add(tabId);
                    const snap = state.tabs[tabId];
                    if (!snap) {
                        continue;
                    }
                    counts[snap.status] += 1;
                }
                return counts;
            };
        },

        /** Lista os `TabId`s com conclusão ou alerta ainda não visualizado (`completed_unseen` ou `error_unseen`). */
        unseenTabIds(state): TabId[] {
            const result: TabId[] = [];
            for (const snap of Object.values(state.tabs)) {
                if (snap.status === 'completed_unseen' || snap.status === 'error_unseen') {
                    result.push(snap.tabId);
                }
            }
            return result;
        },

        /**
         * Indica se há ao menos um processo ativo (`running`, `continuous` ou `activeExecution !== null`)
         * em qualquer painel de qualquer aba registrada, mesmo quando o status agregado da aba é `error_unseen`.
         */
        hasActiveProcesses(state): boolean {
            for (const snap of Object.values(state.tabs)) {
                if (snap.status === 'running' || snap.status === 'continuous') {
                    return true;
                }
                for (const pane of snap.panes) {
                    if (
                        pane.activeExecution !== null ||
                        pane.status === 'running' ||
                        pane.status === 'continuous'
                    ) {
                        return true;
                    }
                }
            }
            return false;
        }
    },

    actions: {
        /**
         * Aplica um `TabStateSnapshot` respeitando revisões monotônicas e tombstones de remoção.
         */
        applySnapshot(incoming: TabStateSnapshot_Serialize): void {
            const tombstoneRev = this.tombstones[incoming.tabId];
            if (tombstoneRev !== undefined) {
                if (incoming.revision <= tombstoneRev) {
                    return;
                }
                delete this.tombstones[incoming.tabId];
            }

            const existing = this.tabs[incoming.tabId];
            if (existing !== undefined && incoming.revision < existing.revision) {
                return;
            }

            this.tabs[incoming.tabId] = incoming;

            const pending = this.pendingSeen[incoming.tabId];
            if (pending) {
                const lastComp = incoming.lastCompletion;
                const cleared =
                    lastComp === null ||
                    lastComp.seen ||
                    (pending.blockId !== null && lastComp.blockId !== pending.blockId) ||
                    (pending.upToRevision !== null && incoming.revision > pending.upToRevision);
                if (cleared) {
                    delete this.pendingSeen[incoming.tabId];
                }
            }
        },

        /**
         * Trata o evento `tab://state-removed`, gravando tombstone para impedir ressurreição por eventos atrasados.
         */
        handleRemoved(ev: TabStateRemovedEvent): void {
            const existing = this.tabs[ev.tabId];
            if (existing !== undefined && existing.revision > ev.revision) {
                return;
            }
            const prevTombstone = this.tombstones[ev.tabId] ?? 0;
            this.tombstones[ev.tabId] = Math.max(prevTombstone, ev.revision);
            delete this.tabs[ev.tabId];
            delete this.pendingSeen[ev.tabId];
        },

        /** Aplica um evento bufferizado durante a inicialização. */
        applyBufferedEvent(evt: BufferedTabEvent): void {
            if (evt.type === 'changed') {
                this.applySnapshot(evt.payload);
            } else {
                this.handleRemoved(evt.payload);
            }
        },

        /**
         * Inicializa a store de forma idempotente: assina `tab://state-changed` e `tab://state-removed`
         * antes de buscar `tabStateList()`, reconciliando revisões sem perder notificações.
         */
        async init(): Promise<void> {
            if (this.initialized) {
                return;
            }
            if (initInFlight !== null) {
                return initInFlight;
            }

            const myGen = ++initGeneration;

            const runInit = async (): Promise<void> => {
                const localUnlisteners: UnlistenFn[] = [];
                const buffered: BufferedTabEvent[] = [];
                let buffering = true;

                const enqueueOrApply = (evt: BufferedTabEvent): void => {
                    if (myGen !== initGeneration) {
                        return;
                    }
                    if (buffering) {
                        if (buffered.length >= MAX_BUFFERED_TAB_EVENTS) {
                            buffered.shift();
                        }
                        buffered.push(evt);
                        return;
                    }
                    this.applyBufferedEvent(evt);
                };

                try {
                    localUnlisteners.push(
                        await onTabStateChanged((payload) => {
                            enqueueOrApply({ type: 'changed', payload });
                        }),
                        await onTabStateRemoved((payload) => {
                            enqueueOrApply({ type: 'removed', payload });
                        })
                    );

                    if (myGen !== initGeneration) {
                        for (const u of localUnlisteners) {
                            u();
                        }
                        return;
                    }

                    const initialList = await tabStateList();
                    if (myGen !== initGeneration) {
                        for (const u of localUnlisteners) {
                            u();
                        }
                        return;
                    }

                    for (const snap of initialList) {
                        this.applySnapshot(snap);
                    }

                    buffering = false;
                    while (buffered.length > 0) {
                        const next = buffered.shift();
                        if (next) {
                            this.applyBufferedEvent(next);
                        }
                    }

                    clearActiveUnlisteners();
                    activeUnlisteners = localUnlisteners;
                    this.initialized = true;
                } catch (err: unknown) {
                    for (const u of localUnlisteners) {
                        try {
                            u();
                        } catch {
                            // Ignora erro de cleanup.
                        }
                    }
                    throw err;
                } finally {
                    if (myGen === initGeneration) {
                        initInFlight = null;
                    }
                }
            };

            initInFlight = runInit();
            return initInFlight;
        },

        /** Encerra listeners ativos e invalida inicializações em andamento. */
        dispose(): void {
            initGeneration += 1;
            initInFlight = null;
            clearActiveUnlisteners();
            this.initialized = false;
        },

        /** Recarrega o snapshot de uma aba específica ou de todas as abas do backend. */
        async refresh(tabId?: TabId): Promise<void> {
            if (tabId !== undefined) {
                const snap = await tabStateGet(tabId);
                this.applySnapshot(snap);
                return;
            }
            const list = await tabStateList();
            for (const snap of list) {
                this.applySnapshot(snap);
            }
        },

        /**
         * Envia confirmação de visualização (`tab_mark_seen`) sem mutar otimisticamente o snapshot inteiro,
         * preservando segurança contra condições de corrida com novas gerações de comando.
         */
        async markSeen(request: SeenRequest_Deserialize): Promise<TabStateSnapshot_Serialize | null> {
            const currentSnap = this.tabs[request.tabId];
            const expectedBlockId = currentSnap?.lastCompletion?.blockId ?? null;
            this.pendingSeen[request.tabId] = {
                blockId: expectedBlockId,
                upToRevision: request.upToRevision ?? currentSnap?.revision ?? null
            };

            try {
                const updated = await tabMarkSeen(request);
                this.applySnapshot(updated);
                return this.tabs[request.tabId] ?? updated;
            } finally {
                const pending = this.pendingSeen[request.tabId];
                if (pending && pending.blockId === expectedBlockId) {
                    delete this.pendingSeen[request.tabId];
                }
            }
        },

        /**
         * Atualiza o foco global da janela e da aba/painel ativo usando sequência monotônica
         * para descartar respostas IPC fora de ordem.
         */
        async setFocus(focus: FocusSnapshot_Deserialize): Promise<TabStateSnapshot_Serialize[]> {
            const seq = ++focusSeq;
            this.focus = {
                windowFocused: focus.windowFocused,
                activeTabId: focus.activeTabId ?? null,
                activePaneId: focus.activePaneId ?? null
            };
            const changed = await tabSetFocus(this.focus);
            if (seq <= focusSeq) {
                for (const snap of changed) {
                    this.applySnapshot(snap);
                }
            }
            return changed;
        },

        /** Define a política de execução contínua (`auto`, `always`, `never`) para uma aba ou painel. */
        async setContinuous(
            tabId: TabId,
            paneId: PaneId | null,
            policy: ContinuousPolicy
        ): Promise<TabStateSnapshot_Serialize> {
            const updated = await tabSetContinuous(tabId, paneId, policy);
            this.applySnapshot(updated);
            return this.tabs[tabId] ?? updated;
        },

        /** Reseta o estado e listeners da store (exclusivo para testes). */
        resetForTests(): void {
            this.dispose();
            focusSeq = 0;
            this.tabs = {};
            this.tombstones = {};
            this.focus = {
                windowFocused: true,
                activeTabId: null,
                activePaneId: null
            };
            this.pendingSeen = {};
        }
    }
});
