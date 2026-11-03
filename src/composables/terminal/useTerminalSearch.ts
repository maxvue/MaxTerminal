import {
    computed,
    onBeforeUnmount,
    ref,
    unref,
    watch,
    type ComputedRef,
    type Ref
} from 'vue';
import type { ISearchOptions } from '@xterm/addon-search';
import type { IDisposable } from '@xterm/xterm';
import type { PaneId } from '@/types/bindings';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { resolveCssToken } from '@/terminal/theme/defaultTheme';

/** Opções de busca incremental no scrollback do terminal. */
export interface SearchOptionsState {
    caseSensitive: boolean;
    wholeWord: boolean;
    regex: boolean;
}

/** Retorno do composable `useTerminalSearch`. */
export interface UseTerminalSearchReturn {
    query: Ref<string>;
    options: Ref<SearchOptionsState>;
    resultIndex: ComputedRef<number>;
    resultCount: ComputedRef<number>;
    limitExceeded: ComputedRef<boolean>;
    regexError: ComputedRef<string | null>;
    next: () => boolean;
    prev: () => boolean;
    close: () => void;
}

function normalizeHexForSearch(color: string, fallbackHex: string): string {
    if (/^#[0-9a-fA-F]{6}$/.test(color)) {
        return color;
    }
    return fallbackHex;
}

function buildSearchOptions(opts: SearchOptionsState, incremental: boolean): ISearchOptions {
    const matchBg = normalizeHexForSearch(resolveCssToken('--max-warning-700', '#b45309'), '#b45309');
    const matchBorder = normalizeHexForSearch(resolveCssToken('--max-warning-500', '#f59e0b'), '#f59e0b');
    const activeBg = normalizeHexForSearch(resolveCssToken('--max-primary-500', '#14b8a6'), '#14b8a6');
    const activeBorder = normalizeHexForSearch(resolveCssToken('--max-primary-300', '#5eead4'), '#5eead4');

    return {
        caseSensitive: opts.caseSensitive,
        wholeWord: opts.wholeWord,
        regex: opts.regex,
        incremental,
        decorations: {
            matchBackground: matchBg,
            matchBorder,
            matchOverviewRuler: matchBorder,
            activeMatchBackground: activeBg,
            activeMatchBorder: activeBorder,
            activeMatchColorOverviewRuler: activeBg
        }
    };
}

/**
 * Composable de busca incremental no buffer do `@xterm/xterm` usando `@xterm/addon-search`.
 */
export function useTerminalSearch(paneId: Ref<PaneId> | PaneId): UseTerminalSearchReturn {
    const store = useTerminalStore();
    const query = ref<string>(store.instance(unref(paneId))?.search.query ?? '');
    const options = ref<SearchOptionsState>({
        caseSensitive: false,
        wholeWord: false,
        regex: false
    });

    let resultsSub: IDisposable | null = null;

    function attachResultsListener(): void {
        resultsSub?.dispose();
        resultsSub = null;
        const currentPaneId = unref(paneId);
        const instance = terminalInstanceRegistry.get(currentPaneId);
        if (!instance?.searchAddon?.onDidChangeResults) {
            return;
        }
        resultsSub = instance.searchAddon.onDidChangeResults((ev) => {
            const currentSearch = store.instance(currentPaneId)?.search;
            if (!currentSearch) {
                return;
            }
            const limitExceeded = ev.resultIndex === -1 && ev.resultCount > 0;
            const oneBased = ev.resultIndex >= 0 ? ev.resultIndex + 1 : 0;
            store.patch(currentPaneId, {
                search: {
                    ...currentSearch,
                    resultIndex: oneBased,
                    resultCount: ev.resultCount,
                    limitExceeded
                }
            });
        });
    }

    attachResultsListener();

    function validateRegexIfNeeded(term: string): boolean {
        const currentPaneId = unref(paneId);
        const currentSearch = store.instance(currentPaneId)?.search;
        if (!options.value.regex || term.length === 0) {
            if (currentSearch && currentSearch.regexError) {
                store.patch(currentPaneId, {
                    search: {
                        ...currentSearch,
                        regexError: null
                    }
                });
            }
            return true;
        }

        try {
            new RegExp(term);
            if (currentSearch && currentSearch.regexError) {
                store.patch(currentPaneId, {
                    search: {
                        ...currentSearch,
                        regexError: null
                    }
                });
            }
            return true;
        } catch {
            if (currentSearch) {
                store.patch(currentPaneId, {
                    search: {
                        ...currentSearch,
                        resultIndex: 0,
                        resultCount: 0,
                        limitExceeded: false,
                        regexError: 'Expressão regular inválida'
                    }
                });
            }
            return false;
        }
    }

    function runSearch(direction: 'next' | 'prev', incremental: boolean): boolean {
        const currentPaneId = unref(paneId);
        const instance = terminalInstanceRegistry.get(currentPaneId);
        const term = query.value;
        const currentSearch = store.instance(currentPaneId)?.search;

        if (currentSearch) {
            store.patch(currentPaneId, {
                search: {
                    ...currentSearch,
                    query: term
                }
            });
        }

        if (!instance) {
            return false;
        }

        if (term.length === 0) {
            instance.searchAddon.clearDecorations?.();
            if (currentSearch) {
                store.patch(currentPaneId, {
                    search: {
                        ...currentSearch,
                        query: '',
                        resultIndex: 0,
                        resultCount: 0,
                        limitExceeded: false,
                        regexError: null
                    }
                });
            }
            return false;
        }

        if (!validateRegexIfNeeded(term)) {
            instance.searchAddon.clearDecorations?.();
            return false;
        }

        const searchOpts = buildSearchOptions(options.value, incremental);
        try {
            if (direction === 'next') {
                return instance.searchAddon.findNext(term, searchOpts);
            }
            return instance.searchAddon.findPrevious(term, searchOpts);
        } catch {
            return false;
        }
    }

    watch(
        [query, options],
        () => {
            attachResultsListener();
            runSearch('next', true);
        },
        { deep: true }
    );

    function next(): boolean {
        return runSearch('next', false);
    }

    function prev(): boolean {
        return runSearch('prev', false);
    }

    function close(): void {
        const currentPaneId = unref(paneId);
        const instance = terminalInstanceRegistry.get(currentPaneId);
        instance?.searchAddon?.clearDecorations?.();
        store.closeSearch(currentPaneId);
        instance?.focus();
    }

    onBeforeUnmount(() => {
        resultsSub?.dispose();
        resultsSub = null;
    });

    const resultIndex = computed<number>(() => store.instance(unref(paneId))?.search.resultIndex ?? 0);
    const resultCount = computed<number>(() => store.instance(unref(paneId))?.search.resultCount ?? 0);
    const limitExceeded = computed<boolean>(() => Boolean(store.instance(unref(paneId))?.search.limitExceeded));
    const regexError = computed<string | null>(() => store.instance(unref(paneId))?.search.regexError ?? null);

    return {
        query,
        options,
        resultIndex,
        resultCount,
        limitExceeded,
        regexError,
        next,
        prev,
        close
    };
}
