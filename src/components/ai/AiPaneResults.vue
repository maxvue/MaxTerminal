<template>
    <div v-if="hasVisibleContent" class="ai-pane-results" data-testid="ai-pane-results">
        <AiPromptInput :pane-id="paneId" />
        <AiSuggestionBlock :pane-id="paneId" />
        <RagResults :pane-id="paneId" />
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import AiPromptInput from '@/components/ai/AiPromptInput.vue';
    import AiSuggestionBlock from '@/components/ai/AiSuggestionBlock.vue';
    import RagResults from '@/components/ai/RagResults.vue';
    import { useAiStore } from '@/stores/ai';
    import type { PaneId } from '@/types/bindings';

    export interface AiPaneResultsProps {
        paneId: PaneId;
    }

    const props = defineProps<AiPaneResultsProps>();

    const aiStore = useAiStore();

    const hasVisibleContent = computed<boolean>(() => {
        return Boolean(
            aiStore.activeModeForPane(props.paneId) ||
                aiStore.suggestionForPane(props.paneId) ||
                aiStore.ragResultsForPane(props.paneId)
        );
    });
</script>

<style lang="scss" scoped>
    .ai-pane-results {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        padding: 0.375rem 0.75rem 0;
        background: var(--mt-term-surface, var(--background-900));
    }
</style>
