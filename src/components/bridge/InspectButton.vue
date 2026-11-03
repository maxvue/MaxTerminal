<template>
    <button v-if="visible" type="button" class="inspect-button" :class="{ 'is-loading': loading }" :disabled="loading" title="Abrir conversa em Plan Mode no MaxCode" aria-label="Inspecionar erro no MaxCode" @click="onClick">
        <MaxIcon :icon="loading ? 'lucide:loader-circle' : 'mdi:magnify-scan'" class="inspect-button__icon" />
        <span class="inspect-button__label">{{ buttonLabel }}</span>
    </button>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import { useMaxCodeInspect, type InspectableBlockLike } from '@/composables/useMaxCodeInspect';
    import type { InspectDispatchResult_Serialize } from '@/types/bindings';

    export interface InspectButtonProps {
        block: InspectableBlockLike;
    }

    const props = defineProps<InspectButtonProps>();

    const emit = defineEmits<{
        inspected: [report: InspectDispatchResult_Serialize];
        error: [message: string];
    }>();

    const { visible, loading, inspect } = useMaxCodeInspect(() => props.block);
    const feedbackState = ref<'idle' | 'sent' | 'copied' | 'error'>('idle');

    const buttonLabel = computed<string>(() => {
        if (loading.value) {
            return 'Inspecionando...';
        }
        if (feedbackState.value === 'copied') {
            return 'Prompt copiado';
        }
        if (feedbackState.value === 'sent') {
            return 'Enviado ao MaxCode';
        }
        return 'Inspecionar';
    });

    async function onClick(): Promise<void> {
        feedbackState.value = 'idle';
        try {
            const report = await inspect();
            feedbackState.value = report.transport === 'clipboard_only' ? 'copied' : 'sent';
            emit('inspected', report);
        } catch (err: unknown) {
            feedbackState.value = 'error';
            const message = err instanceof Error ? err.message : 'Não foi possível abrir o MaxCode';
            emit('error', message);
        }
    }
</script>

<style lang="scss" scoped>
    .inspect-button {
        order: 10;
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.125rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--max-danger-500);
        background: color-mix(in srgb, var(--max-danger-500) 15%, transparent);
        color: var(--max-danger-400);
        font-size: 0.6875rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.65;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: color-mix(in srgb, var(--max-danger-500) 28%, transparent);
            color: var(--max-danger-300);
        }
    }

    .inspect-button__icon {
        font-size: 0.8125rem;
    }
</style>
