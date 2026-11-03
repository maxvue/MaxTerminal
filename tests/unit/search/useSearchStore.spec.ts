import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { SearchHit_Serialize, SearchResponse_Serialize } from '@/types/bindings';

const searchGlobalMock = vi.fn();
const cancelSearchMock = vi.fn();
const releaseSearchMock = vi.fn();

vi.mock('@/ipc/search', () => ({
    searchGlobal: (...args: unknown[]) => searchGlobalMock(...args),
    cancelSearch: (...args: unknown[]) => cancelSearchMock(...args),
    releaseSearch: (...args: unknown[]) => releaseSearchMock(...args),
    getSearchIndexStats: vi.fn(),
    rebuildSearchIndex: vi.fn(),
    onSearchIndexProgress: vi.fn(async () => () => {}),
    onSearchInvalidated: vi.fn(async () => () => {})
}));

import { useSearchStore } from '@/stores/search';

function makeHit(key: string, titleText: string): SearchHit_Serialize {
    return {
        key,
        source: { kind: 'commands' },
        target: {
            kind: 'command',
            projectId: 'proj-1',
            tabId: 'tab-1',
            sessionId: 'sess-1',
            paneId: 'pane-1',
            blockId: 'blk-1'
        },
        title: [{ text: titleText, matched: true }],
        excerpt: [{ text: 'saída do comando', matched: false }],
        occurredAtMs: 1_700_000_000_000,
        sourceTruncated: false,
        fetchedAtMs: null
    };
}

function makeResponse(
    requestId: string,
    generation: number,
    snapshotId: string,
    hits: SearchHit_Serialize[],
    nextCursor: string | null = null
): SearchResponse_Serialize {
    return {
        requestId,
        generation,
        snapshotId,
        expiresAtMs: 1_700_000_120_000,
        sections:
            hits.length > 0
                ? [
                    {
                        source: { kind: 'commands' },
                        items: hits,
                        nextCursor,
                        truncated: false
                    }
                ]
                : [],
        indexedThroughMs: 1_700_000_000_000
    };
}

