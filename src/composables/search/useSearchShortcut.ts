import { onMounted, onScopeDispose } from 'vue';
import { useProjectsStore } from '@/stores/projects';
import { useSearchStore } from '@/stores/search';
import { useTerminalStore } from '@/stores/terminal';
import { isGlobalSearchShortcut, isSearchShortcut } from '@/terminal/keybindings';

export interface SearchShortcutContext {
    /** Indica se o terminal ativo está em modo `passthrough` ou `alt-screen` (TUI ativa). */
    isTerminalPassthroughOrAltScreen: boolean;
    /** Indica se o foco está no input rico (`RichInput.vue`) e seu conteúdo não está vazio. */
    isRichInputNonEmptyFocused: boolean;
    /** Indica se a barra de busca local (`TerminalSearchBar.vue`) está aberta. */
    isLocalSearchOpen: boolean;
    /** Texto atual na barra de busca local, quando aberta. */
    localSearchQuery: string;
    /** Fecha a barra de busca local ao escalonar para a busca global. */
    closeLocalSearch?: () => void;
}

function inspectDomShortcutContext(terminalStore: ReturnType<typeof useTerminalStore>): SearchShortcutContext {
    const activeInst = terminalStore.activeInstance;
    const isAltScreen = Boolean(activeInst?.modes.altScreen);
    const isPassthrough = activeInst?.inputMode === 'passthrough';

    let isFocusInsideXterm = false;
    let isRichInputNonEmptyFocused = false;

    if (typeof document !== 'undefined' && document.activeElement instanceof HTMLElement) {
        const activeEl = document.activeElement;
        isFocusInsideXterm = Boolean(
            activeEl.closest('.xterm') || activeEl.classList.contains('xterm-helper-textarea')
        );

        const richInputEl = activeEl.closest('.rich-input')
            ? (activeEl as HTMLTextAreaElement | HTMLInputElement)
            : null;
        if (richInputEl && typeof richInputEl.value === 'string' && richInputEl.value.length > 0) {
            isRichInputNonEmptyFocused = true;
        }
    }

    const isTerminalPassthroughOrAltScreen =
        isAltScreen || (isFocusInsideXterm && isPassthrough);

    const isLocalSearchOpen = Boolean(activeInst?.search.open);
    const localSearchQuery = activeInst?.search.query ?? '';

    return {
        isTerminalPassthroughOrAltScreen,
        isRichInputNonEmptyFocused,
        isLocalSearchOpen,
        localSearchQuery,
        closeLocalSearch: () => {
            if (activeInst) {
                activeInst.search.open = false;
            }
        }
    };
}

/**
 * Avalia deterministicamente se `Ctrl+K` ou `Ctrl+Shift+F` devem abrir/escalonar para a Busca Global (`CA-11`).
 * Retorna `true` se o evento foi consumido pela Busca Global.
 */
export function handleGlobalSearchKeydown(
    event: KeyboardEvent,
    context: SearchShortcutContext,
    onOpenGlobalSearch: (initialQuery?: string) => void
): boolean {
    if (context.isTerminalPassthroughOrAltScreen) {
        return false;
    }

    if (isGlobalSearchShortcut(event)) {
        if (context.isRichInputNonEmptyFocused) {
            return false;
        }
        event.preventDefault();
        event.stopPropagation();
        onOpenGlobalSearch();
        return true;
    }

    if (isSearchShortcut(event) && context.isLocalSearchOpen) {
        event.preventDefault();
        event.stopPropagation();
        const queryToCarry = context.localSearchQuery;
        context.closeLocalSearch?.();
        onOpenGlobalSearch(queryToCarry);
        return true;
    }

    return false;
}

/**
 * Registra o listener global de `keydown` (em fase de captura) para `Ctrl+K` e escalonamento `Ctrl+Shift+F`.
 */
export function useSearchShortcut(onOpenCallback?: (initialQuery?: string) => void) {
    const searchStore = useSearchStore();
    const terminalStore = useTerminalStore();
    const projectsStore = useProjectsStore();

    function openWithDefaultScope(initialQuery?: string): void {
        if (onOpenCallback) {
            onOpenCallback(initialQuery);
            return;
        }
        searchStore.show(initialQuery, projectsStore.selectedId ?? null);
    }

    function onWindowKeydown(event: KeyboardEvent): void {
        const ctx = inspectDomShortcutContext(terminalStore);
        handleGlobalSearchKeydown(event, ctx, openWithDefaultScope);
    }

    onMounted(() => {
        if (typeof window !== 'undefined') {
            window.addEventListener('keydown', onWindowKeydown, { capture: true });
        }
    });

    onScopeDispose(() => {
        if (typeof window !== 'undefined') {
            window.removeEventListener('keydown', onWindowKeydown, { capture: true });
        }
    });

    return {
        onWindowKeydown
    };
}
