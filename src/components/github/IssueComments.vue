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
                <!-- eslint-disable-next-line vue/no-v-html -->
                <div class="issue-comments__body" v-html="renderSafeMarkdown(comment.bodyMarkdown)" />
            </article>
        </div>
    </section>
</template>

<script setup lang="ts">
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
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .issue-comments__empty {
        margin: 0;
        font-size: 0.7813rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .issue-comments__list {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
    }

    .issue-comments__item {
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
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
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        font-size: 0.75rem;
    }

    .issue-comments__author {
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
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
        color: var(--mt-ui-text, var(--background-800));
    }
</style>
