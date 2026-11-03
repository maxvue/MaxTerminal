import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as portsIpc from '@/ipc/ports';
import { usePortsStore } from '@/stores/ports';
import type { ListeningPort_Serialize } from '@/types/bindings';

vi.mock('@/ipc/ports', () => ({
    portsListForTab: vi.fn(),
    portsOpenBrowser: vi.fn(),
    portsKill: vi.fn(),
    portsSetPollMode: vi.fn(),
    onPortOpened: vi.fn(async () => () => {}),
    onPortUpdated: vi.fn(async () => () => {}),
    onPortClosed: vi.fn(async () => () => {})
}));

function makePort(port: number, paneId = 'pane-1'): ListeningPort_Serialize {
    return {
        id: `${paneId}:tcp:${port}`,
        tabId: 'tab-1',
        paneId,
        ptyId: 'pty-1',
        port,
        protocol: 'tcp',
        addresses: [
            {
                family: 'ipv4',
                bindAddr: '127.0.0.1',
                isWildcard: false,
                isLoopback: true
            }
        ],
        browserUrl: `http://127.0.0.1:${port}/`,
        pid: 4000 + port,
        startTimeTicks: 99999,
        processName: 'vite',
        cmdlinePreview: `vite --port ${port}`,
        detectedAtMs: 1000
    };
}

describe('usePortsStore (Fase 08)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('ordena portas por número, aplica eventos opened/updated/closed e executa open/kill', async () => {
        const portsStore = usePortsStore();

        const p8000 = makePort(8000);
        const p5177 = makePort(5177);

        portsStore.applyOpened({
            tabId: 'tab-1',
            paneId: 'pane-1',
            port: p8000,
            revision: 1
        });
        portsStore.applyOpened({
            tabId: 'tab-1',
            paneId: 'pane-1',
            port: p5177,
            revision: 2
        });

        // Deve estar ordenado de forma crescente: 5177 antes de 8000
        expect(portsStore.portsForTab('tab-1').map((p) => p.port)).toEqual([5177, 8000]);
        expect(portsStore.portsForPane('tab-1', 'pane-1')).toHaveLength(2);

        // Evento updated
        portsStore.applyUpdated({
            tabId: 'tab-1',
            paneId: 'pane-1',
            port: { ...p5177, processName: 'node' },
            revision: 3
        });
        expect(portsStore.portsForTab('tab-1')[0]?.processName).toBe('node');

        // Open browser
        vi.mocked(portsIpc.portsOpenBrowser).mockResolvedValueOnce(null);
        await portsStore.openInBrowser('pane-1', 5177);
        expect(portsIpc.portsOpenBrowser).toHaveBeenCalledWith('pane-1', 5177);

        // Kill port
        vi.mocked(portsIpc.portsKill).mockResolvedValueOnce({
            paneId: 'pane-1',
            port: 5177,
            pid: p5177.pid,
            outcome: 'graceful_term',
            durationMs: 40
        });
        const killRes = await portsStore.killPort('tab-1', 'pane-1', 5177, p5177.pid);
        expect(killRes.outcome).toBe('graceful_term');
        expect(portsStore.portsForTab('tab-1').map((p) => p.port)).toEqual([8000]);

        // Evento closed
        portsStore.applyClosed({
            tabId: 'tab-1',
            paneId: 'pane-1',
            id: p8000.id,
            port: 8000,
            revision: 4
        });
        expect(portsStore.portsForTab('tab-1')).toHaveLength(0);

        // loadForTab e setPollMode
        vi.mocked(portsIpc.portsListForTab).mockResolvedValueOnce({
            tabId: 'tab-1',
            ports: [p5177],
            revision: 5
        });
        await portsStore.loadForTab('tab-1');
        expect(portsStore.portsForTab('tab-1')).toHaveLength(1);

        vi.mocked(portsIpc.portsSetPollMode).mockResolvedValueOnce(null);
        await portsStore.setPollMode('background');
        expect(portsStore.pollMode).toBe('background');

        await portsStore.startListening();
        portsStore.removeTab('tab-1');
        portsStore.resetForTests();
    });
});