describe('useSearchStore (CA-09)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.useFakeTimers();
        searchGlobalMock.mockReset();
        cancelSearchMock.mockReset().mockResolvedValue({ cancelled: true });
        releaseSearchMock.mockReset().mockResolvedValue(undefined);
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('dispara apenas 1 chamada searchGlobal após 10 teclas espaçadas por 30 ms com debounce de 120 ms', async () => {
        const store = useSearchStore();
        store.show();

        searchGlobalMock.mockImplementation(async (req: { requestId: string; generation: number }) =>
            makeResponse(req.requestId, req.generation, 'snap-final', [
                makeHit('commands:1', 'git status')
            ])
        );

        const steps = ['g', 'gi', 'git', 'git ', 'git s', 'git st', 'git sta', 'git stat', 'git statu', 'git status'];
        for (let i = 0; i < steps.length; i++) {
            store.updateQuery(steps[i]!);
            if (i < steps.length - 1) {
                await vi.advanceTimersByTimeAsync(30);
            }
        }

        // Durante os 270ms de digitação, nenhuma chamada IPC deve ter ocorrido
        expect(searchGlobalMock).toHaveBeenCalledTimes(0);

        // Avança 119ms após a 10ª tecla: ainda não disparou
        await vi.advanceTimersByTimeAsync(119);
        expect(searchGlobalMock).toHaveBeenCalledTimes(0);

        // No 120º ms dispara exatamente 1 chamada
        await vi.advanceTimersByTimeAsync(1);
        expect(searchGlobalMock).toHaveBeenCalledTimes(1);
        expect(store.status).toBe('ready');
        expect(store.flatItems).toHaveLength(1);
        expect(store.selectedKey).toBe('commands:1');
    });

    it('descarta respostas atrasadas de geração anterior e libera seus snapshots órfãos', async () => {
        const store = useSearchStore();
        store.show();

        let resolveA!: (val: SearchResponse_Serialize) => void;
        let resolveB!: (val: SearchResponse_Serialize) => void;

        searchGlobalMock
            .mockImplementationOnce(
                () =>
                    new Promise<SearchResponse_Serialize>((res) => {
                        resolveA = res;
                    })
            )
            .mockImplementationOnce(
                () =>
                    new Promise<SearchResponse_Serialize>((res) => {
                        resolveB = res;
                    })
            );

        store.updateQuery('primeira', 0);
        const genA = store.generation;

        store.updateQuery('segunda', 0);
        const genB = store.generation;
        expect(genB).toBeGreaterThan(genA);
        expect(cancelSearchMock).toHaveBeenCalledTimes(1);

        // Resolve B primeiro
        resolveB(makeResponse('req-b', genB, 'snap-B', [makeHit('commands:b', 'segunda')]));
        await Promise.resolve();
        await Promise.resolve();

        expect(store.snapshotId).toBe('snap-B');
        expect(store.flatItems[0]?.key).toBe('commands:b');

        // Agora A resolve atrasada: deve ser descartada e seu snapshot 'snap-A' liberado!
        resolveA(makeResponse('req-a', genA, 'snap-A', [makeHit('commands:a', 'primeira')]));
        await Promise.resolve();
        await Promise.resolve();

        expect(store.snapshotId).toBe('snap-B');
        expect(store.flatItems[0]?.key).toBe('commands:b');
        expect(releaseSearchMock).toHaveBeenCalledWith('snap-A');
    });

    it('bloqueia disparo durante composição IME e executa ao encerrar composição', async () => {
        const store = useSearchStore();
        store.show();
        searchGlobalMock.mockImplementation(async (req: { requestId: string; generation: number }) =>
            makeResponse(req.requestId, req.generation, 'snap-ime', [makeHit('commands:ime', 'ação')])
        );

        store.setComposingIme(true);
        store.updateQuery('ação');
        await vi.advanceTimersByTimeAsync(300);
        expect(searchGlobalMock).toHaveBeenCalledTimes(0);

        store.setComposingIme(false);
        await vi.advanceTimersByTimeAsync(120);
        expect(searchGlobalMock).toHaveBeenCalledTimes(1);
        expect(store.status).toBe('ready');
    });

    it('pagina loadMore de maneira idempotente sem duplicar itens', async () => {
        const store = useSearchStore();
        store.show();

        searchGlobalMock.mockResolvedValueOnce(
            makeResponse('req-1', 1, 'snap-page', [makeHit('commands:1', 'item 1')], 'cursor-p2')
        );

        store.updateQuery('item', 0);
        await Promise.resolve();
        await Promise.resolve();

        expect(store.sections[0]?.items).toHaveLength(1);

        let resolveMore!: (val: SearchResponse_Serialize) => void;
        searchGlobalMock.mockImplementationOnce(
            () =>
                new Promise<SearchResponse_Serialize>((res) => {
                    resolveMore = res;
                })
        );

        // Dois cliques simultâneos em "Carregar mais" com o mesmo cursor disparam apenas 1 chamada IPC
        const p1 = store.loadMore({ kind: 'commands' });
        const p2 = store.loadMore({ kind: 'commands' });
        expect(searchGlobalMock).toHaveBeenCalledTimes(2);

        resolveMore(
            makeResponse(
                'req-2',
                store.generation,
                'snap-page',
                [makeHit('commands:1', 'item 1'), makeHit('commands:2', 'item 2')],
                null
            )
        );
        await Promise.all([p1, p2]);

        expect(store.sections[0]?.items.map((i) => i.key)).toEqual(['commands:1', 'commands:2']);
        expect(store.sections[0]?.nextCursor).toBeNull();
    });

    it('exercita updateFilters, updateSort, moveSelection, select, hide, invalidate, dispose e tratamento de erros', async () => {
        const store = useSearchStore();
        store.show('cargo', 'proj-1');
        expect(store.filters.projectId).toBe('proj-1');

        searchGlobalMock.mockResolvedValueOnce(
            makeResponse('req-nav', 1, 'snap-nav', [
                makeHit('commands:1', 'cargo check'),
                makeHit('commands:2', 'cargo test')
            ])
        );

        await vi.advanceTimersByTimeAsync(120);
        expect(store.status).toBe('ready');
        expect(store.selectedHit?.key).toBe('commands:1');

        store.moveSelection(1);
        expect(store.selectedKey).toBe('commands:2');
        store.moveSelection(1);
        expect(store.selectedKey).toBe('commands:1');
        store.moveSelection(-1);
        expect(store.selectedKey).toBe('commands:2');

        store.select('inexistente');
        store.moveSelection(-1);
        expect(store.selectedKey).toBe('commands:2');
        store.select(null);
        store.moveSelection(1);
        expect(store.selectedKey).toBe('commands:1');

        searchGlobalMock.mockResolvedValueOnce(makeResponse('req-empty', 2, 'snap-empty', []));
        store.updateFilters({ exit: { kind: 'nonzero' } }, 0);
        await Promise.resolve();
        await Promise.resolve();
        expect(store.status).toBe('empty');
        expect(store.selectedKey).toBeNull();
        store.moveSelection(1);
        expect(store.selectedKey).toBeNull();

        searchGlobalMock.mockRejectedValueOnce(new Error('Falha de índice'));
        store.updateSort({ kind: 'recent' }, 0);
        await Promise.resolve();
        await Promise.resolve();
        expect(store.status).toBe('error');
        expect(store.error?.message).toBe('Falha de índice');

        searchGlobalMock.mockRejectedValueOnce({ code: 'search.cancelled', message: 'Cancelado' });
        store.updateQuery('ignorado', 0);
        await Promise.resolve();
        await Promise.resolve();

        store.invalidate('manual');
        store.hide();
        expect(store.open).toBe(false);
        store.invalidate('closed');
        expect(store.status).toBe('idle');

        store.dispose();
        expect(store.query).toBe('');
        expect(store.sections).toHaveLength(0);
    });
});
