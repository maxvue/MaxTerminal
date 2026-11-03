<template>
    <aside v-if="props.open" class="session-drawer" aria-label="Histórico de Sessões da Aba">
        <header class="session-drawer__header">
            <div class="session-drawer__title-group">
                <MaxIcon icon="lucide:history" class="session-drawer__icon" />
                <h3 class="session-drawer__title">Sessões da Aba</h3>
            </div>
            <div class="session-drawer__header-actions">
                <button type="button" class="session-drawer__new-btn" @click="emit('new-session')">
                    <MaxIcon icon="lucide:rotate-ccw" />
                    <span>Iniciar nova Sessão</span>
                </button>
                <button type="button" class="session-drawer__close-btn" aria-label="Fechar histórico de sessões" @click="emit('close')">
                    <MaxIcon icon="lucide:x" />
                </button>
            </div>
        </header>
        <div class="session-drawer__search">
            <input v-model="searchQuery" type="search" class="session-drawer__search-input" placeholder="Buscar no histórico desta aba (FTS5)..." aria-label="Buscar no histórico desta aba" @keydown.enter.prevent="triggerSearch">
            <button type="button" class="session-drawer__search-btn" @click="triggerSearch">Buscar</button>
        </div>
        <div v-if="searchQuery.trim() && props.searchHits.length > 0" class="session-drawer__hits">
            <span class="session-drawer__section-label">Resultados da busca ({{ props.searchHits.length }})</span>
            <ul class="session-drawer__hit-list">
                <li v-for="(hit, idx) in props.searchHits" :key="`${hit.sessionId}:${hit.seq}:${idx}`" class="session-drawer__hit-item">
                    <button type="button" class="session-drawer__hit-btn" @click="emit('inspect-session', hit.tabSessionId, hit.sessionId)">
                        <span class="session-drawer__hit-snippet">{{ hit.snippet }}</span>
                    </button>
                </li>
            </ul>
        </div>
        <div class="session-drawer__body">
            <div v-if="props.sessions.length === 0" class="session-drawer__empty">Nenhuma sessão encontrada para esta aba.</div>
            <ul v-else class="session-drawer__list">
                <li v-for="session in props.sessions" :key="session.id" class="session-drawer__item" :class="{ 'is-selected': selectedId === session.id }">
                    <button type="button" class="session-drawer__item-btn" @click="onSelectSession(session.id)">
                        <div class="session-drawer__item-top">
                            <span class="session-drawer__seq">Sessão #{{ session.seq }}</span>
                            <span class="session-drawer__status" :class="{ 'is-active': !session.archived }">{{ session.archived ? 'Arquivada' : 'Ativa' }}</span>
                        </div>
                        <div class="session-drawer__item-stats">
                            <span>{{ session.commandCount }} comandos</span>
                            <span v-if="session.failedCommandCount > 0" class="session-drawer__failed">{{ session.failedCommandCount }} erros</span>
                            <span v-if="session.truncated" class="session-drawer__truncated">Truncada</span>
                        </div>
                    </button>
                </li>
            </ul>
        </div>
        <div v-if="props.selectedDetail" class="session-drawer__viewer">
            <SessionReadonlyViewer :detail="props.selectedDetail" :chunks="props.selectedChunks" :has-more="props.hasMoreChunks" @close="emit('close-viewer')" @select-pane-session="emit('select-pane-session', $event)" @load-more="emit('load-more-chunks', $event)" />
        </div>
    </aside>
</template>

<script setup lang="ts">
    import { ref } from 'vue';
    import type { ScrollbackChunk, SessionDetail_Serialize, SessionId, SessionSearchHit, SessionSummary_Serialize, TabSessionId } from '@/types/bindings';
    import SessionReadonlyViewer from './SessionReadonlyViewer.vue';

    export interface SessionHistoryDrawerProps {
        open: boolean;
        sessions: SessionSummary_Serialize[];
        searchHits: SessionSearchHit[];
        selectedDetail: SessionDetail_Serialize | null;
        selectedChunks: ScrollbackChunk[];
        hasMoreChunks?: boolean;
    }

    const props = withDefaults(defineProps<SessionHistoryDrawerProps>(), {
        hasMoreChunks: false
    });

    const emit = defineEmits<{
        close: [];
        'new-session': [];
        search: [query: string];
        'select-session': [tabSessionId: TabSessionId];
        'inspect-session': [tabSessionId: TabSessionId, sessionId: SessionId];
        'select-pane-session': [sessionId: SessionId];
        'load-more-chunks': [sessionId: SessionId];
        'close-viewer': [];
    }>();

    const searchQuery = ref<string>('');
    const selectedId = ref<TabSessionId | null>(null);

    function triggerSearch(): void {
        emit('search', searchQuery.value);
    }

    function onSelectSession(id: TabSessionId): void {
        selectedId.value = id;
        emit('select-session', id);
    }
