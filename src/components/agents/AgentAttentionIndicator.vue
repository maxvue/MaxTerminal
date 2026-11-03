<template>
    <div v-if="(isAwaiting || isSuspected) && !isAcknowledged" class="agent-attention-indicator" :class="{ 'is-awaiting': isAwaiting, 'is-suspected': isSuspected }" role="status" aria-live="polite" :title="tooltipText">
        <MaxIcon :icon="isAwaiting ? 'lucide:shield-alert' : 'lucide:circle-help'" size="0.875" class="agent-attention-indicator__icon" aria-hidden="true" />
        <span class="agent-attention-indicator__badge">{{ badgeLabel }}</span>
        <span v-if="sourceLabel" class="agent-attention-indicator__source">({{ sourceLabel }})</span>
        <div class="agent-attention-indicator__actions">
            <MaxButton type="button" variant="secondary" size="small" class="agent-attention-indicator__btn agent-attention-indicator__btn--focus" aria-label="Visualizar aba do agente" @click.stop="onFocus">Visualizar</MaxButton>
            <MaxButton type="button" variant="secondary" size="small" class="agent-attention-indicator__btn agent-attention-indicator__btn--dismiss" aria-label="Dispensar aviso de atenção" @click.stop="onDismiss">Dispensar aviso</MaxButton>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import { useAgentAttention } from '@/composables/useAgentAttention';
    import type { TabId } from '@/types/bindings';

    export interface AgentAttentionIndicatorProps {
        tabId: TabId;
    }

    const props = defineProps<AgentAttentionIndicatorProps>();

    const {
        isAwaiting,
        isSuspected,
        isAcknowledged,
        badgeLabel,
        sourceLabel,
        kindLabel,
        message,
        focusAgentTab,
        dismissNotice
    } = useAgentAttention(() => props.tabId);

    const tooltipText = computed<string>(() => {
        const parts: string[] = [];
        if (badgeLabel.value) {
            parts.push(badgeLabel.value);
        }
        if (kindLabel.value) {
            parts.push(`Tipo: ${kindLabel.value}`);
        }
        if (sourceLabel.value) {
            parts.push(`Origem: ${sourceLabel.value}`);
        }
        if (message.value) {
            parts.push(message.value);
        }
        return parts.join(' • ');
    });

    function onFocus(): void {
        void focusAgentTab();
    }

    function onDismiss(): void {
        void dismissNotice();
    }
</script>

<style lang="scss" scoped>
    .agent-attention-indicator {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.125rem 0.5rem;
        border-radius: 0.3125rem;
        font-size: 0.7188rem;
        font-weight: 600;

        &.is-awaiting {
            background: rgb(239 68 68 / 18%);
            border: 1px solid var(--max-danger-500);
            color: var(--max-danger-300);
        }

        &.is-suspected {
            background: rgb(245 158 11 / 16%);
            border: 1px solid var(--max-warning-500);
            color: var(--max-warning-300);
        }
    }

    .agent-attention-indicator__icon {
        flex-shrink: 0;
    }

    .agent-attention-indicator__badge {
        white-space: nowrap;
    }

    .agent-attention-indicator__source {
        font-size: 0.65rem;
        font-weight: 500;
        opacity: 0.85;
        white-space: nowrap;
    }

    .agent-attention-indicator__actions {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        margin-left: 0.25rem;
    }

    .agent-attention-indicator__btn {
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
        border: 1px solid currentcolor;
        background: transparent;
        color: inherit;
        font-size: 0.65rem;
        font-weight: 600;
        cursor: pointer;

        &:hover {
            background: rgb(255 255 255 / 10%);
        }
    }
</style>
