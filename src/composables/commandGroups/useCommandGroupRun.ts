import { ref, type Ref } from 'vue';
import { isMtIpcError } from '@/ipc/core';
import { useCommandGroupsStore } from '@/stores/commandGroups';
import { useTemplatesStore } from '@/stores/templates';
import type {
    CommandGroup,
    ExecutionMode,
    GroupRun,
    GroupRunRequest
} from '@/types/commandGroups';

let requestCounter = 0;

/**
 * Gera um identificador UUID v7 determinístico/monotônico para idempotência de lançamento (`requestId`).
 */
export function createGroupRequestId(): string {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
        const raw = crypto.randomUUID();
        const tsHex = Date.now().toString(16).padStart(12, '0');
        return `${tsHex.slice(0, 8)}-${tsHex.slice(8, 12)}-7${raw.slice(15)}`;
    }
    requestCounter += 1;
    const tsHex = Date.now().toString(16).padStart(12, '0');
    const randHex = requestCounter.toString(16).padStart(12, '0');
    return `${tsHex.slice(0, 8)}-${tsHex.slice(8, 12)}-7000-8000-${randHex}`;
}

export interface LaunchGroupOptions {
    group: CommandGroup;
    projectId: string;
    originPaneId?: string | null;
    modeOverride?: ExecutionMode | null;
    templateByItem?: Record<string, string>;
    requestId?: string;
}

export interface UseCommandGroupRunReturn {
    launching: Ref<boolean>;
    cancelling: Ref<boolean>;
    lastError: Ref<string | null>;
    lastErrorCode: Ref<string | null>;
    lastRequest: Ref<GroupRunRequest | null>;
    resolveCompatibleTemplatesForMultiTab: (
        group: CommandGroup,
        projectId: string
    ) => Record<string, string>;
    launchGroup: (options: LaunchGroupOptions) => Promise<GroupRun>;
    retryLastLaunch: () => Promise<GroupRun | null>;
    cancelRun: (runId: string, itemId?: string | null) => Promise<GroupRun | null>;
    runInNewTab: (group: CommandGroup, projectId: string) => Promise<GroupRun>;
}

/**
 * Composable de lançamento e acompanhamento de execução de Grupos de Comandos (`useCommandGroupRun`).
 * Garante captura atômica de contexto no gesto de clique, `requestId` estável por tentativa
 * e seleção automática de templates shell compatíveis no modo Multi-Abas.
 */
export function useCommandGroupRun(): UseCommandGroupRunReturn {
    const store = useCommandGroupsStore();
    const templatesStore = useTemplatesStore();

    const launching = ref<boolean>(false);
    const cancelling = ref<boolean>(false);
    const lastError = ref<string | null>(null);
    const lastErrorCode = ref<string | null>(null);
    const lastRequest = ref<GroupRunRequest | null>(null);

    const inFlightByGestureKey = new Map<string, Promise<GroupRun>>();

    function resolveCompatibleTemplatesForMultiTab(
        group: CommandGroup,
        projectId: string
    ): Record<string, string> {
        const templates = templatesStore.templatesFor(projectId);
        const shellTemplates = templates.filter((t) => t.config.environment.kind === 'shell');
        if (shellTemplates.length === 0) {
            return {};
        }

        const defaultShell = shellTemplates.find((t) => t.isDefault) ?? shellTemplates[0]!;
        const map: Record<string, string> = {};

        for (const item of group.items) {
            if (defaultShell.config.continuous === item.continuous) {
                map[item.id] = defaultShell.id;
                continue;
            }
            const matching = shellTemplates.find((t) => t.config.continuous === item.continuous);
            if (matching) {
                map[item.id] = matching.id;
            }
        }

        return map;
    }

    async function launchGroup(options: LaunchGroupOptions): Promise<GroupRun> {
        const effectiveMode = options.modeOverride ?? options.group.mode;
        const gestureKey = `${options.group.id}:${options.projectId}:${effectiveMode}:${options.originPaneId ?? ''}`;

        const existingFlight = inFlightByGestureKey.get(gestureKey);
        if (existingFlight) {
            return existingFlight;
        }

        const templateByItem =
            options.templateByItem ??
            (effectiveMode === 'multi_tabs'
                ? resolveCompatibleTemplatesForMultiTab(options.group, options.projectId)
                : {});

        const request: GroupRunRequest = {
            requestId: options.requestId ?? createGroupRequestId(),
            groupId: options.group.id,
            expectedRevision: options.group.revision,
            projectId: options.projectId,
            originPaneId: options.originPaneId ?? null,
            modeOverride: options.modeOverride ?? null,
            templateByItem
        };

        lastRequest.value = request;
        launching.value = true;
        lastError.value = null;
        lastErrorCode.value = null;

        const promise = (async (): Promise<GroupRun> => {
            try {
                return await store.startRun(request);
            } catch (err: unknown) {
                if (isMtIpcError(err)) {
                    lastError.value = err.message;
                    lastErrorCode.value = err.code;
                } else if (err instanceof Error) {
                    lastError.value = err.message;
                    lastErrorCode.value = 'core.internal';
                } else {
                    lastError.value = 'Não foi possível iniciar a execução do grupo.';
                    lastErrorCode.value = 'core.internal';
                }
                throw err;
            } finally {
                inFlightByGestureKey.delete(gestureKey);
                launching.value = false;
            }
        })();

        inFlightByGestureKey.set(gestureKey, promise);
        return promise;
    }

    async function retryLastLaunch(): Promise<GroupRun | null> {
        const req = lastRequest.value;
        if (!req || launching.value) {
            return null;
        }
        launching.value = true;
        lastError.value = null;
        lastErrorCode.value = null;
        try {
            return await store.startRun(req);
        } catch (err: unknown) {
            if (isMtIpcError(err)) {
                lastError.value = err.message;
                lastErrorCode.value = err.code;
            }
            return null;
        } finally {
            launching.value = false;
        }
    }

    async function cancelRun(
        runId: string,
        itemId: string | null = null
    ): Promise<GroupRun | null> {
        cancelling.value = true;
        try {
            return await store.cancelRun(runId, itemId);
        } catch (err: unknown) {
            if (isMtIpcError(err)) {
                lastError.value = err.message;
                lastErrorCode.value = err.code;
            }
            return null;
        } finally {
            cancelling.value = false;
        }
    }

    async function runInNewTab(group: CommandGroup, projectId: string): Promise<GroupRun> {
        return launchGroup({
            group,
            projectId,
            originPaneId: null,
            modeOverride: 'multi_tabs'
        });
    }

    return {
        launching,
        cancelling,
        lastError,
        lastErrorCode,
        lastRequest,
        resolveCompatibleTemplatesForMultiTab,
        launchGroup,
        retryLastLaunch,
        cancelRun,
        runInNewTab
    };
}
