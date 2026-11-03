<template>
    <div :id="props.optionId" class="search-result-item" :class="{ 'is-selected': props.selected }" role="option" :aria-selected="props.selected" tabindex="-1" @click="emit('activate', props.hit, 'default')" @mouseenter="emit('hover', props.hit.key)">
        <span class="search-result-item__icon" aria-hidden="true">
            <MaxIcon :icon="sourceIcon" />
        </span>

        <div class="search-result-item__body">
            <div class="search-result-item__header">
                <span class="search-result-item__title">
                    <HighlightedText :segments="props.hit.title" />
                </span>
                <span v-if="props.hit.sourceTruncated" class="search-result-item__badge" title="Saída truncada na origem">
                    Truncado
                </span>
                <time v-if="formattedDate" class="search-result-item__time" :datetime="isoTimestamp">
                    {{ formattedDate }}
                </time>
            </div>

            <div v-if="props.hit.excerpt.length > 0" class="search-result-item__excerpt">
                <HighlightedText :segments="props.hit.excerpt" />
            </div>
        </div>

        <span v-if="props.hit.source.kind === 'templates'" class="search-result-item__hint" title="Ctrl+Enter para abrir em nova aba">
            Ctrl+Enter: nova aba
        </span>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import HighlightedText from '@/components/search/HighlightedText.vue';
    import type { SearchNavigationAction } from '@/composables/search/useSearchNavigation';
    import type { SearchHit_Serialize, SearchSourceKind } from '@/types/bindings';

    export interface SearchResultItemProps {
        hit: SearchHit_Serialize;
        optionId: string;
        selected: boolean;
    }

    const props = defineProps<SearchResultItemProps>();

    const emit = defineEmits<{
        activate: [hit: SearchHit_Serialize, action: SearchNavigationAction];
        hover: [key: string];
    }>();

    const SOURCE_ICONS: Readonly<Record<SearchSourceKind['kind'], string>> = Object.freeze({
        commands: 'lucide:terminal',
        output: 'lucide:scroll-text',
        sessions: 'lucide:history',
        projects: 'lucide:folder-git-2',
        templates: 'lucide:layout-template',
        groups: 'lucide:layers',
        group_runs: 'lucide:play-circle',
        issues: 'lucide:circle-dot',
        comments: 'lucide:message-square'
    });

    const ptBrDateFormatter = new Intl.DateTimeFormat('pt-BR', {
        day: '2-digit',
        month: '2-digit',
        year: 'numeric',
        hour: '2-digit',
        minute: '2-digit'
    });

    const sourceIcon = computed<string>(() => SOURCE_ICONS[props.hit.source.kind] ?? 'lucide:search');

    const formattedDate = computed<string>(() => {
        const ts = props.hit.occurredAtMs ?? props.hit.fetchedAtMs ?? null;
        if (typeof ts !== 'number' || !Number.isFinite(ts) || ts <= 0) {
            return '';
        }
        return ptBrDateFormatter.format(new Date(ts));
    });

    const isoTimestamp = computed<string>(() => {
        const ts = props.hit.occurredAtMs ?? props.hit.fetchedAtMs ?? null;
        if (typeof ts !== 'number' || !Number.isFinite(ts) || ts <= 0) {
            return '';
        }
        return new Date(ts).toISOString();
    });
</script>

<style lang="scss" scoped>
    .search-result-item {
        display: flex;
        align-items: flex-start;
        gap: 10px;
        padding: 8px 12px;
        border-radius: 6px;
        cursor: pointer;
        color: var(--mt-ui-text, var(--background-800));
        transition: background-color 120ms ease;

        &:hover,
        &.is-selected {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }

        &.is-selected {
            outline: 1px solid var(--max-primary-500);
        }
    }

    .search-result-item__icon {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        margin-top: 2px;
        color: var(--max-primary-400);
        flex-shrink: 0;
    }

    .search-result-item__body {
        display: flex;
        flex-direction: column;
        gap: 3px;
        min-width: 0;
        flex: 1;
    }

    .search-result-item__header {
        display: flex;
        align-items: center;
        gap: 8px;
        min-width: 0;
    }

    .search-result-item__title {
        font-size: 13px;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        flex: 1;
    }

    .search-result-item__badge {
        padding: 1px 6px;
        border-radius: 4px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--max-warning-400);
        font-size: 10px;
        font-weight: 600;
        flex-shrink: 0;
    }

    .search-result-item__time {
        font-size: 11px;
        color: var(--mt-ui-text-muted, var(--background-400));
        flex-shrink: 0;
    }

    .search-result-item__excerpt {
        font-size: 12px;
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-family: var(--mt-font-mono);
        line-height: 1.4;
        display: -webkit-box;
        -webkit-line-clamp: 2;
        -webkit-box-orient: vertical;
        overflow: hidden;
    }

    .search-result-item__hint {
        align-self: center;
        padding: 2px 6px;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 10px;
        flex-shrink: 0;
    }
</style>
