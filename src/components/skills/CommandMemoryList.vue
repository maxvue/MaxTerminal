<template>
    <div class="command-memory-list" data-testid="command-memory-list">
        <header class="command-memory-list__header">
            <div class="command-memory-list__search">
                <MaxIcon icon="lucide:history" :size="0.875" />
                <input v-model="filterPrefix" type="text" class="command-memory-list__input" placeholder="Filtrar comandos memorizados..." aria-label="Filtrar comandos memorizados" data-testid="memory-filter-input">
            </div>
            <div class="command-memory-list__controls">
                <label class="command-memory-list__checkbox">
                    <input v-model="includeHidden" type="checkbox" data-testid="memory-include-hidden" @change="onReload">
                    <span>Exibir ocultos</span>
                </label>
                <MaxButton type="button" size="sm" severity="secondary" icon="lucide:refresh-cw" data-testid="memory-rebuild-btn" @click="onRebuild">
                    <span>Reconstruir</span>
                </MaxButton>
            </div>
        </header>

        <ul v-if="filteredEntries.length > 0" class="command-memory-list__items" role="listbox" aria-label="Comandos recorrentes do projeto">
            <li v-for="entry in filteredEntries" :key="entry.command" class="command-memory-list__item" :data-testid="`memory-item-${entry.command}`">
                <button type="button" class="command-memory-list__select-btn" :title="`Inserir no draft: ${entry.command}`" @click="onSelectCommand(entry)">
                    <code class="command-memory-list__cmd">{{ entry.command }}</code>
                    <span class="command-memory-list__meta">
                        <span>{{ entry.useCount }}x</span>
                        <span>{{ formatSuccessRate(entry) }}% sucesso</span>
                        <span v-if="entry.lastCwd" class="command-memory-list__cwd">{{ entry.lastCwd }}</span>
                    </span>
                </button>
                <div class="command-memory-list__actions">
                    <MaxIconButton icon="lucide:pin" :size="0.75" class="command-memory-list__action-btn" :class="entry.pinned ? 'command-memory-list__action-btn--active' : ''" :aria-label="entry.pinned ? 'Desafixar comando' : 'Fixar comando'" :data-testid="`memory-pin-${entry.command}`" @click="onTogglePin(entry)" />
                    <MaxIconButton :icon="entry.hidden ? 'lucide:eye' : 'lucide:eye-off'" :size="0.75" class="command-memory-list__action-btn" :class="entry.hidden ? 'command-memory-list__action-btn--active' : ''" :aria-label="entry.hidden ? 'Reexibir comando' : 'Ocultar comando'" :data-testid="`memory-hide-${entry.command}`" @click="onToggleHide(entry)" />
                    <MaxIconButton icon="lucide:trash-2" :size="0.75" class="command-memory-list__action-btn command-memory-list__action-btn--danger" aria-label="Esquecer comando permanentemente" :data-testid="`memory-forget-${entry.command}`" @click="onForget(entry)" />
                </div>
            </li>
        </ul>
        <p v-else class="command-memory-list__empty" data-testid="memory-empty">Nenhum comando recorrente encontrado para este filtro.</p>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, ref, watch } from 'vue';
    import { MaxButton, MaxIcon, MaxIconButton } from '@maxvue/max-components-ui';
    import { useCommandMemorySuggestions } from '@/composables/useCommandMemorySuggestions';
    import { useSkillsStore } from '@/stores/skills';
    import type { MemoryEntry_Serialize, PaneId, ProjectId } from '@/types/bindings';

    export interface CommandMemoryListProps {
        projectId: ProjectId;
        paneId: PaneId;
    }

    const props = defineProps<CommandMemoryListProps>();

    const skillsStore = useSkillsStore();
    const memorySuggestions = useCommandMemorySuggestions(() => props.paneId);

    const filterPrefix = ref<string>('');
    const includeHidden = ref<boolean>(false);

    const entries = computed<MemoryEntry_Serialize[]>(() => {
        return skillsStore.memoryByProject[props.projectId] ?? [];
    });

    const filteredEntries = computed<MemoryEntry_Serialize[]>(() => {
        const prefix = filterPrefix.value.trim();
        return entries.value.filter((item) => {
            if (!includeHidden.value && item.hidden) {
                return false;
            }
            if (prefix && !item.command.startsWith(prefix)) {
                return false;
            }
            return true;
        });
    });

    onMounted(() => {
        void onReload();
    });

    watch(
        () => props.projectId,
        () => {
            void onReload();
        }
    );

    async function onReload(): Promise<void> {
        try {
            await skillsStore.loadMemory(props.projectId, includeHidden.value, 50);
        } catch {
            // Erro já refletido na store.
        }
    }

    function formatSuccessRate(entry: MemoryEntry_Serialize): number {
        if (entry.useCount <= 0) {
            return 0;
        }
        return Math.round((entry.successCount / entry.useCount) * 100);
    }

    function onSelectCommand(entry: MemoryEntry_Serialize): void {
        // CA-32: Insere exclusivamente no draft do input rico, sem executar no PTY.
        memorySuggestions.applySuggestion(entry);
    }

    async function onTogglePin(entry: MemoryEntry_Serialize): Promise<void> {
        try {
            await skillsStore.pin(props.projectId, entry.command, !entry.pinned);
        } catch {
            // Erro registrado na store.
        }
    }

    async function onToggleHide(entry: MemoryEntry_Serialize): Promise<void> {
        try {
            await skillsStore.hide(props.projectId, entry.command, !entry.hidden);
        } catch {
            // Erro registrado na store.
        }
    }

    async function onForget(entry: MemoryEntry_Serialize): Promise<void> {
        try {
            await skillsStore.forget(props.projectId, entry.command);
        } catch {
            // Erro registrado na store.
        }
    }

    async function onRebuild(): Promise<void> {
        try {
            await skillsStore.rebuild(props.projectId);
        } catch {
            // Erro registrado na store.
        }
    }
