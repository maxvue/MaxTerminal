<template>
    <div class="search-filter-bar" role="region" aria-label="Filtros da busca global">
        <div class="search-filter-bar__row">
            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Escopo</span>
                <MaxInputSelect :model-value="props.filters.projectId ?? ''" :options="projectOptions" option-label="label" option-value="value" option-name="label" aria-label="Filtrar por projeto" class="search-filter-bar__select" @update:model-value="onProjectSelectChange" />
            </label>

            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Saída</span>
                <MaxInputSelect :model-value="props.filters.exit.kind" :options="exitOptions" option-label="label" option-value="value" option-name="label" aria-label="Filtrar por código de saída" class="search-filter-bar__select" @update:model-value="onExitSelectChange" />
            </label>

            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Período</span>
                <MaxInputSelect :model-value="selectedPeriodPreset" :options="periodOptions" option-label="label" option-value="value" option-name="label" aria-label="Filtrar por período" class="search-filter-bar__select" @update:model-value="onPeriodPresetSelectChange" />
            </label>

            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Ordenar</span>
                <MaxInputSelect :model-value="props.sort.kind" :options="sortOptions" option-label="label" option-value="value" option-name="label" aria-label="Ordenar resultados" class="search-filter-bar__select" @update:model-value="onSortSelectChange" />
            </label>

            <MaxButton v-if="hasActiveFilters" label="Limpar filtros" variant="text" severity="secondary" size="small" class="search-filter-bar__clear" aria-label="Limpar filtros ativos" @click="clearFilters" />
        </div>

        <div class="search-filter-bar__chips" role="group" aria-label="Fontes de busca">
            <MaxButton v-for="kind in ALL_SEARCH_SOURCE_KINDS" :key="kind" type="button" class="search-filter-bar__chip" :class="`${isSourceSelected(kind) ? 'is-active ' : ''}${!isSourceCompatible(kind) ? 'is-incompatible' : ''}`.trim()" :aria-pressed="isSourceSelected(kind)" :title="chipTooltip(kind)" @click="toggleSource(kind)">
                {{ SEARCH_SECTION_LABELS[kind] }}
            </MaxButton>
        </div>

        <p v-if="incompatibleSourcesNotice" class="search-filter-bar__notice" role="status">
            {{ incompatibleSourcesNotice }}
        </p>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import { MaxButton, MaxInputSelect } from '@maxvue/max-components-ui';
    import {
        ALL_SEARCH_SOURCE_KINDS,
        createDefaultSearchFilters,
        SEARCH_SECTION_LABELS
    } from '@/stores/search';
    import type {
        ExitFilter,
        Project_Serialize,
        SearchFilters_Deserialize,
        SearchSort,
        SearchSourceKind
    } from '@/types/bindings';

    export interface SearchFilterBarProps {
        filters: SearchFilters_Deserialize;
        sort: SearchSort;
        projects?: readonly Project_Serialize[];
    }

    const props = withDefaults(defineProps<SearchFilterBarProps>(), {
        projects: () => []
    });

    const emit = defineEmits<{
        'update:filters': [partial: Partial<SearchFilters_Deserialize>];
        'update:sort': [sort: SearchSort];
    }>();

    const selectedPeriodPreset = ref<'all' | '1h' | '24h' | '7d' | '30d'>('all');

    const projectOptions = computed(() => [
        { label: 'Todos os projetos', value: '' },
        ...props.projects.map((p) => ({ label: p.title, value: p.id }))
    ]);

    const exitOptions = [
        { label: 'Qualquer', value: 'any' },
        { label: 'Sucesso (exit 0)', value: 'success' },
        { label: 'Erro (exit != 0)', value: 'error' }
    ];

    const periodOptions = [
        { label: 'Qualquer data', value: 'all' },
        { label: 'Última hora', value: '1h' },
        { label: 'Últimas 24h', value: '24h' },
        { label: 'Últimos 7 dias', value: '7d' },
        { label: 'Últimos 30 dias', value: '30d' }
    ];

    const sortOptions = [
        { label: 'Mais recentes primeiro', value: 'recent' },
        { label: 'Mais relevantes', value: 'relevance' }
    ];

    const hasActiveFilters = computed<boolean>(() => {
        const f = props.filters;
        return (
            f.projectId !== null ||
            f.tabId !== null ||
            f.sessionId !== null ||
            f.paneId !== null ||
            f.fromMs !== null ||
            f.untilMs !== null ||
            f.exit.kind !== 'any' ||
            f.sources.length > 0
        );
    });

    function isSourceSelected(kind: SearchSourceKind['kind']): boolean {
        return props.filters.sources.some((s) => s.kind === kind);
    }

    function isSourceCompatible(kind: SearchSourceKind['kind']): boolean {
        if (props.filters.exit.kind !== 'any') {
            if (kind !== 'commands' && kind !== 'group_runs') {
                return false;
            }
        }
        if (props.filters.paneId !== null && kind !== 'commands' && kind !== 'output') {
            return false;
        }
        if (
            (props.filters.tabId !== null || props.filters.sessionId !== null) &&
            kind !== 'commands' &&
            kind !== 'output' &&
            kind !== 'sessions'
        ) {
            return false;
        }
        return true;
    }

    function chipTooltip(kind: SearchSourceKind['kind']): string {
        if (!isSourceCompatible(kind)) {
            return `${SEARCH_SECTION_LABELS[kind]} oculto pelos filtros ativos`;
        }
        return `Filtrar por ${SEARCH_SECTION_LABELS[kind]}`;
    }

    const incompatibleSourcesNotice = computed<string | null>(() => {
        if (props.filters.exit.kind !== 'any') {
            return 'Filtro de código de saída ativo: exibindo apenas Blocos de comando e Execuções de grupos.';
        }
        if (props.filters.paneId !== null) {
            return 'Filtro de painel ativo: exibindo apenas Blocos de comando e Histórico de sessões.';
        }
        if (props.filters.tabId !== null || props.filters.sessionId !== null) {
            return 'Filtro de aba/sessão ativo: exibindo apenas Blocos de comando, Histórico de sessões e Sessões.';
        }
        return null;
    });

    function onProjectSelectChange(value: unknown): void {
        const val = typeof value === 'string' ? value.trim() : '';
        emit('update:filters', {
            projectId: val.length > 0 ? val : null
        });
    }

    function onExitSelectChange(value: unknown): void {
        const kind = (typeof value === 'string' ? value : 'any') as ExitFilter['kind'];
        emit('update:filters', {
            exit: { kind }
        });
    }

    function onPeriodPresetSelectChange(value: unknown): void {
        const preset = (typeof value === 'string' ? value : 'all') as 'all' | '1h' | '24h' | '7d' | '30d';
        selectedPeriodPreset.value = preset;
        const now = Date.now();
        if (preset === 'all') {
            emit('update:filters', { fromMs: null, untilMs: null });
            return;
        }
        const deltaMap: Record<'1h' | '24h' | '7d' | '30d', number> = {
            '1h': 3_600_000,
            '24h': 86_400_000,
            '7d': 7 * 86_400_000,
            '30d': 30 * 86_400_000
        };
        emit('update:filters', {
            fromMs: now - deltaMap[preset],
            untilMs: null
        });
    }

    function onSortSelectChange(value: unknown): void {
        const kind = (typeof value === 'string' ? value : 'recent') as SearchSort['kind'];
        emit('update:sort', { kind });
    }

    function toggleSource(kind: SearchSourceKind['kind']): void {
        const current = [...props.filters.sources];
        const idx = current.findIndex((s) => s.kind === kind);
        if (idx >= 0) {
            current.splice(idx, 1);
        } else {
            current.push({ kind });
        }
        emit('update:filters', { sources: current });
    }

    function clearFilters(): void {
        selectedPeriodPreset.value = 'all';
        emit('update:filters', createDefaultSearchFilters());
    }
</script>

<style lang="scss" scoped>
    .search-filter-bar {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding: 8px 12px;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .search-filter-bar__row {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 8px;
    }

    .search-filter-bar__field {
        display: inline-flex;
        align-items: center;
        gap: 6px;
        font-size: 11px;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .search-filter-bar__label {
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .search-filter-bar__select {
        min-width: 130px;
        font-size: 11px;
    }

    .search-filter-bar__clear {
        margin-left: auto;
        padding: 3px 8px;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 11px;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--mt-ui-text-strong, var(--background-950));
        }
    }

    .search-filter-bar__chips {
        display: flex;
        flex-wrap: wrap;
        gap: 6px;
    }

    .search-filter-bar__chip {
        padding: 2px 8px;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 999px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 11px;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
            color: var(--mt-ui-text-strong, var(--background-950));
        }

        &.is-active {
            background: var(--max-primary-600);
            border-color: var(--max-primary-500);
            color: var(--max-primary-content, #fff);
        }

        &.is-incompatible {
            opacity: 0.45;
        }
    }

    .search-filter-bar__notice {
        margin: 0;
        font-size: 11px;
        color: var(--max-warning-400);
    }
</style>
