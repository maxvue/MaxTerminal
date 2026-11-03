import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type {
    PtyCleanupFailedEvent,
    PtyExitEvent,
    PtyForegroundChangedEvent,
    PtySpawnedEvent,
    PtySummary
} from '@/types/bindings';

const invokeMock = vi.fn();
const eventHandlers = new Map<string, Array<(event: { payload: unknown }) => void>>();
const unlistenSpies: Array<ReturnType<typeof vi.fn>> = [];

vi.mock('@tauri-apps/api/core', () => ({
    invoke: (...args: unknown[]) => invokeMock(...args),
    Channel: class {}
}));

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn((eventName: string, cb: (event: { payload: unknown }) => void) => {
        const list = eventHandlers.get(eventName) ?? [];
        list.push(cb);
        eventHandlers.set(eventName, list);
        const unlisten = vi.fn(() => {
            const current = eventHandlers.get(eventName) ?? [];
            const idx = current.indexOf(cb);
            if (idx >= 0) {
                current.splice(idx, 1);
            }
        });
        unlistenSpies.push(unlisten);
        return Promise.resolve(unlisten);
    })
}));

import { usePtyStore } from '@/stores/pty';

function emitMockEvent(eventName: string, payload: unknown): void {
    const list = eventHandlers.get(eventName) ?? [];
    for (const handler of list) {
        handler({ payload });
    }
}

function makeSummary(overrides: Partial<PtySummary> = {}): PtySummary {
    return {
        id: '01960000-0000-7000-8000-000000000010',
        owner: { tabId: '01960000-0000-7000-8000-000000000099', paneId: null },
        shell: '/bin/sh',
        args: [],
        cwdInitial: '/tmp',
        size: { rows: 24, cols: 80, pixelWidth: 0, pixelHeight: 0 },
        status: { kind: 'running' },
        pids: { shellPid: 1001, pgid: 1001, sid: 1001, foregroundPgid: 1001 },
        attached: false,
        createdAtMs: '1730000000000',
        bytesOut: '0',
        seq: '0',
        revision: '1',
        ...overrides
    };
}

