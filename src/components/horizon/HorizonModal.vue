<template>
    <div class="horizon-modal-wrapper">
        <MaxModal :visible="isOpen" title="Laravel Horizon — Monitoramento e Supervisão" :no-button="true" max-width="64rem" class="horizon-modal" :dismissable="!store.isBusy" :close-on-escape="!store.isBusy" :before-close="handleBeforeCloseMain" @update:visible="handleModalVisible">
            <div class="horizon-modal__content">
                <header class="horizon-modal__header">
                    <div class="horizon-modal__meta">
                        <div class="horizon-modal__status-badge" :class="`is-${store.runtimeStatus}`">
                            <span class="horizon-modal__status-dot" />
                            <span class="horizon-modal__status-text">{{ statusLabel }}</span>
                        </div>
                        <div v-if="store.horizonVersion" class="horizon-modal__version">
                            <span>Horizon {{ store.horizonVersion }}</span>
                            <span v-if="store.environment" class="horizon-modal__env-tag">{{ store.environment }}</span>
                        </div>
                    </div>

                    <div class="horizon-modal__metrics">
                        <div class="horizon-modal__metric horizon-modal__metric--pending" title="Tarefas prontas e agendadas na fila">
                            <span class="horizon-modal__metric-label">Pendentes</span>
                            <span class="horizon-modal__metric-value">{{ store.pendingCount !== null ? store.pendingCount : '—' }}</span>
                        </div>
                        <div class="horizon-modal__metric horizon-modal__metric--running" title="Tarefas reservadas em execução pelos workers">
                            <span class="horizon-modal__metric-label">Em Execução</span>
                            <span class="horizon-modal__metric-value">{{ store.runningCount !== null ? store.runningCount : '—' }}</span>
                        </div>
                        <div class="horizon-modal__metric horizon-modal__metric--failed" title="Tarefas que falharam">
                            <span class="horizon-modal__metric-label">Falhas</span>
                            <span class="horizon-modal__metric-value">{{ store.failedCount !== null ? store.failedCount : '—' }}</span>
                        </div>
                    </div>

                    <div class="horizon-modal__actions">
                        <MaxButton v-if="store.canStart" label="Iniciar Horizon" icon="lucide:play" size="small" :loading="store.isStarting" :disabled="store.isBusy" @click="handleStart" />
                        <MaxButton v-if="store.canStop" label="Parar Horizon" icon="lucide:square" severity="danger" size="small" :loading="store.isStopping" :disabled="store.isBusy" @click="handleStop" />
                        <MaxButton label="Limpar Pendentes" icon="lucide:trash-2" severity="warning" size="small" :loading="store.isPreviewing || store.isClearing" :disabled="!store.canClearPending || store.isBusy" :title="!store.canClearPending ? 'Nenhuma tarefa pendente para limpeza nas filas monitoradas' : 'Limpar exclusivamente tarefas prontas e agendadas'" @click="handleOpenClearPreview" />
                    </div>
                </header>

                <div v-if="store.actionError" class="horizon-modal__alert horizon-modal__alert--danger">
                    <span>{{ store.actionError }}</span>
                </div>

                <div v-if="lastClearMessage" class="horizon-modal__alert horizon-modal__alert--info">
                    <span>{{ lastClearMessage }}</span>
                </div>

                <div v-if="store.queues.length > 0" class="horizon-modal__queues-bar">
                    <span class="horizon-modal__queues-title">Filas Monitoradas:</span>
                    <span v-for="q in store.queues" :key="`${q.connection}:${q.queue}`" class="horizon-modal__queue-chip">
                        {{ q.connection }}:{{ q.queue }}
                    </span>
                </div>

                <main class="horizon-modal__body">
                    <div v-if="store.session" class="horizon-modal__terminal-container">
                        <TerminalView :key="`${store.selectedProjectId ?? ''}:${store.session.paneId}:${store.session.ptyId}:${store.session.generation}`" :pane-id="store.session.paneId" :pty-id="store.session.ptyId" :visible="isOpen" />
                    </div>
                    <div v-else-if="store.runtimeStatus === 'external'" class="horizon-modal__external-panel">
                        <div class="horizon-modal__external-card">
                            <MaxIcon icon="lucide:activity" class="horizon-modal__external-icon" />
                            <div class="horizon-modal__external-info">
                                <h4 class="horizon-modal__external-title">Horizon em Execução Externa</h4>
                                <p class="horizon-modal__external-desc">
                                    O daemon do Laravel Horizon está em execução no sistema operacional (processo externo ao MaxTerminal). As métricas de filas, workers e tarefas acima estão sendo sincronizadas em tempo real.
                                </p>
                                <p class="horizon-modal__external-hint">
                                    O terminal integrado e os comandos de parada estão disponíveis exclusivamente para instâncias supervisionadas iniciadas pelo MaxTerminal.
                                </p>
                            </div>
                        </div>
                    </div>
                    <div v-else class="horizon-modal__empty">
                        <MaxIcon icon="lucide:terminal" class="horizon-modal__empty-icon" />
                        <h4 class="horizon-modal__empty-title">Processo do Horizon Inativo</h4>
                        <p class="horizon-modal__empty-desc">
                            O daemon do Horizon não está em execução para este projeto pelo MaxTerminal.
                            Clique no botão "Iniciar Horizon" acima para instanciar o processo supervisionado.
                        </p>
                        <MaxButton v-if="store.canStart" label="Iniciar Horizon Agora" icon="lucide:play" :loading="store.isStarting" :disabled="store.isBusy" @click="handleStart" />
                    </div>
                </main>
            </div>
        </MaxModal>

        <!-- Modal secundário de confirmação de limpeza exclusiva de pendentes -->
        <MaxModal :visible="isClearModalOpen" title="Confirmar Limpeza Exclusiva de Tarefas Pendentes" :no-button="true" max-width="36rem" class="horizon-clear-modal" :dismissable="!store.isClearing" :close-on-escape="!store.isClearing" :before-close="handleBeforeCloseClear" @update:visible="handleClearModalVisible">
            <div class="horizon-clear-modal__content">
                <div class="horizon-clear-modal__warning">
                    <MaxIcon icon="lucide:alert-triangle" class="horizon-clear-modal__warning-icon" />
                    <div class="horizon-clear-modal__warning-text">
                        <p><strong>Atenção: Esta operação é irreversível.</strong></p>
                        <p>
                            Serão removidas exclusivamente as tarefas <strong>prontas (ready)</strong> e
                            <strong>agendadas (delayed)</strong> das filas selecionadas.
                        </p>
                        <p>
                            Tarefas reservadas (atualmente em execução pelos workers) e o histórico de falhas
                            serão <strong>estritamente preservados</strong>.
                        </p>
                    </div>
                </div>

                <div v-if="previewExpiredError" class="horizon-modal__alert horizon-modal__alert--danger">
                    <span>{{ previewExpiredError }}</span>
                </div>

                <div v-if="clearPreviewData" class="horizon-clear-modal__summary">
                    <span class="horizon-clear-modal__summary-title">Escopo da Limpeza (Estimativa Total: {{ clearPreviewData.estimatedPending }} tarefas):</span>
                    <ul class="horizon-clear-modal__queue-list">
                        <li v-for="qc in clearPreviewData.queues" :key="`${qc.connection}:${qc.queue}`" class="horizon-clear-modal__queue-item">
                            <span>{{ qc.connection }}:{{ qc.queue }}</span>
                        </li>
                    </ul>
                </div>

                <footer class="horizon-clear-modal__footer">
                    <MaxButton label="Cancelar" variant="text" :disabled="store.isClearing" @click="closeClearModal" />
                    <MaxButton label="Confirmar Limpeza" severity="danger" icon="lucide:trash-2" :loading="store.isClearing" :disabled="store.isClearing || !!previewExpiredError" @click="handleExecuteClear" />
                </footer>
            </div>
        </MaxModal>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { MaxButton, MaxIcon } from '@maxvue/max-components-ui';
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
    import TerminalView from '@/components/terminal/TerminalView.vue';
    import { useHorizonStore } from '@/stores/horizon';
    import type { HorizonClearPreview } from '@/types/bindings';

    const props = defineProps<{
        modelValue?: boolean;
    }>();

    const emit = defineEmits<{
        'update:modelValue': [value: boolean];
    }>();

    const store = useHorizonStore();

    const isClearModalOpen = ref(false);
    const clearPreviewData = ref<HorizonClearPreview | null>(null);
    const previewExpiredError = ref<string | null>(null);
    const lastClearMessage = ref<string | null>(null);

    const isOpen = computed({
        get(): boolean {
            return props.modelValue !== undefined ? props.modelValue : store.isModalOpen;
        },
        set(val: boolean): void {
            store.isModalOpen = val;
            emit('update:modelValue', val);
        }
    });

    const statusLabel = computed<string>(() => {
        switch (store.runtimeStatus) {
            case 'running':
                return 'Em Execução';
            case 'starting':
                return 'Iniciando...';
            case 'stopping':
                return 'Parando...';
            case 'stopped':
                return 'Parado';
            case 'error':
                return 'Erro';
            case 'external':
                return 'Externo (Ativo)';
            default:
                return store.runtimeStatus;
        }
    });

    async function handleBeforeCloseMain(event?: { preventDefault: () => void }): Promise<boolean> {
        if (store.isBusy) {
            event?.preventDefault();
            return false;
        }
        return true;
    }

    async function handleBeforeCloseClear(event?: { preventDefault: () => void }): Promise<boolean> {
        if (store.isClearing) {
            event?.preventDefault();
            return false;
        }
        return true;
    }

    function handleModalVisible(visible: boolean): void {
        isOpen.value = visible;
        if (!visible) {
            closeClearModal();
        }
    }

    function handleClearModalVisible(visible: boolean): void {
        isClearModalOpen.value = visible;
        if (!visible) {
            clearPreviewData.value = null;
            previewExpiredError.value = null;
        }
    }

    function closeClearModal(): void {
        isClearModalOpen.value = false;
        clearPreviewData.value = null;
        previewExpiredError.value = null;
    }

    async function handleStart(): Promise<void> {
        try {
            await store.start();
        } catch {
            // Tratado pelo store.actionError
        }
    }

    async function handleStop(): Promise<void> {
        try {
            await store.stop();
        } catch {
            // Tratado pelo store.actionError
        }
    }

    async function handleOpenClearPreview(): Promise<void> {
        previewExpiredError.value = null;
        try {
            clearPreviewData.value = await store.previewClear();
            isClearModalOpen.value = true;
        } catch {
            // Tratado pelo store.actionError
        }
    }

    async function handleExecuteClear(): Promise<void> {
        if (!clearPreviewData.value) {
            return;
        }

        if (Date.now() >= clearPreviewData.value.expiresAtMs) {
            previewExpiredError.value = 'O token de confirmação da limpeza expirou (máximo de 30 segundos). Solicite uma nova prévia.';
            return;
        }

        try {
            const result = await store.confirmClear(clearPreviewData.value.token);
            closeClearModal();
            let msg = `Limpeza concluída com sucesso: ${result.totalCleared} tarefa(s) pendente(s) removida(s).`;
            if (result.isPartial) {
                msg += ' (Aviso: limpeza parcial devido ao limite por lote).';
            }
            if (result.warning) {
                msg += ` Observação: ${result.warning}`;
            }
            lastClearMessage.value = msg;
        } catch {
            // Tratado pelo store.actionError
        }
    }

    // Limpa a prévia ao mudar de projeto para isolamento estrito
    watch(
        () => store.selectedProjectId,
        () => {
            closeClearModal();
            lastClearMessage.value = null;
        }
    );
