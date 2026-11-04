<template>
    <div class="search-result-section" role="group" :aria-labelledby="headingId">
        <div class="search-result-section__header">
            <span :id="headingId" class="search-result-section__title">{{ sectionLabel }}</span>
            <span class="search-result-section__count">{{ props.section.items.length }}</span>
            <span v-if="props.section.truncated" class="search-result-section__truncated" title="Limite do snapshot atingido para esta seção">
                Limite atingido
            </span>
        </div>

        <div class="search-result-section__items">
            <SearchResultItem v-for="hit in props.section.items" :key="hit.key" :hit="hit" :option-id="props.optionIdFor(hit.key)" :selected="props.selectedKey === hit.key" @activate="(item, action) => emit('activate', item, action)" @hover="(key) => emit('hover', key)" />
        </div>

        <div v-if="props.section.nextCursor" class="search-result-section__footer">
            <MaxButton :label="props.loadingMore ? 'Carregando…' : 'Carregar mais'" variant="text" severity="secondary" size="small" class="search-result-section__more" :disabled="props.loadingMore" :loading="props.loadingMore" :aria-label="`Carregar mais em ${sectionLabel}`" @click="emit('load-more', props.section.source)" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import SearchResultItem from '@/components/search/SearchResultItem.vue';
    import type { SearchNavigationAction } from '@/composables/search/useSearchNavigation';
    import { SEARCH_SECTION_LABELS } from '@/stores/search';
    import type {
        SearchHit_Serialize,
        SearchSection_Serialize,
        SearchSourceKind
    } from '@/types/bindings';

    export interface SearchResultSectionProps {
        section: SearchSection_Serialize;
        selectedKey: string | null;
        loadingMore?: boolean;
        optionIdFor: (key: string) => string;
    }

    const props = withDefaults(defineProps<SearchResultSectionProps>(), {
        loadingMore: false
    });

    const emit = defineEmits<{
        activate: [hit: SearchHit_Serialize, action: SearchNavigationAction];
        hover: [key: string];
        'load-more': [source: SearchSourceKind];
    }>();

    const headingId = computed<string>(() => `search-section-heading-${props.section.source.kind}`);

    const sectionLabel = computed<string>(
        () => SEARCH_SECTION_LABELS[props.section.source.kind] ?? props.section.source.kind
    );
</script>

<style lang="scss" scoped>
    .search-result-section {
        display: flex;
        flex-direction: column;
        gap: 4px;
        padding: 6px 0;
    }

    .search-result-section__header {
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 4px 12px;
    }

    .search-result-section__title {
        font-size: 11px;
        font-weight: 700;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .search-result-section__count {
        padding: 1px 6px;
        border-radius: 999px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 10px;
        font-weight: 600;
    }

    .search-result-section__truncated {
        margin-left: auto;
        font-size: 11px;
        color: var(--max-warning-400);
    }

    .search-result-section__items {
        display: flex;
        flex-direction: column;
        gap: 2px;
    }

    .search-result-section__footer {
        display: flex;
        justify-content: center;
        padding: 4px 12px 2px;
    }

    .search-result-section__more {
        padding: 5px 14px;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 6px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 12px;
        font-weight: 500;
        cursor: pointer;

        &:disabled {
            opacity: 0.6;
            cursor: default;
        }

        &:hover:not(:disabled) {
            border-color: var(--max-primary-500);
            color: var(--mt-ui-text-strong, var(--background-950));
        }
    }
</style>
