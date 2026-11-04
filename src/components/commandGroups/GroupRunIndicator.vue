<template>
    <button v-if="props.totalRuns > 0" type="button" class="group-run-indicator" :class="{ 'is-running': props.activeCount > 0, 'is-open': props.open }" data-testid="group-run-indicator" :title="tooltipLabel" @click="emit('toggle')">
        <MaxIcon icon="lucide:layers" class="group-run-indicator__icon" />
        <span class="group-run-indicator__label">
            Grupos
        </span>
        <span class="group-run-indicator__badge">
            {{ props.activeCount > 0 ? `${props.activeCount} ativo(s)` : props.totalRuns }}
        </span>
    </button>
</template>

<script setup lang="ts">
    import { computed } from 'vue';

    export interface GroupRunIndicatorProps {
        activeCount: number;
        totalRuns: number;
        open?: boolean;
    }

    const props = withDefaults(defineProps<GroupRunIndicatorProps>(), {
        open: false
    });

    export interface GroupRunIndicatorEmits {
        (e: 'toggle'): void;
    }

    const emit = defineEmits<GroupRunIndicatorEmits>();

    const tooltipLabel = computed<string>(() => {
        if (props.activeCount > 0) {
            return `${props.activeCount} grupo(s) de comandos em execução`;
        }
        return 'Exibir ou ocultar painel de execução de grupos de comandos';
    });
</script>

<style lang="scss" scoped>
    .group-run-indicator {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.7188rem;
        font-weight: 600;
        cursor: pointer;
    }

    .group-run-indicator.is-running {
        border-color: var(--max-info-500);
        background: var(--mt-ui-info-subtle, color-mix(in srgb, var(--max-info-500) 15%, transparent));
        color: var(--max-info-400);
    }

    .group-run-indicator.is-open {
        border-color: var(--max-primary-500);
    }

    .group-run-indicator__badge {
        padding: 0.0625rem 0.375rem;
        border-radius: 999px;
        background: var(--mt-ui-surface, var(--background-0));
        font-size: 0.6563rem;
    }
</style>
