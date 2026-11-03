import { getCurrentScope, isRef, onScopeDispose, unref, watch, type Ref } from 'vue';
import { useTabStatusStore } from '@/stores/tabStatus';
import type { PaneId, TabId, TabStateSnapshot_Serialize } from '@/types/bindings';

/** Opções de inicialização do rastreador de visualização de abas (`useTabSeenTracker`). */
export interface UseTabSeenTrackerOptions {
    activeTabId: Ref<TabId | null> | (() => TabId | null);
    activePaneId?: Ref<PaneId | null> | (() => PaneId | null) | undefined;
    ackDelayMs?: number | undefined;
    /**
     * Quando `true` (usado por `useProjectTabFocus` na Fase 06), o foco sozinho não agenda `markSeen`;
     * a confirmação depende de evidência explícita de apresentação do renderer (`presentedConclusions`).
     */
    requireExplicitPresentation?: boolean | undefined;
}

function readValue<T>(source: Ref<T> | (() => T) | undefined, fallback: T): T {
    if (source === undefined) {
        return fallback;
    }
    if (typeof source === 'function') {
        return source();
    }
    return isRef(source) ? unref(source) : fallback;
}

function needsSeenConfirmation(snapshot: TabStateSnapshot_Serialize | null): boolean {
    if (!snapshot) {
        return false;
    }
    if (snapshot.status === 'completed_unseen' || snapshot.status === 'error_unseen') {
        return true;
    }
    if (snapshot.lastCompletion && !snapshot.lastCompletion.seen) {
        return true;
    }
    if (snapshot.attention && !snapshot.attention.seen) {
        return true;
    }
    return false;
}

/**
 * Composable que coordena `tab_set_focus` e `tab_mark_seen`:
 * - Registra mudanças de foco da janela, visibilidade do documento e aba/painel ativo.
 * - Nunca confirma automaticamente durante `running`/`continuous`.
 * - Quando uma conclusão não vista (`completed_unseen` / `error_unseen`) é efetivamente apresentada
 *   com janela focada e documento visível, envia `tab_mark_seen` com `upToRevision` exata.
 * - Cancela imediatamente qualquer confirmação pendente no `blur` ou ocultação do documento.
 */
