import { defineStore } from 'pinia';
import { ref } from 'vue';
import { useIpcError } from '@/composables/useIpcError';
import * as sessionsIpc from '@/ipc/sessions';
import { useTabsStore } from '@/stores/tabs';
import type {
    ScrollbackChunk,
    SessionDetail_Serialize,
    SessionId,
    SessionSearchHit,
    SessionSummary_Serialize,
    TabId,
    TabSessionId,
    TabView_Serialize
} from '@/types/bindings';

export const useSessionsStore = defineStore('sessions', () => {
    const tabsStore = useTabsStore();
    const ipcError = useIpcError();

    const summariesByTab = ref<Record<TabId, SessionSummary_Serialize[]>>({});
    const nextCursorByTab = ref<Record<TabId, string | null>>({});
    const detailsById = ref<Record<TabSessionId, SessionDetail_Serialize>>({});
    const scrollbackByPaneSession = ref<Record<SessionId, ScrollbackChunk[]>>({});
    const scrollbackCursorByPaneSession = ref<Record<SessionId, string | null>>({});
    const searchHitsByTab = ref<Record<TabId, SessionSearchHit[]>>({});
    const loading = ref<boolean>(false);
    const error = ref<string | null>(null);

    function sessionsForTab(tabId: TabId): SessionSummary_Serialize[] {
        return summariesByTab.value[tabId] ?? [];
    }

    function detailFor(tabSessionId: TabSessionId): SessionDetail_Serialize | null {
        return detailsById.value[tabSessionId] ?? null;
    }

    function scrollbackFor(sessionId: SessionId): ScrollbackChunk[] {
        return scrollbackByPaneSession.value[sessionId] ?? [];
    }

    function upsertSummary(summary: SessionSummary_Serialize): void {
        const list = [...(summariesByTab.value[summary.tabId] ?? [])];
        const idx = list.findIndex((s) => s.id === summary.id);
        if (idx !== -1) {
            list[idx] = summary;
        } else {
            list.push(summary);
        }
        list.sort((a, b) => b.seq - a.seq);
        summariesByTab.value = {
            ...summariesByTab.value,
            [summary.tabId]: list
        };

        const existingDetail = detailsById.value[summary.id];
        if (existingDetail) {
            detailsById.value = {
                ...detailsById.value,
                [summary.id]: {
                    ...existingDetail,
                    summary
                }
            };
        }
    }

    async function startNewSession(tabId: TabId): Promise<TabView_Serialize> {
        loading.value = true;
        error.value = null;
        try {
            const updatedTab = await tabsStore.startNewTabSession(tabId);
            await loadSessions(tabId, true);
            return updatedTab;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function loadSessions(
        tabId: TabId,
        reset = true,
        limit = 30
    ): Promise<SessionSummary_Serialize[]> {
        const cursor = reset ? null : (nextCursorByTab.value[tabId] ?? null);
        if (!reset && cursor === null) {
            return summariesByTab.value[tabId] ?? [];
        }

        loading.value = true;
        error.value = null;
        try {
            const page = await sessionsIpc.sessionList(tabId, cursor, limit);
            const merged = reset
                ? page.items
                : [...(summariesByTab.value[tabId] ?? []), ...page.items];
            const dedup = new Map<TabSessionId, SessionSummary_Serialize>();
            for (const item of merged) {
                dedup.set(item.id, item);
            }
            const sorted = Array.from(dedup.values()).sort((a, b) => b.seq - a.seq);
            summariesByTab.value = {
                ...summariesByTab.value,
                [tabId]: sorted
            };
            nextCursorByTab.value = {
                ...nextCursorByTab.value,
                [tabId]: page.nextCursor
            };
            return sorted;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function loadSessionDetail(
        tabSessionId: TabSessionId
    ): Promise<SessionDetail_Serialize> {
        loading.value = true;
        error.value = null;
        try {
            const detail = await sessionsIpc.sessionGet(tabSessionId);
            detailsById.value = {
                ...detailsById.value,
                [tabSessionId]: detail
            };
            upsertSummary(detail.summary);
            return detail;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function loadScrollback(
        tabSessionId: TabSessionId,
        sessionId: SessionId,
        reset = true,
        limit = 100
    ): Promise<ScrollbackChunk[]> {
        const cursor = reset ? null : (scrollbackCursorByPaneSession.value[sessionId] ?? null);
        if (!reset && cursor === null) {
            return scrollbackByPaneSession.value[sessionId] ?? [];
        }

        loading.value = true;
        error.value = null;
        try {
            const page = await sessionsIpc.sessionScrollbackGet(
                tabSessionId,
                sessionId,
                cursor,
                limit
            );
            const merged = reset
                ? page.items
                : [...(scrollbackByPaneSession.value[sessionId] ?? []), ...page.items];
            const bySeq = new Map<number, ScrollbackChunk>();
            for (const chunk of merged) {
                bySeq.set(chunk.seq, chunk);
            }
            const sorted = Array.from(bySeq.values()).sort((a, b) => a.seq - b.seq);
            scrollbackByPaneSession.value = {
                ...scrollbackByPaneSession.value,
                [sessionId]: sorted
            };
            scrollbackCursorByPaneSession.value = {
                ...scrollbackCursorByPaneSession.value,
                [sessionId]: page.nextCursor
            };
            return sorted;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function searchInTab(
        tabId: TabId,
        query: string,
        limit = 50
    ): Promise<SessionSearchHit[]> {
        const trimmed = query.trim();
        if (!trimmed) {
            searchHitsByTab.value = {
                ...searchHitsByTab.value,
                [tabId]: []
            };
            return [];
        }

        loading.value = true;
        error.value = null;
        try {
            const hits = await sessionsIpc.sessionSearch(tabId, trimmed, limit);
            searchHitsByTab.value = {
                ...searchHitsByTab.value,
                [tabId]: hits
            };
            return hits;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    function handleSessionStarted(summary: SessionSummary_Serialize): void {
        upsertSummary(summary);
    }

    function handleSessionArchived(summary: SessionSummary_Serialize): void {
        upsertSummary(summary);
    }

    function resetForTests(): void {
        summariesByTab.value = {};
        nextCursorByTab.value = {};
        detailsById.value = {};
        scrollbackByPaneSession.value = {};
        scrollbackCursorByPaneSession.value = {};
        searchHitsByTab.value = {};
        loading.value = false;
        error.value = null;
    }

    return {
        summariesByTab,
        nextCursorByTab,
        detailsById,
        scrollbackByPaneSession,
        scrollbackCursorByPaneSession,
        searchHitsByTab,
        loading,
        error,
        sessionsForTab,
        detailFor,
        scrollbackFor,
        startNewSession,
        loadSessions,
        loadSessionDetail,
        loadScrollback,
        searchInTab,
        handleSessionStarted,
        handleSessionArchived,
        resetForTests
    };
});
