import { createPinia, setActivePinia } from 'pinia';
import { ref } from 'vue';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { TabStateSnapshot_Serialize } from '@/types/bindings';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
    invoke: (...args: unknown[]) => invokeMock(...args),
    Channel: class {}
}));

vi.mock('@tauri-apps/api/event', () => ({
    listen: vi.fn(() => Promise.resolve(vi.fn()))
}));

import { useTabSeenTracker } from '@/composables/useTabSeenTracker';
import { useTabStatusStore } from '@/stores/tabStatus';

const TAB_1 = '01960000-0000-7000-8000-000000000101';
const PANE_1 = '01960000-0000-7000-8000-000000000201';
const PTY_1 = '01960000-0000-7000-8000-000000000301';

function makeSnap(overrides: Partial<TabStateSnapshot_Serialize> = {}): TabStateSnapshot_Serialize {
    return {
        tabId: TAB_1,
        revision: 1,
        status: 'running',
        focused: true,
        panes: [],
        lastCompletion: null,
        attention: null,
        updatedAt: 1000,
        ...overrides
    };
}

describe('useTabSeenTracker', () => {
    beforeEach(() => {
        vi.useFakeTimers();
        setActivePinia(createPinia());
        const store = useTabStatusStore();
        store.resetForTests();
        invokeMock.mockReset();
        invokeMock.mockImplementation((cmd: string, args: Record<string, unknown>) => {
            if (cmd === 'tab_set_focus') {
                return Promise.resolve([]);
            }
            if (cmd === 'tab_mark_seen') {
                const req = args.request as { tabId: string; upToRevision?: number };
                return Promise.resolve(
                    makeSnap({
                        tabId: req.tabId,
                        revision: (req.upToRevision ?? 1) + 1,
                        status: 'idle'
                    })
                );
            }
            return Promise.resolve(null);
        });
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('não confirma durante running, confirma após render de conclusão visível e cancela imediatamente no blur', async () => {
        const store = useTabStatusStore();
        const activeTabId = ref<string | null>(TAB_1);
        const activePaneId = ref<string | null>(PANE_1);

        store.applySnapshot(makeSnap({ tabId: TAB_1, revision: 1, status: 'running' }));

        const tracker = useTabSeenTracker({
            activeTabId,
            activePaneId,
            ackDelayMs: 20
        });
        tracker.setWindowFocusedForTests(true, true);

        // Durante running nenhum tab_mark_seen deve ser enviado
        vi.advanceTimersByTime(50);
        expect(invokeMock.mock.calls.filter((c) => c[0] === 'tab_mark_seen')).toHaveLength(0);

        // Conclusão chega (revision 2, completed_unseen), mas ocorre blur antes de 20ms
        store.applySnapshot(
            makeSnap({
                tabId: TAB_1,
                revision: 2,
                status: 'completed_unseen',
                lastCompletion: {
                    blockId: 'blk-1',
                    ptyId: PTY_1,
                    paneId: PANE_1,
                    outcome: 'success',
                    exitCode: 0,
                    signal: null,
                    endReason: 'exit_zero',
                    endedAt: 1100,
                    durationMs: 100,
                    seen: false
                }
            })
        );
        await Promise.resolve();
        tracker.evaluateAndScheduleAck();

        vi.advanceTimersByTime(10);
        tracker.setWindowFocusedForTests(false, true);
        vi.advanceTimersByTime(30);

        // Blur cancelou o ACK pendente
        expect(invokeMock.mock.calls.filter((c) => c[0] === 'tab_mark_seen')).toHaveLength(0);

        // Quando a janela recupera o foco e o render completa, envia tab_mark_seen com upToRevision: 2
        tracker.setWindowFocusedForTests(true, true);
        vi.advanceTimersByTime(25);
        await Promise.resolve();

        const markCalls = invokeMock.mock.calls.filter((c) => c[0] === 'tab_mark_seen');
        expect(markCalls).toHaveLength(1);
        expect(markCalls[0]?.[1]).toEqual({
            request: {
                tabId: TAB_1,
                paneId: PANE_1,
                upToRevision: 2
            }
        });

        tracker.stop();
    });
});