</script>

<style lang="scss" scoped>
    .command-memory-list {
        display: flex;
        flex-direction: column;
        gap: 0.625rem;
        padding: 0.75rem;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .command-memory-list__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .command-memory-list__search {
        flex: 1;
        min-width: 12rem;
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
    }

    .command-memory-list__input {
        flex: 1;
        border: none;
        outline: none;
        background: transparent;
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.7813rem;
    }

    .command-memory-list__controls {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
    }

    .command-memory-list__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        font-size: 0.75rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;
    }

    .command-memory-list__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.3125rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }

    .command-memory-list__items {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .command-memory-list__item {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.5rem;
        padding: 0.375rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
    }

    .command-memory-list__select-btn {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        align-items: flex-start;
        gap: 0.125rem;
        border: none;
        background: transparent;
        color: var(--mt-ui-text, var(--background-800));
        text-align: left;
        cursor: pointer;
        padding: 0;
    }

    .command-memory-list__cmd {
        font-size: 0.7813rem;
        color: var(--max-primary-300);
        word-break: break-all;
    }

    .command-memory-list__meta {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .command-memory-list__cwd {
        font-family: monospace;
    }

    .command-memory-list__actions {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
    }

    .command-memory-list__action-btn {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        padding: 0.25rem;
        border-radius: 0.25rem;
        border: 1px solid transparent;
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        cursor: pointer;

        &:hover {
            color: var(--mt-ui-text-strong, var(--background-950));
            background: var(--mt-ui-surface-hover, var(--background-75));
        }
    }

    .command-memory-list__action-btn--active {
        color: var(--max-primary-400);
    }

    .command-memory-list__action-btn--danger:hover {
        color: var(--max-danger-400);
    }

    .command-memory-list__empty {
        margin: 0;
        padding: 0.5rem;
        font-size: 0.75rem;
        color: var(--background-400);
        text-align: center;
    }
</style>
