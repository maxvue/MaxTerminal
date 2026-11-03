<template>
    <div class="group-run-item-row" :class="`is-${statusVisual.tone}`" :data-testid="`group-run-item-${props.item.id}`">
        <div class="group-run-item-row__main">
            <span class="group-run-item-row__index">#{{ props.item.position + 1 }}</span>
            <div class="group-run-item-row__info">
                <div class="group-run-item-row__title-line">
                    <span class="group-run-item-row__label">{{ displayTitle }}</span>
                    <code v-if="props.item.snapshot.title" class="group-run-item-row__cmd">{{ props.item.snapshot.command }}</code>
                </div>
                <div class="group-run-item-row__meta">
                    <span v-if="displayCwd" class="group-run-item-row__meta-chip" title="Diretório de trabalho">
                        <MaxIcon icon="lucide:folder" />
                        <span>{{ displayCwd }}</span>
                    </span>
                    <span v-if="durationLabel" class="group-run-item-row__meta-chip">
                        {{ durationLabel }}
                    </span>
                    <span v-if="reasonMessage" class="group-run-item-row__skip-reason" data-testid="group-run-item-skip-reason">
                        {{ reasonMessage }}
                    </span>
                </div>
            </div>
        </div>

        <div class="group-run-item-row__right">
            <span class="group-run-item-row__badge" :class="`badge-${statusVisual.tone}`" data-testid="group-run-item-badge">
                <span class="group-run-item-row__dot" />
                <span>{{ statusVisual.label }}</span>
            </span>

            <div class="group-run-item-row__actions">
                <button v-if="canShowOutput" type="button" class="group-run-item-row__action-btn" data-testid="group-run-item-show-output" title="Ver saída ao vivo (somente leitura)" @click="emitAction('show_output')">
                    <MaxIcon icon="lucide:terminal" />
                    <span>Ver saída</span>
                </button>
                <button v-if="props.item.tabId" type="button" class="group-run-item-row__action-btn" data-testid="group-run-item-focus-tab" title="Focar aba vinculada" @click="emitAction('focus_tab')">
                    <MaxIcon icon="lucide:external-link" />
                    <span>Focar aba</span>
                </button>
                <button v-if="canCancel" type="button" class="group-run-item-row__action-btn is-danger" data-testid="group-run-item-cancel" title="Cancelar este comando" @click="emitAction('cancel')">
                    <MaxIcon icon="lucide:square" />
                    <span>Cancelar</span>
                </button>
            </div>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import type {
        GroupRun,
        GroupRunItem,
        GroupRunItemAction
    } from '@/types/commandGroups';

    export interface GroupRunItemRowProps {
        item: GroupRunItem;
        mode: GroupRun['mode'];
    }

    const props = defineProps<GroupRunItemRowProps>();

    export interface GroupRunItemRowEmits {
        (e: 'action', payload: { action: GroupRunItemAction; item: GroupRunItem }): void;
    }

    const emit = defineEmits<GroupRunItemRowEmits>();

    interface StatusVisual {
        tone: 'success' | 'info' | 'warning' | 'danger' | 'neutral';
        label: string;
    }

    const displayTitle = computed<string>(() => {
        const lbl = props.item.snapshot.title?.trim();
        return lbl && lbl.length > 0 ? lbl : props.item.snapshot.command;
    });

    const displayCwd = computed<string | null>(() => {
        return props.item.snapshot.definedCwd || props.item.snapshot.resolvedCwd || null;
    });

    const statusVisual = computed<StatusVisual>(() => {
        const { status, exitCode, snapshot } = props.item;
        switch (status) {
            case 'running':
                if (snapshot.continuous) {
                    return {
                        tone: 'info',
                        label: 'Execução Contínua'
                    };
                }
                return {
                    tone: 'success',
                    label: 'Executando'
                };
            case 'succeeded':
                return {
                    tone: 'warning',
                    label: 'Finalizado (0)'
                };
            case 'failed':
                return {
                    tone: 'danger',
                    label: `Falhou (exit ${exitCode ?? 1})`
                };
            case 'interrupted':
                return {
                    tone: 'danger',
                    label: 'Interrompido'
                };
            case 'starting':
                return {
                    tone: 'neutral',
                    label: 'Iniciando'
                };
            case 'cancelling':
                return {
                    tone: 'neutral',
                    label: 'Cancelando'
                };
            case 'cancelled':
                return {
                    tone: 'neutral',
                    label: 'Cancelado'
                };
            case 'skipped':
                return {
                    tone: 'neutral',
                    label: 'Ignorado'
                };
            case 'pending':
            default:
                return {
                    tone: 'neutral',
                    label: 'Aguardando'
                };
        }
    });

    const reasonMessage = computed<string | null>(() => {
        if (props.item.errorMessage) {
            return props.item.errorMessage;
        }
        if (props.item.status === 'skipped') {
            return 'Pulado: comando anterior falhou ou execução foi interrompida';
        }
        return null;
    });

    const durationLabel = computed<string | null>(() => {
        const startedAt = props.item.startedAt ?? null;
        const finishedAt = props.item.finishedAt ?? null;
        if (startedAt === null) {
            return null;
        }
        const end = finishedAt ?? Date.now();
        const diffSec = Math.max(0, Math.round((end - startedAt) / 1000));
        if (diffSec < 60) {
            return `${diffSec}s`;
        }
        const mins = Math.floor(diffSec / 60);
        const secs = diffSec % 60;
        return `${mins}m ${secs}s`;
    });

    const canShowOutput = computed<boolean>(() => {
        return (
            props.mode === 'parallel' ||
            props.item.outputTruncated ||
            props.item.status === 'running'
        );
    });

    const canCancel = computed<boolean>(() => {
        return (
            props.item.status === 'pending' ||
            props.item.status === 'starting' ||
            props.item.status === 'running'
        );
    });

    function emitAction(action: GroupRunItemAction): void {
        emit('action', {
            action,
            item: props.item
        });
    }
