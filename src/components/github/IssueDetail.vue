<template>
    <div class="issue-detail" role="region" aria-label="Detalhe da Issue">
        <div v-if="!props.detail" class="issue-detail__empty">
            <MaxIcon icon="lucide:circle-dot" />
            <p class="issue-detail__empty-text">Selecione uma issue na lista ao lado para visualizar a descrição, o plano de implementação e os comentários.</p>
        </div>
        <div v-else class="issue-detail__content">
            <header class="issue-detail__header">
                <div class="issue-detail__title-row">
                    <span class="issue-detail__number">#{{ props.detail.summary.number }}</span>
                    <h2 class="issue-detail__title">{{ props.detail.summary.title }}</h2>
                    <span class="issue-detail__state" :class="`issue-detail__state--${props.detail.summary.state}`">{{ props.detail.summary.state === 'open' ? 'Aberta' : 'Fechada' }}</span>
                </div>
                <div class="issue-detail__sub">
                    <span>Aberta por <strong>@{{ props.detail.summary.authorLogin }}</strong></span>
                    <div class="issue-detail__chips">
                        <IssueLabelChip v-for="label in sortedLabels" :key="label.name" :label="label" />
                    </div>
                </div>
                <IssueActionsMenu :busy="props.mutating" @send-maxcode="emit('send-maxcode', $event)" @open-tab-modal="emit('open-tab-modal')" />
            </header>
            <IssueLabelEditor :project-id="props.projectId" :issue-number="props.detail.summary.number" :labels="props.detail.summary.labels" :disabled="props.mutating" @update-labels="emit('update-labels', $event)" />
            <IssuePlanSection :detail="props.detail" />
            <section class="issue-detail__body-section" aria-label="Descrição da issue">
                <h4 class="issue-detail__section-title">Descrição</h4>
                <!-- eslint-disable-next-line vue/no-v-html -->
                <div class="issue-detail__markdown" v-html="bodyHtml" />
            </section>
            <IssueComments :comments="props.detail.comments" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { renderSafeMarkdown } from '@/composables/useMarkdownRenderer';
    import { sortLabelsCanonically } from '@/constants/githubLabels';
    import type {
        IssueDetail,
        LabelUpdatePatch,
        MaxCodeIssueAction,
        ProjectId,
        RepoLabel
    } from '@/types/bindings';
    import IssueActionsMenu from './IssueActionsMenu.vue';
    import IssueComments from './IssueComments.vue';
    import IssueLabelChip from './IssueLabelChip.vue';
    import IssueLabelEditor from './IssueLabelEditor.vue';
    import IssuePlanSection from './IssuePlanSection.vue';

    export interface IssueDetailProps {
        projectId: ProjectId;
        detail: IssueDetail | null;
        mutating?: boolean;
    }

    const props = withDefaults(defineProps<IssueDetailProps>(), {
        mutating: false
    });

    const emit = defineEmits<{
        'update-labels': [patch: LabelUpdatePatch];
        'send-maxcode': [action: MaxCodeIssueAction];
        'open-tab-modal': [];
    }>();

    const sortedLabels = computed<RepoLabel[]>(() =>
        props.detail ? sortLabelsCanonically(props.detail.summary.labels) : []
    );

    const bodyHtml = computed<string>(() => {
        if (!props.detail?.bodyMarkdown) {
            return '<p><em>Sem descrição informada.</em></p>';
        }
        return renderSafeMarkdown(props.detail.bodyMarkdown);
    });
</script>

<style lang="scss" scoped>
    .issue-detail {
        flex: 1;
        height: 100%;
        min-width: 0;
        min-height: 0;
        overflow-y: auto;
        background: var(--background-900);
    }

    .issue-detail__empty {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.625rem;
        height: 100%;
        padding: 2rem;
        text-align: center;
        color: var(--background-400);
    }

    .issue-detail__empty-text {
        margin: 0;
        max-width: 24rem;
        font-size: 0.8125rem;
    }

    .issue-detail__content {
        display: flex;
        flex-direction: column;
        gap: 1rem;
        padding: 1rem 1.25rem;
    }

    .issue-detail__header {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding-bottom: 0.875rem;
        border-bottom: 1px solid var(--background-700);
    }

    .issue-detail__title-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .issue-detail__number {
        font-family: var(--mt-font-mono, monospace);
        font-size: 1rem;
        font-weight: 700;
        color: var(--max-purple-400, #c084fc);
    }

    .issue-detail__title {
        margin: 0;
        flex: 1;
        font-size: 1.0625rem;
        font-weight: 700;
        color: var(--background-50);
    }

    .issue-detail__state {
        padding: 0.125rem 0.5rem;
        border-radius: 9999px;
        font-size: 0.7188rem;
        font-weight: 700;

        &--open {
            background: rgb(34 197 94 / 20%);
            color: var(--max-success-400);
        }

        &--closed {
            background: rgb(168 85 247 / 20%);
            color: var(--max-purple-400, #c084fc);
        }
    }

    .issue-detail__sub {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        font-size: 0.75rem;
        color: var(--background-300);
    }

    .issue-detail__chips {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
    }

    .issue-detail__body-section {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.875rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-850, var(--background-800));
    }

    .issue-detail__section-title {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 700;
        color: var(--background-200);
    }

    .issue-detail__markdown {
        font-size: 0.8125rem;
        line-height: 1.5;
        color: var(--background-100);
    }
</style>
