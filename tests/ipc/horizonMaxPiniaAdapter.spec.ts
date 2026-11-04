import { describe, expect, it, vi, beforeEach } from 'vitest';
import axios, { CanceledError, type InternalAxiosRequestConfig } from 'axios';
import {
    createHorizonAxiosInstance,
    horizonMaxPiniaAdapter,
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
            projectId: 'proj-123',
            root: '/var/www/laravel',
            contextRevision: 'rev-abc',
            scopeId: 'scope-xyz'
        },
        availability: 'available',
        horizonVersion: '5.48.3',
        environment: 'local',
        queues: [{ connection: 'redis', queue: 'default' }],
        status: 'running',
        pending: 10,
        running: 2,
        failed: 0,
        sampledAtMs: 1700000000000,
        revision: 1,
        diagnostic: null,
        canStart: false,
        canStop: true,
        canClearPending: true,
        session: {
            ptyId: 'pty-1',
            paneId: 'pane-1',
            generation: 1
        }
    };

    beforeEach(() => {
        vi.clearAllMocks();
    });

    it('intercepta a rota /app/horizon/snapshot e retorna o snapshot do projeto', async () => {
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(mockSnapshot);

        const config: InternalAxiosRequestConfig = {
            url: `${HORIZON_SNAPSHOT_ROUTE}?projectId=proj-123`,
            method: 'get',
            headers: new axios.AxiosHeaders()
        };

        const response = await horizonMaxPiniaAdapter(config);

        expect(horizonIpc.getHorizonSnapshot).toHaveBeenCalledWith('proj-123');
        expect(response.status).toBe(200);
        expect(response.data).toEqual(mockSnapshot);
    });

    it('extrai o projectId a partir de config.params quando presente', async () => {
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(mockSnapshot);

        const config: InternalAxiosRequestConfig = {
            url: HORIZON_SNAPSHOT_ROUTE,
            method: 'get',
            params: { projectId: 'proj-params-456' },
            headers: new axios.AxiosHeaders()
        };

        const response = await horizonMaxPiniaAdapter(config);

        expect(horizonIpc.getHorizonSnapshot).toHaveBeenCalledWith('proj-params-456');
        expect(response.data).toEqual(mockSnapshot);
    });

    it('rejeita com CanceledError se o signal já estiver abortado no início', async () => {
        const controller = new AbortController();
        controller.abort();

        const config: InternalAxiosRequestConfig = {
            url: `${HORIZON_SNAPSHOT_ROUTE}?projectId=proj-123`,
            method: 'get',
            signal: controller.signal,
            headers: new axios.AxiosHeaders()
        };

        await expect(horizonMaxPiniaAdapter(config)).rejects.toThrow(CanceledError);
        expect(horizonIpc.getHorizonSnapshot).not.toHaveBeenCalled();
    });

    it('rejeita com CanceledError se o signal for abortado durante a requisição', async () => {
        const controller = new AbortController();
        vi.mocked(horizonIpc.getHorizonSnapshot).mockImplementation(
            () =>
                new Promise((resolve) => {
                    setTimeout(() => resolve(mockSnapshot), 200);
                })
        );

        const config: InternalAxiosRequestConfig = {
            url: `${HORIZON_SNAPSHOT_ROUTE}?projectId=proj-123`,
            method: 'get',
            signal: controller.signal,
            headers: new axios.AxiosHeaders()
        };

        const promise = horizonMaxPiniaAdapter(config);
        controller.abort();

        await expect(promise).rejects.toThrow(CanceledError);
    });

    it('falha se projectId estiver ausente na URL e nos parâmetros', async () => {
        const config: InternalAxiosRequestConfig = {
            url: HORIZON_SNAPSHOT_ROUTE,
            method: 'get',
            headers: new axios.AxiosHeaders()
        };

        await expect(horizonMaxPiniaAdapter(config)).rejects.toThrow(
            'Identificador de projeto ausente'
        );
    });

    it('createHorizonAxiosInstance instancia um cliente Axios com o adapter configurado', () => {
        const instance = createHorizonAxiosInstance();
        expect(instance.defaults.adapter).toBe(horizonMaxPiniaAdapter);
    });
});
