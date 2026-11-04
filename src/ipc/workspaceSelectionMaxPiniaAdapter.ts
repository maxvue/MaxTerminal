import axios, {
    CanceledError,
    type AxiosAdapter,
    type AxiosResponse,
    type InternalAxiosRequestConfig
} from 'axios';
import { useSettingsStore } from '@/stores/settings';
import {
    createDefaultWorkspaceSelection,
    isWorkspaceSelection,
    WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
    WORKSPACE_SELECTION_ROUTE,
    type WorkspaceSelection
} from '@/types/workspaceSelection';

/** Fila encadeada de escritas para garantir ordenação estrita de POSTs no SQLite. */
let writeQueuePromise: Promise<unknown> = Promise.resolve();

/**
 * Lê de forma segura o snapshot pendente do armazenamento local síncrono.
 */
function readPendingStorage(): WorkspaceSelection | null {
    try {
        if (typeof localStorage === 'undefined') {
            return null;
        }
        const raw = localStorage.getItem(WORKSPACE_SELECTION_PENDING_STORAGE_KEY);
        if (!raw) {
            return null;
        }
        const parsed = JSON.parse(raw);
        if (isWorkspaceSelection(parsed)) {
            return parsed;
        }
        return null;
    } catch {
        return null;
    }
}

/**
 * Limpa o snapshot pendente do armazenamento local caso o `mutationId` ainda corresponda.
 */
function cleanupPendingStorageIfMatching(mutationId: string): void {
    try {
        if (typeof localStorage === 'undefined') {
            return;
        }
        const raw = localStorage.getItem(WORKSPACE_SELECTION_PENDING_STORAGE_KEY);
        if (!raw) {
            return;
        }
        const parsed = JSON.parse(raw);
        if (parsed && typeof parsed === 'object' && parsed.mutationId === mutationId) {
            localStorage.removeItem(WORKSPACE_SELECTION_PENDING_STORAGE_KEY);
        }
    } catch {
        // Falhas não críticas no storage não devem interromper o fluxo
    }
}

/**
 * Extrai o corpo JSON enviado na requisição POST de forma segura.
 */
function extractRequestBody(data: unknown): unknown {
    if (typeof data === 'string') {
        try {
            return JSON.parse(data);
        } catch {
            return data;
        }
    }
    return data;
}

/**
 * Adapter do Axios para interceptar a rota virtual `/app/workspace/selection` do MaxPinia,
 * convertendo requisições GET e POST em leituras e escritas duráveis na settings store (SQLite)
 * com suporte a recuperação de snapshot pendente síncrono (§3 da spec).
 */
export const workspaceSelectionMaxPiniaAdapter: AxiosAdapter = async (
    config: InternalAxiosRequestConfig
): Promise<AxiosResponse> => {
    const isSelectionRoute =
        config.url?.startsWith(WORKSPACE_SELECTION_ROUTE) ||
        config.url?.includes(WORKSPACE_SELECTION_ROUTE);

    if (!isSelectionRoute) {
        const defaultAdapter = axios.getAdapter(axios.defaults.adapter);
        return defaultAdapter(config);
    }

    if (config.signal?.aborted) {
        throw new CanceledError('canceled');
    }

    const method = (config.method ?? 'get').toLowerCase();
    const settingsStore = useSettingsStore();

    if (method === 'get') {
        // 1. Precedência de boot: snapshot pendente local válido tem prioridade
        const pending = readPendingStorage();
        if (pending) {
            return {
                data: pending,
                status: 200,
                statusText: 'OK',
                headers: {},
                config
            };
        }

        // 2. Se a store de settings ainda não estiver hidratada, aguarda hidratação
        if (!settingsStore.hydrated && !settingsStore.loading) {
            await settingsStore.hydrate();
        } else if (settingsStore.loading) {
            // Aguarda o término da hidratação caso já esteja em andamento
            let attempts = 0;
            while (settingsStore.loading && attempts < 50) {
                await new Promise((resolve) => setTimeout(resolve, 20));
                attempts++;
            }
        }

        if (config.signal?.aborted) {
            throw new CanceledError('canceled');
        }

        const persisted = settingsStore.get('ui.workspace.selection');
        if (isWorkspaceSelection(persisted)) {
            return {
                data: persisted,
                status: 200,
                statusText: 'OK',
                headers: {},
                config
            };
        }

        // 3. Fallback para chave legada `projects.selected_id`
        const legacyProjectId = settingsStore.get('projects.selected_id');
        if (typeof legacyProjectId === 'string' && legacyProjectId.length > 0) {
            const migrated: WorkspaceSelection = {
                version: 1,
                mutationId: 'legacy-migration',
                selectedProjectId: legacyProjectId,
                activeViewByProject: {}
            };
            return {
                data: migrated,
                status: 200,
                statusText: 'OK',
                headers: {},
                config
            };
        }

        // 4. Default inicial
        return {
            data: createDefaultWorkspaceSelection(),
            status: 200,
            statusText: 'OK',
            headers: {},
            config
        };
    }

    if (method === 'post') {
        const rawBody = extractRequestBody(config.data);
        if (!isWorkspaceSelection(rawBody)) {
            throw new Error('Contrato de WorkspaceSelection inválido enviado na requisição POST.');
        }

        const selection = rawBody;

        // Serializa gravações concorrentes no SQLite
        const runWrite = async (): Promise<AxiosResponse<WorkspaceSelection>> => {
            if (config.signal?.aborted) {
                throw new CanceledError('canceled');
            }

            const success = await settingsStore.set('ui.workspace.selection', selection);
            if (!success) {
                throw new Error('Falha ao persistir a seleção do workspace no banco de dados SQLite.');
            }

            // Remove o snapshot pendente do storage apenas se o mutationId ainda coincidir
            cleanupPendingStorageIfMatching(selection.mutationId);

            return {
                data: selection,
                status: 200,
                statusText: 'OK',
                headers: {},
                config
            };
        };

        const currentTask = writeQueuePromise.then(runWrite, runWrite);
        writeQueuePromise = currentTask.catch(() => {});
        return await currentTask;
    }

    throw new Error(`Método HTTP '${config.method}' não suportado para a rota de seleção.`);
};
