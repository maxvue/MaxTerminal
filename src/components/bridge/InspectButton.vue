<template>
    <MaxButton v-if="visible" type="button" variant="danger" size="sm" :class="[{ 'is-loading': loading }, 'inspect-button']" :disabled="loading" :icon="loading ? 'lucide:loader-circle' : 'mdi:magnify-scan'" title="Abrir conversa em Plan Mode no MaxCode" aria-label="Inspecionar erro no MaxCode" @click="onClick">
        <span>{{ buttonLabel }}</span>
    </MaxButton>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
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
