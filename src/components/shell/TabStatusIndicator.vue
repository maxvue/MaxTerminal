<template>
    <span class="tab-status-indicator" :class="[bucketClass, `size-${size}`, { 'is-degraded': confidence === 'degraded' }]" role="status" :aria-label="ariaDescription" :title="tooltipText" :data-status="status" :data-bucket="effectiveBucket">
        <span class="indicator-dot" :class="{ 'is-pulsing': effectiveBucket === 'running' || effectiveBucket === 'continuous' }" />
        <span v-if="showLabel" class="indicator-label">{{ bucketLabel }}</span>
        <span v-if="unknownOutcome" class="indicator-unknown">Resultado desconhecido</span>
    </span>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import type { ObservationConfidence, TabStatus } from '@/types/bindings';

    export interface TabStatusIndicatorProps {
        status: TabStatus;
        seen?: boolean;
        confidence?: ObservationConfidence | null;
        unknownOutcome?: boolean;
        showLabel?: boolean;
        size?: 'sm' | 'md';
    }

    const props = withDefaults(defineProps<TabStatusIndicatorProps>(), {
        seen: false,
        confidence: null,
        unknownOutcome: false,
        showLabel: true,
        size: 'md'
    });

    const effectiveBucket = computed<TabStatus>(() => {
        if (props.seen && (props.status === 'completed_unseen' || props.status === 'error_unseen')) {
            return 'idle';
        }
        return props.status;
    });

    const bucketClass = computed<string>(() => `bucket-${effectiveBucket.value.replace('_', '-')}`);

    const bucketLabel = computed<string>(() => {
        switch (effectiveBucket.value) {
            case 'running':
                return 'Executando';
            case 'continuous':
                return 'Execução Contínua';
            case 'completed_unseen':
                return 'Finalizado';
            case 'error_unseen':
                return 'Com Erro';
            default:
                return 'Parado';
        }
    });

    const tooltipText = computed<string>(() => {
        const parts: string[] = [bucketLabel.value];
        if (props.unknownOutcome) {
            parts.push('Resultado desconhecido');
        }
        if (props.confidence === 'degraded') {
            parts.push('Monitoramento aproximado (/proc)');
        }
        return parts.join(' — ');
    });

    const ariaDescription = computed<string>(() => {
        if (props.unknownOutcome) {
            return `${bucketLabel.value}: Resultado desconhecido`;
        }
        return bucketLabel.value;
    });
</script>

<style lang="scss" scoped>
    .tab-status-indicator {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        border-radius: 999px;
        font-weight: 600;
        line-height: 1.2;
        white-space: nowrap;
        user-select: none;

        &.size-sm {
            padding: 1px 6px;
            font-size: 10px;
        }

        &.size-md {
            padding: 2px 8px;
            font-size: 11px;
        }

        &.bucket-running {
            background: rgb(34 197 94 / 15%);
            color: var(--max-success-500);
            border: 1px solid var(--max-success-500);
        }

        &.bucket-continuous {
            background: rgb(14 165 233 / 15%);
            color: var(--max-info-500);
            border: 1px solid var(--max-info-500);
        }

        &.bucket-completed-unseen {
            background: rgb(245 158 11 / 15%);
            color: var(--max-warning-500);
            border: 1px solid var(--max-warning-500);
        }

        &.bucket-error-unseen {
            background: rgb(239 68 68 / 15%);
            color: var(--max-danger-500);
            border: 1px solid var(--max-danger-500);
        }

        &.bucket-idle {
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text-muted, var(--background-400));
            border: 1px solid var(--mt-ui-border, var(--background-100));
        }

        &.is-degraded {
            border-style: dashed;
        }
    }

    .indicator-dot {
        width: 7px;
        height: 7px;
        border-radius: 50%;
        background: currentcolor;
        flex-shrink: 0;
    }

    .indicator-unknown {
        padding-left: 4px;
        border-left: 1px solid var(--mt-ui-border, var(--background-100));
        color: var(--max-warning-400);
        font-weight: 500;
    }
</style>
