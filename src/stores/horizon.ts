import { defineStore } from 'pinia';
import { computed, ref, watch } from 'vue';
import { useDocumentVisibility } from '@maxvue/max-use';
import { useProjectsStore } from '@/stores/projects';
import {
    clearPending,
    getHorizonSnapshot,
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
    const id = ref<string>('');
    const enabled = ref(false);
    const data = ref<HorizonSnapshot_Serialize | null>(null);

    const options = {
        get: {
            route: HORIZON_SNAPSHOT_ROUTE,
            data: computed(() => ({ projectId: id.value }))
        }
    };

    // Estado local da UI e ações
    const isModalOpen = ref(false);
    const isStarting = ref(false);
    const isStopping = ref(false);
    const isPreviewing = ref(false);
    const isClearing = ref(false);
    const actionError = ref<string | null>(null);

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
    const status = computed<HorizonStatus>(() => data.value?.status ?? 'stopped');
    const isRunning = computed<boolean>(() => status.value === 'running');
    const isStartingState = computed<boolean>(() => status.value === 'starting');
    const isStoppingState = computed<boolean>(() => status.value === 'stopping');
    const isBusy = computed<boolean>(
        () =>
            isStarting.value ||
            isStopping.value ||
            isClearing.value ||
            isStartingState.value ||
            isStoppingState.value
    );

    const canStart = computed<boolean>(
        () => !!data.value?.canStart && !isBusy.value && isAvailable.value
    );
    const canStop = computed<boolean>(
        () => !!data.value?.canStop && !isBusy.value && isAvailable.value
    );
    const canClearPending = computed<boolean>(
        () => !!data.value?.canClearPending && !isClearing.value && isAvailable.value
    );

    const pendingCount = computed<number | null>(() => data.value?.pending ?? null);
    const runningCount = computed<number | null>(() => data.value?.running ?? null);
    const failedCount = computed<number | null>(() => data.value?.failed ?? null);
    const queues = computed<QueueScope[]>(() => data.value?.queues ?? []);
    const horizonVersion = computed<string | null>(() => data.value?.horizonVersion ?? null);
    const environment = computed<string | null>(() => data.value?.environment ?? null);
    const session = computed<HorizonSessionDto | null>(() => data.value?.session ?? null);
    const contextRevision = computed<string>(() => data.value?.context.contextRevision ?? '');
    const generation = computed<number>(() => data.value?.session?.generation ?? 0);

    let pollTimer: ReturnType<typeof setInterval> | null = null;

    /**
     * Executa a atualização manual do snapshot do projeto atual via IPC.
     */
    async function refreshSnapshot(): Promise<void> {
        const currentProject = selectedProjectId.value;
        if (!currentProject) {
            data.value = null;
            return;
        }

        try {
            const snap = await getHorizonSnapshot(currentProject);
            // Atualiza apenas se o projeto ainda for o mesmo
            if (selectedProjectId.value === currentProject) {
                data.value = snap;
            }
        } catch (err: unknown) {
            if (selectedProjectId.value === currentProject) {
                const diagnosticMsg = err instanceof Error ? err.message : String(err);
                if (data.value) {
                    data.value = {
                        ...data.value,
                        status: 'error',
                        diagnostic: diagnosticMsg
                    };
                } else {
                    data.value = {
                        context: {
                            projectId: currentProject,
                            root: '',
                            contextRevision: '',
                            scopeId: ''
                        },
                        availability: 'error',
                        horizonVersion: null,
                        queues: [],
                        environment: null,
                        status: 'error',
                        pending: null,
                        running: null,
                        failed: null,
                        sampledAtMs: Date.now(),
                        revision: 0,
                        diagnostic: diagnosticMsg,
                        canStart: false,
                        canStop: false,
                        canClearPending: false,
                        session: null
                    };
                }
            }
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
        const requestId = crypto.randomUUID();

        try {
            const snap = await startHorizon(currentProject, contextRevision.value, requestId);
            if (selectedProjectId.value === currentProject) {
                data.value = snap;
            }
        } catch (err: unknown) {
            actionError.value = err instanceof Error ? err.message : String(err);
            throw err;
        } finally {
            isStarting.value = false;
        }
    }

    /**
     * Encerra graciosamente o processo master do Horizon.
     */
    async function stop(): Promise<void> {
        const currentProject = selectedProjectId.value;
        if (!currentProject || !canStop.value) {
            return;
        }

        isStopping.value = true;
        actionError.value = null;
        const requestId = crypto.randomUUID();

        try {
            const snap = await stopHorizon(currentProject, generation.value, requestId);
            if (selectedProjectId.value === currentProject) {
                data.value = snap;
            }
        } catch (err: unknown) {
            actionError.value = err instanceof Error ? err.message : String(err);
            throw err;
        } finally {
            isStopping.value = false;
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
            return await previewClearPending(currentProject);
        } catch (err: unknown) {
            actionError.value = err instanceof Error ? err.message : String(err);
            throw err;
        } finally {
            isPreviewing.value = false;
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

        isClearing.value = true;
        actionError.value = null;
        const requestId = crypto.randomUUID();

        try {
            const result = await clearPending(currentProject, token, requestId);
            // Atualiza o snapshot após a remoção para refletir as novas contagens
            await refreshSnapshot();
            return result;
        } catch (err: unknown) {
            actionError.value = err instanceof Error ? err.message : String(err);
            throw err;
        } finally {
            isClearing.value = false;
        }
    }

    function openModal(): void {
        isModalOpen.value = true;
    }

    function closeModal(): void {
        isModalOpen.value = false;
    }

    function toggleModal(): void {
        isModalOpen.value = !isModalOpen.value;
    }

    /**
     * Ajusta o temporizador de polling conforme o contexto ativo:
     * - Pausado quando o documento estiver invisível
     * - 2s quando o modal estiver aberto
     * - 5s quando disponível e modal fechado
     * - Pausado se indisponível ou sem projeto selecionado
     */
    function updatePolling(): void {
        if (pollTimer) {
            clearInterval(pollTimer);
            pollTimer = null;
        }

        if (!selectedProjectId.value || availability.value === 'unavailable') {
            return;
        }

        if (visibility.value !== 'visible') {
            return;
        }

        const intervalMs = isModalOpen.value ? 2000 : 5000;
        pollTimer = setInterval(() => {
            void refreshSnapshot();
        }, intervalMs);
    }

    // Monitora mudanças de projeto selecionado
    watch(
        selectedProjectId,
        (newId) => {
            if (pollTimer) {
                clearInterval(pollTimer);
                pollTimer = null;
            }

            if (!newId) {
                id.value = '';
                enabled.value = false;
                data.value = null;
                isModalOpen.value = false;
                actionError.value = null;
                return;
            }

            id.value = newId;
            enabled.value = true;
            actionError.value = null;
            void refreshSnapshot().then(() => {
                updatePolling();
            });
        },
        { immediate: true }
    );

    // Ajusta o intervalo quando a visibilidade do documento ou o estado do modal muda
    watch([isModalOpen, visibility, availability], () => {
        updatePolling();
    });

    return {
        // MaxPinia contratos
        isCached,
        id,
        enabled,
        data,
        options,

        // Estado reativo da UI
        isModalOpen,
        isStarting,
        isStopping,
        isPreviewing,
        isClearing,
        isBusy,
        actionError,

        // Getters
        availability,
        isAvailable,
        status,
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
