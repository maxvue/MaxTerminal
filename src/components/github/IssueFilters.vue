<template>
    <div class="issue-filters" role="search" aria-label="Filtros de Issues">
        <div class="issue-filters__row">
            <div class="issue-filters__states" role="group" aria-label="Estado da issue">
                <button type="button" class="issue-filters__state-btn" :class="{ 'issue-filters__state-btn--active': props.filter.state === 'open' }" @click="emit('set-state', 'open')">Abertas</button>
                <button type="button" class="issue-filters__state-btn" :class="{ 'issue-filters__state-btn--active': props.filter.state === 'closed' }" @click="emit('set-state', 'closed')">Fechadas</button>
                <button type="button" class="issue-filters__state-btn" :class="{ 'issue-filters__state-btn--active': props.filter.state === 'all' }" @click="emit('set-state', 'all')">Todas</button>
            </div>
            <div class="issue-filters__search">
                <MaxIcon icon="lucide:search" />
                <input type="search" class="issue-filters__search-input" :value="props.filter.searchText ?? ''" placeholder="Buscar por #número, título ou descrição..." aria-label="Buscar issues" @input="onSearchInput">
            </div>
            <select class="issue-filters__select" aria-label="Filtrar por label de estado" :value="selectedStateLabel" @change="onStateLabelSelect">
                <option value="">Todos os Estados (15)</option>
                <option v-for="spec in stateSpecs" :key="spec.name" :value="spec.name">{{ spec.name }}</option>
            </select>
            <select class="issue-filters__select" aria-label="Filtrar por categoria" :value="selectedCategoryLabel" @change="onCategoryLabelSelect">
                <option value="">Todas as Categorias (6)</option>
                <option v-for="spec in categorySpecs" :key="spec.name" :value="spec.name">{{ spec.name }}</option>
            </select>
            <button type="button" class="issue-filters__plan-btn" :class="{ 'issue-filters__plan-btn--active': props.filter.hasPlan === true }" @click="onToggleHasPlan">
                <MaxIcon icon="lucide:file-check-2" />
                <span>Com Plano</span>
            </button>
            <select class="issue-filters__select" aria-label="Ordenação das issues" :value="props.filter.sort ?? 'updated_desc'" @change="onSortChange">
                <option value="updated_desc">Atualizadas recentemente</option>
                <option value="created_desc">Mais recentes</option>
                <option value="created_asc">Mais antigas</option>
                <option value="comments_desc">Mais comentadas</option>
            </select>
            <button v-if="props.hasActiveFilters" type="button" class="issue-filters__reset-btn" title="Limpar filtros" @click="emit('reset')">Limpar</button>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed } from 'vue';
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

    const selectedStateLabel = computed<string>(() => props.filter.stateLabels?.[0] ?? '');
    const selectedCategoryLabel = computed<string>(() => props.filter.categoryLabels?.[0] ?? '');

    function onSearchInput(event: Event): void {
        const target = event.target as HTMLInputElement;
        emit('set-search', target.value);
    }

    function onStateLabelSelect(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const val = target.value;
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

    function onCategoryLabelSelect(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const val = target.value;
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

    function onSortChange(event: Event): void {
        const target = event.target as HTMLSelectElement;
        emit('set-sort', target.value as IssueSortOrder);
    }
</script>

<style lang="scss" scoped>
    .issue-filters {
        padding: 0.5rem 0.875rem;
        border-bottom: 1px solid var(--background-700);
        background: var(--background-850, var(--background-800));
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
        border: 1px solid var(--background-600);
        overflow: hidden;
    }

    .issue-filters__state-btn {
        height: 1.75rem;
        padding: 0 0.625rem;
        border: none;
        background: var(--background-900);
        color: var(--background-300);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--active {
            background: var(--max-primary-500);
            color: var(--background-950, #090d16);
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
        border: 1px solid var(--background-600);
        background: var(--background-900);
        color: var(--background-300);
    }

    .issue-filters__search-input {
        flex: 1;
        border: none;
        background: transparent;
        color: var(--background-100);
        font-size: 0.75rem;
        outline: none;
    }

    .issue-filters__select {
        height: 1.75rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-900);
        color: var(--background-100);
        font-size: 0.75rem;
    }

    .issue-filters__plan-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        height: 1.75rem;
        padding: 0 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-900);
        color: var(--background-200);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--active {
            border-color: var(--max-info-500);
            background: rgb(14 165 233 / 20%);
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
