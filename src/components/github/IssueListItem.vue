<template>
    <MaxButton type="button" class="issue-list-item" :class="props.selected ? 'issue-list-item--selected' : ''" :aria-selected="props.selected" @click="emit('select', props.issue.number)">
        <div class="issue-list-item__top">
            <span class="issue-list-item__number">#{{ props.issue.number }}</span>
            <span class="issue-list-item__title">{{ props.issue.title }}</span>
            <span v-if="props.issue.hasPlan" class="issue-list-item__plan-badge" title="Possui Plano de Implementação">Plano</span>
        </div>
        <div v-if="sortedLabels.length > 0" class="issue-list-item__labels">
            <IssueLabelChip v-for="label in sortedLabels" :key="label.name" :label="label" :compact="true" />
        </div>
        <div class="issue-list-item__meta">
            <span class="issue-list-item__author">@{{ props.issue.authorLogin }}</span>
            <span v-if="props.issue.commentsCount > 0" class="issue-list-item__comments">
                <MaxIcon icon="lucide:message-square" />
                <span>{{ props.issue.commentsCount }}</span>
            </span>
        </div>
    </MaxButton>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import { sortLabelsCanonically } from '@/constants/githubLabels';
    import type { IssueSummary, RepoLabel } from '@/types/bindings';
    import IssueLabelChip from './IssueLabelChip.vue';

    export interface IssueListItemProps {
        issue: IssueSummary;
        selected?: boolean;
    }

    const props = withDefaults(defineProps<IssueListItemProps>(), {
        selected: false
    });

    const emit = defineEmits<{
        select: [issueNumber: number];
    }>();

    const sortedLabels = computed<RepoLabel[]>(() => sortLabelsCanonically(props.issue.labels));
</script>

<style lang="scss" scoped>
    .issue-list-item {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
        width: 100%;
        padding: 0.625rem 0.75rem;
        border: none;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        background: transparent;
        color: var(--mt-ui-text, var(--background-800));
        text-align: left;
        cursor: pointer;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }

        &--selected {
            background: var(--mt-ui-issue-subtle, color-mix(in srgb, var(--max-purple-500, #a855f7) 14%, transparent));
            border-left: 3px solid var(--mt-ui-issue-category, var(--max-purple-500, #a855f7));
        }
    }

    .issue-list-item__top {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        min-width: 0;
    }

    .issue-list-item__number {
        font-family: var(--mt-font-mono, monospace);
        font-size: 0.75rem;
        font-weight: 700;
        color: var(--mt-ui-issue-category-hover, var(--max-purple-400, #c084fc));
        flex-shrink: 0;
    }

    .issue-list-item__title {
        flex: 1;
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .issue-list-item__plan-badge {
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
        background: var(--mt-ui-info-subtle, color-mix(in srgb, var(--max-info-500) 20%, transparent));
        color: var(--max-info-300);
        font-size: 0.625rem;
        font-weight: 700;
        flex-shrink: 0;
    }

    .issue-list-item__labels {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
    }

    .issue-list-item__meta {
        display: flex;
        align-items: center;
        justify-content: space-between;
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .issue-list-item__comments {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
    }
</style>