</script>

<style lang="scss" scoped>
    .horizon-modal__content {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .horizon-modal__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        flex-wrap: wrap;
        gap: 0.75rem;
        padding-bottom: 0.75rem;
        border-bottom: 1px solid var(--mt-ui-border, rgb(255 255 255 / 8%));
    }

    .horizon-modal__meta {
        display: flex;
        align-items: center;
        gap: 0.75rem;
    }

    .horizon-modal__status-dot {
        width: 6px;
        height: 6px;
        border-radius: 50%;
    }

    .horizon-modal__status-badge {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.25rem 0.5rem;
        border-radius: 4px;
        font-size: 0.75rem;
        font-weight: 600;
        background: var(--mt-ui-surface-elevated, var(--background-50));

        &.is-running {
            color: var(--max-success-500);
            background: var(--mt-ui-success-subtle, color-mix(in srgb, var(--max-success-500) 12%, transparent));

            .horizon-modal__status-dot {
                background: var(--max-success-500);
            }
        }

        &.is-starting,
        &.is-stopping {
            color: var(--max-warning-500);
            background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 12%, transparent));

            .horizon-modal__status-dot {
                background: var(--max-warning-500);
            }
        }

        &.is-error {
            color: var(--max-danger-500);
            background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 12%, transparent));

            .horizon-modal__status-dot {
                background: var(--max-danger-500);
            }
        }

        &.is-external {
            color: var(--max-info-500);
            background: var(--mt-ui-info-subtle, color-mix(in srgb, var(--max-info-500) 12%, transparent));

            .horizon-modal__status-dot {
                background: var(--max-info-500);
            }
        }

        &.is-stopped {
            color: var(--mt-ui-text-muted, var(--background-400));

            .horizon-modal__status-dot {
                background: var(--mt-ui-text-muted, var(--background-400));
            }
        }
    }

    .horizon-modal__version {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        font-size: 0.8125rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .horizon-modal__env-tag {
        padding: 0.125rem 0.375rem;
        border-radius: 3px;
        font-size: 0.6875rem;
        font-weight: 500;
        text-transform: uppercase;
        background: var(--mt-ui-surface-hover, var(--background-75));
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .horizon-modal__metrics {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .horizon-modal__metric-label {
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
        text-transform: uppercase;
    }

    .horizon-modal__metric-value {
        font-size: 1rem;
        font-weight: 700;
    }

    .horizon-modal__metric {
        display: flex;
        flex-direction: column;
        align-items: center;
        padding: 0.25rem 0.625rem;
        border-radius: 4px;
        min-width: 4rem;

        &--pending {
            border-left: 3px solid var(--max-info-500);
            background: var(--mt-ui-info-subtle, color-mix(in srgb, var(--max-info-500) 8%, transparent));

            .horizon-modal__metric-value {
                color: var(--max-info-500);
            }
        }

        &--running {
            border-left: 3px solid var(--max-success-500);
            background: var(--mt-ui-success-subtle, color-mix(in srgb, var(--max-success-500) 8%, transparent));

            .horizon-modal__metric-value {
                color: var(--max-success-500);
            }
        }

        &--failed {
            border-left: 3px solid var(--max-danger-500);
            background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 8%, transparent));

            .horizon-modal__metric-value {
                color: var(--max-danger-500);
            }
        }
    }

    .horizon-modal__actions {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .horizon-modal__alert {
        padding: 0.5rem 0.75rem;
        border-radius: 4px;
        font-size: 0.8125rem;

        &--danger {
            background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 15%, transparent));
            color: var(--max-danger-500);
            border: 1px solid color-mix(in srgb, var(--max-danger-500) 30%, transparent);
        }

        &--info {
            background: var(--mt-ui-info-subtle, color-mix(in srgb, var(--max-info-500) 15%, transparent));
            color: var(--max-info-500);
            border: 1px solid color-mix(in srgb, var(--max-info-500) 30%, transparent);
        }
    }

    .horizon-modal__queues-bar {
        display: flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 0.375rem;
        font-size: 0.75rem;
    }

    .horizon-modal__queues-title {
        color: var(--mt-ui-text-muted, var(--background-400));
        margin-right: 0.25rem;
    }

    .horizon-modal__queue-chip {
        padding: 0.125rem 0.375rem;
        border-radius: 3px;
        font-family: monospace;
        font-size: 0.6875rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .horizon-modal__body {
        min-height: 380px;
        height: 420px;
        max-height: 60vh;
        display: flex;
        flex-direction: column;
    }

    .horizon-modal__terminal-container {
        flex: 1;
        width: 100%;
        height: 100%;
        min-height: 380px;
        background: var(--mt-term-bg, var(--background-0));
        border-radius: 4px;
        overflow: hidden;
    }

    .horizon-modal__empty {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        padding: 2rem;
        text-align: center;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-radius: 4px;
        border: 1px dashed var(--mt-ui-border, var(--background-100));
    }

    .horizon-modal__empty-icon {
        font-size: 2.5rem;
        color: var(--mt-ui-text-muted, var(--background-400));
        margin-bottom: 0.75rem;
    }

    .horizon-modal__empty-title {
        font-size: 1.125rem;
        font-weight: 600;
        margin-bottom: 0.5rem;
    }

    .horizon-modal__empty-desc {
        font-size: 0.875rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
        max-width: 28rem;
        margin-bottom: 1.25rem;
    }

    .horizon-modal__external-panel {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        padding: 2rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-radius: 4px;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        gap: 1.5rem;
        text-align: center;
    }

    .horizon-modal__external-card {
        display: flex;
        flex-direction: column;
        align-items: center;
        gap: 0.75rem;
        max-width: 32rem;
    }

    .horizon-modal__external-icon {
        font-size: 2.75rem;
        color: var(--max-info-500);
    }

    .horizon-modal__external-title {
        font-size: 1.125rem;
        font-weight: 600;
        margin: 0;
        color: var(--mt-ui-text-strong, var(--background-900));
    }

    .horizon-modal__external-desc {
        font-size: 0.875rem;
        line-height: 1.5;
        color: var(--mt-ui-text-secondary, var(--background-600));
        margin: 0;
    }

    .horizon-modal__external-hint {
        font-size: 0.8125rem;
        color: var(--mt-ui-text-muted, var(--background-400));
        margin: 0;
    }

    // Modal de Confirmação Secundário
    .horizon-clear-modal__content {
        display: flex;
        flex-direction: column;
        gap: 1rem;
    }

    .horizon-clear-modal__warning {
        display: flex;
        gap: 0.75rem;
        padding: 0.75rem;
        border-radius: 6px;
        background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 12%, transparent));
        border: 1px solid color-mix(in srgb, var(--max-warning-500) 25%, transparent);
    }

    .horizon-clear-modal__warning-icon {
        font-size: 1.5rem;
        color: var(--max-warning-500);
        flex-shrink: 0;
    }

    .horizon-clear-modal__warning-text {
        font-size: 0.8125rem;
        line-height: 1.4;
        color: var(--mt-ui-text, var(--background-800));

        p {
            margin: 0 0 0.375rem;

            &:last-child {
                margin-bottom: 0;
            }
        }
    }

    .horizon-clear-modal__summary {
        font-size: 0.8125rem;
    }

    .horizon-clear-modal__summary-title {
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
        margin-bottom: 0.375rem;
        display: block;
    }

    .horizon-clear-modal__queue-list {
        list-style: none;
        padding: 0;
        margin: 0;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .horizon-clear-modal__queue-item {
        display: flex;
        justify-content: space-between;
        padding: 0.25rem 0.5rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border-radius: 4px;
        font-family: monospace;
        font-size: 0.75rem;
    }

    .horizon-clear-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding-top: 0.5rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }
</style>
