<template>
    <MaxModal :visible="props.open" title="Corrigir todos os Bugs — Prévia do Ciclo MaxCode" :no-button="true" max-width="36rem" class="bug-cycle-confirm-modal" :dismissable="!props.submitting" :close-on-escape="!props.submitting" @update:visible="emit('close')">
        <div class="bug-cycle-confirm-modal__body">
            <div v-if="!props.preview" class="bug-cycle-confirm-modal__loading">Calculando fila elegível de bugs...</div>
            <template v-else>
                <div class="bug-cycle-confirm-modal__section">
                    <h4 class="bug-cycle-confirm-modal__subtitle">Issues Elegíveis em Fila ({{ props.preview.candidates.length }})</h4>
                    <p v-if="props.preview.candidates.length === 0" class="bug-cycle-confirm-modal__empty">Nenhum bug elegível encontrado para o ciclo automatizado.</p>
                    <ul v-else class="bug-cycle-confirm-modal__list">
                        <li v-for="item in props.preview.candidates" :key="item.issueNumber" class="bug-cycle-confirm-modal__item">
                            <span class="bug-cycle-confirm-modal__num">#{{ item.issueNumber }}</span>
                            <span class="bug-cycle-confirm-modal__name">{{ item.title }}</span>
                            <span class="bug-cycle-confirm-modal__stage">{{ item.plannedStage }}</span>
                        </li>
                    </ul>
                </div>
                <div v-if="props.preview.skipped.length > 0" class="bug-cycle-confirm-modal__section">
                    <h4 class="bug-cycle-confirm-modal__subtitle">Issues Ignoradas ({{ props.preview.skipped.length }})</h4>
                    <ul class="bug-cycle-confirm-modal__list bug-cycle-confirm-modal__list--skipped">
                        <li v-for="skip in props.preview.skipped" :key="skip.issueNumber" class="bug-cycle-confirm-modal__item">
                            <span class="bug-cycle-confirm-modal__num">#{{ skip.issueNumber }}</span>
                            <span class="bug-cycle-confirm-modal__name">{{ skip.title }}</span>
                            <span class="bug-cycle-confirm-modal__reason">{{ skip.reason }}</span>
                        </li>
                    </ul>
                </div>
            </template>
        </div>
        <footer class="bug-cycle-confirm-modal__footer">
            <MaxButton label="Cancelar" variant="secondary" class="bug-cycle-confirm-modal__btn" @click="emit('close')" />
            <MaxButton :label="props.submitting ? 'Iniciando...' : `Iniciar Ciclo (${props.preview?.candidates.length ?? 0} Bugs)`" icon="lucide:play" variant="primary" class="bug-cycle-confirm-modal__btn bug-cycle-confirm-modal__btn--primary" :disabled="!props.preview || props.preview.candidates.length === 0 || props.submitting" :loading="props.submitting" @click="onStartCycle" />
        </footer>
    </MaxModal>
</template>

<script setup lang="ts">
    import { MaxButton, MaxModal } from '@maxvue/max-components-ui';
    import type { BugCyclePreview, ProjectId, StartBugCycleRequest } from '@/types/bindings';

    export interface BugCycleConfirmModalProps {
        open: boolean;
        projectId: ProjectId;
        preview: BugCyclePreview | null;
        submitting?: boolean;
    }

    const props = withDefaults(defineProps<BugCycleConfirmModalProps>(), {
        submitting: false
    });

    const emit = defineEmits<{
        close: [];
        start: [request: StartBugCycleRequest];
    }>();

    function onStartCycle(): void {
        if (!props.preview || props.preview.candidates.length === 0) {
            return;
        }
        emit('start', {
            projectId: props.projectId,
            issueNumbers: props.preview.candidates.map((c) => c.issueNumber),
            includeReplanAndRejected: true
        });
    }
</script>

<style lang="scss" scoped>
    .bug-cycle-confirm-modal {
        position: fixed;
        inset: 0;
        z-index: 60;
        display: flex;
        align-items: center;
        justify-content: center;
        background: var(--mt-ui-overlay, rgb(0 0 0 / 60%));
    }

    .bug-cycle-confirm-modal__card {
        width: 100%;
        max-width: 38rem;
        max-height: 80vh;
        display: flex;
        flex-direction: column;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
        overflow: hidden;
    }

    .bug-cycle-confirm-modal__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.75rem 1rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .bug-cycle-confirm-modal__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
    }

    .bug-cycle-confirm-modal__close {
        border: none;
        background: transparent;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;
    }

    .bug-cycle-confirm-modal__body {
        flex: 1;
        overflow-y: auto;
        display: flex;
        flex-direction: column;
        gap: 1rem;
        padding: 1rem;
        font-size: 0.8125rem;
    }

    .bug-cycle-confirm-modal__section {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .bug-cycle-confirm-modal__subtitle {
        margin: 0;
        font-size: 0.8125rem;
        font-weight: 700;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .bug-cycle-confirm-modal__empty {
        margin: 0;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .bug-cycle-confirm-modal__list {
        margin: 0;
        padding: 0;
        list-style: none;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .bug-cycle-confirm-modal__item {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.375rem 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .bug-cycle-confirm-modal__num {
        font-family: var(--mt-font-mono, monospace);
        font-weight: 700;
        color: var(--mt-ui-issue-category-hover, var(--max-purple-400, #c084fc));
    }

    .bug-cycle-confirm-modal__name {
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
    }

    .bug-cycle-confirm-modal__stage,
    .bug-cycle-confirm-modal__reason {
        font-size: 0.6875rem;
        color: var(--mt-ui-text-muted, var(--background-400));
    }

    .bug-cycle-confirm-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding: 0.75rem 1rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .bug-cycle-confirm-modal__btn {
        display: inline-flex;
        align-items: center;
        gap: 0.375rem;
        height: 2rem;
        padding: 0 0.875rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.5;
            cursor: not-allowed;
        }

        &--primary {
            border-color: var(--max-primary-500);
            background: color-mix(in srgb, var(--max-primary-500) 20%, transparent);
            color: var(--max-primary-400);
        }
    }
</style>
