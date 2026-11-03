<template>
    <div class="issue-list" role="listbox" aria-label="Lista de issues do repositório">
        <div v-if="props.stale" class="issue-list__stale" role="status">
            <MaxIcon icon="lucide:cloud-off" />
            <span>Exibindo cache local SQLite (última sincronização offline).</span>
        </div>
        <div v-if="props.loading && props.issues.length === 0" class="issue-list__loading">
            <MaxIcon icon="lucide:loader-2" />
            <span>Carregando issues...</span>
        </div>
        <div v-else-if="props.issues.length === 0" class="issue-list__empty">
            <p class="issue-list__empty-title">Nenhuma issue encontrada</p>
            <p class="issue-list__empty-desc">Ajuste os filtros aplicados ou crie uma nova issue para iniciar o fluxo.</p>
        </div>
        <div v-else class="issue-list__items">
            <IssueListItem v-for="issue in props.issues" :key="issue.number" :issue="issue" :selected="issue.number === props.selectedNumber" @select="emit('select', $event)" />
        </div>
        <div v-if="props.totalMatching > props.issues.length" class="issue-list__footer">
            <span>Exibindo {{ props.issues.length }} de {{ props.totalMatching }} issues</span>
        </div>
    </div>
</template>

<script setup lang="ts">
    import type { IssueSummary } from '@/types/bindings';
    import IssueListItem from './IssueListItem.vue';

    export interface IssueListProps {
        issues: IssueSummary[];
        selectedNumber: number | null;
        loading?: boolean;
        stale?: boolean;
        totalMatching?: number;
    }

    const props = withDefaults(defineProps<IssueListProps>(), {
        loading: false,
        stale: false,
        totalMatching: 0
    });

    const emit = defineEmits<{
        select: [issueNumber: number];
    }>();
</script>

<style lang="scss" scoped>
    .issue-list {
        display: flex;
        flex-direction: column;
        height: 100%;
        min-height: 0;
        overflow: hidden;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-right: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .issue-list__stale {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.375rem 0.75rem;
        background: rgb(245 158 11 / 15%);
        border-bottom: 1px solid var(--max-warning-500);
        color: var(--max-warning-300);
        font-size: 0.7188rem;
    }

    .issue-list__loading,
    .issue-list__empty {
        flex: 1;
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 0.5rem;
        padding: 1.5rem;
        text-align: center;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.8125rem;
    }

    .issue-list__empty-title {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .issue-list__empty-desc {
        margin: 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .issue-list__items {
        flex: 1;
        overflow-y: auto;
    }

    .issue-list__footer {
        padding: 0.375rem 0.75rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.6875rem;
        text-align: center;
    }
</style>
