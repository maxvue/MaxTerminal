<template>
    <section class="issue-comments" aria-label="Comentários da issue">
        <h4 class="issue-comments__heading">Comentários ({{ props.comments.length }})</h4>
        <p v-if="props.comments.length === 0" class="issue-comments__empty">Nenhum comentário registrado nesta issue.</p>
        <div v-else class="issue-comments__list">
            <article v-for="comment in props.comments" :key="comment.commentId" class="issue-comments__item" :class="{ 'issue-comments__item--plan': comment.containsActivePlan }">
                <header class="issue-comments__item-header">
                    <span class="issue-comments__author">@{{ comment.authorLogin }}</span>
                    <span v-if="comment.containsActivePlan" class="issue-comments__plan-tag">Contém Plano de Implementação</span>
                </header>
                <MaxMarkdownViewer :source="comment.bodyMarkdown" :render-markdown="renderSafeMarkdown" class="issue-comments__body" />
            </article>
        </div>
    </section>
</template>

<script setup lang="ts">
    import { MaxMarkdownViewer } from '@maxvue/max-components-ui';
    import { renderSafeMarkdown } from '@/composables/useMarkdownRenderer';
    import type { IssueComment } from '@/types/bindings';

    export interface IssueCommentsProps {
        comments: IssueComment[];
    }

    const props = defineProps<IssueCommentsProps>();
</script>

<style lang="scss" scoped>
    .issue-comments {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .issue-comments__heading {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 700;
        color: var(--background-200);
    }

    .issue-comments__empty {
        margin: 0;
        font-size: 0.7813rem;
        color: var(--background-400);
    }

    .issue-comments__list {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .issue-comments__item {
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-850, var(--background-800));
        overflow: hidden;

        &--plan {
            border-color: var(--max-info-500);
        }
    }

    .issue-comments__item-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.375rem 0.75rem;
        background: var(--background-800);
        border-bottom: 1px solid var(--background-700);
        font-size: 0.75rem;
    }

    .issue-comments__author {
        font-weight: 600;
        color: var(--background-200);
    }

    .issue-comments__plan-tag {
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
        background: rgb(14 165 233 / 20%);
        color: var(--max-info-300);
        font-size: 0.6875rem;
        font-weight: 600;
    }

    .issue-comments__body {
        padding: 0.75rem;
        font-size: 0.8125rem;
        line-height: 1.5;
        color: var(--background-100);
    }
</style>
