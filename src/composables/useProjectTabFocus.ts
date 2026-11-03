import { computed, getCurrentScope, onScopeDispose, ref, watch, type Ref } from 'vue';
import { useTabSeenTracker } from '@/composables/useTabSeenTracker';
import { useProjectsStore } from '@/stores/projects';
import { useTabStatusStore } from '@/stores/tabStatus';
import type { PaneId, SeenRequest_Deserialize, TabId } from '@/types/bindings';

/**
 * Fonte de evidência de apresentação do renderer para confirmação de visualização (`SeenRequest`).
 * Na Fase 06 pura (sem Fase 07), os arrays permanecem vazios por padrão; testes e a Fase 07
 * fornecem os painéis visíveis e conclusões efetivamente renderizadas.
 */
export interface ProjectPresentationSource {
    visiblePaneIds: Readonly<Ref<readonly PaneId[]>>;
    presentedConclusions: Readonly<Ref<readonly SeenRequest_Deserialize[]>>;
}

export interface UseProjectTabFocusReturn {
    activeTabId: Readonly<Ref<TabId | null>>;
    activePaneId: Readonly<Ref<PaneId | null>>;
    setWindowFocusedForTests: (focused: boolean, docVisible?: boolean) => void;
    stop: () => void;
}

/**
 * Coordena o foco de aba/painel do projeto selecionado com o `useTabSeenTracker` da Fase 05 (§6.5 da spec).
 * - Foco sozinho atualiza `tab_set_focus`, mas NUNCA envia `tab_mark_seen`.
 * - Apenas conclusões efetivamente apresentadas em `source.presentedConclusions` para a aba ativa,
 *   com painel visível e janela focada/visível, disparam `tab_mark_seen` com `upToRevision` exata.
 */
export function useProjectTabFocus(source?: ProjectPresentationSource): UseProjectTabFocusReturn {
    const projectsStore = useProjectsStore();
    const tabStatusStore = useTabStatusStore();

    const emptyPanes = ref<readonly PaneId[]>([]);
    const emptyConclusions = ref<readonly SeenRequest_Deserialize[]>([]);

    const visiblePaneIds = source?.visiblePaneIds ?? emptyPanes;
    const presentedConclusions = source?.presentedConclusions ?? emptyConclusions;

    const activeTabId = computed<TabId | null>(() => {
        const selectedProjectId = projectsStore.selectedId;
        if (!selectedProjectId || !projectsStore.projectById(selectedProjectId)) {
            return null;
        }
        const candidateTabId = projectsStore.activeTabByProject[selectedProjectId] ?? null;
        if (!candidateTabId) {
            return null;
        }
        if (projectsStore.tabOwner[candidateTabId] !== selectedProjectId) {
            return null;
        }
        return candidateTabId;
    });

    const activePaneId = computed<PaneId | null>(() => {
        if (!activeTabId.value) {
            return null;
        }
        return visiblePaneIds.value[0] ?? null;
    });

    let windowFocused =
        typeof document !== 'undefined' && typeof document.hasFocus === 'function'
            ? document.hasFocus()
            : true;
    let documentVisible =
        typeof document !== 'undefined' ? document.visibilityState !== 'hidden' : true;
    let disposed = false;
    let focusGeneration = 0;
    const ackedKeys = new Set<string>();

    const tracker = useTabSeenTracker({
        activeTabId,
        activePaneId,
        requireExplicitPresentation: true
    });

    function isWindowEffectivelyVisible(): boolean {
        return !disposed && windowFocused && documentVisible;
    }

    function isPaneCurrentlyVisible(paneId: PaneId | null | undefined): boolean {
        const panes = visiblePaneIds.value;
        if (paneId === null || paneId === undefined) {
            return panes.length === 0;
        }
        return panes.includes(paneId);
    }

    function processPresentedConclusions(): void {
        if (!isWindowEffectivelyVisible()) {
            return;
        }
        const currentTabId = activeTabId.value;
        if (!currentTabId) {
            return;
        }
        const snap = tabStatusStore.snapshotOf(currentTabId);
        if (!snap) {
            return;
        }

        const myGen = focusGeneration;
        for (const req of presentedConclusions.value) {
            if (req.tabId !== currentTabId) {
                continue;
            }
            if (!isPaneCurrentlyVisible(req.paneId)) {
                continue;
            }

            const targetPane =
                req.paneId !== undefined && req.paneId !== null
                    ? snap.panes.find((p) => p.paneId === req.paneId)
                    : (snap.panes[0] ?? null);

            const hasUnseenInPane =
                (targetPane?.lastCompletion !== null &&
                    targetPane?.lastCompletion !== undefined &&
                    !targetPane.lastCompletion.seen) ||
                (snap.lastCompletion !== null && !snap.lastCompletion.seen) ||
                (snap.attention !== null && !snap.attention.seen) ||
                snap.status === 'completed_unseen' ||
                snap.status === 'error_unseen';

            if (!hasUnseenInPane) {
                continue;
            }

            const upToRevision = req.upToRevision ?? snap.revision;
            if (upToRevision !== snap.revision) {
                continue;
            }

            const blockKey =
                targetPane?.lastCompletion?.blockId ??
                snap.lastCompletion?.blockId ??
                'attention';
            const dedupKey = `${currentTabId}:${req.paneId ?? 'default'}:${upToRevision}:${blockKey}`;
            if (ackedKeys.has(dedupKey)) {
                continue;
            }
            ackedKeys.add(dedupKey);

            void tabStatusStore
                .markSeen({
                    tabId: currentTabId,
                    paneId: req.paneId ?? null,
                    upToRevision
                })
                .catch(() => {
                    if (myGen === focusGeneration) {
                        ackedKeys.delete(dedupKey);
                    }
                });
        }
    }

    function onWindowFocus(): void {
        windowFocused = true;
        processPresentedConclusions();
    }

    function onWindowBlur(): void {
        windowFocused = false;
        focusGeneration += 1;
    }

    function onVisibilityChange(): void {
        if (typeof document !== 'undefined') {
            documentVisible = document.visibilityState !== 'hidden';
        }
        if (!documentVisible) {
            focusGeneration += 1;
        } else {
            processPresentedConclusions();
        }
    }

    if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
        window.addEventListener('focus', onWindowFocus);
        window.addEventListener('blur', onWindowBlur);
    }
    if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
        document.addEventListener('visibilitychange', onVisibilityChange);
    }

    const stopWatchActive = watch(
        () => [activeTabId.value, visiblePaneIds.value] as const,
        () => {
            focusGeneration += 1;
            processPresentedConclusions();
        }
    );

    const stopWatchConclusions = watch(
        () => [
            presentedConclusions.value,
            activeTabId.value ? tabStatusStore.snapshotOf(activeTabId.value)?.revision : null
        ] as const,
        () => {
            processPresentedConclusions();
        },
        { deep: true }
    );

    function setWindowFocusedForTests(focused: boolean, docVisible = true): void {
        windowFocused = focused;
        documentVisible = docVisible;
        if (!focused || !docVisible) {
            focusGeneration += 1;
        }
        tracker.setWindowFocusedForTests(focused, docVisible);
        if (focused && docVisible) {
            processPresentedConclusions();
        }
    }

    function stop(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        focusGeneration += 1;
        stopWatchActive();
        stopWatchConclusions();
        tracker.stop();
        if (typeof window !== 'undefined' && typeof window.removeEventListener === 'function') {
            window.removeEventListener('focus', onWindowFocus);
            window.removeEventListener('blur', onWindowBlur);
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
        setWindowFocusedForTests,
        stop
    };
}
