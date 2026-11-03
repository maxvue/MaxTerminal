import { computed, getCurrentScope, onScopeDispose, watch } from 'vue';
import type { ComputedRef } from 'vue';
import { collectLeafPaneIds } from '@/composables/workspace/layoutTree';
import { useProjectsStore } from '@/stores/projects';
import { useTabsStore } from '@/stores/tabs';
import { useTabStatusStore } from '@/stores/tabStatus';
import type { PaneId, TabId } from '@/types/bindings';

export interface UseActiveTabSeenReturn {
    activeTabId: ComputedRef<TabId | null>;
    activePaneId: ComputedRef<PaneId | null>;
    visiblePaneIds: ComputedRef<readonly PaneId[]>;
    stop: () => void;
}

/**
 * Coordena o foco da aba/painel ativo da Fase 07 com `tabsStore.focusTab` e `tabStatusStore.markSeen`
 * quando a aba do projeto selecionado está visível e a janela está em foco.
 */
export function useActiveTabSeen(): UseActiveTabSeenReturn {
    const projectsStore = useProjectsStore();
    const tabsStore = useTabsStore();
    const tabStatusStore = useTabStatusStore();

    const activeTab = computed(() => tabsStore.activeTabForSelectedProject);
    const activeTabId = computed<TabId | null>(() => activeTab.value?.id ?? null);
    const activePaneId = computed<PaneId | null>(() => activeTab.value?.activePaneId ?? null);

    const visiblePaneIds = computed<readonly PaneId[]>(() => {
        const tab = activeTab.value;
        if (!tab) {
            return [];
        }
        return collectLeafPaneIds(tab.layout);
    });

    let windowFocused =
        typeof document !== 'undefined' && typeof document.hasFocus === 'function'
            ? document.hasFocus()
            : true;
    let documentVisible =
        typeof document !== 'undefined' ? document.visibilityState !== 'hidden' : true;
    let disposed = false;

    function syncActiveTabSeen(): void {
        if (disposed || !windowFocused || !documentVisible) {
            return;
        }
        const tab = activeTab.value;
        if (!tab || projectsStore.selectedId !== tab.projectId) {
            return;
        }

        const snap = tabStatusStore.snapshotOf(tab.id);
        if (
            snap &&
            (snap.status === 'completed_unseen' ||
                snap.status === 'error_unseen' ||
                (snap.lastCompletion && !snap.lastCompletion.seen) ||
                (snap.attention && !snap.attention.seen))
        ) {
            void tabStatusStore
                .markSeen({
                    tabId: tab.id,
                    paneId: tab.activePaneId,
                    upToRevision: snap.revision
                })
                .catch(() => {
                    // Ignora falha transitória de markSeen.
                });
        }
    }

    function onFocus(): void {
        windowFocused = true;
        syncActiveTabSeen();
    }

    function onBlur(): void {
        windowFocused = false;
    }

    function onVisibilityChange(): void {
        if (typeof document !== 'undefined') {
            documentVisible = document.visibilityState !== 'hidden';
        }
        if (documentVisible) {
            syncActiveTabSeen();
        }
    }

    if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
        window.addEventListener('focus', onFocus);
        window.addEventListener('blur', onBlur);
    }
    if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
        document.addEventListener('visibilitychange', onVisibilityChange);
    }

    const stopWatch = watch(
        () =>
            [
                projectsStore.selectedId,
                activeTabId.value,
                activePaneId.value,
                activeTabId.value ? tabStatusStore.snapshotOf(activeTabId.value)?.status : null,
                activeTabId.value ? tabStatusStore.snapshotOf(activeTabId.value)?.revision : null
            ] as const,
        () => {
            syncActiveTabSeen();
        },
        { immediate: true }
    );

    function stop(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        stopWatch();
        if (typeof window !== 'undefined' && typeof window.removeEventListener === 'function') {
            window.removeEventListener('focus', onFocus);
            window.removeEventListener('blur', onBlur);
        }
        if (typeof document !== 'undefined' && typeof document.removeEventListener === 'function') {
            document.removeEventListener('visibilitychange', onVisibilityChange);
        }
    }

    if (getCurrentScope()) {
        onScopeDispose(stop);
    }

    return {
        activeTabId,
        activePaneId,
        visiblePaneIds,
        stop
    };
}
