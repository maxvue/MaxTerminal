import { defineStore } from 'pinia';
import { computed, onScopeDispose, ref, watch } from 'vue';
import { useDocumentVisibility } from '@maxvue/max-use';
import { useProjectsStore } from '@/stores/projects';
import {
    clearPending,
    previewClearPending,
    startHorizon,
    stopHorizon
} from '@/ipc/horizon';
import { HORIZON_SNAPSHOT_ROUTE } from '@/ipc/horizonMaxPiniaAdapter';
import type {
    HorizonAvailability,
    HorizonClearPreview,
    HorizonClearResult_Serialize,
    HorizonSessionDto,
    HorizonSnapshot_Serialize,
    HorizonStatus,
    ProjectId,
    QueueScope
} from '@/types/bindings';

export const useHorizonStore = defineStore('horizon', () => {
    // Configurações reativas de opt-in para o plugin MaxPinia
    const isCached = ref(true);
    const is_cached = ref(true);
    const id = ref<string>('');
    const enabled = ref(false);
    const default_value = ref<HorizonSnapshot_Serialize | null>(null);
    const data = ref<HorizonSnapshot_Serialize | null>(null);

    // Contrato estrito com MaxPinia: data como dicionário cujos valores são resolvidos via toValue
    const options = {
        get: {
            route: HORIZON_SNAPSHOT_ROUTE,
            data: {
                projectId: computed(() => id.value)
            }
        }
    };

    // Indicador se os dados atuais foram confirmados pelo backend nesta ativação
    const isConfirmedCurrentActivation = ref(false);

    // Estado local da UI e ações
    const isModalOpen = ref(false);
    const isStarting = ref(false);
    const isStopping = ref(false);
    const isPreviewing = ref(false);
    const isClearing = ref(false);
    const actionError = ref<string | null>(null);

    // Prévia temporária de limpeza em voo vinculada ao projeto ativo
    const clearPreviewData = ref<HorizonClearPreview | null>(null);

    const projectsStore = useProjectsStore();
    const visibility = useDocumentVisibility();

    const selectedProjectId = computed<ProjectId | null>(
        () => projectsStore.selectedProject?.id ?? null
    );

    // Getters computados
    const availability = computed<HorizonAvailability>(
        () => data.value?.availability ?? 'checking'
    );
    const isAvailable = computed<boolean>(
        () => availability.value === 'available' || availability.value === 'error'
    );

    // O status do daemon do Horizon é exposto exclusivamente como `runtimeStatus`,
    // preservando `status` para o estado de requisição/cache injetado pelo MaxPinia
    const runtimeStatus = computed<HorizonStatus>(() => data.value?.status ?? 'stopped');
    const isRunning = computed<boolean>(() => runtimeStatus.value === 'running');
    const isStartingState = computed<boolean>(() => runtimeStatus.value === 'starting');
    const isStoppingState = computed<boolean>(() => runtimeStatus.value === 'stopping');

    const isBusy = computed<boolean>(
        () =>
            isStarting.value ||
            isStopping.value ||
            isPreviewing.value ||
            isClearing.value ||
            isStartingState.value ||
            isStoppingState.value
    );

    // Capacidades operacionais exigem confirmação pelo backend nesta ativação para evitar dados obsoletos
    const canStart = computed<boolean>(
        () =>
            isConfirmedCurrentActivation.value &&
            !!data.value?.canStart &&
            !isBusy.value &&
            isAvailable.value
    );
    const canStop = computed<boolean>(
        () =>
            isConfirmedCurrentActivation.value &&
            !!data.value?.canStop &&
            !isBusy.value &&
            isAvailable.value
    );
    const canClearPending = computed<boolean>(
        () =>
            isConfirmedCurrentActivation.value &&
            !!data.value?.canClearPending &&
            !isClearing.value &&
            isAvailable.value
    );

    const pendingCount = computed<number | null>(() => data.value?.pending ?? null);
    const runningCount = computed<number | null>(() => data.value?.running ?? null);
    const failedCount = computed<number | null>(() => data.value?.failed ?? null);
    const queues = computed<QueueScope[]>(() => data.value?.queues ?? []);
    const horizonVersion = computed<string | null>(() => data.value?.horizonVersion ?? null);
    const environment = computed<string | null>(() => data.value?.environment ?? null);

    // A sessão supervisionada (PTY e pane) exige confirmação nativa nesta ativação
    const session = computed<HorizonSessionDto | null>(() => {
        if (!isConfirmedCurrentActivation.value) {
            return null;
        }
        return data.value?.session ?? null;
    });

    const contextRevision = computed<string>(() => data.value?.context.contextRevision ?? '');
    const generation = computed<number>(() => data.value?.session?.generation ?? 0);

    let pollTimeoutId: ReturnType<typeof setTimeout> | null = null;
    let isFetching = false;
    let backoffDelayMs = 0;

    function clearPollingTimer(): void {
        if (pollTimeoutId) {
            clearTimeout(pollTimeoutId);
            pollTimeoutId = null;
        }
    }

    /**
     * Interceptor do MaxPinia chamado ao receber dados do servidor.
     * Valida estritamente a identidade do projeto ativo e a revisão para evitar dados fora de ordem.
     */
    function onServerData(
        serverData: HorizonSnapshot_Serialize | { data?: HorizonSnapshot_Serialize } | null,
        _requestMeta?: unknown
    ): void {
        const snap =
            serverData &&
            typeof serverData === 'object' &&
            'data' in serverData &&
            serverData.data &&
            typeof serverData.data === 'object' &&
            'context' in (serverData.data as object)
                ? (serverData.data as HorizonSnapshot_Serialize)
                : (serverData as HorizonSnapshot_Serialize | null);

        if (!snap || !snap.context || typeof snap.context !== 'object') {
            return;
        }

        // Descarta respostas cujo projeto não corresponda ao projeto atualmente selecionado
        if (snap.context.projectId !== selectedProjectId.value) {
            return;
        }

        // Evita sobreposição com snapshot de revisão anterior caso haja resposta atrasada
        if (
            data.value?.context?.projectId === snap.context.projectId &&
            snap.revision < (data.value?.revision ?? 0)
        ) {
            return;
        }

        data.value = snap;
        isConfirmedCurrentActivation.value = true;
        if (snap.availability === 'available') {
            actionError.value = null;
        }
    }

    /**
     * Executa a atualização do snapshot do projeto atual delegando exclusivamente ao `reload` do MaxPinia.
     */
    async function refreshSnapshot(): Promise<void> {
        const currentProject = selectedProjectId.value;
        if (!currentProject) {
            data.value = null;
            isConfirmedCurrentActivation.value = false;
            return;
        }

        // Delega ao reload() injetado pelo MaxPinia quando disponível
        const storeInstance = useHorizonStore();
        if (typeof (storeInstance as unknown as { reload?: () => Promise<void> }).reload === 'function') {
            await (storeInstance as unknown as { reload: () => Promise<void> }).reload();
        }
    }

    /**
     * Agenda a próxima rodada de polling sequencial sem sobreposição de requisições.
     */
    function scheduleNextPoll(): void {
        clearPollingTimer();
        if (!selectedProjectId.value || availability.value === 'unavailable') {
            return;
        }
        if (visibility.value !== 'visible') {
            return;
        }

        const baseInterval = isModalOpen.value ? 2000 : 5000;
        const delay = backoffDelayMs > 0 ? backoffDelayMs : baseInterval;
        pollTimeoutId = setTimeout(() => {
            void triggerPoll();
        }, delay);
    }

    /**
     * Dispara uma leitura lógica controlada, aplicando recuo exponencial em caso de erro.
     */
    async function triggerPoll(): Promise<void> {
        clearPollingTimer();
        if (!selectedProjectId.value || availability.value === 'unavailable') {
            return;
        }
        if (visibility.value !== 'visible') {
            return;
        }
        if (isFetching) {
            return;
        }

        isFetching = true;
        try {
            await refreshSnapshot();
            backoffDelayMs = 0;
        } catch {
            backoffDelayMs = backoffDelayMs
                ? Math.min(30000, Math.round(backoffDelayMs * 1.5))
                : 5000;
        } finally {
            isFetching = false;
            scheduleNextPoll();
        }
    }

    /**
     * Inicia o daemon do Laravel Horizon em um PTY supervisionado.
     */
    async function start(): Promise<void> {
        const currentProject = selectedProjectId.value;
        if (!currentProject || !canStart.value) {
            return;
        }

        isStarting.value = true;
        actionError.value = null;
        const capturedRevision = contextRevision.value;
        const requestId = crypto.randomUUID();

        try {
            const snap = await startHorizon(currentProject, capturedRevision, requestId);
            if (selectedProjectId.value === currentProject) {
                data.value = snap;
                isConfirmedCurrentActivation.value = true;
            }
        } catch (err: unknown) {
            if (selectedProjectId.value === currentProject) {
                actionError.value = err instanceof Error ? err.message : String(err);
            }
            throw err;
        } finally {
            if (selectedProjectId.value === currentProject) {
                isStarting.value = false;
            }
        }
    }

    /**
     * Encerra graciosamente o processo master do Horizon supervisionado.
     */
    async function stop(): Promise<void> {
        const currentProject = selectedProjectId.value;
        if (!currentProject || !canStop.value) {
            return;
        }

        isStopping.value = true;
        actionError.value = null;
        const capturedGen = generation.value;
        const requestId = crypto.randomUUID();

        try {
            const snap = await stopHorizon(currentProject, capturedGen, requestId);
            if (selectedProjectId.value === currentProject) {
                data.value = snap;
                isConfirmedCurrentActivation.value = true;
            }
        } catch (err: unknown) {
            if (selectedProjectId.value === currentProject) {
                actionError.value = err instanceof Error ? err.message : String(err);
            }
            throw err;
        } finally {
            if (selectedProjectId.value === currentProject) {
                isStopping.value = false;
            }
        }
    }

    /**
     * Gera a prévia de contagens das filas e o token de confirmação temporário (30s) para limpeza.
     */
    async function previewClear(): Promise<HorizonClearPreview> {
        const currentProject = selectedProjectId.value;
        if (!currentProject) {
            throw new Error('Nenhum projeto selecionado para prévia de limpeza do Horizon.');
        }

        isPreviewing.value = true;
        actionError.value = null;
        try {
            const preview = await previewClearPending(currentProject);
            if (selectedProjectId.value === currentProject) {
                clearPreviewData.value = preview;
            }
            return preview;
        } catch (err: unknown) {
            if (selectedProjectId.value === currentProject) {
                actionError.value = err instanceof Error ? err.message : String(err);
            }
            throw err;
        } finally {
            if (selectedProjectId.value === currentProject) {
                isPreviewing.value = false;
            }
        }
    }

    /**
     * Executa a limpeza confirmada de tarefas pendentes (ready + delayed), preservando tarefas reservadas.
     */
    async function confirmClear(token: string): Promise<HorizonClearResult_Serialize> {
        const currentProject = selectedProjectId.value;
        if (!currentProject) {
            throw new Error('Nenhum projeto selecionado para confirmação de limpeza.');
        }

        if (!clearPreviewData.value || clearPreviewData.value.token !== token) {
            throw new Error('Prévia de confirmação de limpeza ausente ou token divergente.');
        }

        if (Date.now() >= clearPreviewData.value.expiresAtMs) {
            throw new Error('O tempo para confirmação da limpeza expirou (máximo de 30 segundos).');
        }

        isClearing.value = true;
        actionError.value = null;
        const requestId = crypto.randomUUID();

        try {
            const result = await clearPending(currentProject, token, requestId);
            clearPreviewData.value = null;
            await refreshSnapshot();
            return result;
        } catch (err: unknown) {
            if (selectedProjectId.value === currentProject) {
                actionError.value = err instanceof Error ? err.message : String(err);
            }
            throw err;
        } finally {
            if (selectedProjectId.value === currentProject) {
                isClearing.value = false;
            }
        }
    }

    function openModal(): void {
        isModalOpen.value = true;
        // Ao abrir o modal, solicita atualização imediata deduplicada
        if (!isFetching) {
            void triggerPoll();
        }
    }

    function closeModal(): void {
        isModalOpen.value = false;
        clearPreviewData.value = null;
        scheduleNextPoll();
    }

    function toggleModal(): void {
        if (isModalOpen.value) {
            closeModal();
        } else {
            openModal();
        }
    }

    // Monitora mudanças de projeto selecionado
    watch(
        selectedProjectId,
        (newId) => {
            clearPollingTimer();
            clearPreviewData.value = null;
            actionError.value = null;
            isConfirmedCurrentActivation.value = false;

            if (!newId) {
                id.value = '';
                enabled.value = false;
                data.value = null;
                isModalOpen.value = false;
                return;
            }

            id.value = newId;
            enabled.value = true;
            data.value = null;
            void triggerPoll();
        },
        { immediate: true }
    );

    // Ajusta o intervalo de polling conforme modal, visibilidade ou disponibilidade
    watch([isModalOpen, visibility, availability], () => {
        scheduleNextPoll();
    });

    onScopeDispose(() => {
        clearPollingTimer();
    });

    return {
        // MaxPinia contratos
        isCached,
        is_cached,
        id,
        enabled,
        default_value,
        data,
        options,
        onServerData,

        // Estado reativo da UI
        isModalOpen,
        isStarting,
        isStopping,
        isPreviewing,
        isClearing,
        isBusy,
        actionError,
        isConfirmedCurrentActivation,
        clearPreviewData,

        // Getters
        selectedProjectId,
        availability,
        isAvailable,
        runtimeStatus,
        isRunning,
        isStartingState,
        isStoppingState,
        canStart,
        canStop,
        canClearPending,
        pendingCount,
        runningCount,
        failedCount,
        queues,
        horizonVersion,
        environment,
        session,
        contextRevision,
        generation,

        // Métodos e Actions
        refreshSnapshot,
        start,
        stop,
        previewClear,
        confirmClear,
        openModal,
        closeModal,
        toggleModal
    };
});
