import {
    computed,
    onScopeDispose,
    ref,
    unref,
    watch,
    type ComputedRef,
    type Ref
} from 'vue';
import type { PaneId } from '@/types/bindings';
import type {
    CompletionItem,
    CompletionSection,
    TriggerSelectOutcome
} from '@/types/terminal';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { resolveTerminalContext } from '@/terminal/extensions/terminalContext';
import {
    CompletionSession,
    type CompletionSessionSnapshot
} from '@/terminal/triggers/completionSession';

/** Retorno reativo do composable `useCompletion`. */
export interface UseCompletionReturn {
    open: ComputedRef<boolean>;
    loading: ComputedRef<boolean>;
    selecting: ComputedRef<boolean>;
    title: ComputedRef<string>;
    stage: ComputedRef<number>;
    stageLabel: ComputedRef<string | undefined>;
    items: ComputedRef<CompletionItem[]>;
    sections: ComputedRef<CompletionSection[]>;
    activeIndex: ComputedRef<number>;
    activeItem: ComputedRef<CompletionItem | null>;
    error: ComputedRef<string | null>;
    emptyMessage: ComputedRef<string>;
    move: (delta: number) => void;
    setActiveIndex: (index: number) => void;
    selectFirst: () => void;
    selectLast: () => void;
    handleBackspace: () => boolean;
    popStage: () => boolean;
    confirm: (mode?: 'primary' | 'secondary') => Promise<TriggerSelectOutcome | null>;
    submitWithoutSelection: () => Promise<TriggerSelectOutcome | null>;
    suppress: () => void;
    cancel: () => void;
}

/**
 * Composable reativo sobre a `CompletionSession` para um painel.
 */
export function useCompletion(
    paneId: Ref<PaneId> | PaneId,
    textRef: Ref<string>,
    cursorRef: Ref<number>
): UseCompletionReturn {
    const store = useTerminalStore();
    const snapshot = ref<CompletionSessionSnapshot>({
        status: 'idle',
        open: false,
        loading: false,
        trigger: null,
        stage: 0,
        stageLabel: undefined,
        items: [],
        sections: [],
        activeIndex: -1,
        error: null,
        emptyMessage: 'Nenhum resultado encontrado',
        selecting: false
    });

    const session = new CompletionSession({
        paneId: unref(paneId),
        getContext: () => {
            const currentPaneId = unref(paneId);
            const inst = terminalInstanceRegistry.get(currentPaneId);
            if (inst) {
                return inst.getContext();
            }
            const ptyId = store.instance(currentPaneId)?.ptyId ?? '';
            return resolveTerminalContext({ paneId: currentPaneId, ptyId });
        },
        onStateChange: (next) => {
            snapshot.value = next;
        }
    });

    watch(
        [textRef, cursorRef],
        ([nextText, nextCursor]) => {
            const currentPaneId = unref(paneId);
            if (store.isPassthrough(currentPaneId)) {
                session.cancel();
                return;
            }
            session.updateInput(nextText, nextCursor);
        },
        { immediate: true }
    );

    watch(
        () => store.instance(unref(paneId))?.inputMode,
        (mode) => {
            if (mode === 'passthrough') {
                session.cancel();
            }
        }
    );

    onScopeDispose(() => {
        session.dispose();
    });

    const open = computed<boolean>(() => snapshot.value.open);
    const loading = computed<boolean>(() => snapshot.value.loading);
    const selecting = computed<boolean>(() => snapshot.value.selecting);
    const title = computed<string>(() => snapshot.value.trigger?.label ?? '');
    const stage = computed<number>(() => snapshot.value.stage);
    const stageLabel = computed<string | undefined>(() => snapshot.value.stageLabel);
    const items = computed<CompletionItem[]>(() => snapshot.value.items);
    const sections = computed<CompletionSection[]>(() => snapshot.value.sections);
    const activeIndex = computed<number>(() => snapshot.value.activeIndex);
    const activeItem = computed<CompletionItem | null>(() => {
        const idx = snapshot.value.activeIndex;
        return idx >= 0 ? (snapshot.value.items[idx] ?? null) : null;
    });
    const error = computed<string | null>(() => snapshot.value.error);
    const emptyMessage = computed<string>(() => snapshot.value.emptyMessage);

    return {
        open,
        loading,
        selecting,
        title,
        stage,
        stageLabel,
        items,
        sections,
        activeIndex,
        activeItem,
        error,
        emptyMessage,
        move: (delta) => session.moveSelection(delta),
        setActiveIndex: (index) => session.setSelectionIndex(index),
        selectFirst: () => session.selectFirst(),
        selectLast: () => session.selectLast(),
        handleBackspace: () => session.handleBackspaceOnEmptyQuery(),
        popStage: () => session.popStage(),
        confirm: (mode = 'primary') => session.confirmSelection(mode),
        submitWithoutSelection: () => session.submitWithoutSelection(),
        suppress: () => session.suppressCurrentTrigger(),
        cancel: () => session.cancel()
    };
}
