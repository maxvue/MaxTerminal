<template>
    <div class="issue-filters" role="search" aria-label="Filtros de Issues">
        <div class="issue-filters__row">
            <MaxSegmentedControl v-bind="{ ariaLabel: 'Estado da issue' }" :model-value="props.filter.state ?? 'open'" :options="stateFilterOptions" size="small" class="issue-filters__states" @update:model-value="onStateChange" />
            <MaxInputText :model-value="props.filter.searchText ?? ''" placeholder="Buscar por #número, título ou descrição..." aria-label="Buscar issues" icon="lucide:search" class="issue-filters__search" @update:model-value="(val) => emit('set-search', String(val ?? ''))" />
            <MaxInputSelect :model-value="selectedStateLabel" :options="stateSelectOptions" option-label="label" option-value="value" option-name="label" aria-label="Filtrar por label de estado" class="issue-filters__select" @change="onStateLabelChange" />
            <MaxInputSelect :model-value="selectedCategoryLabel" :options="categorySelectOptions" option-label="label" option-value="value" option-name="label" aria-label="Filtrar por categoria" class="issue-filters__select" @change="onCategoryLabelChange" />
            <MaxButton v-if="props.filter.hasPlan === true" type="button" size="sm" icon="lucide:file-check-2" label="Com Plano" class="issue-filters__plan-btn" @click="onToggleHasPlan" />
            <MaxButton v-else type="button" severity="secondary" size="sm" icon="lucide:file-check-2" label="Com Plano" class="issue-filters__plan-btn" @click="onToggleHasPlan" />
            <MaxInputSelect :model-value="props.filter.sort ?? 'updated_desc'" :options="sortOptions" option-label="label" option-value="value" option-name="label" aria-label="Ordenação das issues" class="issue-filters__select" @change="onSortSelectChange" />
            <MaxButton v-if="props.hasActiveFilters" type="button" severity="secondary" size="sm" label="Limpar" title="Limpar filtros" class="issue-filters__reset-btn" @click="emit('reset')" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
    import { MaxButton, MaxInputSelect, MaxInputText, MaxSegmentedControl } from '@maxvue/max-components-ui';
    import { CANONICAL_CATEGORY_LABELS, CANONICAL_STATE_LABELS } from '@/constants/githubLabels';
    import type { IssueFilter, IssueSortOrder, IssueStateFilter } from '@/types/bindings';

    export interface IssueFiltersProps {
        filter: IssueFilter;
        hasActiveFilters?: boolean;
    }

    const props = withDefaults(defineProps<IssueFiltersProps>(), {
        hasActiveFilters: false
    });

    const emit = defineEmits<{
        'set-state': [state: IssueStateFilter];
        'toggle-state-label': [labelName: string];
        'toggle-category-label': [labelName: string];
        'set-search': [searchText: string];
        'set-has-plan': [hasPlan: boolean | null];
        'set-sort': [sort: IssueSortOrder];
        reset: [];
    }>();

    const stateSpecs = CANONICAL_STATE_LABELS;
    const categorySpecs = CANONICAL_CATEGORY_LABELS;

    const stateFilterOptions = [
        { label: 'Abertas', value: 'open' },
        { label: 'Fechadas', value: 'closed' },
        { label: 'Todas', value: 'all' }
    ];

    const stateSelectOptions = computed(() => [
        { label: 'Todos os Estados (15)', value: '' },
        ...stateSpecs.map((s) => ({ label: s.name, value: s.name }))
    ]);

    const categorySelectOptions = computed(() => [
        { label: 'Todas as Categorias (6)', value: '' },
        ...categorySpecs.map((s) => ({ label: s.name, value: s.name }))
    ]);

    const sortOptions = [
        { label: 'Atualizadas recentemente', value: 'updated_desc' },
        { label: 'Mais recentes', value: 'created_desc' },
        { label: 'Mais antigas', value: 'created_asc' },
        { label: 'Mais comentadas', value: 'comments_desc' }
    ];

    const selectedStateLabel = computed<string>(() => props.filter.stateLabels?.[0] ?? '');
    const selectedCategoryLabel = computed<string>(() => props.filter.categoryLabels?.[0] ?? '');

    function onStateChange(valueOrEv: unknown): void {
        const nextState = (typeof valueOrEv === 'string' ? valueOrEv : (valueOrEv as { value?: string })?.value ?? 'open') as IssueStateFilter;
        emit('set-state', nextState);
    }

    function onStateLabelChange(valOrEv: unknown): void {
        const val = typeof valOrEv === 'string' ? valOrEv : ((valOrEv as { target?: HTMLSelectElement })?.target?.value ?? '');
        if (!val && selectedStateLabel.value) {
            emit('toggle-state-label', selectedStateLabel.value);
            return;
        }
        if (val) {
            if (selectedStateLabel.value && selectedStateLabel.value !== val) {
                emit('toggle-state-label', selectedStateLabel.value);
            }
            emit('toggle-state-label', val);
        }
    }

    function onCategoryLabelChange(valOrEv: unknown): void {
        const val = typeof valOrEv === 'string' ? valOrEv : ((valOrEv as { target?: HTMLSelectElement })?.target?.value ?? '');
        if (!val && selectedCategoryLabel.value) {
            emit('toggle-category-label', selectedCategoryLabel.value);
            return;
        }
        if (val) {
            if (selectedCategoryLabel.value && selectedCategoryLabel.value !== val) {
                emit('toggle-category-label', selectedCategoryLabel.value);
            }
            emit('toggle-category-label', val);
        }
    }

    function onToggleHasPlan(): void {
        const next = props.filter.hasPlan === true ? null : true;
        emit('set-has-plan', next);
    }

    function onSortSelectChange(valOrEv: unknown): void {
        const val = (typeof valOrEv === 'string' ? valOrEv : ((valOrEv as { target?: HTMLSelectElement })?.target?.value ?? 'updated_desc')) as IssueSortOrder;
        emit('set-sort', val);
    }
</script>

<style lang="scss" scoped>
    .issue-filters {
        padding: 0.5rem 0.875rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .issue-filters__row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.5rem;
    }

    .issue-filters__states {
        display: inline-flex;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        overflow: hidden;
    }

    .issue-filters__state-btn {
        height: 1.75rem;
        padding: 0 0.625rem;
        border: none;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--active {
            background: var(--max-primary-500);
            color: var(--max-primary-content, #fff);
        }
    }

    .issue-filters__search {
        flex: 1;
        min-width: 12rem;
        display: flex;
        align-items: center;
        gap: 0.375rem;
        height: 1.75rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .issue-filters__search-input {
        flex: 1;
        border: none;
        background: transparent;
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.75rem;
        outline: none;
    }

    .issue-filters__select {
        height: 1.75rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
    }

    .issue-filters__plan-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        height: 1.75rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--active {
            border-color: var(--max-info-500);
            background: var(--mt-ui-info-subtle, color-mix(in srgb, var(--max-info-500) 20%, transparent));
            color: var(--max-info-300);
        }
    }

    .issue-filters__reset-btn {
        height: 1.75rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: transparent;
        color: var(--background-300);
        font-size: 0.75rem;
        cursor: pointer;
    }
</style>
