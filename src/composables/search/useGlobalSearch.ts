import { onScopeDispose, ref } from 'vue';
import type { UnlistenFn } from '@tauri-apps/api/event';
import { onSearchIndexProgress, onSearchInvalidated } from '@/ipc/search';
import { useSearchStore, DEFAULT_SEARCH_DEBOUNCE_MS } from '@/stores/search';
import type { SearchIndexProgressEvent } from '@/types/bindings';

/**
 * Coordena o ciclo de vida da paleta de Busca Global (`GlobalSearchPalette`),
 * restauração de foco ao elemento anterior (`document.activeElement`),
 * estado de composição IME e listeners Tauri (`search://invalidated`, `search://index-progress`).
 */
export function useGlobalSearch() {
    const searchStore = useSearchStore();
    const indexProgress = ref<SearchIndexProgressEvent | null>(null);
    let previousFocusedElement: HTMLElement | null = null;
    const unlistenFns: UnlistenFn[] = [];
    let listenersAttached = false;

    function rememberFocusBeforeOpen(): void {
        if (typeof document !== 'undefined' && document.activeElement instanceof HTMLElement) {
            previousFocusedElement = document.activeElement;
        }
    }

    function restorePreviousFocus(): void {
        const target = previousFocusedElement;
        previousFocusedElement = null;
        if (target && typeof target.focus === 'function' && target.isConnected) {
            target.focus();
        }
    }

    function openPalette(initialQuery?: string, initialProjectId?: string | null): void {
        if (!searchStore.open) {
            rememberFocusBeforeOpen();
        }
        searchStore.show(initialQuery, initialProjectId);
    }

    function closePalette(): void {
        searchStore.hide();
        restorePreviousFocus();
    }

    function onCompositionStart(): void {
        searchStore.setComposingIme(true);
    }

    function onCompositionEnd(nextQuery?: string): void {
        searchStore.setComposingIme(false);
        if (typeof nextQuery === 'string' && nextQuery !== searchStore.query) {
            searchStore.updateQuery(nextQuery, DEFAULT_SEARCH_DEBOUNCE_MS);
        }
    }

    async function attachEvents(): Promise<void> {
        if (listenersAttached) {
            return;
        }
        listenersAttached = true;
        try {
            const unInvalidated = await onSearchInvalidated((payload) => {
                searchStore.invalidate(payload.reason);
            });
            const unProgress = await onSearchIndexProgress((payload) => {
                indexProgress.value = payload.done ? null : payload;
                if (payload.done) {
                    searchStore.invalidate('index_rebuilt');
                }
            });
            unlistenFns.push(unInvalidated, unProgress);
        } catch {
            // Ambiente de teste unitário sem IPC de eventos ativo.
        }
    }

    function detachEvents(): void {
        while (unlistenFns.length > 0) {
            const fn = unlistenFns.pop();
            fn?.();
        }
        listenersAttached = false;
    }

    onScopeDispose(() => {
        detachEvents();
    });

    return {
        searchStore,
        indexProgress,
        openPalette,
        closePalette,
        onCompositionStart,
        onCompositionEnd,
        attachEvents,
        detachEvents,
        rememberFocusBeforeOpen,
        restorePreviousFocus
    };
}
