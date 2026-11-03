import {
    computed,
    getCurrentScope,
    onScopeDispose,
    toValue,
    type ComputedRef,
    type MaybeRefOrGetter
} from 'vue';
import {
    formatAttentionBadgeLabel,
    formatAttentionKindLabel,
    formatAttentionSourceLabel
} from '@/composables/agents/adapters';
import { useAgentsStore } from '@/stores/agents';
import { useProjectsStore } from '@/stores/projects';
import { useTabsStore } from '@/stores/tabs';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { registerTerminalExtension } from '@/terminal/extensions/terminalExtensions';
import type { AttentionSnapshot_Serialize, TabId } from '@/types/bindings';
import type { Disposable, TerminalInstanceHandle } from '@/types/terminal';

let attentionExtRefCount = 0;
let attentionExtSub: Disposable | null = null;

function ensureAttentionExtensionRegistered(): void {
    attentionExtRefCount += 1;
    if (attentionExtSub !== null) {
        return;
    }

    attentionExtSub = registerTerminalExtension({
        id: 'agents.attention',
        activate(instance: TerminalInstanceHandle): Disposable {
            const dataSub = instance.xterm.onData?.((data: string) => {
                if (!data || data.length === 0) {
                    return;
                }
                const agentsStore = useAgentsStore();
                for (const snap of Object.values(agentsStore.attentionByTab)) {
                    if (snap.paneId !== instance.paneId) {
                        continue;
                    }
                    if (
                        (snap.state.state === 'awaiting' || snap.state.state === 'suspected') &&
                        !snap.acknowledged
                    ) {
                        const episode = snap.state.state === 'awaiting' ? snap.state.episode : 0;
                        void agentsStore
                            .ackAttention(snap.tabId, snap.runId, episode, 'userInput')
                            .catch(() => {
                                // Ignora falha de ack visual.
                            });
                    }
                }
            });

            return {
                dispose(): void {
                    dataSub?.dispose();
                }
            };
        }
    });
}

function releaseAttentionExtension(): void {
    attentionExtRefCount = Math.max(0, attentionExtRefCount - 1);
    if (attentionExtRefCount === 0 && attentionExtSub !== null) {
        attentionExtSub.dispose();
        attentionExtSub = null;
    }
}

export interface UseAgentAttentionReturn {
    snapshot: ComputedRef<AttentionSnapshot_Serialize | null>;
    isAwaiting: ComputedRef<boolean>;
    isSuspected: ComputedRef<boolean>;
    isAcknowledged: ComputedRef<boolean>;
    badgeLabel: ComputedRef<string | null>;
    sourceLabel: ComputedRef<string | null>;
    kindLabel: ComputedRef<string | null>;
    message: ComputedRef<string | null>;
    focusAgentTab: () => Promise<void>;
    dismissNotice: () => Promise<void>;
    stop: () => void;
}

/**
 * Composable de atenção correlacionada de uma aba de agente CLI (`spec.md` §6.3, `CA-22..CA-25`).
 * Distingue espera confirmada (`Awaiting`) de indício heurístico (`Suspected`) e registra `xterm.onData`
 * para reconhecer visualização (`userInput`) sem resolver a espera real no agente.
 */
export function useAgentAttention(
    tabId: MaybeRefOrGetter<TabId>
): UseAgentAttentionReturn {
    const agentsStore = useAgentsStore();
    const projectsStore = useProjectsStore();
    const tabsStore = useTabsStore();

    ensureAttentionExtensionRegistered();

    let stopped = false;
    function stop(): void {
        if (stopped) {
            return;
        }
        stopped = true;
        releaseAttentionExtension();
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            stop();
        });
    }

    const snapshot = computed<AttentionSnapshot_Serialize | null>(() => {
        const id = toValue(tabId);
        return agentsStore.attentionForTab(id);
    });

    const isAwaiting = computed<boolean>(() => {
        return snapshot.value?.state.state === 'awaiting';
    });

    const isSuspected = computed<boolean>(() => {
        return snapshot.value?.state.state === 'suspected';
    });

    const isAcknowledged = computed<boolean>(() => {
        return Boolean(snapshot.value?.acknowledged);
    });

    const badgeLabel = computed<string | null>(() => {
        return formatAttentionBadgeLabel(snapshot.value);
    });

    const sourceLabel = computed<string | null>(() => {
        const st = snapshot.value?.state;
        if (!st || (st.state !== 'awaiting' && st.state !== 'suspected')) {
            return null;
        }
        return formatAttentionSourceLabel(st.source);
    });

    const kindLabel = computed<string | null>(() => {
        const st = snapshot.value?.state;
        if (!st || (st.state !== 'awaiting' && st.state !== 'suspected')) {
            return null;
        }
        return formatAttentionKindLabel(st.kind);
    });

    const message = computed<string | null>(() => {
        const st = snapshot.value?.state;
        if (st?.state === 'awaiting') {
            return st.message;
        }
        return null;
    });

    async function focusAgentTab(): Promise<void> {
        const id = toValue(tabId);
        const snap = snapshot.value;
        const projectId = snap?.projectId ?? tabsStore.byId[id]?.projectId ?? null;
        if (projectId && projectsStore.selectedId !== projectId) {
            projectsStore.select(projectId);
        }
        const paneId = snap?.paneId ?? tabsStore.byId[id]?.activePaneId ?? null;
        if (tabsStore.byId[id]) {
            await tabsStore.focusTab(id, paneId);
        } else if (projectId) {
            projectsStore.setActiveTab(projectId, id);
        }
        if (paneId) {
            terminalInstanceRegistry.get(paneId)?.focus();
        }
    }

    async function dismissNotice(): Promise<void> {
        const snap = snapshot.value;
        if (!snap) {
            return;
        }
        const episode = snap.state.state === 'awaiting' ? snap.state.episode : 0;
        await agentsStore.ackAttention(snap.tabId, snap.runId, episode, 'dismissed');
    }

    return {
        snapshot,
        isAwaiting,
        isSuspected,
        isAcknowledged,
        badgeLabel,
        sourceLabel,
        kindLabel,
        message,
        focusAgentTab,
        dismissNotice,
        stop
    };
}
