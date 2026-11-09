<template>
    <div v-if="searchStore.open" class="global-search-backdrop" @mousedown.self="handleClose">
        <div class="global-search-palette" role="dialog" aria-modal="true" aria-labelledby="global-search-dialog-title" @keydown="onDialogKeydown">
            <h2 id="global-search-dialog-title" class="sr-only">Buscar em tudo…</h2>

            <div class="global-search-palette__top">
                <MaxIcon icon="lucide:search" class="global-search-palette__search-icon" aria-hidden="true" />
                <input ref="inputRef" :value="searchStore.query" type="text" class="global-search-palette__input" role="combobox" aria-label="Buscar em tudo…" :aria-expanded="hasResults" aria-controls="global-search-results-listbox" aria-autocomplete="list" :aria-activedescendant="activeDescendantId" placeholder="Buscar em tudo… (comandos, saídas, sessões, projetos, templates, grupos, issues)" @input="onQueryInput" @compositionstart="onCompositionStart" @compositionend="onCompositionEndEvent">
                <MaxButton v-if="searchStore.query.length > 0" type="button" class="global-search-palette__clear" aria-label="Limpar consulta" @click="clearQuery">Limpar</MaxButton>
                <MaxButton v-if="searchStore.query.length > 0" type="button" class="global-search-palette__refresh" aria-label="Atualizar resultados da busca" @click="() => void searchStore.run()">Atualizar</MaxButton>
                <MaxButton type="button" class="global-search-palette__close" aria-label="Fechar busca global (Escape)" @click="handleClose">Esc</MaxButton>
            </div>

            <SearchFilterBar :filters="searchStore.filters" :sort="searchStore.sort" :projects="projectsStore.projects" @update:filters="(p) => searchStore.updateFilters(p)" @update:sort="(s) => searchStore.updateSort(s)" />

            <div v-if="indexProgress" class="global-search-palette__progress" role="status">
                Reconstruindo índice de busca ({{ indexProgress.completedSources }}/{{ indexProgress.totalSources }})…
            </div>

            <div id="global-search-results-listbox" class="global-search-palette__results" role="listbox" aria-label="Resultados da busca global">
                <div v-if="searchStore.status === 'idle'" class="global-search-palette__state" role="option" aria-selected="false">
                    Digite um termo para buscar em Blocos de comando, Histórico de sessões, Projetos, Templates, Grupos de comandos e Issues.
                </div>

                <div v-else-if="(searchStore.status === 'loading' || searchStore.status === 'debouncing') && searchStore.sections.length === 0" class="global-search-palette__state" role="option" aria-selected="false">
                    Buscando resultados…
                </div>

                <div v-else-if="searchStore.status === 'error' && searchStore.error" class="global-search-palette__state is-error" role="option" aria-selected="false">
                    <p class="global-search-palette__error-msg">{{ errorBannerMessage }}</p>
                    <MaxButton type="button" class="global-search-palette__retry" @click="() => void searchStore.run()">Atualizar resultados</MaxButton>
                </div>

                <div v-else-if="searchStore.status === 'empty' || searchStore.sections.length === 0" class="global-search-palette__state" role="option" aria-selected="false">
                    {{ emptyStateLabel }}
                </div>

                <template v-else>
                    <SearchResultSection v-for="section in searchStore.sections" :key="section.source.kind" :section="section" :selected-key="searchStore.selectedKey" :loading-more="searchStore.loadingMoreSources.has(section.source.kind)" :option-id-for="optionIdFor" @activate="handleActivateHit" @hover="(k) => searchStore.select(k)" @load-more="(src) => void searchStore.loadMore(src)" />
                </template>
            </div>

            <footer class="global-search-palette__footer">
                <span>↑↓ Navegar</span>
                <span>Enter Abrir</span>
                <span>Ctrl+Enter Nova aba (template)</span>
                <span>Esc Fechar</span>
            </footer>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, nextTick, onMounted, ref, watch } from 'vue';
    import { MaxButton } from '@maxvue/max-components-ui';
    import SearchFilterBar from '@/components/search/SearchFilterBar.vue';
    import SearchResultSection from '@/components/search/SearchResultSection.vue';
    import { useGlobalSearch } from '@/composables/search/useGlobalSearch';
    import {
        useSearchNavigation,
        type SearchNavigationAction
    } from '@/composables/search/useSearchNavigation';
    import { useProjectsStore } from '@/stores/projects';
    import type { SearchHit_Serialize } from '@/types/bindings';

    const projectsStore = useProjectsStore();
    const {
        searchStore,
        indexProgress,
        closePalette,
        onCompositionStart,
        onCompositionEnd,
        attachEvents,
        rememberFocusBeforeOpen
    } = useGlobalSearch();
    const { openTarget } = useSearchNavigation();

    const inputRef = ref<HTMLInputElement | null>(null);

    const hasResults = computed<boolean>(() => searchStore.flatItems.length > 0);

    function optionIdFor(key: string): string {
        return `search-hit-opt-${key.replace(/[^a-zA-Z0-9_-]/g, '_')}`;
    }

    const activeDescendantId = computed<string | undefined>(() => {
        if (!searchStore.selectedKey || !hasResults.value) {
            return undefined;
        }
        return optionIdFor(searchStore.selectedKey);
    });

    const emptyStateLabel = computed<string>(() => {
        const trimmed = searchStore.query.trim();
        return `Nenhum resultado para “${trimmed}”`;
    });

    const errorBannerMessage = computed<string>(() => {
        const err = searchStore.error;
        if (!err) {
            return '';
        }
        if (err.code === 'search.cursor_expired' || err.code === 'search.invalid_cursor') {
            return 'Os resultados expiraram. Clique em Atualizar resultados.';
        }
        return err.message;
    });

    function onQueryInput(event: Event): void {
        const target = event.target as HTMLInputElement;
        searchStore.updateQuery(target.value);
    }

    function onCompositionEndEvent(event: CompositionEvent): void {
        const target = event.target as HTMLInputElement | null;
        onCompositionEnd(target?.value);
    }

    function clearQuery(): void {
        searchStore.updateQuery('');
        inputRef.value?.focus();
    }

    function handleClose(): void {
        closePalette();
    }

    async function handleActivateHit(
        hit: SearchHit_Serialize,
        action: SearchNavigationAction = 'default'
    ): Promise<void> {
        const res = await openTarget(hit.target, action);
        if (res.ok) {
            closePalette();
        }
    }

    function onDialogKeydown(event: KeyboardEvent): void {
        if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            handleClose();
            return;
        }

        if (event.key === 'ArrowDown') {
            event.preventDefault();
            searchStore.moveSelection(1);
            return;
        }

        if (event.key === 'ArrowUp') {
            event.preventDefault();
            searchStore.moveSelection(-1);
            return;
        }

        if (event.key === 'Home' && event.ctrlKey) {
            event.preventDefault();
            const first = searchStore.flatItems[0];
            if (first) {
                searchStore.select(first.key);
            }
            return;
        }

        if (event.key === 'End' && event.ctrlKey) {
            event.preventDefault();
            const items = searchStore.flatItems;
            const last = items[items.length - 1];
            if (last) {
                searchStore.select(last.key);
            }
            return;
        }

        if (event.key === 'Enter') {
            const activeHit = searchStore.selectedHit;
            if (!activeHit) {
                return;
            }
            event.preventDefault();
            const action: SearchNavigationAction =
                event.ctrlKey || event.metaKey ? 'open-in-new-tab' : 'default';
            void handleActivateHit(activeHit, action);
        }
    }

    watch(
        () => searchStore.open,
        async (isOpen, wasOpen) => {
            if (isOpen && !wasOpen) {
                rememberFocusBeforeOpen();
                await nextTick();
                inputRef.value?.focus();
                inputRef.value?.select();
            }
        },
        { immediate: true }
    );

    onMounted(() => {
        void attachEvents();
    });
