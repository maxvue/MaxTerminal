<template>
    <div class="agent-status-line" :class="{ 'is-compact': props.compact, 'is-inactive': !agentActive }" role="group" aria-label="Telemetria do Agente CLI">
        <div class="agent-status-line__item agent-status-line__item--model" :class="{ 'is-unavailable': !model.isAvailable, 'is-stale': model.isStale, 'is-configured': model.isConfigured }" :title="model.tooltip">
            <MaxIcon icon="lucide:cpu" size="0.8125" class="agent-status-line__icon" aria-hidden="true" />
            <span class="agent-status-line__label">{{ model.label }}:</span>
            <span class="agent-status-line__value">{{ model.display }}</span>
            <span v-if="model.isStale" class="agent-status-line__stale">desatualizado</span>
        </div>
        <div class="agent-status-line__item agent-status-line__item--context" :class="[`tone-${context.tone}`, { 'is-unavailable': !context.isAvailable, 'is-stale': context.isStale }]" :title="context.tooltip">
            <span class="agent-status-line__label">{{ context.label }}:</span>
            <span class="agent-status-line__value">{{ context.display }}</span>
            <div v-if="context.isAvailable && context.percent !== null" class="agent-status-line__bar">
                <MaxProgressBar :value="context.percent" :max="100" :severity="contextSeverity" :aria-label="`${context.label}: ${context.display}`" class="agent-status-line__progress" />
            </div>
            <span v-if="context.isStale" class="agent-status-line__stale">desatualizado</span>
        </div>
        <div class="agent-status-line__item agent-status-line__item--quota5h" :class="[`tone-${quota5h.tone}`, { 'is-unavailable': !quota5h.isAvailable, 'is-stale': quota5h.isStale }]" :title="quota5h.tooltip">
            <span class="agent-status-line__label">5h:</span>
            <span class="agent-status-line__value">{{ quota5h.display }}</span>
            <span v-if="!props.compact && quota5h.resetText" class="agent-status-line__reset">({{ quota5h.resetText }})</span>
            <span v-if="quota5h.isStale" class="agent-status-line__stale">desatualizado</span>
        </div>
        <div class="agent-status-line__item agent-status-line__item--quota-weekly" :class="[`tone-${quotaWeekly.tone}`, { 'is-unavailable': !quotaWeekly.isAvailable, 'is-stale': quotaWeekly.isStale }]" :title="quotaWeekly.tooltip">
            <span class="agent-status-line__label">Semanal:</span>
            <span class="agent-status-line__value">{{ quotaWeekly.display }}</span>
            <span v-if="!props.compact && quotaWeekly.resetText" class="agent-status-line__reset">({{ quotaWeekly.resetText }})</span>
            <span v-if="quotaWeekly.isStale" class="agent-status-line__stale">desatualizado</span>
        </div>
        <div v-if="!props.compact && cost.isAvailable" class="agent-status-line__item agent-status-line__item--cost" :class="{ 'is-stale': cost.isStale }" :title="cost.tooltip">
            <span class="agent-status-line__label">Custo:</span>
            <span class="agent-status-line__value">{{ cost.display }}</span>
            <span v-if="cost.isStale" class="agent-status-line__stale">desatualizado</span>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import MaxProgressBar from '@maxvue/max-components-ui/components/MaxProgressBar';
    import { useAgentTelemetry } from '@/composables/useAgentTelemetry';
    import type { TabId } from '@/types/bindings';

    export interface AgentStatusLineProps {
        tabId: TabId;
        compact?: boolean;
    }

    const props = withDefaults(defineProps<AgentStatusLineProps>(), {
        compact: false
    });

    const { model, context, quota5h, quotaWeekly, cost, agentActive } = useAgentTelemetry(
        () => props.tabId
    );

    const contextSeverity = computed<'info' | 'warning' | 'danger'>(() => {
        if (context.value.tone === 'danger') return 'danger';
        if (context.value.tone === 'warning') return 'warning';
        return 'info';
    });
</script>

<style lang="scss" scoped>
    .agent-status-line {
        display: inline-flex;
        align-items: center;
        flex-wrap: wrap;
        gap: 0.5rem;
        padding: 0.125rem 0.5rem;
        border-radius: 0.3125rem;
        background: var(--background-850, var(--background-900));
        border: 1px solid var(--background-700);
        font-size: 0.7188rem;
        color: var(--background-200);

        &.is-compact {
            gap: 0.375rem;
            padding: 0.0625rem 0.375rem;
            font-size: 0.6875rem;
        }

        &.is-inactive {
            opacity: 0.8;
        }
    }

    .agent-status-line__item {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        white-space: nowrap;

        &.is-unavailable {
            color: var(--background-400);
        }
    }

    .agent-status-line__icon {
        color: var(--max-primary-400);
    }

    .agent-status-line__label {
        color: var(--background-400);
        font-weight: 500;
    }

    .agent-status-line__value {
        color: var(--background-100);
        font-weight: 600;
    }

    .agent-status-line__item.tone-warning .agent-status-line__value {
        color: var(--max-warning-400);
    }

    .agent-status-line__item.tone-danger .agent-status-line__value {
        color: var(--max-danger-400);
        font-weight: 700;
    }

    .agent-status-line__reset {
        color: var(--background-400);
        font-size: 0.65rem;
    }

    .agent-status-line__stale {
        padding: 0 0.25rem;
        border-radius: 0.2rem;
        background: rgb(245 158 11 / 16%);
        border: 1px solid var(--max-warning-500);
        color: var(--max-warning-300);
        font-size: 0.625rem;
        font-weight: 600;
    }

    .agent-status-line__bar {
        width: 2.5rem;
        height: 0.3125rem;
        border-radius: 999px;
        background: var(--background-700);
        overflow: hidden;
    }

    .agent-status-line__bar-fill {
        height: 100%;
        background: var(--max-primary-500);

        &.tone-warning {
            background: var(--max-warning-500);
        }

        &.tone-danger {
            background: var(--max-danger-500);
        }
    }
</style>
