<template>
    <section class="session-viewer" aria-label="Visualizador somente leitura de sessão">
        <header class="session-viewer__header">
            <div class="session-viewer__meta">
                <span class="session-viewer__title">Sessão #{{ props.detail.summary.seq }}</span>
                <span class="session-viewer__badge" :class="{ 'is-archived': props.detail.summary.archived }">{{ props.detail.summary.archived ? 'Arquivada' : 'Ativa' }}</span>
                <span v-if="props.detail.summary.endReason" class="session-viewer__reason">{{ reasonLabel }}</span>
            </div>
            <button type="button" class="session-viewer__close" aria-label="Fechar visualizador de sessão" @click="emit('close')">
                <MaxIcon icon="lucide:x" />
            </button>
        </header>
        <div v-if="props.detail.panes.length > 1" class="session-viewer__panes" role="tablist" aria-label="Painéis da sessão">
            <button v-for="(pane, idx) in props.detail.panes" :key="pane.sessionId" type="button" class="session-viewer__pane-tab" :class="{ 'is-active': selectedSessionId === pane.sessionId }" role="tab" :aria-selected="selectedSessionId === pane.sessionId" @click="selectPaneSession(pane.sessionId)">Painel {{ idx + 1 }} (geração {{ pane.generation }})</button>
        </div>
        <div class="session-viewer__scrollback" role="log" aria-live="off">
            <div v-if="props.chunks.length === 0" class="session-viewer__empty">Nenhum scrollback gravado para este painel nesta sessão.</div>
            <template v-else>
                <div v-for="chunk in props.chunks" :key="chunk.seq" class="session-viewer__chunk" :class="{ 'is-gap': chunk.kind === 'gap' }">{{ chunk.text }}</div>
                <button v-if="props.hasMore" type="button" class="session-viewer__more" @click="emit('load-more', selectedSessionId)">Carregar mais saída</button>
            </template>
        </div>
    </section>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import type { ScrollbackChunk, SessionDetail_Serialize, SessionEndReason, SessionId } from '@/types/bindings';

    export interface SessionReadonlyViewerProps {
        detail: SessionDetail_Serialize;
        chunks: ScrollbackChunk[];
        hasMore?: boolean;
    }

    const props = withDefaults(defineProps<SessionReadonlyViewerProps>(), {
        hasMore: false
    });

    const emit = defineEmits<{
        close: [];
        'select-pane-session': [sessionId: SessionId];
        'load-more': [sessionId: SessionId];
    }>();

    const selectedSessionId = ref<SessionId>(props.detail.panes[0]?.sessionId ?? '');

    watch(
        () => props.detail,
        (next) => {
            const first = next.panes[0]?.sessionId ?? '';
            if (!next.panes.some((p) => p.sessionId === selectedSessionId.value)) {
                selectedSessionId.value = first;
                if (first) {
                    emit('select-pane-session', first);
                }
            }
        },
        { immediate: true }
    );

    const REASON_LABELS: Record<SessionEndReason, string> = {
        new_session: 'Nova sessão iniciada',
        tab_closed: 'Aba encerrada',
        app_exit: 'Encerramento da aplicação',
        crash_recovered: 'Recuperada após encerramento abrupto',
        project_removed: 'Projeto removido',
        spawn_failed: 'Falha ao iniciar processo'
    };

    const reasonLabel = computed<string>(() => {
        const r = props.detail.summary.endReason;
        if (!r) {
            return '';
        }
        return REASON_LABELS[r] ?? r;
    });

    function selectPaneSession(sessionId: SessionId): void {
        selectedSessionId.value = sessionId;
        emit('select-pane-session', sessionId);
    }
</script>

<style lang="scss" scoped>
    .session-viewer {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        min-height: 0;
        background: var(--background-950);
        border-top: 1px solid var(--background-700);
    }

    .session-viewer__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.5rem 0.75rem;
        background: var(--background-850, var(--background-900));
        border-bottom: 1px solid var(--background-700);
    }

    .session-viewer__meta {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        flex-wrap: wrap;
    }

    .session-viewer__title {
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--background-50);
    }

    .session-viewer__badge {
        font-size: 0.6875rem;
        font-weight: 600;
        padding: 0.0625rem 0.4375rem;
        border-radius: 999px;
        background: rgb(34 197 94 / 18%);
        color: var(--max-success-400);

        &.is-archived {
            background: var(--background-700);
            color: var(--background-200);
        }
    }

    .session-viewer__reason {
        font-size: 0.75rem;
        color: var(--background-400);
    }

    .session-viewer__close {
        border: none;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;
        padding: 0.25rem;
        border-radius: 0.25rem;

        &:hover {
            background: var(--background-700);
            color: var(--background-50);
        }
    }

    .session-viewer__panes {
        display: flex;
        gap: 0.25rem;
        padding: 0.375rem 0.75rem;
        background: var(--background-900);
        border-bottom: 1px solid var(--background-800);
        overflow-x: auto;
    }

    .session-viewer__pane-tab {
        padding: 0.25rem 0.5rem;
        border-radius: 0.25rem;
        border: 1px solid var(--background-700);
        background: var(--background-800);
        color: var(--background-300);
        font-size: 0.75rem;
        cursor: pointer;

        &.is-active {
            border-color: var(--max-primary-500);
            color: var(--max-primary-400);
            font-weight: 600;
        }
    }

    .session-viewer__scrollback {
        flex: 1;
        min-height: 0;
        padding: 0.75rem;
        overflow-y: auto;
        font-family: var(--mt-font-mono);
        font-size: 0.7813rem;
        line-height: 1.45;
        color: var(--background-100);
        white-space: pre-wrap;
        word-break: break-word;
    }

    .session-viewer__chunk {
        &.is-gap {
            margin: 0.375rem 0;
            padding: 0.25rem 0.5rem;
            border-radius: 0.25rem;
            background: rgb(245 158 11 / 15%);
            color: var(--max-warning-300);
            font-size: 0.7188rem;
        }
    }

    .session-viewer__empty {
        color: var(--background-400);
        font-family: var(--mt-font-sans, sans-serif);
        text-align: center;
        padding: 1.5rem;
    }

    .session-viewer__more {
        margin-top: 0.5rem;
        padding: 0.375rem 0.75rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--background-600);
        background: var(--background-800);
        color: var(--background-200);
        font-size: 0.75rem;
        cursor: pointer;
    }
</style>
