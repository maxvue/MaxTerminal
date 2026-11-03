<template>
    <div class="command-block-status" role="status" :aria-label="primaryLabel">
        <span class="cbs-badge" :class="badgeClass" :title="primaryLabel">
            <MaxIcon :icon="statusIcon" class="cbs-icon" />
            <span class="cbs-label status-text">{{ primaryLabel }}</span>
        </span>
        <span v-if="showExitCode" class="cbs-chip cbs-chip-danger block-exit-code">
            Código de saída {{ effectiveExitCode }} (exit {{ effectiveExitCode }})
        </span>
        <span v-if="isUnknownOutcome" class="cbs-chip cbs-chip-warning">
            Resultado desconhecido
        </span>
        <span v-if="persistence === 'failed'" class="cbs-chip cbs-chip-warning" title="Falha ao persistir este bloco no banco SQLite">
            Histórico não salvo
        </span>
        <span v-if="outputTruncated" class="cbs-chip cbs-chip-neutral" title="A saída excedeu o limite de captura e foi truncada">
            Saída parcial
        </span>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import type { BlockStatus, ObservationConfidence, PersistenceState } from '@/types/bindings';
    import type { CommandBlockStatus as FrontendBlockStatus } from '@/types/terminal';

    export interface CommandBlockStatusProps {
        status: FrontendBlockStatus;
        backendStatus?: BlockStatus | null;
        exitCode?: number | null;
        durationMs?: number | null;
        continuous?: boolean;
        persistence?: PersistenceState | null;
        outputTruncated?: boolean;
        confidence?: ObservationConfidence | null;
    }

    const props = withDefaults(defineProps<CommandBlockStatusProps>(), {
        backendStatus: null,
        exitCode: null,
        durationMs: null,
        continuous: false,
        persistence: null,
        outputTruncated: false,
        confidence: null
    });

    const effectiveExitCode = computed<number | null>(() => props.exitCode ?? null);

    const isUnknownOutcome = computed<boolean>(() => {
        if (props.status === 'running' || props.status === 'success') {
            return false;
        }
        if (props.backendStatus === 'cancelled') {
            return false;
        }
        return effectiveExitCode.value === null && props.status === 'aborted';
    });

    const showExitCode = computed<boolean>(() => {
        return effectiveExitCode.value !== null && effectiveExitCode.value !== 0;
    });

    const primaryLabel = computed<string>(() => {
        if (props.status === 'running') {
            return props.continuous ? 'Execução Contínua' : 'Executando';
        }
        if (props.status === 'success') {
            return 'Concluído';
        }
        if (props.status === 'error') {
            return 'Com erro';
        }
        if (props.backendStatus === 'cancelled') {
            return 'Interrompido';
        }
        return isUnknownOutcome.value ? 'Resultado desconhecido' : 'Interrompido';
    });

    const badgeClass = computed<string>(() => {
        if (props.status === 'running') {
            return props.continuous ? 'is-continuous' : 'is-running';
        }
        if (props.status === 'success') {
            return 'is-success';
        }
        if (props.status === 'error') {
            return 'is-error';
        }
        return 'is-aborted';
    });

    const statusIcon = computed<string>(() => {
        if (props.status === 'running') {
            return props.continuous ? 'mdi:infinity' : 'mdi:play-circle-outline';
        }
        if (props.status === 'success') {
            return 'mdi:check-circle-outline';
        }
        if (props.status === 'error') {
            return 'mdi:alert-circle-outline';
        }
        return 'mdi:help-circle-outline';
    });
</script>

<style lang="scss" scoped>
    .command-block-status {
        display: inline-flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 6px;
        font-size: 10px;
    }

    .cbs-badge {
        display: inline-flex;
        align-items: center;
        gap: 4px;
        padding: 1px 6px;
        border-radius: 999px;
        font-weight: 600;
        flex-shrink: 0;

        &.is-running {
            background: rgb(34 197 94 / 15%);
            color: var(--max-success-500);
            border: 1px solid var(--max-success-500);
        }

        &.is-continuous {
            background: rgb(14 165 233 / 15%);
            color: var(--max-info-500);
            border: 1px solid var(--max-info-500);
        }

        &.is-success {
            background: transparent;
            color: var(--max-success-500);
            border: 1px solid var(--max-success-500);
        }

        &.is-error {
            background: rgb(239 68 68 / 15%);
            color: var(--max-danger-500);
            border: 1px solid var(--max-danger-500);
        }

        &.is-aborted {
            background: rgb(245 158 11 / 15%);
            color: var(--max-warning-500);
            border: 1px solid var(--max-warning-500);
        }
    }

    .cbs-icon {
        font-size: 12px;
    }

    .cbs-chip {
        display: inline-flex;
        align-items: center;
        padding: 1px 5px;
        border-radius: 4px;
        font-family: var(--mt-font-mono);
        font-weight: 600;

        &.cbs-chip-danger {
            background: rgb(239 68 68 / 20%);
            border: 1px solid var(--max-danger-500);
            color: var(--max-danger-400);
        }

        &.cbs-chip-warning {
            background: rgb(245 158 11 / 20%);
            border: 1px solid var(--max-warning-500);
            color: var(--max-warning-400);
        }

        &.cbs-chip-neutral {
            background: var(--background-800);
            border: 1px solid var(--background-600);
            color: var(--background-300);
        }
    }
</style>
