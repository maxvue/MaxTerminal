import axios, {
    CanceledError,
    type AxiosAdapter,
    type AxiosInstance,
    type AxiosResponse,
    type InternalAxiosRequestConfig
} from 'axios';
import { getHorizonSnapshot } from '@/ipc/horizon';
import type { HorizonSnapshot_Serialize, ProjectId } from '@/types/bindings';

export const HORIZON_SNAPSHOT_ROUTE = '/app/horizon/snapshot';

/**
 * Extrai o identificador do projeto a partir dos parâmetros de configuração ou da query string da URL.
 */
export function extractProjectId(config: InternalAxiosRequestConfig): ProjectId | null {
    if (config.params && typeof config.params.projectId === 'string' && config.params.projectId.length > 0) {
        return config.params.projectId;
    }

    if (config.url) {
        try {
            const parsedUrl = new URL(config.url, 'http://127.0.0.1');
            const param = parsedUrl.searchParams.get('projectId');
            if (param && param.length > 0) {
                return param;
            }
        } catch {
            const match = config.url.match(/[?&]projectId=([^&]+)/);
            if (match && match[1]) {
                return decodeURIComponent(match[1]);
            }
        }
    }

    return null;
}

/**
 * Compara se a requisição corresponde estritamente à rota GET do snapshot do Horizon.
 */
export function isHorizonSnapshotRoute(config: InternalAxiosRequestConfig): boolean {
    const method = (config.method || 'get').toLowerCase();
    if (method !== 'get') {
        return false;
    }

    const rawUrl = config.url;
    if (!rawUrl || typeof rawUrl !== 'string') {
        return false;
    }

    try {
        const parsed = new URL(rawUrl, 'http://127.0.0.1');
        return parsed.pathname === HORIZON_SNAPSHOT_ROUTE;
    } catch {
        const [path] = rawUrl.split('?');
        return path === HORIZON_SNAPSHOT_ROUTE;
    }
}

/**
 * Adapter restrito do Axios para o ecossistema MaxPinia.
 * Intercepta chamadas à rota `/app/horizon/snapshot`, executando o IPC seguro `horizon_snapshot`
 * e respeitando o ciclo de vida do AbortSignal com descarte e liberação estrita de listeners em finally.
 */
export const horizonMaxPiniaAdapter: AxiosAdapter = async (
    config: InternalAxiosRequestConfig
): Promise<AxiosResponse> => {
    if (!isHorizonSnapshotRoute(config)) {
        const defaultAdapter = axios.getAdapter(axios.defaults.adapter);
        return defaultAdapter(config);
    }

    if (config.signal?.aborted) {
        throw new CanceledError('canceled');
    }

    const projectId = extractProjectId(config);
    if (!projectId) {
        throw new Error('Identificador de projeto ausente na requisição de snapshot do Horizon.');
    }

    const signal = config.signal as AbortSignal | undefined;
    let removeAbortListener: (() => void) | undefined;
    const abortPromise = new Promise<never>((_, reject) => {
        if (!signal) {
            return;
        }
        const onAbort = () => {
            reject(new CanceledError('canceled'));
        };
        signal.addEventListener('abort', onAbort);
        removeAbortListener = () => {
            signal.removeEventListener('abort', onAbort);
        };
    });

    try {
        const snapshotPromise = getHorizonSnapshot(projectId);
        const snapshot = await Promise.race([snapshotPromise, abortPromise]);

        const response: AxiosResponse<HorizonSnapshot_Serialize> = {
            data: snapshot,
            status: 200,
            statusText: 'OK',
            headers: {},
            config
        };

        return response;
    } finally {
        removeAbortListener?.();
    }
};

/**
 * Cria a instância customizada de Axios com o adapter Horizon injetado para o MaxPinia.
 */
export function createHorizonAxiosInstance(): AxiosInstance {
    return axios.create({
        adapter: horizonMaxPiniaAdapter
    });
}
