import { getCurrentScope, onScopeDispose } from 'vue';
import { useTabsStore } from '@/stores/tabs';
import type { LayoutSizePatch, TabId } from '@/types/bindings';

const DEFAULT_RESIZE_DEBOUNCE_MS = 150;

export interface UsePaneLayoutOptions {
    debounceMs?: number;
}

export interface UsePaneLayoutReturn {
    scheduleNodeResize: (tabId: TabId, nodeId: string, sizes: [number, number]) => void;
    flushPendingResizes: (tabId: TabId) => Promise<void>;
    stop: () => void;
}

/**
 * Gerencia o redimensionamento de splitters (`Splitpanes @resized`) com debounce de `150ms`
 * e reconciliação otimista via `pane_layout_update`.
 */
export function usePaneLayout(options: UsePaneLayoutOptions = {}): UsePaneLayoutReturn {
    const tabsStore = useTabsStore();
    const debounceMs = options.debounceMs ?? DEFAULT_RESIZE_DEBOUNCE_MS;

    const pendingPatchesByTab = new Map<TabId, Map<string, [number, number]>>();
    const timersByTab = new Map<TabId, ReturnType<typeof setTimeout>>();
    let disposed = false;

    async function flushPendingResizes(tabId: TabId): Promise<void> {
        const timer = timersByTab.get(tabId);
        if (timer !== undefined) {
            clearTimeout(timer);
            timersByTab.delete(tabId);
        }
        const pendingMap = pendingPatchesByTab.get(tabId);
        if (!pendingMap || pendingMap.size === 0 || disposed) {
            return;
        }
        pendingPatchesByTab.delete(tabId);

        const patches: LayoutSizePatch[] = [];
        for (const [nodeId, sizes] of pendingMap.entries()) {
            patches.push({
                nodeId,
                sizes: [sizes[0], sizes[1]]
            });
        }

        try {
            await tabsStore.updateLayoutSizes(tabId, patches);
        } catch {
            // Erros de layout já são registrados em tabsStore.error.
        }
    }

    function scheduleNodeResize(tabId: TabId, nodeId: string, rawSizes: [number, number]): void {
        if (disposed) {
            return;
        }
        const first = Math.max(5, Math.min(95, Number(rawSizes[0]) || 50));
        const second = Number((100 - first).toFixed(2));
        const normalizedFirst = Number((100 - second).toFixed(2));

        let tabMap = pendingPatchesByTab.get(tabId);
        if (!tabMap) {
            tabMap = new Map<string, [number, number]>();
            pendingPatchesByTab.set(tabId, tabMap);
        }
        tabMap.set(nodeId, [normalizedFirst, second]);

        const prevTimer = timersByTab.get(tabId);
        if (prevTimer !== undefined) {
            clearTimeout(prevTimer);
        }

        if (debounceMs <= 0) {
            void flushPendingResizes(tabId);
            return;
        }

        const timer = setTimeout(() => {
            timersByTab.delete(tabId);
            void flushPendingResizes(tabId);
        }, debounceMs);
        timersByTab.set(tabId, timer);
    }

    function stop(): void {
        disposed = true;
        for (const timer of timersByTab.values()) {
            clearTimeout(timer);
        }
        timersByTab.clear();
        pendingPatchesByTab.clear();
    }

    if (getCurrentScope()) {
        onScopeDispose(stop);
    }

    return {
        scheduleNodeResize,
        flushPendingResizes,
        stop
    };
}
