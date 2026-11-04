<template>
    <section v-if="props.run || props.busyPaneFallback" class="group-run-panel" data-testid="group-run-panel" aria-label="Painel de execução do grupo de comandos">
        <div v-if="props.busyPaneFallback" class="group-run-panel__busy-alert" role="alert" data-testid="group-run-busy-fallback">
            <div class="group-run-panel__busy-text">
                <MaxIcon icon="lucide:alert-circle" />
                <span>{{ props.busyPaneFallback.reason || 'O painel atual está ocupado com um processo em execução.' }}</span>
            </div>
            <div class="group-run-panel__busy-actions">
                <MaxButton label="Executar em nova aba" icon="lucide:plus-square" severity="secondary" size="small" class="group-run-panel__new-tab-btn" data-testid="group-run-in-new-tab-btn" @click="onRunInNewTab" />
            </div>
        </div>

        <template v-if="props.run">
            <header class="group-run-panel__header">
                <div class="group-run-panel__title-group">
                    <MaxIcon icon="lucide:layers" class="group-run-panel__icon" />
                    <h4 class="group-run-panel__title">{{ props.run.snapshot.name }}</h4>
                    <span class="group-run-panel__chip">{{ scopeLabel }}</span>
                    <span class="group-run-panel__chip is-mode">{{ modeLabel }}</span>
                    <span class="group-run-panel__status-badge" :class="`status-${props.run.status}`" data-testid="group-run-status">
                        {{ runStatusLabel }}
                    </span>
                </div>

                <div class="group-run-panel__header-actions">
                    <span class="group-run-panel__counters" data-testid="group-run-counters">
                        {{ completedCount }}/{{ props.run.items.length }} concluídos
                        <template v-if="failedCount > 0"> · {{ failedCount }} falhas</template>
                    </span>

                    <MaxButton v-if="canRestartBusyOriginPane" label="Encerrar e reiniciar painel" icon="lucide:rotate-ccw" severity="secondary" size="small" class="group-run-panel__restart-btn" data-testid="group-run-restart-pane-btn" @click="onRestartBusyPane" />
                    <MaxButton v-if="isRunActive" label="Cancelar tudo" icon="lucide:octagon-x" severity="danger" size="small" class="group-run-panel__cancel-all-btn" data-testid="group-run-cancel-all-btn" @click="emit('cancel-run', props.run.id)" />
                    <MaxIconButton icon="lucide:x" label="Fechar painel de execução" variant="text" class="group-run-panel__close-btn" @click="emit('close')" />
                </div>
            </header>

            <div v-if="props.run.mode === 'parallel'" class="group-run-panel__parallel-note" data-testid="group-run-parallel-note">
                <MaxIcon icon="lucide:info" />
                <span>Modo em background sem entrada interativa — use Multi-Abas para comandos interativos</span>
            </div>

            <div class="group-run-panel__items">
                <GroupRunItemRow v-for="item in props.run.items" :key="item.id" :item="item" :mode="props.run.mode" @action="onItemAction" />
            </div>
        </template>
    </section>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton, MaxIcon, MaxIconButton } from '@maxvue/max-components-ui';
    import type { BusyPaneFallbackState } from '@/stores/commandGroups';
    import type { PaneId, TabId } from '@/types/bindings';
    import type {
        GroupRun,
        GroupRunId,
        GroupRunItem,
        GroupRunItemAction,
        GroupRunItemId
    } from '@/types/commandGroups';
    import GroupRunItemRow from './GroupRunItemRow.vue';

    export interface GroupRunPanelProps {
        run: GroupRun | null;
        busyPaneFallback?: BusyPaneFallbackState | null;
    }

    const props = withDefaults(defineProps<GroupRunPanelProps>(), {
        busyPaneFallback: null
    });

    export interface GroupRunPanelEmits {
        (e: 'cancel-run', runId: GroupRunId): void;
        (e: 'cancel-item', payload: { runId: GroupRunId; itemId: GroupRunItemId }): void;
        (e: 'show-output', payload: { runId: GroupRunId; item: GroupRunItem }): void;
        (e: 'focus-tab', payload: { tabId: TabId; paneId: PaneId | null }): void;
        (e: 'run-in-new-tab', payload: { groupId: string; projectId: string }): void;
        (e: 'restart-busy-pane', paneId: PaneId): void;
        (e: 'close'): void;
    }

    const emit = defineEmits<GroupRunPanelEmits>();

    const scopeLabel = computed<string>(() => {
        if (!props.run) {
            return '';
        }
        return props.run.snapshot.scope === 'local' ? 'Local' : 'Global';
    });

    const modeLabel = computed<string>(() => {
        if (!props.run) {
            return '';
        }
        switch (props.run.mode) {
            case 'sequential':
                return 'Sequencial';
            case 'parallel':
                return 'Paralelo';
            case 'multi_tabs':
                return 'Multi-Abas';
            default:
                return props.run.mode;
        }
    });

    const runStatusLabel = computed<string>(() => {
        if (!props.run) {
            return '';
        }
        switch (props.run.status) {
            case 'queued':
                return 'Na fila';
            case 'running':
                return 'Em execução';
            case 'cancelling':
                return 'Cancelando...';
            case 'succeeded':
                return 'Concluído com sucesso';
            case 'partial':
                return 'Concluído com falhas';
            case 'failed':
                return 'Falhou';
            case 'cancelled':
                return 'Cancelado';
            case 'interrupted':
                return 'Interrompido';
            default:
                return props.run.status;
        }
    });

    const isRunActive = computed<boolean>(() => {
        if (!props.run) {
            return false;
        }
        return (
            props.run.status === 'queued' ||
            props.run.status === 'running' ||
            props.run.status === 'cancelling'
        );
    });

    const canRestartBusyOriginPane = computed<boolean>(() => {
        if (!props.run) {
            return false;
        }
        return (
            props.run.mode === 'sequential' &&
            props.run.status === 'cancelling' &&
            Boolean(props.run.snapshot.originPaneId)
        );
    });

    const completedCount = computed<number>(() => {
        if (!props.run) {
            return 0;
        }
        return props.run.items.filter((i) => i.status === 'succeeded').length;
    });

    const failedCount = computed<number>(() => {
        if (!props.run) {
            return 0;
        }
        return props.run.items.filter(
            (i) => i.status === 'failed' || i.status === 'interrupted'
        ).length;
    });

    function onItemAction(payload: { action: GroupRunItemAction; item: GroupRunItem }): void {
        if (!props.run) {
            return;
        }
        const { action, item } = payload;
        if (action === 'show_output') {
            emit('show-output', { runId: props.run.id, item });
        } else if (action === 'focus_tab' && item.tabId) {
            emit('focus-tab', { tabId: item.tabId, paneId: item.paneId ?? null });
        } else if (action === 'cancel') {
            emit('cancel-item', { runId: props.run.id, itemId: item.id });
        }
    }

    function onRunInNewTab(): void {
        if (!props.busyPaneFallback) {
            return;
        }
        emit('run-in-new-tab', {
            groupId: props.busyPaneFallback.groupId,
            projectId: props.busyPaneFallback.projectId
        });
    }

    function onRestartBusyPane(): void {
        const originPaneId = props.run?.snapshot.originPaneId ?? null;
        if (!originPaneId) {
            return;
        }
        emit('restart-busy-pane', originPaneId);
    }
