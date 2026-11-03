import type { UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentScope, onScopeDispose, watch } from 'vue';
import { osIpc } from '@/ipc';
import { useProjectsStore } from '@/stores/projects';
import { useTabsStore } from '@/stores/tabs';
import { useWindowStore } from '@/stores/window';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import type { FocusTabPayload_Serialize } from '@/types/bindings';

export interface UseOsFocusTabReturn {
    /** Aplica imediatamente um pedido de foco em aba/painel vindo do subsistema `os`. */
    applyFocusPayload: (payload: FocusTabPayload_Serialize) => Promise<void>;
    /** Encerra a escuta de `os://focus-tab`. */
    stop: () => void;
}

/**
 * Conecta o evento `os://focus-tab` (acionado pela bandeja do sistema, ações de notificação
 * ou ativação single-instance `--focus-tab`) às stores de projetos, abas e instância do terminal.
 */
export function useOsFocusTab(): UseOsFocusTabReturn {
    const projectsStore = useProjectsStore();
    const tabsStore = useTabsStore();
    const windowStore = useWindowStore();

    let unlisten: UnlistenFn | null = null;
    let disposed = false;

    async function applyFocusPayload(payload: FocusTabPayload_Serialize): Promise<void> {
        if (disposed) {
            return;
        }
        const { tabId, paneId, projectId } = payload;
        const resolvedProjectId =
            projectId ??
            tabsStore.byId[tabId]?.projectId ??
            projectsStore.tabOwner[tabId] ??
            null;

        if (resolvedProjectId && projectsStore.selectedId !== resolvedProjectId) {
            projectsStore.select(resolvedProjectId);
        }

        if (tabsStore.byId[tabId]) {
            await tabsStore.focusTab(tabId, paneId ?? null);
        } else if (resolvedProjectId) {
            projectsStore.setActiveTab(resolvedProjectId, tabId);
        }

        const targetPaneId = paneId ?? tabsStore.byId[tabId]?.activePaneId ?? null;
        if (targetPaneId) {
            terminalInstanceRegistry.get(targetPaneId)?.focus();
        }
    }

    void osIpc
        .onFocusTab((ev) => {
            void applyFocusPayload(ev.payload);
        })
        .then((fn) => {
            if (disposed) {
                fn();
            } else {
                unlisten = fn;
            }
        })
        .catch(() => {
            // Ambiente de teste sem mock de eventos Tauri.
        });

    const stopWatchPending = watch(
        () => windowStore.pendingFocus,
        (pending) => {
            if (pending) {
                const payload = windowStore.consumePendingFocus();
                if (payload) {
                    void applyFocusPayload(payload);
                }
            }
        },
        { immediate: true }
    );

    function stop(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        stopWatchPending();
        if (unlisten) {
            unlisten();
            unlisten = null;
        }
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            stop();
        });
    }

    return {
        applyFocusPayload,
        stop
    };
}
