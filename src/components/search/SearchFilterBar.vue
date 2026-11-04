<template>
    <div class="search-filter-bar" role="region" aria-label="Filtros da busca global">
        <div class="search-filter-bar__row">
            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Escopo</span>
                <select class="search-filter-bar__select" :value="props.filters.projectId ?? ''" aria-label="Filtrar por projeto" @change="onProjectChange">
                    <option value="">Todos os projetos</option>
                    <option v-for="proj in props.projects" :key="proj.id" :value="proj.id">{{ proj.title }}</option>
                </select>
            </label>

            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Saída</span>
                <select class="search-filter-bar__select" :value="props.filters.exit.kind" aria-label="Filtrar por código de saída" @change="onExitChange">
                    <option value="any">Qualquer</option>
                    <option value="zero">Sucesso (0)</option>
                    <option value="nonzero">Com erro (!= 0)</option>
                    <option value="unknown">Desconhecido</option>
                </select>
            </label>

            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Período</span>
                <select class="search-filter-bar__select" :value="selectedPeriodPreset" aria-label="Filtrar por período" @change="onPeriodPresetChange">
                    <option value="all">Qualquer data</option>
                    <option value="1h">Última hora</option>
                    <option value="24h">Últimas 24 horas</option>
                    <option value="7d">Últimos 7 dias</option>
                    <option value="30d">Últimos 30 dias</option>
                </select>
            </label>

            <label class="search-filter-bar__field">
                <span class="search-filter-bar__label">Ordenar</span>
                <select class="search-filter-bar__select" :value="props.sort.kind" aria-label="Ordenar resultados" @change="onSortChange">
                    <option value="relevance">Relevância</option>
                    <option value="recent">Mais recentes</option>
                </select>
            </label>

            <MaxButton v-if="hasActiveFilters" label="Limpar filtros" variant="text" severity="secondary" size="small" class="search-filter-bar__clear" aria-label="Limpar filtros ativos" @click="clearFilters" />
        </div>

        <div class="search-filter-bar__chips" role="group" aria-label="Fontes de busca">
            <button v-for="kind in ALL_SEARCH_SOURCE_KINDS" :key="kind" type="button" class="search-filter-bar__chip" :class="{ 'is-active': isSourceSelected(kind), 'is-incompatible': !isSourceCompatible(kind) }" :aria-pressed="isSourceSelected(kind)" :title="chipTooltip(kind)" @click="toggleSource(kind)">
                {{ SEARCH_SECTION_LABELS[kind] }}
            </button>
        </div>

        <p v-if="incompatibleSourcesNotice" class="search-filter-bar__notice" role="status">
            {{ incompatibleSourcesNotice }}
        </p>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
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

    function onProjectChange(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const val = target.value.trim();
        emit('update:filters', {
            projectId: val.length > 0 ? val : null
        });
    }

    function onExitChange(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const kind = target.value as ExitFilter['kind'];
        emit('update:filters', {
            exit: { kind }
        });
    }

    function onPeriodPresetChange(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const preset = target.value as 'all' | '1h' | '24h' | '7d' | '30d';
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

    function onSortChange(event: Event): void {
        const target = event.target as HTMLSelectElement;
        const kind = target.value as SearchSort['kind'];
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
        padding: 3px 8px;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 4px;
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 11px;
        outline: none;

        &:focus {
            border-color: var(--max-primary-500);
        }
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