</script>

<style lang="scss" scoped>
    .group-run-panel {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem 0.875rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        max-height: 18rem;
        overflow-y: auto;
    }

    .group-run-panel__busy-alert {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-warning-500);
        background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 14%, transparent));
        color: var(--max-warning-300);
        font-size: 0.7813rem;
    }

    .group-run-panel__busy-text {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .group-run-panel__new-tab-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.3125rem 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--max-primary-500);
        background: color-mix(in srgb, var(--max-primary-500) 20%, transparent);
        color: var(--max-primary-300);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }

    .group-run-panel__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        flex-wrap: wrap;
    }

    .group-run-panel__title-group {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .group-run-panel__icon {
        color: var(--max-primary-400);
    }

    .group-run-panel__title {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 700;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .group-run-panel__chip {
        padding: 0.125rem 0.4375rem;
        border-radius: 0.25rem;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.6875rem;
        font-weight: 600;
    }

    .group-run-panel__chip.is-mode {
        color: var(--max-info-400);
        border-color: var(--max-info-500);
    }

    .group-run-panel__status-badge {
        padding: 0.125rem 0.5rem;
        border-radius: 999px;
        font-size: 0.6875rem;
        font-weight: 600;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .group-run-panel__header-actions {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .group-run-panel__counters {
        font-size: 0.75rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .group-run-panel__restart-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--max-warning-500);
        background: var(--mt-ui-warning-subtle, color-mix(in srgb, var(--max-warning-500) 16%, transparent));
        color: var(--max-warning-300);
        font-size: 0.7188rem;
        font-weight: 600;
        cursor: pointer;
    }

    .group-run-panel__cancel-all-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        padding: 0.25rem 0.625rem;
        border-radius: 0.25rem;
        border: 1px solid var(--max-danger-500);
        background: var(--mt-ui-danger-subtle, color-mix(in srgb, var(--max-danger-500) 15%, transparent));
        color: var(--max-danger-300);
        font-size: 0.7188rem;
        font-weight: 600;
        cursor: pointer;
    }

    .group-run-panel__close-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border-radius: 0.25rem;
        border: 1px solid transparent;
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        cursor: pointer;
    }

    .group-run-panel__parallel-note {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.375rem 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--max-info-500);
        background: var(--mt-ui-info-subtle, color-mix(in srgb, var(--max-info-500) 12%, transparent));
        color: var(--max-info-300);
        font-size: 0.75rem;
    }

    .group-run-panel__items {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }
</style>
