<template>
    <div v-if="response" class="rag-results" data-testid="rag-results">
        <header class="rag-results__header">
            <div class="rag-results__title-wrap">
                <MaxIcon icon="lucide:book-open" :size="0.875" class="rag-results__icon" />
                <span class="rag-results__title">Documentação ({{ response.slug }})</span>
                <span class="rag-results__query">“{{ response.query }}” · {{ response.latencyMs }} ms</span>
            </div>

            <div class="rag-results__controls">
                <select v-if="slugs.length > 0" :value="selectedSlug" class="rag-results__slug-select" aria-label="Selecionar base RAG" data-testid="rag-slug-select" @change="onChangeSlug">
                    <option v-for="s in slugs" :key="s.slug" :value="s.slug">{{ s.title || s.slug }}</option>
                </select>
                <MaxIconButton icon="lucide:x" :size="0.75" class="rag-results__close" title="Fechar resultados RAG" aria-label="Fechar resultados RAG" data-testid="btn-close-rag" @click="clear" />
            </div>
        </header>

        <p v-if="results.length === 0" class="rag-results__empty" data-testid="rag-empty">Nenhum trecho relevante encontrado para esta consulta.</p>

        <ul v-else class="rag-results__list" data-testid="rag-results-list">
            <li v-for="(item, idx) in results" :key="item.id ?? idx" class="rag-results__item" :data-testid="`rag-item-${idx}`">
                <div class="rag-results__item-head">
                    <strong class="rag-results__item-title">{{ item.title || item.source || `Trecho #${idx + 1}` }}</strong>
                    <span v-if="item.score !== null && item.score !== undefined" class="rag-results__item-score">{{ Math.round(item.score * 100) }}%</span>
                </div>
                <pre class="rag-results__item-content">{{ item.content }}</pre>
                <div class="rag-results__item-actions">
                    <span v-if="item.source" class="rag-results__item-source">{{ item.source }}</span>
                    <div class="rag-results__item-btns">
                        <MaxButton type="button" size="sm" variant="secondary" icon="lucide:copy" :data-testid="`btn-copy-rag-${idx}`" @click="onCopy(item, idx)">
                            <span>{{ copiedIdx === idx ? 'Copiado' : 'Copiar' }}</span>
                        </MaxButton>
                        <MaxButton type="button" size="sm" variant="secondary" icon="lucide:corner-down-left" :data-testid="`btn-insert-rag-${idx}`" @click="insertResultIntoInput(item)">
                            <span>Inserir</span>
                        </MaxButton>
                    </div>
                </div>
            </li>
        </ul>
    </div>
</template>

<script setup lang="ts">
    import { ref } from 'vue';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxIconButton from '@maxvue/max-components-ui/components/MaxIconButton';
    import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
    import { useRagSearch } from '@/composables/ai/useRagSearch';
    import type { PaneId, RagResult_Serialize } from '@/types/bindings';

    export interface RagResultsProps {
        paneId: PaneId;
    }

    const props = defineProps<RagResultsProps>();

    const {
        slugs,
        selectedSlug,
        response,
        results,
        search,
        copyResultContent,
        insertResultIntoInput,
        clear
    } = useRagSearch(() => props.paneId);

    const copiedIdx = ref<number | null>(null);

    async function onChangeSlug(ev: Event): Promise<void> {
        const nextSlug = (ev.target as HTMLSelectElement).value;
        selectedSlug.value = nextSlug;
        if (response.value?.query) {
            await search(response.value.query, nextSlug).catch(() => {});
        }
    }

    async function onCopy(item: RagResult_Serialize, idx: number): Promise<void> {
        const ok = await copyResultContent(item);
        if (ok) {
            copiedIdx.value = idx;
            setTimeout(() => {
                if (copiedIdx.value === idx) {
                    copiedIdx.value = null;
                }
            }, 1500);
        }
    }
</script>

<style lang="scss" scoped>
    .rag-results {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.625rem 0.75rem;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.75rem;
    }

    .rag-results__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .rag-results__title-wrap {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        min-width: 0;
    }

    .rag-results__icon {
        color: var(--max-info-400);
    }

    .rag-results__title {
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .rag-results__query {
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.6875rem;
    }

    .rag-results__controls {
        display: flex;
        align-items: center;
        gap: 0.375rem;
    }

    .rag-results__slug-select {
        padding: 0.1875rem 0.375rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.6875rem;
    }

    .rag-results__close {
        color: var(--mt-ui-text-secondary, var(--background-300));

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
            color: var(--mt-ui-text-strong, var(--background-50));
        }
    }

    .rag-results__empty {
        margin: 0;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .rag-results__list {
        margin: 0;
        padding: 0;
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        max-height: 14rem;
        overflow-y: auto;
    }

    .rag-results__item {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
        padding: 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .rag-results__item-head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .rag-results__item-title {
        font-size: 0.75rem;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .rag-results__item-score {
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
    }

    .rag-results__item-content {
        margin: 0;
        font-family: var(--mt-font-mono);
        font-size: 0.6875rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
        white-space: pre-wrap;
        max-height: 6rem;
        overflow-y: auto;
    }

    .rag-results__item-actions {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .rag-results__item-source {
        font-family: var(--mt-font-mono);
        font-size: 0.625rem;
        color: var(--mt-ui-text-muted, var(--background-400));
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .rag-results__item-btns {
        display: flex;
        align-items: center;
        gap: 0.25rem;
    }
    .rag-results__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.125rem 0.375rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.6875rem;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
        }
    }
</style>