describe('usePtyStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        invokeMock.mockReset();
        eventHandlers.clear();
        unlistenSpies.length = 0;
    });

    it('registra listeners antes de pty_list, deduplica init() concorrente e reconcilia eventos por revision (CA-30, CA-32)', async () => {
        const store = usePtyStore();
        const ptyId = '01960000-0000-7000-8000-000000000010';

        let resolveList: ((items: PtySummary[]) => void) | null = null;
        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'pty_list') {
                return new Promise<PtySummary[]>((resolve) => {
                    resolveList = resolve;
                });
            }
            return Promise.resolve(null);
        });

        // Dispara duas chamadas concorrentes a init()
        const p1 = store.init();
        const p2 = store.init();

        // Aguarda registro dos 4 listeners
        for (let i = 0; i < 6; i++) {
            await Promise.resolve();
        }
        expect(unlistenSpies).toHaveLength(4);

        // Enquanto pty_list está em voo, chega um evento pty://exit com revision '5'
        const exitEvent: PtyExitEvent = {
            ptyId,
            owner: { tabId: '01960000-0000-7000-8000-000000000099', paneId: null },
            exit: {
                code: 0,
                signal: null,
                signalName: null,
                reason: 'exited',
                atMs: '1730000001000'
            },
            finalSeq: '12',
            revision: '5'
        };
        emitMockEvent('pty://exit', exitEvent);

        // Agora pty_list resolve com um snapshot antigo (revision '2', status 'running')
        expect(resolveList).not.toBeNull();
        resolveList!([makeSummary({ id: ptyId, revision: '2', status: { kind: 'running' } })]);

        await Promise.all([p1, p2]);
        expect(store.initialized).toBe(true);
        expect(invokeMock.mock.calls.filter((c) => c[0] === 'pty_list')).toHaveLength(1);

        // O estado reconciliado deve preservar o tombstone 'exited' (revision '5'), nunca regredindo para 'running'
        const stored = store.ptys[ptyId];
        expect(stored).toBeDefined();
        expect(stored?.status.kind).toBe('exited');
        expect(stored?.revision).toBe('5');
        expect(store.exitOf(ptyId)?.code).toBe(0);
        expect(store.alive).toHaveLength(0);
        expect(store.byTab('01960000-0000-7000-8000-000000000099')).toHaveLength(1);

        store.dispose();
    });

    it('mantém invariante terminal (exited nunca volta para running) e atualiza foreground e cleanupFailed', async () => {
        const store = usePtyStore();
        const ptyId = '01960000-0000-7000-8000-000000000020';

        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'pty_list') {
                return Promise.resolve([]);
            }
            return Promise.resolve(null);
        });

        await store.init();

        const spawned: PtySpawnedEvent = {
            summary: makeSummary({ id: ptyId, revision: '10' }),
            revision: '10'
        };
        emitMockEvent('pty://spawned', spawned);
        expect(store.alive).toHaveLength(1);

        // Evento foreground-changed com revisão maior ('11') atualiza foregroundPgid
        const fgChanged: PtyForegroundChangedEvent = {
            ptyId,
            pgid: 2048,
            process: null,
            revision: '11'
        };
        emitMockEvent('pty://foreground-changed', fgChanged);
        expect(store.ptys[ptyId]?.pids.foregroundPgid).toBe(2048);

        // Evento foreground-changed atrasado ('9') é ignorado
        emitMockEvent('pty://foreground-changed', {
            ptyId,
            pgid: 9999,
            process: null,
            revision: '9'
        } satisfies PtyForegroundChangedEvent);
        expect(store.ptys[ptyId]?.pids.foregroundPgid).toBe(2048);

        // Evento cleanup-failed ('12') mantém em alive
        const cleanupFailed: PtyCleanupFailedEvent = {
            ptyId,
            survivors: [{ bootId: 'boot-1', pid: 3333, startTime: '500' }],
            revision: '12'
        };
        emitMockEvent('pty://cleanup-failed', cleanupFailed);
        expect(store.ptys[ptyId]?.status.kind).toBe('cleanupFailed');
        expect(store.alive).toHaveLength(1);

        // Evento exit ('13') move para tombstone exited
        emitMockEvent('pty://exit', {
            ptyId,
            owner: { tabId: '01960000-0000-7000-8000-000000000099', paneId: null },
            exit: {
                code: 137,
                signal: 9,
                signalName: 'SIGKILL',
                reason: 'killedByUser',
                atMs: '1730000002000'
            },
            finalSeq: '42',
            revision: '13'
        } satisfies PtyExitEvent);

        expect(store.ptys[ptyId]?.status.kind).toBe('exited');
        expect(store.alive).toHaveLength(0);

        // Tentativa de aplicar summary running atrasado (mesmo com revisão maior artificial) não regride exited
        store.applySummaryWithRevision(
            makeSummary({ id: ptyId, revision: '99', status: { kind: 'running' } })
        );
        expect(store.ptys[ptyId]?.status.kind).toBe('exited');

        store.dispose();
        expect(store.initialized).toBe(false);
        for (const spy of unlistenSpies) {
            expect(spy).toHaveBeenCalledTimes(1);
        }
    });

    it('faz rollback de listeners se pty_list falhar durante init()', async () => {
        const store = usePtyStore();
        invokeMock.mockRejectedValueOnce({
            code: 'pty.io',
            message: 'Falha simulada em pty_list'
        });

        await expect(store.init()).rejects.toThrow('Falha simulada em pty_list');
        expect(store.initialized).toBe(false);
        expect(unlistenSpies).toHaveLength(4);
        for (const spy of unlistenSpies) {
            expect(spy).toHaveBeenCalledTimes(1);
        }
    });
});
