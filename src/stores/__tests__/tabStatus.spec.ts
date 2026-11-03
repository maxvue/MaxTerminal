import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { TabStateSnapshot_Serialize } from '@/types/bindings';

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

import { useTabStatusStore } from '@/stores/tabStatus';

function emitMockEvent(eventName: string, payload: unknown): void {
    const list = eventHandlers.get(eventName) ?? [];
    for (const handler of list) {
        handler({ payload });
    }
}

function makeSnapshot(overrides: Partial<TabStateSnapshot_Serialize> = {}): TabStateSnapshot_Serialize {
    return {
        tabId: '01960000-0000-7000-8000-000000000101',
        revision: 1,
        status: 'idle',
        focused: false,
        panes: [
            {
                paneId: '01960000-0000-7000-8000-000000000201',
                ptyId: '01960000-0000-7000-8000-000000000301',
                sessionId: '01960000-0000-7000-8000-000000000401',
                status: 'idle',
                integrationMode: 'osc133',
                continuousPolicy: 'auto',
                cwd: '/tmp',
                activeExecution: null,
                lastCompletion: null,
                updatedAt: 1730000000000
            }
        ],
        lastCompletion: null,
        attention: null,
        updatedAt: 1730000000000,
        ...overrides
    };
}

describe('useTabStatusStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        const store = useTabStatusStore();
        store.resetForTests();
        invokeMock.mockReset();
        eventHandlers.clear();
        unlistenSpies.length = 0;
    });

    it('assina eventos antes de tab_state_list, reconcilia revisões e ignora revisões antigas', async () => {
        const store = useTabStatusStore();
        const tabId = '01960000-0000-7000-8000-000000000101';

        let resolveList: ((items: TabStateSnapshot_Serialize[]) => void) | null = null;
        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'tab_state_list') {
                return new Promise<TabStateSnapshot_Serialize[]>((resolve) => {
                    resolveList = resolve;
                });
            }
            return Promise.resolve(null);
        });

        const p1 = store.init();
        const p2 = store.init();

        for (let i = 0; i < 6; i++) {
            await Promise.resolve();
        }
        expect(unlistenSpies).toHaveLength(2);

        // Evento mais recente chega enquanto tab_state_list está em voo
        emitMockEvent(
            'tab://state-changed',
            makeSnapshot({ tabId, revision: 5, status: 'error_unseen' })
        );

        resolveList!([makeSnapshot({ tabId, revision: 2, status: 'running' })]);
        await Promise.all([p1, p2]);

        expect(store.initialized).toBe(true);
        expect(store.statusOf(tabId)).toBe('error_unseen');
        expect(store.snapshotOf(tabId)?.revision).toBe(5);

        // Evento atrasado com revisão menor é descartado
        emitMockEvent(
            'tab://state-changed',
            makeSnapshot({ tabId, revision: 3, status: 'idle' })
        );
        expect(store.statusOf(tabId)).toBe('error_unseen');
    });

    it('respeita tombstones de tab://state-removed contra eventos atrasados e calcula countsFor deduplicado', () => {
        const store = useTabStatusStore();
        const tab1 = '01960000-0000-7000-8000-000000000101';
        const tab2 = '01960000-0000-7000-8000-000000000102';
        const tab3 = '01960000-0000-7000-8000-000000000103';

        store.applySnapshot(makeSnapshot({ tabId: tab1, revision: 2, status: 'running' }));
        store.applySnapshot(makeSnapshot({ tabId: tab2, revision: 4, status: 'completed_unseen' }));
        store.applySnapshot(makeSnapshot({ tabId: tab3, revision: 3, status: 'continuous' }));

        // Remove tab3 na revisão 6
        store.handleRemoved({ tabId: tab3, revision: 6 });
        expect(store.snapshotOf(tab3)).toBeNull();

        // Evento atrasado da tab3 (revision 5 <= 6) não ressuscita a aba
        store.applySnapshot(makeSnapshot({ tabId: tab3, revision: 5, status: 'running' }));
        expect(store.snapshotOf(tab3)).toBeNull();

        // countsFor deduplica tab1 e não conta tab3 (removida) como idle
        const counts = store.countsFor([tab1, tab1, tab2, tab3, 'tab-inexistente']);
        expect(counts).toEqual({
            running: 1,
            continuous: 0,
            completed_unseen: 1,
            error_unseen: 0,
            idle: 0
        });
        expect(store.unseenTabIds).toEqual([tab2]);
    });

    it('calcula hasActiveProcesses em todos os painéis mesmo quando o status agregado da aba é error_unseen', () => {
        const store = useTabStatusStore();
        const tabId = '01960000-0000-7000-8000-000000000101';

        store.applySnapshot(
            makeSnapshot({
                tabId,
                revision: 4,
                status: 'error_unseen',
                panes: [
                    {
                        paneId: '01960000-0000-7000-8000-000000000201',
                        ptyId: '01960000-0000-7000-8000-000000000301',
                        sessionId: '01960000-0000-7000-8000-000000000401',
                        status: 'error_unseen',
                        integrationMode: 'osc133',
                        continuousPolicy: 'auto',
                        cwd: '/tmp',
                        activeExecution: null,
                        lastCompletion: null,
                        updatedAt: 1730000000000
                    },
                    {
                        paneId: '01960000-0000-7000-8000-000000000202',
                        ptyId: '01960000-0000-7000-8000-000000000302',
                        sessionId: '01960000-0000-7000-8000-000000000402',
                        status: 'continuous',
                        integrationMode: 'osc133',
                        continuousPolicy: 'always',
                        cwd: '/tmp',
                        activeExecution: {
                            blockId: '01960000-0000-7000-8000-000000000501',
                            commandSeq: 1,
                            startedAt: 1730000000000,
                            commandPreview: 'npm run dev',
                            source: 'osc133'
                        },
                        lastCompletion: null,
                        updatedAt: 1730000000000
                    }
                ]
            })
        );

        expect(store.statusOf(tabId)).toBe('error_unseen');
        expect(store.hasActiveProcesses).toBe(true);
    });

    it('envia markSeen sem mutação otimista prematura sobre outra geração', async () => {
        const store = useTabStatusStore();
        const tabId = '01960000-0000-7000-8000-000000000101';
        const block1 = '01960000-0000-7000-8000-000000000501';

        store.applySnapshot(
            makeSnapshot({
                tabId,
                revision: 3,
                status: 'completed_unseen',
                lastCompletion: {
                    blockId: block1,
                    ptyId: '01960000-0000-7000-8000-000000000301',
                    paneId: '01960000-0000-7000-8000-000000000201',
                    outcome: 'success',
                    exitCode: 0,
                    signal: null,
                    endReason: 'exit_zero',
                    endedAt: 1730000001000,
                    durationMs: 100,
                    seen: false
                }
            })
        );

        let resolveMarkSeen: ((snap: TabStateSnapshot_Serialize) => void) | null = null;
        invokeMock.mockImplementation((cmd: string) => {
            if (cmd === 'tab_mark_seen') {
                return new Promise<TabStateSnapshot_Serialize>((resolve) => {
                    resolveMarkSeen = resolve;
                });
            }
            return Promise.resolve(null);
        });

        const promise = store.markSeen({ tabId, paneId: null, upToRevision: 3 });

        // Enquanto a requisição está em voo, o status não é alterado otimisticamente
        expect(store.statusOf(tabId)).toBe('completed_unseen');
        expect(store.pendingSeen[tabId]?.blockId).toBe(block1);

        resolveMarkSeen!(
            makeSnapshot({
                tabId,
                revision: 4,
                status: 'idle',
                lastCompletion: {
                    blockId: block1,
                    ptyId: '01960000-0000-7000-8000-000000000301',
                    paneId: '01960000-0000-7000-8000-000000000201',
                    outcome: 'success',
                    exitCode: 0,
                    signal: null,
                    endReason: 'exit_zero',
                    endedAt: 1730000001000,
                    durationMs: 100,
                    seen: true
                }
            })
        );

        await promise;
        expect(store.statusOf(tabId)).toBe('idle');
        expect(store.pendingSeen[tabId]).toBeUndefined();
    });
});