</script>

<style lang="scss" scoped>
    .group-run-item-row {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .group-run-item-row__main {
        display: flex;
        align-items: flex-start;
        gap: 0.5rem;
        min-width: 0;
        flex: 1;
    }

    .group-run-item-row__index {
        font-size: 0.6875rem;
        font-weight: 700;
        color: var(--mt-ui-text-muted, var(--background-400));
        padding-top: 0.125rem;
    }

    .group-run-item-row__info {
        display: flex;
        flex-direction: column;
        gap: 0.1875rem;
        min-width: 0;
        flex: 1;
    }

    .group-run-item-row__title-line {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .group-run-item-row__label {
        font-size: 0.7813rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .group-run-item-row__cmd {
        font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
        font-size: 0.7188rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
    }

    .group-run-item-row__meta {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        flex-wrap: wrap;
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .group-run-item-row__meta-chip {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
    }

    .group-run-item-row__skip-reason {
        color: var(--max-warning-400);
        font-style: italic;
    }

    .group-run-item-row__right {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        flex-shrink: 0;
    }

    .group-run-item-row__badge {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        padding: 0.1875rem 0.5rem;
        border-radius: 999px;
        font-size: 0.6875rem;
        font-weight: 600;
        border: 1px solid transparent;
    }

    .group-run-item-row__dot {
        width: 0.4375rem;
        height: 0.4375rem;
        border-radius: 999px;
        background: currentcolor;
    }

    .badge-success {
        background: rgb(34 197 94 / 15%);
        border-color: var(--max-success-500);
        color: var(--max-success-400);
    }

    .badge-info {
        background: rgb(14 165 233 / 15%);
        border-color: var(--max-info-500);
        color: var(--max-info-400);
    }

    .badge-warning {
        background: rgb(245 158 11 / 15%);
        border-color: var(--max-warning-500);
        color: var(--max-warning-400);
    }

    .badge-danger {
        background: rgb(239 68 68 / 15%);
        border-color: var(--max-danger-500);
        color: var(--max-danger-400);
    }

    .badge-neutral {
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border-color: var(--mt-ui-border, var(--background-100));
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .group-run-item-row__actions {
        display: flex;
        align-items: center;
        gap: 0.3125rem;
    }

    .group-run-item-row__action-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.6875rem;
        font-weight: 600;
        cursor: pointer;
    }

    .group-run-item-row__action-btn.is-danger {
        border-color: var(--max-danger-500);
        color: var(--max-danger-400);
    }
</style>
