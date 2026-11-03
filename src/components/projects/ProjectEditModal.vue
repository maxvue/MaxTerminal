<template>
    <div v-if="props.visible && props.project" class="project-edit-modal" role="dialog" aria-modal="true" aria-labelledby="project-edit-modal-title">
        <div class="project-edit-modal__backdrop" @click="handleClose" />
        <div class="project-edit-modal__dialog">
            <header class="project-edit-modal__header">
                <h2 id="project-edit-modal-title" class="project-edit-modal__title">Editar projeto</h2>
                <button type="button" class="project-edit-modal__close" aria-label="Fechar modal" @click="handleClose">
                    <MaxIcon icon="mdi:close" />
                </button>
            </header>

            <form class="project-edit-modal__form" @submit.prevent="handleSubmit">
                <div class="project-edit-modal__field">
                    <label for="project-edit-title" class="project-edit-modal__label">Título</label>
                    <input id="project-edit-title" v-model="titleInput" type="text" class="project-edit-modal__input" maxlength="120">
                </div>

                <div class="project-edit-modal__field">
                    <label for="project-edit-icon" class="project-edit-modal__label">Ícone (Iconify)</label>
                    <div class="project-edit-modal__icon-row" :style="{ '--project-color': effectiveColor, '--mt-project-color': effectiveColor }">
                        <span class="project-edit-modal__icon-preview" aria-hidden="true">
                            <MaxIcon :icon="effectiveIcon" />
                        </span>
                        <input id="project-edit-icon" v-model="iconInput" type="text" class="project-edit-modal__input" @input="identityEdited = true">
                    </div>
                    <div class="project-edit-modal__suggestions">
                        <button v-for="sugIcon in PROJECT_ICON_SUGGESTIONS" :key="sugIcon" type="button" class="project-edit-modal__sug-btn" :title="sugIcon" @click="selectIcon(sugIcon)">
                            <MaxIcon :icon="sugIcon" />
                        </button>
                    </div>
                </div>

                <div class="project-edit-modal__field">
                    <label for="project-edit-color" class="project-edit-modal__label">Cor (#RRGGBB)</label>
                    <div class="project-edit-modal__color-row">
                        <input v-model="colorInput" type="color" class="project-edit-modal__color-picker" aria-label="Selecionar cor" @input="identityEdited = true">
                        <input id="project-edit-color" v-model="colorInput" type="text" class="project-edit-modal__input" maxlength="7" @input="identityEdited = true">
                    </div>
                    <div class="project-edit-modal__palette">
                        <button v-for="swatch in PROJECT_COLOR_SUGGESTIONS" :key="swatch" type="button" class="project-edit-modal__swatch" :style="{ backgroundColor: swatch }" :title="swatch" @click="selectColor(swatch)" />
                    </div>
                </div>

                <div class="project-edit-modal__reset-row">
                    <button type="button" class="project-edit-modal__reset-btn" :disabled="submitting" @click="handleResetIdentity">
                        Restaurar identidade automática
                    </button>
                </div>

                <div v-if="errorMessage" class="project-edit-modal__error" role="alert">
                    {{ errorMessage }}
                </div>

                <footer class="project-edit-modal__footer">
                    <button type="button" class="project-edit-modal__btn project-edit-modal__btn--cancel" @click="handleClose">
                        Cancelar
                    </button>
                    <button type="submit" class="project-edit-modal__btn project-edit-modal__btn--submit" :disabled="submitting">
                        {{ submitting ? 'Salvando...' : 'Salvar alterações' }}
                    </button>
                </footer>
            </form>
        </div>
    </div>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
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

    export interface ProjectEditModalProps {
        visible: boolean;
        project: Project_Serialize | null;
    }

    const props = defineProps<ProjectEditModalProps>();

    const emit = defineEmits<{
        'update:visible': [v: boolean];
        saved: [p: Project_Serialize];
    }>();

    const projectsStore = useProjectsStore();
    const ipcError = useIpcError();

    const titleInput = ref<string>('');
    const iconInput = ref<string>('mdi:folder');
    const colorInput = ref<string>('#14B8A6');
    const identityEdited = ref<boolean>(false);
    const submitting = ref<boolean>(false);
    const errorMessage = ref<string | null>(null);

    const effectiveIcon = computed<string>(() =>
        isValidProjectIcon(iconInput.value) ? iconInput.value.trim() : 'mdi:folder'
    );

    const effectiveColor = computed<string>(() =>
        isValidProjectColor(colorInput.value) ? normalizeProjectColor(colorInput.value) : '#14B8A6'
    );

    watch(
        () => [props.visible, props.project] as const,
        ([isOpen, proj]) => {
            if (isOpen && proj) {
                titleInput.value = proj.title;
                iconInput.value = proj.icon;
                colorInput.value = proj.color;
                identityEdited.value = false;
                submitting.value = false;
                errorMessage.value = null;
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

    function handleClose(): void {
        emit('update:visible', false);
    }

    async function handleResetIdentity(): Promise<void> {
        if (!props.project || submitting.value) {
            return;
        }
        submitting.value = true;
        errorMessage.value = null;
        try {
            const updated = await projectsStore.updateProject(props.project.id, {
                title: titleInput.value.trim() !== props.project.title ? titleInput.value.trim() : null,
                resetIdentity: true
            });
            emit('saved', updated);
            emit('update:visible', false);
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
        }
    }

    async function handleSubmit(): Promise<void> {
        if (!props.project || submitting.value) {
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
        errorMessage.value = null;
        try {
            const updated = await projectsStore.updateProject(props.project.id, {
                title: trimmedTitle,
                icon: identityEdited.value ? iconInput.value.trim() : null,
                color: identityEdited.value ? normalizeProjectColor(colorInput.value) : null,
                resetIdentity: null
            });
            emit('saved', updated);
            emit('update:visible', false);
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    .project-edit-modal {
        position: fixed;
        inset: 0;
        z-index: 100;
        display: flex;
        align-items: center;
        justify-content: center;
        padding: 1rem;
    }

    .project-edit-modal__backdrop {
        position: absolute;
        inset: 0;
        background: rgb(0 0 0 / 60%);
    }

    .project-edit-modal__dialog {
        position: relative;
        z-index: 1;
        width: 100%;
        max-width: 30rem;
        border-radius: 0.625rem;
        background: var(--background-800);
        border: 1px solid var(--background-600);
        color: var(--background-100);
        box-shadow: 0 16px 40px rgb(0 0 0 / 55%);
        overflow: hidden;
    }

    .project-edit-modal__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 0.875rem 1.125rem;
        border-bottom: 1px solid var(--background-700);
    }

    .project-edit-modal__title {
        margin: 0;
        font-size: 1rem;
        font-weight: 600;
        color: var(--background-50);
    }

    .project-edit-modal__close {
        border: none;
        background: transparent;
        color: var(--background-300);
        cursor: pointer;
        padding: 0.25rem;
        border-radius: 0.375rem;
    }

    .project-edit-modal__form {
        display: flex;
        flex-direction: column;
        gap: 0.875rem;
        padding: 1.125rem;
    }

    .project-edit-modal__field {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .project-edit-modal__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--background-200);
    }

    .project-edit-modal__input {
        flex: 1;
        min-width: 0;
        padding: 0.4375rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-600);
        background: var(--background-900);
        color: var(--background-50);
        font-size: 0.8125rem;
    }

    .project-edit-modal__icon-row,
    .project-edit-modal__color-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .project-edit-modal__icon-preview {
        width: 2rem;
        height: 2rem;
        border-radius: 0.375rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--mt-project-color, #14B8A6) 15%, transparent);
        color: var(--mt-project-color, #14B8A6);
        border: 1px solid color-mix(in srgb, var(--mt-project-color, #14B8A6) 35%, transparent);
    }

    .project-edit-modal__suggestions,
    .project-edit-modal__palette {
        display: flex;
        flex-wrap: wrap;
        gap: 0.25rem;
        margin-top: 0.25rem;
    }

    .project-edit-modal__sug-btn {
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
    }

    .project-edit-modal__swatch {
        width: 1.375rem;
        height: 1.375rem;
        border-radius: 9999px;
        border: 1px solid var(--background-500);
        cursor: pointer;
    }

    .project-edit-modal__color-picker {
        width: 2rem;
        height: 2rem;
        padding: 0;
        border: 1px solid var(--background-600);
        border-radius: 0.375rem;
        background: transparent;
        cursor: pointer;
    }

    .project-edit-modal__reset-row {
        display: flex;
        justify-content: flex-start;
    }

    .project-edit-modal__reset-btn {
        border: 1px solid var(--background-600);
        background: var(--background-700);
        color: var(--background-100);
        border-radius: 0.375rem;
        padding: 0.375rem 0.625rem;
        font-size: 0.75rem;
        cursor: pointer;
    }

    .project-edit-modal__error {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: color-mix(in srgb, var(--max-danger-500) 15%, var(--background-900));
        border: 1px solid var(--max-danger-500);
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .project-edit-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
    }

    .project-edit-modal__btn {
        padding: 0.4375rem 0.875rem;
        border-radius: 0.375rem;
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-edit-modal__btn--cancel {
        border: 1px solid var(--background-600);
        background: transparent;
        color: var(--background-200);
    }

    .project-edit-modal__btn--submit {
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--background-50);
    }
</style>