</script>

<style lang="scss" scoped>
    .session-drawer {
        width: 26rem;
        max-width: 100%;
        height: 100%;
        background: var(--mt-ui-surface-panel, var(--background-25));
        border-left: 1px solid var(--mt-ui-border, var(--background-100));
        display: flex;
        flex-direction: column;
        min-height: 0;
        flex-shrink: 0;
    }

    .session-drawer__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.625rem 0.875rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .session-drawer__title-group {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .session-drawer__icon {
        color: var(--max-primary-400);
        font-size: 1rem;
    }

    .session-drawer__title {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .session-drawer__header-actions {
        display: flex;
        align-items: center;
        gap: 0.375rem;
    }

    .session-drawer__new-btn {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        padding: 0.25rem 0.5rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--max-primary-500);
        background: rgb(20 184 166 / 15%);
        color: var(--max-primary-400);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }

    .session-drawer__close-btn {
        border: none;
        background: transparent;
        color: var(--mt-ui-text-muted, var(--background-400));
        cursor: pointer;
        padding: 0.25rem;
        border-radius: 0.25rem;
    }

    .session-drawer__search {
        display: flex;
        gap: 0.375rem;
        padding: 0.5rem 0.875rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .session-drawer__search-input {
        flex: 1;
        padding: 0.375rem 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.7813rem;
    }

    .session-drawer__search-btn {
        padding: 0.375rem 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }

    .session-drawer__hits {
        padding: 0.5rem 0.875rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
        max-height: 9rem;
        overflow-y: auto;
    }

    .session-drawer__section-label {
        display: block;
        font-size: 0.6875rem;
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
        margin-bottom: 0.25rem;
    }

    .session-drawer__hit-list {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .session-drawer__hit-btn {
        width: 100%;
        text-align: left;
        padding: 0.3125rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-family: var(--mt-font-mono);
        font-size: 0.7188rem;
        cursor: pointer;

        &:hover {
            border-color: var(--max-primary-500);
        }
    }

    .session-drawer__hit-snippet {
        display: block;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .session-drawer__body {
        flex: 1;
        min-height: 0;
        overflow-y: auto;
        padding: 0.5rem 0.875rem;
    }

    .session-drawer__empty {
        text-align: center;
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.8125rem;
        padding: 1.5rem 0;
    }

    .session-drawer__list {
        list-style: none;
        margin: 0;
        padding: 0;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .session-drawer__item {
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));

        &.is-selected {
            border-color: var(--max-primary-500);
        }
    }

    .session-drawer__item-btn {
        width: 100%;
        padding: 0.5rem 0.625rem;
        border: none;
        background: transparent;
        color: inherit;
        text-align: left;
        cursor: pointer;
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .session-drawer__item-top {
        display: flex;
        align-items: center;
        justify-content: space-between;
    }

    .session-drawer__seq {
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .session-drawer__status {
        font-size: 0.6875rem;
        font-weight: 600;
        color: var(--mt-ui-text-muted, var(--background-400));

        &.is-active {
            color: var(--max-success-400);
        }
    }

    .session-drawer__item-stats {
        display: flex;
        gap: 0.5rem;
        font-size: 0.7188rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .session-drawer__failed {
        color: var(--max-danger-400);
        font-weight: 600;
    }

    .session-drawer__truncated {
        color: var(--max-warning-400);
    }

    .session-drawer__viewer {
        height: 50%;
        min-height: 12rem;
        display: flex;
        flex-direction: column;
    }
</style>
