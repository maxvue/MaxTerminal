<template>
    <div v-if="props.cycle" class="bug-cycle-progress" role="status" aria-label="Progresso do Ciclo de Resolução de Bugs">
        <div class="bug-cycle-progress__info">
            <MaxIcon icon="lucide:bot" />
            <span class="bug-cycle-progress__title">Ciclo de Bugs MaxCode ({{ statusLabel }}):</span>
            <span class="bug-cycle-progress__stats">{{ props.cycle.completedIssues }} / {{ props.cycle.totalIssues }} concluídas</span>
            <span v-if="props.cycle.currentIssue" class="bug-cycle-progress__current">Processando #{{ props.cycle.currentIssue }} ({{ props.cycle.currentStage }})</span>
        </div>
        <div class="bug-cycle-progress__actions">
            <MaxButton v-if="props.cycle.status === 'running'" label="Pausar" severity="secondary" size="small" class="bug-cycle-progress__btn" @click="emit('control', 'pause')" />
            <MaxButton v-if="props.cycle.status === 'paused'" label="Retomar" severity="success" size="small" class="bug-cycle-progress__btn bug-cycle-progress__btn--resume" @click="emit('control', 'resume')" />
            <MaxButton v-if="props.cycle.status === 'running' || props.cycle.status === 'paused'" label="Cancelar" severity="danger" size="small" class="bug-cycle-progress__btn bug-cycle-progress__btn--cancel" @click="emit('control', 'cancel')" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton, MaxIcon } from '@maxvue/max-components-ui';
    import type { BugCycleRunState } from '@/types/bindings';

    export interface BugCycleProgressProps {
        cycle: BugCycleRunState | null;
    }

    const props = defineProps<BugCycleProgressProps>();

    const emit = defineEmits<{
        control: [action: 'pause' | 'resume' | 'cancel'];
    }>();

    const statusLabel = computed<string>(() => {
        const s = props.cycle?.status;
        if (s === 'running') {
            return 'Em execução';
        }
        if (s === 'paused') {
            return 'Pausado';
        }
        if (s === 'completed') {
            return 'Concluído';
        }
        if (s === 'cancelled') {
            return 'Cancelado';
        }
        return 'Com falha';
    });
</script>

<style lang="scss" scoped>
    .bug-cycle-progress {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        padding: 0.5rem 0.875rem;
        background: var(--mt-ui-primary-subtle, color-mix(in srgb, var(--max-primary-500) 12%, transparent));
        border-bottom: 1px solid color-mix(in srgb, var(--max-primary-500) 35%, transparent);
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.7813rem;
    }

    .bug-cycle-progress__info {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.5rem;
    }

    .bug-cycle-progress__title {
        font-weight: 700;
        color: var(--max-primary-400);
    }

    .bug-cycle-progress__stats {
        font-weight: 600;
    }

    .bug-cycle-progress__current {
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .bug-cycle-progress__actions {
        display: flex;
        align-items: center;
        gap: 0.375rem;
    }

    .bug-cycle-progress__btn {
        height: 1.625rem;
        padding: 0 0.625rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.7188rem;
        font-weight: 600;
        cursor: pointer;

        &--resume {
            border-color: var(--max-success-500);
            color: var(--max-success-400);
        }

        &--cancel {
            border-color: var(--max-danger-500);
            color: var(--max-danger-300);
        }
    }
</style>