export function useTabSeenTracker(options: UseTabSeenTrackerOptions) {
    const tabStatusStore = useTabStatusStore();
    const ackDelayMs = options.ackDelayMs ?? 16;

    let windowFocused =
        typeof document !== 'undefined' && typeof document.hasFocus === 'function'
            ? document.hasFocus()
            : true;
    let documentVisible =
        typeof document !== 'undefined' ? document.visibilityState !== 'hidden' : true;

    let pendingTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingTarget: { tabId: TabId; revision: number; blockId: string | null } | null = null;
    let lastAckedKey: string | null = null;
    let disposed = false;

    function getActiveTabId(): TabId | null {
        return readValue(options.activeTabId, null);
    }

    function getActivePaneId(): PaneId | null {
        return readValue(options.activePaneId, null);
    }

    function isEffectivelyVisible(): boolean {
        return !disposed && windowFocused && documentVisible;
    }

    function cancelPendingAck(): void {
        if (pendingTimer !== null) {
            clearTimeout(pendingTimer);
            pendingTimer = null;
        }
        pendingTarget = null;
    }

    async function syncFocusToBackend(): Promise<void> {
        if (disposed) {
            return;
        }
        const effectiveWindowFocus = isEffectivelyVisible();
        await tabStatusStore.setFocus({
            windowFocused: effectiveWindowFocus,
            activeTabId: getActiveTabId(),
            activePaneId: getActivePaneId()
        });
    }

    function evaluateAndScheduleAck(): void {
        if (options.requireExplicitPresentation) {
            return;
        }
        if (!isEffectivelyVisible()) {
            cancelPendingAck();
            return;
        }

        const tabId = getActiveTabId();
        if (!tabId) {
            cancelPendingAck();
            return;
        }

        const snapshot = tabStatusStore.snapshotOf(tabId);
        if (!needsSeenConfirmation(snapshot) || !snapshot) {
            cancelPendingAck();
            return;
        }

        const blockId = snapshot.lastCompletion?.blockId ?? null;
        const revision = snapshot.revision;
        const ackKey = `${tabId}:${revision}:${blockId ?? 'attention'}`;

        if (lastAckedKey === ackKey) {
            return;
        }
        if (
            pendingTarget &&
            pendingTarget.tabId === tabId &&
            pendingTarget.revision === revision &&
            pendingTarget.blockId === blockId
        ) {
            return;
        }

        cancelPendingAck();
        pendingTarget = { tabId, revision, blockId };

        pendingTimer = setTimeout(() => {
            pendingTimer = null;
            const target = pendingTarget;
            pendingTarget = null;
            if (!target || !isEffectivelyVisible() || getActiveTabId() !== target.tabId) {
                return;
            }
            const currentSnap = tabStatusStore.snapshotOf(target.tabId);
            if (!currentSnap || !needsSeenConfirmation(currentSnap)) {
                return;
            }
            lastAckedKey = `${target.tabId}:${currentSnap.revision}:${currentSnap.lastCompletion?.blockId ?? 'attention'}`;
            void tabStatusStore
                .markSeen({
                    tabId: target.tabId,
                    paneId: getActivePaneId(),
                    upToRevision: currentSnap.revision
                })
                .catch(() => {
                    lastAckedKey = null;
                });
        }, ackDelayMs);
    }

    function handleWindowFocus(): void {
        windowFocused = true;
        void syncFocusToBackend();
        evaluateAndScheduleAck();
    }

    function handleWindowBlur(): void {
        windowFocused = false;
        cancelPendingAck();
        void syncFocusToBackend();
    }

    function handleVisibilityChange(): void {
        if (typeof document !== 'undefined') {
            documentVisible = document.visibilityState !== 'hidden';
        }
        if (!documentVisible) {
            cancelPendingAck();
        }
        void syncFocusToBackend();
        if (documentVisible) {
            evaluateAndScheduleAck();
        }
    }

    if (typeof window !== 'undefined' && typeof window.addEventListener === 'function') {
        window.addEventListener('focus', handleWindowFocus);
        window.addEventListener('blur', handleWindowBlur);
    }
    if (typeof document !== 'undefined' && typeof document.addEventListener === 'function') {
        document.addEventListener('visibilitychange', handleVisibilityChange);
    }

    const stopWatchActive = watch(
        () => [getActiveTabId(), getActivePaneId()] as const,
        () => {
            cancelPendingAck();
            void syncFocusToBackend();
            evaluateAndScheduleAck();
        }
    );

    const stopWatchSnapshot = watch(
        () => {
            const tabId = getActiveTabId();
            if (!tabId) {
                return null;
            }
            const snap = tabStatusStore.snapshotOf(tabId);
            if (!snap) {
                return null;
            }
            return `${snap.tabId}:${snap.revision}:${snap.status}:${snap.lastCompletion?.seen ?? true}:${snap.attention?.seen ?? true}`;
        },
        () => {
            evaluateAndScheduleAck();
        }
    );

    function setWindowFocusedForTests(focused: boolean, docVisible = true): void {
        windowFocused = focused;
        documentVisible = docVisible;
        if (!focused || !docVisible) {
            cancelPendingAck();
        }
        void syncFocusToBackend();
        if (focused && docVisible) {
            evaluateAndScheduleAck();
        }
    }

    function stop(): void {
        if (disposed) {
            return;
        }
        disposed = true;
        cancelPendingAck();
        stopWatchActive();
        stopWatchSnapshot();
        if (typeof window !== 'undefined' && typeof window.removeEventListener === 'function') {
            window.removeEventListener('focus', handleWindowFocus);
            window.removeEventListener('blur', handleWindowBlur);
        }
        if (typeof document !== 'undefined' && typeof document.removeEventListener === 'function') {
            document.removeEventListener('visibilitychange', handleVisibilityChange);
        }
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            stop();
        });
    }

    return {
        syncFocusToBackend,
        evaluateAndScheduleAck,
        cancelPendingAck,
        setWindowFocusedForTests,
        stop
    };
}
