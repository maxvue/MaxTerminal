<template>
    <aside class="group-suggest-panel" aria-label="Assistente IA e Comandos Recorrentes" data-testid="command-group-suggest-panel">
        <div class="group-suggest-panel__section">
            <h3 class="group-suggest-panel__title">Sugestão Assistida com IA</h3>
            <p class="group-suggest-panel__desc">Descreva o objetivo do grupo em português para gerar uma proposta de itens e modo de execução.</p>

            <MaxInputTextArea v-model="goalText" :rows="3" class="group-suggest-panel__textarea" data-testid="suggest-goal-input" placeholder="Ex.: Subir servidor Vite, fila Horizon e acompanhar logs em desenvolvimento" :disabled="loading" />

            <div class="group-suggest-panel__actions">
                <MaxButton :label="loading ? 'Gerando sugestão…' : 'Sugerir com IA'" icon="lucide:sparkles" class="group-suggest-panel__btn group-suggest-panel__btn--primary" data-testid="btn-suggest-ai" :disabled="loading || goalText.trim().length === 0" :loading="loading" @click="onRequestSuggestion" />
                <MaxButton v-if="loading" label="Cancelar" icon="lucide:x" variant="text" class="group-suggest-panel__btn group-suggest-panel__btn--ghost" data-testid="btn-cancel-suggest-ai" @click="onCancelSuggestion" />
                <MaxButton v-if="hasUndoSnapshot" label="Desfazer aplicação" icon="lucide:undo-2" variant="text" class="group-suggest-panel__btn group-suggest-panel__btn--ghost" data-testid="btn-undo-suggestion" @click="emit('undo-suggestion')" />
            </div>

            <p v-if="errorMessage" class="group-suggest-panel__error" role="alert" data-testid="suggest-error">{{ errorMessage }}</p>
            <p v-if="staleWarning" class="group-suggest-panel__warn" role="status" data-testid="suggest-stale-warning">{{ staleWarning }}</p>

            <div v-if="previewSuggestion" class="group-suggest-panel__preview" data-testid="suggest-preview-box">
                <div class="group-suggest-panel__preview-header">
                    <strong>{{ previewSuggestion.name }}</strong>
                    <span class="group-suggest-panel__preview-mode">{{ previewSuggestion.mode }}</span>
                </div>
                <p v-if="previewSuggestion.description" class="group-suggest-panel__preview-desc">{{ previewSuggestion.description }}</p>
                <ul class="group-suggest-panel__preview-list">
                    <li v-for="(item, idx) in previewSuggestion.items" :key="idx" class="group-suggest-panel__preview-item">
                        <span class="group-suggest-panel__preview-item-title">{{ item.title }}:</span>
                        <code>{{ item.command }}</code>
                    </li>
                </ul>
                <MaxButton label="Aplicar ao rascunho" icon="lucide:check" class="group-suggest-panel__btn group-suggest-panel__btn--apply" data-testid="btn-apply-suggestion" @click="onApplyPreview" />
            </div>
        </div>

        <div v-if="recurringAvailable && recurringItems.length > 0" class="group-suggest-panel__section" data-testid="recurring-commands-section">
            <h3 class="group-suggest-panel__title">Comandos recorrentes do projeto</h3>
            <p class="group-suggest-panel__desc">Clique em um comando recorrente para adicioná-lo como item editável ao rascunho.</p>
            <div class="group-suggest-panel__chips">
                <MaxButton v-for="(entry, idx) in recurringItems" :key="idx" type="button" class="group-suggest-panel__chip" :data-testid="`recurring-chip-${idx}`" @click="emit('append-recurring-item', entry)">
                    <code>{{ entry.command }}</code>
                    <span class="group-suggest-panel__chip-meta">{{ entry.frequency }}x</span>
                </MaxButton>
            </div>
        </div>
    </aside>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import { MaxButton, MaxInputTextArea } from '@maxvue/max-components-ui';
    import { createGroupRequestId } from '@/composables/commandGroups/useCommandGroupRun';
    import { aiIpc } from '@/ipc/ai';
    import { isMtIpcError } from '@/ipc/core';
    import { useCommandGroupsStore } from '@/stores/commandGroups';
    import type {
        CommandGroupInput,
        GroupScope,
        RecurringCommandEntry
    } from '@/types/commandGroups';

    export interface CommandGroupSuggestPanelProps {
        scope: GroupScope;
        projectId: string | null;
        paneId: string | null;
        draftRevision: number;
        currentDraft: CommandGroupInput;
        hasUndoSnapshot: boolean;
    }

    export interface CommandGroupSuggestPanelEmits {
        (e: 'apply-suggestion', suggestion: CommandGroupInput): void;
        (e: 'undo-suggestion'): void;
        (e: 'append-recurring-item', entry: RecurringCommandEntry): void;
    }

    const props = defineProps<CommandGroupSuggestPanelProps>();
    const emit = defineEmits<CommandGroupSuggestPanelEmits>();

    const store = useCommandGroupsStore();

    const goalText = ref<string>('');
    const loading = ref<boolean>(false);
    const activeRequestId = ref<string | null>(null);
    const errorMessage = ref<string | null>(null);
    const staleWarning = ref<string | null>(null);
    const previewSuggestion = ref<CommandGroupInput | null>(null);
    const previewOriginRevision = ref<number | null>(null);

    const recurringData = computed(() => {
        if (!props.projectId) {
            return null;
        }
        return store.recurringByProject[props.projectId] ?? null;
    });

    const recurringAvailable = computed<boolean>(() => Boolean(recurringData.value?.available));
    const recurringItems = computed<RecurringCommandEntry[]>(() => recurringData.value?.items ?? []);

    watch(
        () => props.projectId,
        (projectId) => {
            if (projectId) {
                void store.loadRecurring(projectId).catch(() => {});
            }
        },
        { immediate: true }
    );

    async function onRequestSuggestion(): Promise<void> {
        const trimmedGoal = goalText.value.trim();
        if (!trimmedGoal || loading.value) {
            return;
        }

        const reqId = createGroupRequestId();
        const revisionAtRequest = props.draftRevision;
        activeRequestId.value = reqId;
        loading.value = true;
        errorMessage.value = null;
        staleWarning.value = null;

        try {
            const response = await store.suggest({
                requestId: reqId,
                projectId: props.projectId,
                paneId: props.paneId,
                scope: props.scope,
                goal: trimmedGoal,
                draft: props.currentDraft,
                draftRevision: revisionAtRequest
            });

            if (activeRequestId.value !== reqId) {
                return;
            }

            previewSuggestion.value = response.suggestion;
            previewOriginRevision.value = response.draftRevision;

            if (props.draftRevision !== revisionAtRequest) {
                staleWarning.value =
                    'O rascunho foi editado após o envio do pedido à IA. Revise a prévia antes de aplicar.';
            }
        } catch (err: unknown) {
            if (activeRequestId.value !== reqId) {
                return;
            }
            if (isMtIpcError(err)) {
                errorMessage.value = err.message;
            } else if (err instanceof Error) {
                errorMessage.value = err.message;
            } else {
                errorMessage.value = 'Não foi possível gerar sugestão com IA.';
            }
        } finally {
            if (activeRequestId.value === reqId) {
                loading.value = false;
                activeRequestId.value = null;
            }
        }
    }

    async function onCancelSuggestion(): Promise<void> {
        const reqId = activeRequestId.value;
        activeRequestId.value = null;
        loading.value = false;
        if (reqId) {
            await aiIpc.cancel(reqId).catch(() => {});
        }
    }

    function onApplyPreview(): void {
        if (!previewSuggestion.value) {
            return;
        }
        emit('apply-suggestion', previewSuggestion.value);
        staleWarning.value = null;
    }
