<template>
    <div class="terminal-search-bar" role="search" aria-label="Buscar no terminal" @keydown="onBarKeydown">
        <input ref="inputRef" v-model="query" type="text" class="search-input" placeholder="Buscar no terminal..." aria-label="Termo de busca">
        <div class="search-toggles">
            <button type="button" class="toggle-btn" :class="{ 'is-active': options.caseSensitive }" :aria-pressed="options.caseSensitive" title="Diferenciar maiúsculas/minúsculas" @click="toggleCaseSensitive">
                Aa
            </button>
            <button type="button" class="toggle-btn" :class="{ 'is-active': options.wholeWord }" :aria-pressed="options.wholeWord" title="Palavra inteira" @click="toggleWholeWord">
                Palavra
            </button>
            <button type="button" class="toggle-btn" :class="{ 'is-active': options.regex }" :aria-pressed="options.regex" title="Expressão regular" @click="toggleRegex">
                .*
            </button>
        </div>
        <span class="search-counter" :class="{ 'is-error': Boolean(regexError) }" role="status">
            {{ statusLabel }}
        </span>
        <div class="search-nav">
            <MaxIconButton icon="lucide:arrow-up" label="Resultado anterior" variant="ghost" class="nav-btn" title="Resultado anterior (Shift+Enter)" @click="prev" />
            <MaxIconButton icon="lucide:arrow-down" label="Próximo resultado" variant="ghost" class="nav-btn" title="Próximo resultado (Enter)" @click="next" />
            <MaxIconButton icon="lucide:x" label="Fechar busca" variant="ghost" class="nav-btn close-btn" title="Fechar busca (Escape)" @click="handleClose" />
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, ref, toRef } from 'vue';
    import { MaxIconButton } from '@maxvue/max-components-ui';
    import type { PaneId } from '@/types/bindings';
    import { useTerminalSearch } from '@/composables/terminal/useTerminalSearch';
    import { useSearchStore } from '@/stores/search';
    import { isSearchShortcut } from '@/terminal/keybindings';

    export interface TerminalSearchBarProps {
        paneId: PaneId;
    }

    const props = defineProps<TerminalSearchBarProps>();

    const emit = defineEmits<{
        close: [];
        escalate: [query: string];
    }>();

    const searchStore = useSearchStore();
    const inputRef = ref<HTMLInputElement | null>(null);
    const paneIdRef = toRef(props, 'paneId');
    const { query, options, resultIndex, resultCount, limitExceeded, regexError, next, prev, close } =
        useTerminalSearch(paneIdRef);

    const statusLabel = computed<string>(() => {
        if (regexError.value) {
            return regexError.value;
        }
        if (query.value.length === 0) {
            return 'Sem resultados';
        }
        if (limitExceeded.value) {
            return 'Limite de destaques atingido';
        }
        if (resultCount.value === 0) {
            return 'Sem resultados';
        }
        return `${resultIndex.value} de ${resultCount.value}`;
    });

    function toggleCaseSensitive(): void {
        options.value = {
            ...options.value,
            caseSensitive: !options.value.caseSensitive
        };
    }

    function toggleWholeWord(): void {
        options.value = {
            ...options.value,
            wholeWord: !options.value.wholeWord
        };
    }

    function toggleRegex(): void {
        options.value = {
            ...options.value,
            regex: !options.value.regex
        };
    }

    function handleClose(): void {
        close();
        emit('close');
    }

    function onBarKeydown(event: KeyboardEvent): void {
        if (isSearchShortcut(event)) {
            event.preventDefault();
            event.stopPropagation();
            const currentQuery = query.value;
            handleClose();
            emit('escalate', currentQuery);
            searchStore.show(currentQuery);
            return;
        }
        if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            handleClose();
            return;
        }
        if (event.key === 'Enter') {
            event.preventDefault();
            if (event.shiftKey) {
                prev();
            } else {
                next();
            }
        }
    }

    onMounted(() => {
        inputRef.value?.focus();
        inputRef.value?.select();
    });
</script>

<style lang="scss" scoped>
    .terminal-search-bar {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 6px 10px;
        background: var(--background-900);
        border: 1px solid var(--background-700);
        border-radius: 6px;
        color: var(--background-100);
        font-size: 12px;
        box-shadow: 0 4px 12px rgb(0 0 0 / 35%);
    }

    .search-input {
        min-width: 180px;
        padding: 4px 8px;
        background: var(--background-950);
        border: 1px solid var(--background-700);
        border-radius: 4px;
        color: var(--background-50);
        font-size: 12px;
        outline: none;

        &:focus {
            border-color: var(--max-primary-500);
        }
    }

    .search-toggles,
    .search-nav {
        display: flex;
        align-items: center;
        gap: 4px;
    }

    .toggle-btn,
    .nav-btn {
        padding: 3px 6px;
        background: var(--background-800);
        border: 1px solid var(--background-700);
        border-radius: 4px;
        color: var(--background-200);
        font-size: 11px;
        cursor: pointer;

        &:hover {
            background: var(--background-700);
            color: var(--background-50);
        }

        &.is-active {
            background: var(--max-primary-600);
            border-color: var(--max-primary-500);
            color: var(--background-50);
        }
    }

    .search-counter {
        min-width: 90px;
        text-align: center;
        color: var(--background-300);

        &.is-error {
            color: var(--max-danger-400);
        }
    }
</style>
