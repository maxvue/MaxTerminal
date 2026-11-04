import { beforeEach, describe, expect, it, vi } from 'vitest';
import { CanceledError, type InternalAxiosRequestConfig } from 'axios';
import {
    extractProjectId,
    horizonMaxPiniaAdapter,
    isHorizonSnapshotRoute,
    HORIZON_SNAPSHOT_ROUTE
} from '@/ipc/horizonMaxPiniaAdapter';
import * as horizonIpc from '@/ipc/horizon';
import type { HorizonSnapshot_Serialize } from '@/types/bindings';

vi.mock('@/ipc/horizon', () => ({
    getHorizonSnapshot: vi.fn(),
    startHorizon: vi.fn(),
    stopHorizon: vi.fn(),
    previewClearPending: vi.fn(),
    clearPending: vi.fn()
}));

describe('horizonMaxPiniaAdapter', () => {
    const mockSnapshot: HorizonSnapshot_Serialize = {
        context: {
            projectId: 'proj-abc',
            root: '/var/www/html',
            contextRevision: 'rev-1',
            scopeId: 'scope-1'
        },
        availability: 'available',
        horizonVersion: '5.48.0',
        environment: 'production',
        queues: [{ connection: 'redis', queue: 'default' }],
        status: 'running',
        pending: 5,
        running: 2,
        failed: 0,
        sampledAtMs: 1700000000000,
        revision: 1,
        diagnostic: null,
        canStart: false,
        canStop: true,
        canClearPending: true,
        session: { ptyId: 'pty-1', paneId: 'pane-1', generation: 1 }
    };

    beforeEach(() => {
        vi.clearAllMocks();
    });

    describe('isHorizonSnapshotRoute', () => {
        it('identifica a rota exata de snapshot do Horizon via GET', () => {
            expect(
                isHorizonSnapshotRoute({
                    url: HORIZON_SNAPSHOT_ROUTE,
                    method: 'get'
                } as InternalAxiosRequestConfig)
            ).toBe(true);

            expect(
                isHorizonSnapshotRoute({
                    url: `${HORIZON_SNAPSHOT_ROUTE}?projectId=proj-123`,
                    method: 'GET'
                } as InternalAxiosRequestConfig)
            ).toBe(true);
        });

        it('rejeita métodos que não sejam GET', () => {
            expect(
                isHorizonSnapshotRoute({
                    url: HORIZON_SNAPSHOT_ROUTE,
                    method: 'post'
                } as InternalAxiosRequestConfig)
            ).toBe(false);
        });

        it('rejeita rotas de outros recursos ou caminhos aproximados', () => {
            expect(
                isHorizonSnapshotRoute({
                    url: '/app/themes/catalog',
                    method: 'get'
                } as InternalAxiosRequestConfig)
            ).toBe(false);

            expect(
                isHorizonSnapshotRoute({
                    url: '/app/horizon/snapshot/other',
                    method: 'get'
                } as InternalAxiosRequestConfig)
            ).toBe(false);
        });
    });

    describe('extractProjectId', () => {
        it('extrai projectId de config.params', () => {
            const config = {
                params: { projectId: 'proj-42' }
            } as unknown as InternalAxiosRequestConfig;
            expect(extractProjectId(config)).toBe('proj-42');
        });

        it('extrai projectId da query string da URL', () => {
            const config = {
                url: `${HORIZON_SNAPSHOT_ROUTE}?projectId=proj-99`
            } as InternalAxiosRequestConfig;
            expect(extractProjectId(config)).toBe('proj-99');
        });

        it('retorna null se não houver projectId', () => {
            const config = {
                url: HORIZON_SNAPSHOT_ROUTE
            } as InternalAxiosRequestConfig;
            expect(extractProjectId(config)).toBeNull();
        });
    });

    describe('adapter execution', () => {
        it('retorna resposta Axios formatada com snapshot quando a requisição é bem-sucedida', async () => {
            vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(mockSnapshot);

            const config = {
                url: HORIZON_SNAPSHOT_ROUTE,
                method: 'get',
                params: { projectId: 'proj-abc' },
                headers: {}
            } as unknown as InternalAxiosRequestConfig;

            const res = await horizonMaxPiniaAdapter(config);

            expect(horizonIpc.getHorizonSnapshot).toHaveBeenCalledWith('proj-abc');
            expect(res.status).toBe(200);
            expect(res.statusText).toBe('OK');
            expect(res.data).toEqual(mockSnapshot);
        });

        it('lança CanceledError imediatamente se o sinal já estiver abortado', async () => {
            const controller = new AbortController();
            controller.abort();

            const config = {
                url: HORIZON_SNAPSHOT_ROUTE,
                method: 'get',
                params: { projectId: 'proj-abc' },
                signal: controller.signal
            } as unknown as InternalAxiosRequestConfig;

            await expect(horizonMaxPiniaAdapter(config)).rejects.toThrow(CanceledError);
            expect(horizonIpc.getHorizonSnapshot).not.toHaveBeenCalled();
        });

        it('rejeita com CanceledError e remove listener quando o sinal aborta durante o IPC', async () => {
            const controller = new AbortController();
            let resolver: (val: HorizonSnapshot_Serialize) => void;
            const pendingPromise = new Promise<HorizonSnapshot_Serialize>((resolve) => {
                resolver = resolve;
            });
            vi.mocked(horizonIpc.getHorizonSnapshot).mockReturnValue(pendingPromise);

            const config = {
                url: HORIZON_SNAPSHOT_ROUTE,
                method: 'get',
                params: { projectId: 'proj-abc' },
                signal: controller.signal
            } as unknown as InternalAxiosRequestConfig;

            const adapterPromise = horizonMaxPiniaAdapter(config);
            controller.abort();

            await expect(adapterPromise).rejects.toThrow(CanceledError);
            // Libera promise pendente do IPC
            resolver!(mockSnapshot);
        });

        it('falha caso projectId esteja ausente na requisição do Horizon', async () => {
            const config = {
                url: HORIZON_SNAPSHOT_ROUTE,
                method: 'get',
                params: {}
            } as unknown as InternalAxiosRequestConfig;

            await expect(horizonMaxPiniaAdapter(config)).rejects.toThrow(
                'Identificador de projeto ausente na requisição de snapshot do Horizon.'
            );
        });
    });
});
