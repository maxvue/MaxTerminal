import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as sessionsIpc from '@/ipc/sessions';
import { useSessionsStore } from '@/stores/sessions';
import type { SessionSummary_Serialize } from '@/types/bindings';

vi.mock('@/ipc/sessions', () => ({
    sessionNew: vi.fn(),
    sessionList: vi.fn(),
    sessionGet: vi.fn(),
    sessionScrollbackGet: vi.fn(),
    sessionSearch: vi.fn(),
    onSessionStarted: vi.fn(),
    onSessionArchived: vi.fn()
}));

function makeSummary(overrides: Partial<SessionSummary_Serialize> = {}): SessionSummary_Serialize {
    return {
        id: 'tsess-1',
        tabId: 'tab-1',
        seq: 1,
        archived: false,
        startedAt: 1000,
        endedAt: null,
        endReason: null,
        commandCount: 2,
        failedCommandCount: 0,
        scrollbackBytes: 128,
        truncated: false,
        ...overrides
    };
}

describe('useSessionsStore (Fase 07)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('lista sessões ordenadas por seq decrescente e carrega detalhes/scrollback', async () => {
        const store = useSessionsStore();
        const s1 = makeSummary({ id: 'tsess-1', seq: 1, archived: true });
        const s2 = makeSummary({ id: 'tsess-2', seq: 2, archived: false });

        vi.mocked(sessionsIpc.sessionList).mockResolvedValueOnce({
            items: [s1, s2],
            nextCursor: null
        });

        const list = await store.loadSessions('tab-1', true);
        expect(list.map((s) => s.id)).toEqual(['tsess-2', 'tsess-1']);

        vi.mocked(sessionsIpc.sessionGet).mockResolvedValueOnce({
            summary: s1,
            panes: [
                {
                    sessionId: 'psess-1',
                    paneId: 'pane-1',
                    generation: 1,
                    cwd: '/tmp',
                    closedAt: 2000
                }
            ]
        });
        vi.mocked(sessionsIpc.sessionScrollbackGet).mockResolvedValueOnce({
            items: [
                {
                    sessionId: 'psess-1',
                    seq: 1,
                    ts: 1500,
                    text: 'linha sanitizada 1\n',
                    kind: 'output',
                    byteLen: 19
                }
            ],
            nextCursor: null
        });

        const detail = await store.loadSessionDetail('tsess-1');
        expect(detail.panes).toHaveLength(1);

        const chunks = await store.loadScrollback('tsess-1', 'psess-1', true);
        expect(chunks).toHaveLength(1);
        expect(chunks[0]?.text).toBe('linha sanitizada 1\n');
    });

    it('executa busca FTS5 na aba e limpa resultados quando query vazia', async () => {
        const store = useSessionsStore();
        vi.mocked(sessionsIpc.sessionSearch).mockResolvedValueOnce([
            {
                tabSessionId: 'tsess-1',
                sessionId: 'psess-1',
                paneId: 'pane-1',
                seq: 1,
                snippet: 'cargo test --workspace'
            }
        ]);

        const hits = await store.searchInTab('tab-1', 'cargo');
        expect(hits).toHaveLength(1);
        expect(hits[0]?.snippet).toBe('cargo test --workspace');

        const cleared = await store.searchInTab('tab-1', '   ');
        expect(cleared).toHaveLength(0);

        const sStarted = makeSummary({ id: 'tsess-3', tabId: 'tab-1', seq: 3, archived: false });
        store.handleSessionStarted(sStarted);
        expect(store.sessionsForTab('tab-1')[0]?.id).toBe('tsess-3');

        store.handleSessionArchived({ ...sStarted, archived: true, endedAt: 3000 });
        expect(store.sessionsForTab('tab-1')[0]?.archived).toBe(true);

        store.resetForTests();
        expect(store.sessionsForTab('tab-1')).toHaveLength(0);
    });
});