</script>

<style lang="scss" scoped>
    .sr-only {
        position: absolute;
        width: 1px;
        height: 1px;
        padding: 0;
        margin: -1px;
        overflow: hidden;
        clip: rect(0, 0, 0, 0);
        white-space: nowrap;
        border: 0;
    }

    .global-search-backdrop {
        position: fixed;
        inset: 0;
        z-index: 1200;
        display: flex;
        align-items: flex-start;
        justify-content: center;
        padding-top: 6vh;
        background: var(--mt-ui-overlay, rgb(0 0 0 / 55%));
    }

    .global-search-palette {
        display: flex;
        flex-direction: column;
        width: min(780px, 94vw);
        max-height: 82vh;
        background: var(--mt-ui-surface, var(--background-0));
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 10px;
        color: var(--mt-ui-text-strong, var(--background-950));
        box-shadow: var(--mt-ui-shadow-lg, 0 18px 48px rgb(0 0 0 / 55%));
        overflow: hidden;
    }

    .global-search-palette__top {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 10px 14px;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .global-search-palette__search-icon {
        color: var(--max-primary-400);
        flex-shrink: 0;
    }

    .global-search-palette__input {
        flex: 1;
        min-width: 0;
        border: none;
        background: transparent;
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 14px;
        outline: none;

        &::placeholder {
            color: var(--mt-ui-text-muted, var(--background-400));
        }
    }

    .global-search-palette__clear,
    .global-search-palette__refresh,
    .global-search-palette__close {
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

    .global-search-palette__progress {
        padding: 6px 14px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        color: var(--max-info-400);
        font-size: 11px;
    }

    .global-search-palette__results {
        flex: 1;
        min-height: 160px;
        max-height: 56vh;
        overflow-y: auto;
        padding: 6px 8px;
    }

    .global-search-palette__state {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 10px;
        padding: 32px 16px;
        text-align: center;
        font-size: 13px;
        color: var(--mt-ui-text-muted, var(--background-400));

        &.is-error {
            color: var(--max-danger-400);
        }
    }

    .global-search-palette__error-msg {
        margin: 0;
    }

    .global-search-palette__retry {
        padding: 5px 12px;
        border: 1px solid var(--max-primary-500);
        border-radius: 6px;
        background: var(--max-primary-600);
        color: var(--max-primary-content, #fff);
        font-size: 12px;
        cursor: pointer;
    }

    .global-search-palette__footer {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 16px;
        padding: 8px 14px;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 11px;
    }
</style>
