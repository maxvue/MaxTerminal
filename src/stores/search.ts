import { defineStore } from 'pinia';
import { computed, ref, shallowRef } from 'vue';
import { isMtIpcError } from '@/ipc/core';
import { cancelSearch, releaseSearch, searchGlobal } from '@/ipc/search';
import type {
    MtError,
    SearchFilters_Deserialize,
    SearchHit_Serialize,
    SearchSection_Serialize,
    SearchSort,
    SearchSourceKind
} from '@/types/bindings';

export type SearchUiStatus = 'idle' | 'debouncing' | 'loading' | 'ready' | 'empty' | 'error';

export const DEFAULT_SEARCH_DEBOUNCE_MS = 120;
export const DEFAULT_SEARCH_PAGE_SIZE = 25;

/** Rótulos canônicos em `pt-BR` para as 9 seções da busca global (`CA-15`). */
export const SEARCH_SECTION_LABELS: Readonly<Record<SearchSourceKind['kind'], string>> =
    Object.freeze({
        commands: 'Blocos de comando',
        output: 'Histórico de sessões',
        sessions: 'Sessões',
        projects: 'Projetos',
        templates: 'Templates',
        groups: 'Grupos de comandos',
        group_runs: 'Execuções de grupos',
        issues: 'Issues',
        comments: 'Comentários de issues'
    });

/** Ordem canônica das 9 fontes da busca global. */
export const ALL_SEARCH_SOURCE_KINDS: readonly SearchSourceKind['kind'][] = Object.freeze([
    'commands',
    'output',
    'sessions',
    'projects',
    'templates',
    'groups',
    'group_runs',
    'issues',
    'comments'
]);

export function createDefaultSearchFilters(): SearchFilters_Deserialize {
    return {
        projectId: null,
        tabId: null,
        sessionId: null,
        paneId: null,
        fromMs: null,
        untilMs: null,
        exit: { kind: 'any' },
        sources: []
    };
}

function toMtError(err: unknown): MtError {
    if (isMtIpcError(err)) {
        return {
            code: err.code,
            message: err.message,
            details: (err.details as never) ?? undefined
        };
    }
    if (err && typeof err === 'object' && 'code' in err && 'message' in err) {
        const candidate = err as { code: unknown; message: unknown; details?: unknown };
        if (typeof candidate.code === 'string' && typeof candidate.message === 'string') {
            return {
                code: candidate.code,
                message: candidate.message,
                details: (candidate.details as never) ?? undefined
            };
        }
    }
    if (err instanceof Error) {
        return {
            code: 'search.busy',
            message: err.message
        };
    }
    return {
        code: 'search.busy',
        message: 'Falha ao executar busca global.'
    };
}

