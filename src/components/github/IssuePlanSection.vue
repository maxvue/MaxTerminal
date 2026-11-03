<template>
    <section v-if="hasPlan" class="issue-plan-section" aria-label="Plano de Implementação">
        <header class="issue-plan-section__header">
            <div class="issue-plan-section__title-group">
                <MaxIcon icon="lucide:compass" />
                <h4 class="issue-plan-section__title">{{ headingTitle || 'PLANO DE IMPLEMENTAÇÃO' }}</h4>
                <span v-if="isProvisional" class="issue-plan-section__provisional" title="A issue está em estado Waiting Plan, Replan ou Rejected">Provisório</span>
            </div>
            <span class="issue-plan-section__meta">Origem: {{ sourceLabel }} — @{{ authorLogin }}</span>
        </header>
        <MaxMarkdownViewer v-if="plan?.markdown" :source="plan.markdown" :render-markdown="renderSafeMarkdown" class="issue-plan-section__body" />
    </section>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxIcon, MaxMarkdownViewer } from '@maxvue/max-components-ui';
    import { renderSafeMarkdown } from '@/composables/useMarkdownRenderer';
    import { useIssuePlan } from '@/composables/useIssuePlan';
    import type { IssueDetail } from '@/types/bindings';

    export interface IssuePlanSectionProps {
        detail: IssueDetail | null;
    }

    const props = defineProps<IssuePlanSectionProps>();

    const detailRef = computed<IssueDetail | null>(() => props.detail);
    const { plan, hasPlan, isProvisional, sourceLabel, authorLogin, headingTitle } = useIssuePlan(detailRef);
</script>

<style lang="scss" scoped>
    .issue-plan-section {
        display: flex;
        flex-direction: column;
        border-radius: 0.5rem;
        border: 1px solid var(--max-info-500);
        background: rgb(14 165 233 / 8%);
        overflow: hidden;
    }

    .issue-plan-section__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.625rem 0.875rem;
        background: rgb(14 165 233 / 15%);
        border-bottom: 1px solid rgb(14 165 233 / 30%);
    }

    .issue-plan-section__title-group {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        color: var(--max-info-300);
    }

    .issue-plan-section__title {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 700;
    }

    .issue-plan-section__provisional {
        padding: 0.0625rem 0.375rem;
        border-radius: 0.25rem;
        background: rgb(245 158 11 / 25%);
        color: var(--max-warning-300);
        font-size: 0.6875rem;
        font-weight: 700;
    }

    .issue-plan-section__meta {
        font-size: 0.6875rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .issue-plan-section__body {
        padding: 0.875rem;
        font-size: 0.8125rem;
        line-height: 1.5;
        color: var(--mt-ui-text, var(--background-800));
    }
</style>
