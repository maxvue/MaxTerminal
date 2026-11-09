<template>
    <MaxTag :value="!compact && reasons.length > 0 ? `${labelText} (${reasons.length})` : labelText" :icon="iconName" :severity="level === 'destructive' ? 'danger' : (level === 'caution' ? 'warning' : 'success')" class="risk-badge" :class="`is-${level}${compact ? ' is-compact' : ''}`" :title="tooltipText" :no-uppercase="true" data-testid="risk-badge" />
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxIcon, MaxTag } from '@maxvue/max-components-ui';
    import type { RiskAssessment_Serialize, RiskLevel, RiskReason } from '@/types/bindings';

    export interface RiskBadgeProps {
        risk: RiskAssessment_Serialize;
        compact?: boolean;
    }

    const props = withDefaults(defineProps<RiskBadgeProps>(), {
        compact: false
    });

    const level = computed<RiskLevel>(() => props.risk.level);
    const reasons = computed<RiskReason[]>(() => props.risk.reasons ?? []);

    const iconName = computed<string>(() => {
        switch (level.value) {
            case 'destructive':
                return 'lucide:skull';
            case 'caution':
                return 'lucide:triangle-alert';
            case 'safe':
            default:
                return 'lucide:shield-check';
        }
    });

    const labelText = computed<string>(() => {
        switch (level.value) {
            case 'destructive':
                return 'Destrutivo';
            case 'caution':
                return 'Atenção';
            case 'safe':
            default:
                return 'Seguro';
        }
    });

    const tooltipText = computed<string>(() => {
        if (reasons.value.length === 0) {
            return `Classificação de risco: ${labelText.value}`;
        }
        const details = reasons.value.map((r) => `• ${r.message} (${r.segment})`).join('\n');
        return `${labelText.value}:\n${details}`;
    });
</script>

<style lang="scss" scoped>
    .risk-badge {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.125rem 0.5rem;
        border-radius: 999px;
        font-size: 0.6875rem;
        font-weight: 600;
        border: 1px solid transparent;

        &.is-compact {
            padding: 0.0625rem 0.375rem;
        }

        &.is-safe {
            border-color: var(--max-success-500);
            background: color-mix(in srgb, var(--max-success-500) 16%, transparent);
            color: var(--max-success-400);
        }

        &.is-caution {
            border-color: var(--max-warning-500);
            background: color-mix(in srgb, var(--max-warning-500) 16%, transparent);
            color: var(--max-warning-400);
        }

        &.is-destructive {
            border-color: var(--max-danger-500);
            background: color-mix(in srgb, var(--max-danger-500) 20%, transparent);
            color: var(--max-danger-400);
        }
    }

    .risk-badge__icon {
        flex-shrink: 0;
    }

    .risk-badge__label {
        white-space: nowrap;
    }

    .risk-badge__count {
        opacity: 0.85;
    }
</style>
