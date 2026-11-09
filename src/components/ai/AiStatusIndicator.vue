<template>
    <MaxButton type="button" class="ai-status-indicator" :class="`is-${indicator.color}${compact ? ' is-compact' : ''}`" :title="tooltipText" :aria-label="tooltipText" data-testid="ai-status-indicator" @click="onOpenSettings">
        <span class="ai-status-indicator__dot" />
        <span v-if="!compact" class="ai-status-indicator__label">{{ indicator.label }}</span>
    </MaxButton>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import { useAiStore } from '@/stores/ai';
    import { useWindowStore } from '@/stores/window';

    export interface AiStatusIndicatorProps {
        compact?: boolean;
    }

    withDefaults(defineProps<AiStatusIndicatorProps>(), {
        compact: false
    });

    const emit = defineEmits<{
        openSettings: [];
    }>();

    const aiStore = useAiStore();
    const windowStore = useWindowStore();

    const indicator = computed(() => aiStore.indicator);

    const tooltipText = computed<string>(() => {
        const h = aiStore.health;
        const parts: string[] = [indicator.value.label];
        if (h?.baseUrl) {
            parts.push(h.baseUrl);
        }
        if (h?.version) {
            parts.push(`v${h.version}`);
        }
        if (h?.latencyMs !== null && h?.latencyMs !== undefined) {
            parts.push(`${h.latencyMs} ms`);
        }
        return parts.join(' · ');
    });

    function onOpenSettings(): void {
        emit('openSettings');
        windowStore.openPreferences('ai');
    }
</script>

<style lang="scss" scoped>
    .ai-status-indicator__dot {
        width: 0.5rem;
        height: 0.5rem;
        border-radius: 50%;
        flex-shrink: 0;
    }

    .ai-status-indicator__label {
        white-space: nowrap;
    }

    .ai-status-indicator {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        height: 1.5rem;
        padding: 0 0.5rem;
        border-radius: 999px;
        border: 1px solid var(--background-700);
        background: var(--background-900);
        color: var(--background-200);
        font-size: 0.6875rem;
        font-weight: 500;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--background-50);
        }

        &.is-compact {
            padding: 0 0.375rem;
        }

        &.is-success .ai-status-indicator__dot {
            background: var(--max-success-500);
        }

        &.is-warning .ai-status-indicator__dot {
            background: var(--max-warning-500);
        }

        &.is-danger .ai-status-indicator__dot {
            background: var(--max-danger-500);
        }

        &.is-neutral .ai-status-indicator__dot {
            background: var(--background-500);
        }
    }
</style>
