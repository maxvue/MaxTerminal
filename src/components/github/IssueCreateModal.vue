<template>
    <MaxModal :visible="props.open" title="Criar Nova Issue" :no-button="true" max-width="36rem" class="issue-create-modal" :dismissable="!props.submitting" :close-on-escape="!props.submitting" @update:visible="emit('close')">
        <form class="issue-create-modal__card" @submit.prevent="onSubmit">
            <div class="issue-create-modal__body">
                <label class="issue-create-modal__field">
                    <span class="issue-create-modal__label">Título da Issue *</span>
                    <input v-model="title" type="text" class="issue-create-modal__input" placeholder="Descreva sucintamente o bug ou funcionalidade..." required maxlength="256">
                </label>
                <label class="issue-create-modal__field">
                    <span class="issue-create-modal__label">Descrição (Markdown)</span>
                    <MaxInputTextArea v-model="bodyMarkdown" class="issue-create-modal__textarea" :rows="6" placeholder="Passos para reproduzir, comportamento esperado ou especificação..." />
                </label>
                <label class="issue-create-modal__field">
                    <span class="issue-create-modal__label">Estado Inicial MaxCode</span>
                    <select v-model="stateLabel" class="issue-create-modal__select">
                        <option v-for="spec in stateSpecs" :key="spec.name" :value="spec.name">{{ spec.name }}</option>
                    </select>
                </label>
                <div class="issue-create-modal__categories" role="group" aria-label="Categorias da nova issue">
                    <span class="issue-create-modal__label">Categorias:</span>
                    <label v-for="spec in categorySpecs" :key="spec.name" class="issue-create-modal__checkbox">
                        <input type="checkbox" :checked="selectedCategories.includes(spec.name)" @change="onToggleCategory(spec.name)">
                        <span>{{ spec.name }}</span>
                    </label>
                </div>
                <p v-if="validationError" class="issue-create-modal__error" role="alert">{{ validationError }}</p>
            </div>
            <footer class="issue-create-modal__footer">
                <MaxButton label="Cancelar" variant="secondary" class="issue-create-modal__btn" @click="emit('close')" />
                <MaxButton type="submit" :label="props.submitting ? 'Criando...' : 'Criar Issue'" variant="primary" class="issue-create-modal__btn issue-create-modal__btn--primary" :disabled="props.submitting" :loading="props.submitting" />
            </footer>
        </form>
    </MaxModal>
</template>

<script setup lang="ts">
    import { ref } from 'vue';
    import { MaxButton, MaxInputTextArea, MaxModal } from '@maxvue/max-components-ui';
    import { CANONICAL_CATEGORY_LABELS, CANONICAL_STATE_LABELS } from '@/constants/githubLabels';
    import type { CreateIssueInput, ProjectId } from '@/types/bindings';

    export interface IssueCreateModalProps {
        open: boolean;
        projectId: ProjectId;
        submitting?: boolean;
    }

    const props = withDefaults(defineProps<IssueCreateModalProps>(), {
        submitting: false
    });

    const emit = defineEmits<{
        close: [];
        create: [input: CreateIssueInput];
    }>();

    const stateSpecs = CANONICAL_STATE_LABELS;
    const categorySpecs = CANONICAL_CATEGORY_LABELS;

    const title = ref<string>('');
    const bodyMarkdown = ref<string>('');
    const stateLabel = ref<string>('Waiting Plan');
    const selectedCategories = ref<string[]>(['bug']);
    const validationError = ref<string | null>(null);

    function onToggleCategory(name: string): void {
        const idx = selectedCategories.value.indexOf(name);
        if (idx >= 0) {
            selectedCategories.value.splice(idx, 1);
        } else {
            selectedCategories.value.push(name);
        }
    }

    function onSubmit(): void {
        const cleanTitle = title.value.trim();
        if (!cleanTitle) {
            validationError.value = 'O título da issue não pode ser vazio.';
            return;
        }
        validationError.value = null;
        emit('create', {
            projectId: props.projectId,
            title: cleanTitle,
            body: bodyMarkdown.value,
            stateLabel: stateLabel.value || 'Waiting Plan',
            categoryLabels: [...selectedCategories.value],
            extraLabels: [],
            assignees: [],
            clientMutationId: null
        });
        title.value = '';
        bodyMarkdown.value = '';
    }
</script>

<style lang="scss" scoped>
    .issue-create-modal {
        position: fixed;
        inset: 0;
        z-index: 60;
        display: flex;
        align-items: center;
        justify-content: center;
        background: rgb(0 0 0 / 60%);
    }

    .issue-create-modal__card {
        width: 100%;
        max-width: 36rem;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text-strong, var(--background-950));
        overflow: hidden;
    }

    .issue-create-modal__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.75rem 1rem;
        border-bottom: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .issue-create-modal__title {
        margin: 0;
        font-size: 0.9375rem;
        font-weight: 600;
    }

    .issue-create-modal__close {
        border: none;
        background: transparent;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;
    }

    .issue-create-modal__body {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        padding: 1rem;
        font-size: 0.8125rem;
    }

    .issue-create-modal__field {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .issue-create-modal__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .issue-create-modal__input,
    .issue-create-modal__select {
        height: 2rem;
        padding: 0 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.8125rem;
    }

    .issue-create-modal__textarea {
        padding: 0.5rem 0.625rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.8125rem;
        resize: vertical;
    }

    .issue-create-modal__categories {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: 0.625rem;
    }

    .issue-create-modal__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.25rem;
        cursor: pointer;
    }

    .issue-create-modal__error {
        margin: 0;
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .issue-create-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding: 0.75rem 1rem;
        border-top: 1px solid var(--mt-ui-border, var(--background-100));
    }

    .issue-create-modal__btn {
        height: 2rem;
        padding: 0 0.875rem;
        border-radius: 0.3125rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text-secondary, var(--background-600));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &--primary {
            border-color: var(--max-primary-500);
            background: rgb(20 184 166 / 20%);
            color: var(--max-primary-400);
        }
    }
</style>