function makeRequestId(generation: number): string {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
        return `search-${generation}-${crypto.randomUUID()}`;
    }
    return `search-${generation}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
}

export const useSearchStore = defineStore('search', () => {
    const open = ref<boolean>(false);
    const query = ref<string>('');
    const filters = ref<SearchFilters_Deserialize>(createDefaultSearchFilters());
    const sort = ref<SearchSort>({ kind: 'relevance' });
    const generation = ref<number>(0);
    const activeRequestId = ref<string | null>(null);
    const snapshotId = ref<string | null>(null);
    const sections = shallowRef<SearchSection_Serialize[]>([]);
    const selectedKey = ref<string | null>(null);
    const status = ref<SearchUiStatus>('idle');
    const error = ref<MtError | null>(null);
    const loadingMoreSources = ref<Set<SearchSourceKind['kind']>>(new Set());
    const isComposingIme = ref<boolean>(false);
    const indexedThroughMs = ref<number | null>(null);

    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    let pendingImeRun = false;
    const inFlightLoadMoreKeys = new Set<string>();

    const flatItems = computed<SearchHit_Serialize[]>(() =>
        sections.value.flatMap((section) => section.items)
    );

    const selectedHit = computed<SearchHit_Serialize | null>(() => {
        if (!selectedKey.value) {
            return null;
        }
        return flatItems.value.find((item) => item.key === selectedKey.value) ?? null;
    });

    function clearDebounceTimer(): void {
        if (debounceTimer !== null) {
            clearTimeout(debounceTimer);
            debounceTimer = null;
        }
    }

    function cancelAndReleaseCurrent(): void {
        clearDebounceTimer();
        inFlightLoadMoreKeys.clear();
        loadingMoreSources.value = new Set();

        const prevReq = activeRequestId.value;
        activeRequestId.value = null;
        if (prevReq) {
            void cancelSearch(prevReq).catch(() => {});
        }

        const prevSnap = snapshotId.value;
        snapshotId.value = null;
        if (prevSnap) {
            void releaseSearch(prevSnap).catch(() => {});
        }
    }

    function scheduleDebouncedRun(debounceMs = DEFAULT_SEARCH_DEBOUNCE_MS): void {
        clearDebounceTimer();
        if (query.value.trim().length === 0) {
            sections.value = [];
            selectedKey.value = null;
            status.value = 'idle';
            error.value = null;
            return;
        }

        if (isComposingIme.value) {
            pendingImeRun = true;
            status.value = 'debouncing';
            return;
        }

        status.value = 'debouncing';
        error.value = null;

        if (debounceMs <= 0) {
            void run();
            return;
        }

        debounceTimer = setTimeout(() => {
            debounceTimer = null;
            void run();
        }, debounceMs);
    }

    function show(initialQuery?: string, initialProjectId?: string | null): void {
        open.value = true;
        if (initialProjectId !== undefined) {
            filters.value = {
                ...filters.value,
                projectId: initialProjectId
            };
        }
        if (typeof initialQuery === 'string') {
            updateQuery(initialQuery, DEFAULT_SEARCH_DEBOUNCE_MS);
        } else if (query.value.trim().length > 0 && sections.value.length === 0) {
            scheduleDebouncedRun(DEFAULT_SEARCH_DEBOUNCE_MS);
        }
    }

    function updateQuery(nextQuery: string, debounceMs = DEFAULT_SEARCH_DEBOUNCE_MS): void {
        query.value = nextQuery;
        generation.value += 1;
        cancelAndReleaseCurrent();
        scheduleDebouncedRun(debounceMs);
    }

    function updateFilters(
        partial: Partial<SearchFilters_Deserialize>,
        debounceMs = DEFAULT_SEARCH_DEBOUNCE_MS
    ): void {
        filters.value = {
            ...filters.value,
            ...partial
        };
        generation.value += 1;
        cancelAndReleaseCurrent();
        scheduleDebouncedRun(debounceMs);
    }

    function updateSort(nextSort: SearchSort, debounceMs = DEFAULT_SEARCH_DEBOUNCE_MS): void {
        sort.value = nextSort;
        generation.value += 1;
        cancelAndReleaseCurrent();
        scheduleDebouncedRun(debounceMs);
    }

    function setComposingIme(composing: boolean): void {
        const wasComposing = isComposingIme.value;
        isComposingIme.value = composing;
        if (wasComposing && !composing && pendingImeRun) {
            pendingImeRun = false;
            scheduleDebouncedRun(DEFAULT_SEARCH_DEBOUNCE_MS);
        }
    }

    async function run(): Promise<void> {
        clearDebounceTimer();
        const trimmed = query.value.trim();
        if (trimmed.length === 0) {
            sections.value = [];
            selectedKey.value = null;
            status.value = 'idle';
            error.value = null;
            return;
        }

        const myGen = generation.value;
        const reqId = makeRequestId(myGen);
        activeRequestId.value = reqId;
        status.value = 'loading';
        error.value = null;

        try {
            const response = await searchGlobal({
                requestId: reqId,
                generation: myGen,
                query: query.value,
                filters: {
                    ...filters.value,
                    sources: [...filters.value.sources]
                },
                sort: sort.value,
                pageSize: DEFAULT_SEARCH_PAGE_SIZE,
                cursor: null
            });

            if (!open.value || generation.value !== myGen || activeRequestId.value !== reqId) {
                if (response.snapshotId) {
                    void releaseSearch(response.snapshotId).catch(() => {});
                }
                return;
            }

            activeRequestId.value = null;
            snapshotId.value = response.snapshotId;
            indexedThroughMs.value = response.indexedThroughMs ?? null;
            sections.value = response.sections;

            const allItems = response.sections.flatMap((s) => s.items);
            if (allItems.length === 0) {
                selectedKey.value = null;
                status.value = 'empty';
            } else {
                const keepsSelection =
                    selectedKey.value !== null &&
                    allItems.some((item) => item.key === selectedKey.value);
                selectedKey.value = keepsSelection ? selectedKey.value : (allItems[0]?.key ?? null);
                status.value = 'ready';
            }
        } catch (err: unknown) {
            if (!open.value || generation.value !== myGen || activeRequestId.value !== reqId) {
                return;
            }
            activeRequestId.value = null;
            const mtErr = toMtError(err);
            if (mtErr.code === 'search.cancelled') {
                return;
            }
            error.value = mtErr;
            status.value = 'error';
        }
    }

    async function loadMore(source: SearchSourceKind): Promise<void> {
        const myGen = generation.value;
        const mySnapshotId = snapshotId.value;
        if (!open.value || !mySnapshotId) {
            return;
        }

        const targetSection = sections.value.find((s) => s.source.kind === source.kind);
        if (!targetSection || !targetSection.nextCursor) {
            return;
        }

        const dedupKey = `${mySnapshotId}:${source.kind}:${targetSection.nextCursor}`;
        if (inFlightLoadMoreKeys.has(dedupKey)) {
            return;
        }

        inFlightLoadMoreKeys.add(dedupKey);
        const nextSet = new Set(loadingMoreSources.value);
        nextSet.add(source.kind);
        loadingMoreSources.value = nextSet;

        const reqId = makeRequestId(myGen);

        try {
            const response = await searchGlobal({
                requestId: reqId,
                generation: myGen,
                query: query.value,
                filters: {
                    ...filters.value,
                    sources: [...filters.value.sources]
                },
                sort: sort.value,
                pageSize: DEFAULT_SEARCH_PAGE_SIZE,
                cursor: targetSection.nextCursor
            });

            if (
                !open.value ||
                generation.value !== myGen ||
                snapshotId.value !== mySnapshotId
            ) {
                return;
            }

            const incomingSection = response.sections.find((s) => s.source.kind === source.kind);
            if (!incomingSection) {
                return;
            }

            sections.value = sections.value.map((sec) => {
                if (sec.source.kind !== source.kind) {
                    return sec;
                }
                const existingKeys = new Set(sec.items.map((item) => item.key));
                const uniqueNewItems = incomingSection.items.filter(
                    (item) => !existingKeys.has(item.key)
                );
                return {
                    ...sec,
                    items: [...sec.items, ...uniqueNewItems],
                    nextCursor: incomingSection.nextCursor,
                    truncated: incomingSection.truncated
                };
            });
        } catch (err: unknown) {
            if (
                !open.value ||
                generation.value !== myGen ||
                snapshotId.value !== mySnapshotId
            ) {
                return;
            }
            const mtErr = toMtError(err);
            if (mtErr.code === 'search.cancelled') {
                return;
            }
            error.value = mtErr;
            status.value = 'error';
        } finally {
            inFlightLoadMoreKeys.delete(dedupKey);
            if (generation.value === myGen && snapshotId.value === mySnapshotId) {
                const updatedSet = new Set(loadingMoreSources.value);
                updatedSet.delete(source.kind);
                loadingMoreSources.value = updatedSet;
            }
        }
    }

    function select(key: string | null): void {
        selectedKey.value = key;
    }

    function moveSelection(delta: 1 | -1): void {
        const items = flatItems.value;
        if (items.length === 0) {
            selectedKey.value = null;
            return;
        }
        const currentIdx = selectedKey.value
            ? items.findIndex((item) => item.key === selectedKey.value)
            : -1;
        if (currentIdx === -1) {
            selectedKey.value = delta > 0 ? (items[0]?.key ?? null) : (items[items.length - 1]?.key ?? null);
            return;
        }
        const nextIdx = (currentIdx + delta + items.length) % items.length;
        selectedKey.value = items[nextIdx]?.key ?? null;
    }

    function hide(): void {
        open.value = false;
        generation.value += 1;
        cancelAndReleaseCurrent();
        if (status.value === 'loading' || status.value === 'debouncing') {
            status.value = sections.value.length > 0 ? 'ready' : 'idle';
        }
    }

    function invalidate(_reason?: string): void {
        generation.value += 1;
        cancelAndReleaseCurrent();
        if (open.value && query.value.trim().length > 0) {
            scheduleDebouncedRun(DEFAULT_SEARCH_DEBOUNCE_MS);
        } else {
            sections.value = [];
            selectedKey.value = null;
            status.value = 'idle';
        }
    }

    function dispose(): void {
        open.value = false;
        generation.value += 1;
        cancelAndReleaseCurrent();
        query.value = '';
        filters.value = createDefaultSearchFilters();
        sort.value = { kind: 'relevance' };
        sections.value = [];
        selectedKey.value = null;
        status.value = 'idle';
        error.value = null;
        isComposingIme.value = false;
        pendingImeRun = false;
        indexedThroughMs.value = null;
    }

    return {
        open,
        query,
        filters,
        sort,
        generation,
        activeRequestId,
        snapshotId,
        sections,
        selectedKey,
        selectedHit,
        status,
        error,
        loadingMoreSources,
        isComposingIme,
        indexedThroughMs,
        flatItems,
        show,
        updateQuery,
        updateFilters,
        updateSort,
        setComposingIme,
        run,
        loadMore,
        select,
        moveSelection,
        hide,
        invalidate,
        dispose
    };
});