</script>

<style lang="scss" scoped>
    .group-suggest-panel {
        display: flex;
        flex-direction: column;
        gap: 1rem;
        padding: 0.875rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 0.375rem;
        background: var(--mt-ui-surface, var(--background-0));
    }

    .group-suggest-panel__section {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .group-suggest-panel__title {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .group-suggest-panel__desc {
        margin: 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .group-suggest-panel__textarea {
        width: 100%;
        font-size: 0.8125rem;
    }

    .group-suggest-panel__actions {
        display: flex;
        flex-wrap: wrap;
        gap: 0.375rem;
    }

    .group-suggest-panel__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.375rem 0.625rem;
        border-radius: 0.25rem;
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--primary {
            border: 1px solid var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 18%, transparent);
            color: var(--max-primary-400);
        }

        &--apply {
            border: 1px solid var(--max-success-500);
            background: var(--mt-ui-success-subtle, color-mix(in srgb, var(--max-success-500) 18%, transparent));
            color: var(--max-success-300);
        }

        &--ghost {
            border: 1px solid var(--mt-ui-border, var(--background-100));
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text-secondary, var(--background-600));
        }

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }
    }

    .group-suggest-panel__error {
        margin: 0;
        font-size: 0.75rem;
        color: var(--max-danger-400);
    }

    .group-suggest-panel__warn {
        margin: 0;
        font-size: 0.75rem;
        color: var(--max-warning-400);
    }

    .group-suggest-panel__preview {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
        padding: 0.625rem;
        border: 1px solid var(--max-primary-600);
        border-radius: 0.375rem;
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .group-suggest-panel__preview-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        font-size: 0.8125rem;
    }

    .group-suggest-panel__preview-mode {
        padding: 0.0625rem 0.375rem;
        border-radius: 999px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--max-primary-400);
        font-size: 0.6875rem;
    }

    .group-suggest-panel__preview-desc {
        margin: 0;
        font-size: 0.75rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .group-suggest-panel__preview-list {
        margin: 0;
        padding-left: 1rem;
        font-size: 0.75rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .group-suggest-panel__preview-item {
        margin-bottom: 0.25rem;
    }

    .group-suggest-panel__preview-item-title {
        margin-right: 0.375rem;
        font-weight: 600;
    }

    .group-suggest-panel__chips {
        display: flex;
        flex-wrap: wrap;
        gap: 0.375rem;
    }

    .group-suggest-panel__chip {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        padding: 0.25rem 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 999px;
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        cursor: pointer;
    }

    .group-suggest-panel__chip-meta {
        color: var(--mt-ui-text-muted, var(--background-400));
        font-size: 0.6875rem;
    }
</style>
