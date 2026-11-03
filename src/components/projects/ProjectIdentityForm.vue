<template>
    <form class="project-identity-form" @submit.prevent="handleSubmit">
        <div class="project-identity-form__field">
            <MaxInputText v-model="titleInput" label="Título" :disabled="submitting" maxlength="120" />
        </div>

        <div class="project-identity-form__field">
            <div class="project-identity-form__icon-row" :style="{ '--project-color': effectiveColor, '--mt-project-color': effectiveColor }">
                <span class="project-identity-form__icon-preview" aria-hidden="true">
                    <MaxIcon :icon="effectiveIcon" />
                </span>
                <MaxInputText v-model="iconInput" label="Ícone (Iconify)" :disabled="submitting" class="project-identity-form__icon-input" @input="identityEdited = true" />
            </div>
            <div class="project-identity-form__suggestions">
                <button v-for="sugIcon in PROJECT_ICON_SUGGESTIONS" :key="sugIcon" type="button" class="project-identity-form__sug-btn" :title="sugIcon" :disabled="submitting" @click="selectIcon(sugIcon)">
                    <MaxIcon :icon="sugIcon" />
                </button>
            </div>
        </div>

        <div class="project-identity-form__field">
            <div class="project-identity-form__color-row">
                <input v-model="colorInput" type="color" class="project-identity-form__color-picker" aria-label="Selecionar cor" :disabled="submitting" @input="identityEdited = true">
                <MaxInputText v-model="colorInput" label="Cor (#RRGGBB)" maxlength="7" :disabled="submitting" class="project-identity-form__color-input" @input="identityEdited = true" />
            </div>
            <div class="project-identity-form__palette">
                <button v-for="swatch in PROJECT_COLOR_SUGGESTIONS" :key="swatch" type="button" class="project-identity-form__swatch" :style="{ backgroundColor: swatch }" :title="swatch" :disabled="submitting" @click="selectColor(swatch)" />
            </div>
        </div>

        <div class="project-identity-form__reset-row">
            <button type="button" class="project-identity-form__reset-btn" :disabled="submitting" @click="handleResetIdentity">
                Restaurar identidade automática
            </button>
        </div>

        <div v-if="errorMessage" class="project-identity-form__error" role="alert">
            {{ errorMessage }}
        </div>

        <div v-if="successMessage" class="project-identity-form__success" role="status">
            {{ successMessage }}
        </div>

        <footer class="project-identity-form__footer">
            <button type="button" class="project-identity-form__btn project-identity-form__btn--cancel" :disabled="submitting" @click="handleCancel">
                Cancelar
            </button>
            <button type="submit" class="project-identity-form__btn project-identity-form__btn--submit" :disabled="submitting">
                {{ submitting ? 'Salvando...' : 'Salvar alterações' }}
            </button>
        </footer>
    </form>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import MaxInputText from '@maxvue/max-components-ui/components/MaxInputText';
    import { useIpcError } from '@/composables/useIpcError';
    import {
        isValidProjectColor,
        isValidProjectIcon,
        isValidProjectTitle,
        normalizeProjectColor,
        PROJECT_COLOR_SUGGESTIONS,
        PROJECT_ICON_SUGGESTIONS
    } from '@/constants/projectIdentity';
    import { useProjectsStore } from '@/stores/projects';
    import type { Project_Serialize } from '@/types/bindings';

    export interface ProjectIdentityFormProps {
        project: Project_Serialize;
        active: boolean;
    }

    const props = defineProps<ProjectIdentityFormProps>();

    const emit = defineEmits<{
        saved: [project: Project_Serialize];
        cancel: [];
        'busy-change': [busy: boolean];
    }>();

    const projectsStore = useProjectsStore();
    const ipcError = useIpcError();

    const titleInput = ref<string>('');
    const iconInput = ref<string>('mdi:folder');
    const colorInput = ref<string>('#14B8A6');
    const identityEdited = ref<boolean>(false);
    const submitting = ref<boolean>(false);
    const errorMessage = ref<string | null>(null);
    const successMessage = ref<string | null>(null);

    const effectiveIcon = computed<string>(() =>
        isValidProjectIcon(iconInput.value) ? iconInput.value.trim() : 'mdi:folder'
    );

    const effectiveColor = computed<string>(() =>
        isValidProjectColor(colorInput.value) ? normalizeProjectColor(colorInput.value) : '#14B8A6'
    );

    watch(
        [() => props.active, () => props.project.id],
        ([isActive, currentId], [prevActive, prevId] = [false, undefined]) => {
            if (isActive && (!prevActive || currentId !== prevId)) {
                titleInput.value = props.project.title;
                iconInput.value = props.project.icon;
                colorInput.value = props.project.color;
                identityEdited.value = false;
                submitting.value = false;
                errorMessage.value = null;
                successMessage.value = null;
            }
        },
        { immediate: true }
    );

    function selectIcon(icon: string): void {
        iconInput.value = icon;
        identityEdited.value = true;
    }

    function selectColor(color: string): void {
        colorInput.value = color;
        identityEdited.value = true;
    }

    function handleCancel(): void {
        titleInput.value = props.project.title;
        iconInput.value = props.project.icon;
        colorInput.value = props.project.color;
        identityEdited.value = false;
        errorMessage.value = null;
        successMessage.value = null;
        emit('cancel');
    }

    async function handleResetIdentity(): Promise<void> {
        if (submitting.value) {
            return;
        }
        submitting.value = true;
        emit('busy-change', true);
        errorMessage.value = null;
        successMessage.value = null;
        try {
            const updated = await projectsStore.updateProject(props.project.id, {
                title: titleInput.value.trim() !== props.project.title ? titleInput.value.trim() : null,
                resetIdentity: true
            });
            identityEdited.value = false;
            titleInput.value = updated.title;
            iconInput.value = updated.icon;
            colorInput.value = updated.color;
            successMessage.value = 'Identidade restaurada com sucesso.';
            emit('saved', updated);
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
            emit('busy-change', false);
        }
    }

    async function handleSubmit(): Promise<void> {
        if (submitting.value) {
            return;
        }
        const trimmedTitle = titleInput.value.trim();
        if (!isValidProjectTitle(trimmedTitle)) {
            errorMessage.value = 'O título do projeto deve ter entre 1 e 120 caracteres válidos.';
            return;
        }
        if (!isValidProjectIcon(iconInput.value)) {
            errorMessage.value = 'O identificador de ícone informado é inválido.';
            return;
        }
        if (!isValidProjectColor(colorInput.value)) {
            errorMessage.value = 'A cor do projeto deve estar no formato hexadecimal #RRGGBB.';
            return;
        }

        submitting.value = true;
        emit('busy-change', true);
        errorMessage.value = null;
        successMessage.value = null;
        try {
            const updated = await projectsStore.updateProject(props.project.id, {
                title: trimmedTitle,
                icon: identityEdited.value ? iconInput.value.trim() : null,
                color: identityEdited.value ? normalizeProjectColor(colorInput.value) : null,
                resetIdentity: null
            });
            identityEdited.value = false;
            successMessage.value = 'Identidade salva com sucesso.';
            emit('saved', updated);
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
            emit('busy-change', false);
        }
    }
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .project-identity-form {
        display: flex;
        flex-direction: column;
        gap: 0.875rem;
    }

    .project-identity-form__field {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .project-identity-form__icon-row,
    .project-identity-form__color-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .project-identity-form__icon-input,
    .project-identity-form__color-input {
        flex: 1;
        min-width: 0;
    }

    .project-identity-form__icon-preview {
        width: 2.25rem;
        height: 2.25rem;
        border-radius: 0.375rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--mt-project-color, #14B8A6) 15%, transparent);
        color: var(--mt-project-color, #14B8A6);
        border: 1px solid color-mix(in srgb, var(--mt-project-color, #14B8A6) 35%, transparent);
        flex-shrink: 0;
    }

    .project-identity-form__suggestions,
    .project-identity-form__palette {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
        margin-top: 0.25rem;
    }

    .project-identity-form__sug-btn {
        width: 1.75rem;
        height: 1.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-600);
        background: var(--background-900);
        color: var(--background-100);
        display: inline-flex;
        align-items: center;
        justify-content: center;
        cursor: pointer;

        &:disabled {
            opacity: 0.6;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: var(--background-700);
        }
    }

    .project-identity-form__swatch {
        width: 1.375rem;
        height: 1.375rem;
        border-radius: 9999px;
        border: 1px solid var(--background-500);
        cursor: pointer;

        &:disabled {
            opacity: 0.6;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            transform: scale(1.1);
        }
    }

    .project-identity-form__color-picker {
        width: 2.25rem;
        height: 2.25rem;
        padding: 0;
        border: 1px solid var(--background-600);
        border-radius: 0.375rem;
        background: transparent;
        cursor: pointer;
        flex-shrink: 0;

        &:disabled {
            opacity: 0.6;
            cursor: not-allowed;
        }
    }

    .project-identity-form__reset-row {
        display: flex;
        justify-content: flex-start;
    }

    .project-identity-form__reset-btn {
        border: 1px solid var(--background-600);
        background: var(--background-700);
        color: var(--background-100);
        border-radius: 0.375rem;
        padding: 0.375rem 0.625rem;
        font-size: 0.75rem;
        cursor: pointer;

        &:disabled {
            opacity: 0.6;
            cursor: not-allowed;
        }

        &:hover:not(:disabled) {
            background: var(--background-600);
        }
    }

    .project-identity-form__error {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: color-mix(in srgb, var(--max-danger-500) 15%, var(--background-900));
        border: 1px solid var(--max-danger-500);
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .project-identity-form__success {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: color-mix(in srgb, var(--max-success-500) 15%, var(--background-900));
        border: 1px solid var(--max-success-500);
        color: var(--max-success-400);
        font-size: 0.75rem;
    }

    .project-identity-form__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        margin-top: 0.5rem;
    }

    .project-identity-form__btn {
        padding: 0.4375rem 0.875rem;
        border-radius: 0.375rem;
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;

        &:focus-visible {
            @include mt-focus-ring;
        }

        &:disabled {
            opacity: 0.6;
            cursor: not-allowed;
        }
    }

    .project-identity-form__btn--cancel {
        border: 1px solid var(--background-600);
        background: transparent;
        color: var(--background-200);

        &:hover:not(:disabled) {
            background: var(--background-700);
        }
    }

    .project-identity-form__btn--submit {
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--background-50);

        &:hover:not(:disabled) {
            background: var(--max-primary-500);
        }
    }
</style>
