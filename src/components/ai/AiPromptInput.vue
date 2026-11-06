<template>
    <form v-if="activeMode" class="ai-prompt-input" data-testid="ai-prompt-input" @submit.prevent="onSubmit">
        <div class="ai-prompt-input__badge" data-testid="ai-prompt-badge">
            <MaxIcon :icon="activeMode.kind === 'rag' ? 'lucide:book-open' : 'lucide:sparkles'" />
            <span>{{ badgeText }}</span>
        </div>

        <MaxInputSelect v-if="activeMode.kind === 'rag' && slugs.length > 0" :model-value="selectedSlug" :options="ragSlugOptions" option-label="label" option-value="value" option-name="label" class="ai-prompt-input__slug" aria-label="Base de documentação RAG" data-testid="ai-prompt-rag-slug" @update:model-value="onSlugSelectChange" />

        <MaxInputText ref="inputEl" v-model="promptText" class="ai-prompt-input__field" :placeholder="placeholderText" aria-label="Prompt de inteligência artificial" autocomplete="off" data-testid="ai-prompt-field" @keydown.esc.prevent="onCancel" />

        <MaxButton type="submit" size="small" class="ai-prompt-input__submit" :disabled="!promptText.trim() || loading" :loading="loading" data-testid="ai-prompt-submit">
            <MaxIcon :icon="loading ? 'lucide:loader-circle' : 'lucide:send'" />
            <span>{{ activeMode.kind === 'rag' ? 'Pesquisar' : 'Gerar Comando' }}</span>
        </MaxButton>

        <MaxIconButton icon="lucide:x" size="1" class="ai-prompt-input__cancel" title="Cancelar (Esc)" aria-label="Cancelar prompt de IA" data-testid="ai-prompt-cancel" @click="onCancel" />
    </form>
</template>

<script setup lang="ts">
    import { computed, nextTick, ref, watch } from 'vue';
    import { MaxButton, MaxIcon, MaxIconButton, MaxInputSelect, MaxInputText } from '@maxvue/max-components-ui';
    import { useAiSuggestion } from '@/composables/ai/useAiSuggestion';
    import { useRagSearch } from '@/composables/ai/useRagSearch';
    import { useAiStore } from '@/stores/ai';
    import type { PaneId } from '@/types/bindings';

    export interface AiPromptInputProps {
        paneId: PaneId;
    }

    const props = defineProps<AiPromptInputProps>();

    const aiStore = useAiStore();
    const { activeMode, isStreaming, suggest } = useAiSuggestion(() => props.paneId);
    const { slugs, selectedSlug, loading: ragLoading, loadSlugs, search } = useRagSearch(() => props.paneId);

    const promptText = ref<string>('');
    const inputEl = ref<{ focus?: () => void; setFocus?: () => void; input?: { focus?: () => void } } | HTMLInputElement | null>(null);

    const ragSlugOptions = computed(() =>
        slugs.value.map((s) => ({ label: s.title || s.slug, value: s.slug }))
    );

    function onSlugSelectChange(value: unknown): void {
        if (typeof value === 'string') {
            selectedSlug.value = value;
        }
    }

    const loading = computed<boolean>(() => isStreaming.value || ragLoading.value);

    const badgeText = computed<string>(() => {
        const mode = activeMode.value;
        if (!mode) {
            return '';
        }
        if (mode.kind === 'rag') {
            return 'Consulta RAG';
        }
        const sel = mode.selection;
        if (!sel) {
            return 'NL → Shell';
        }
        return sel.reasoningId
            ? `${sel.harnessId} · ${sel.modelId} (${sel.reasoningId})`
            : `${sel.harnessId} · ${sel.modelId}`;
    });

    const placeholderText = computed<string>(() => {
        if (activeMode.value?.kind === 'rag') {
            return 'Descreva o que deseja buscar na documentação indexada...';
        }
        return 'Descreva em português o comando que deseja executar...';
    });

    watch(
        activeMode,
        async (mode) => {
            if (!mode) {
                promptText.value = '';
                return;
            }
            promptText.value = mode.initialPrompt ?? '';
            if (mode.kind === 'rag' && slugs.value.length === 0) {
                void loadSlugs();
            }
            await nextTick();
            if (inputEl.value) {
                if (typeof inputEl.value.focus === 'function') {
                    inputEl.value.focus();
                } else if ('setFocus' in inputEl.value && typeof inputEl.value.setFocus === 'function') {
                    inputEl.value.setFocus();
                }
            }
            if (mode.kind === 'rag' && mode.initialPrompt && mode.initialPrompt.trim().length > 0) {
                await search(mode.initialPrompt.trim()).catch(() => {});
            }
        },
        { immediate: true }
    );

    async function onSubmit(): Promise<void> {
        const text = promptText.value.trim();
        if (!text || !activeMode.value) {
            return;
        }
        if (activeMode.value.kind === 'rag') {
            await search(text, selectedSlug.value).catch(() => {});
            return;
        }
        await suggest(text, {
            trigger: activeMode.value.trigger,
            selection: activeMode.value.selection
        }).catch(() => {});
    }

    function onCancel(): void {
        promptText.value = '';
        aiStore.closePanePrompt(props.paneId);
    }
</script>

<style lang="scss" scoped>
    .ai-prompt-input {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.4375rem 0.625rem;
        border-radius: 0.5rem;
        border: 1px solid var(--max-primary-500);
        background: var(--background-925);
    }

    .ai-prompt-input__badge {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.1875rem 0.5rem;
        border-radius: 999px;
        background: color-mix(in srgb, var(--max-primary-500) 20%, transparent);
        color: var(--max-primary-300);
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
        font-weight: 600;
        white-space: nowrap;
    }

    .ai-prompt-input__slug {
        padding: 0.25rem 0.375rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-100);
        font-size: 0.75rem;
    }

    .ai-prompt-input__field {
        flex: 1;
        min-width: 0;
    }

    .ai-prompt-input__submit {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.25rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--background-50);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }
    }

    .ai-prompt-input__cancel {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        width: 1.5rem;
        height: 1.5rem;
        border-radius: 0.25rem;
        border: none;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;

        &:hover {
            background: var(--background-800);
            color: var(--background-50);
        }
    }
</style>
