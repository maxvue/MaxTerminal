<template>
    <MaxModal :visible="props.visible" title="Adicionar projeto" :no-button="true" max-width="32rem" class="project-add-modal" :dismissable="!submitting" :close-on-escape="!submitting" @update:visible="emit('update:visible', $event)">
        <form class="project-add-modal__form" @submit.prevent="handleSubmit">
            <div class="project-add-modal__field">
                <label for="project-add-path" class="project-add-modal__label">Pasta do projeto</label>
                <div class="project-add-modal__path-row">
                    <input id="project-add-path" v-model="pathInput" type="text" class="project-add-modal__input" placeholder="~/GitHub/meu-projeto" autocomplete="off" @input="onPathInputChanged">
                    <MaxButton type="button" variant="secondary" class="project-add-modal__pick-btn" data-testid="pick-folder-btn" @click="handlePickFolder">
                        Escolher pasta
                    </MaxButton>
                </div>
            </div>

            <div class="project-add-modal__field">
                <label for="project-add-title" class="project-add-modal__label">Título</label>
                <input id="project-add-title" v-model="titleInput" type="text" class="project-add-modal__input" placeholder="Nome do projeto" maxlength="120" @input="onTitleManualEdit">
            </div>

            <div class="project-add-modal__identity-grid">
                <div class="project-add-modal__field">
                    <label for="project-add-icon" class="project-add-modal__label">Ícone (Iconify)</label>
                    <div class="project-add-modal__icon-row" :style="{ '--project-color': effectiveColor, '--mt-project-color': effectiveColor }">
                        <span class="project-add-modal__icon-preview" aria-hidden="true">
                            <MaxIcon :icon="effectiveIcon" />
                        </span>
                        <input id="project-add-icon" v-model="iconInput" type="text" class="project-add-modal__input" placeholder="logos:laravel" @input="onIconManualEdit">
                    </div>
                </div>

                <div class="project-add-modal__field">
                    <label for="project-add-color" class="project-add-modal__label">Cor (#RRGGBB)</label>
                    <div class="project-add-modal__color-row">
                        <input v-model="colorInput" type="color" class="project-add-modal__color-picker" aria-label="Selecionar cor" @input="onColorManualEdit">
                        <input id="project-add-color" v-model="colorInput" type="text" class="project-add-modal__input" placeholder="#14B8A6" maxlength="7" @input="onColorManualEdit">
                    </div>
                </div>
            </div>

            <div v-if="detectedStackLabel" class="project-add-modal__detected">
                <span>Stack detectada: <strong>{{ detectedStackLabel }}</strong></span>
            </div>

            <MaxAlert v-if="errorMessage" type="danger" role="alert" class="project-add-modal__error">
                {{ errorMessage }}
            </MaxAlert>

            <footer class="project-add-modal__footer">
                <MaxButton type="button" variant="secondary" class="project-add-modal__btn project-add-modal__btn--cancel" @click="handleClose">
                    Cancelar
                </MaxButton>
                <MaxButton type="submit" variant="primary" class="project-add-modal__btn project-add-modal__btn--submit" :disabled="submitting" :loading="submitting" data-testid="submit-project-btn">
                    {{ submitting ? 'Adicionando...' : 'Adicionar projeto' }}
                </MaxButton>
            </footer>
        </form>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, onScopeDispose, ref, watch } from 'vue';
    import MaxAlert from '@maxvue/max-components-ui/components/MaxAlert';
    import MaxButton from '@maxvue/max-components-ui/components/MaxButton';
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
    import { useIpcError } from '@/composables/useIpcError';
    import {
        isValidProjectColor,
        isValidProjectIcon,
        isValidProjectTitle,
        normalizeProjectColor,
        STACK_LABELS_PT_BR
    } from '@/constants/projectIdentity';
    import * as projectsIpc from '@/ipc/projects';
    import { useProjectsStore } from '@/stores/projects';
    import type { Project_Serialize, StackKind } from '@/types/bindings';

    export interface ProjectAddModalProps {
        visible: boolean;
    }

    const props = defineProps<ProjectAddModalProps>();

    const emit = defineEmits<{
        'update:visible': [v: boolean];
        created: [p: Project_Serialize];
    }>();

    const PREVIEW_DEBOUNCE_MS = 250;

    const projectsStore = useProjectsStore();
    const ipcError = useIpcError();

    const pathInput = ref<string>('');
    const titleInput = ref<string>('');
    const iconInput = ref<string>('mdi:folder-code-outline');
    const colorInput = ref<string>('#14B8A6');
    const detectedStack = ref<StackKind | null>(null);

    const titleTouched = ref<boolean>(false);
    const iconTouched = ref<boolean>(false);
    const colorTouched = ref<boolean>(false);

    const submitting = ref<boolean>(false);
    const errorMessage = ref<string | null>(null);

    let previewTimer: ReturnType<typeof setTimeout> | null = null;
    let previewRequestToken = 0;

    const effectiveIcon = computed<string>(() =>
        isValidProjectIcon(iconInput.value) ? iconInput.value.trim() : 'mdi:folder'
    );

    const effectiveColor = computed<string>(() =>
        isValidProjectColor(colorInput.value) ? normalizeProjectColor(colorInput.value) : '#14B8A6'
    );

    const detectedStackLabel = computed<string | null>(() => {
        if (!detectedStack.value) {
            return null;
        }
        return STACK_LABELS_PT_BR[detectedStack.value] ?? null;
    });

    function clearPreviewTimer(): void {
        if (previewTimer !== null) {
            clearTimeout(previewTimer);
            previewTimer = null;
        }
    }

    function deriveDefaultTitleFromPath(rawPath: string): string {
        const trimmed = rawPath.trim().replace(/\/+$/, '');
        if (!trimmed) {
            return '';
        }
        const parts = trimmed.split('/');
        return parts[parts.length - 1] ?? '';
    }

    function resetForm(): void {
        clearPreviewTimer();
        previewRequestToken += 1;
        pathInput.value = '';
        titleInput.value = '';
        iconInput.value = 'mdi:folder-code-outline';
        colorInput.value = '#14B8A6';
        detectedStack.value = null;
        titleTouched.value = false;
        iconTouched.value = false;
        colorTouched.value = false;
        submitting.value = false;
        errorMessage.value = null;
    }

    watch(
        () => props.visible,
        (isOpen) => {
            if (isOpen) {
                resetForm();
            } else {
                clearPreviewTimer();
                previewRequestToken += 1;
            }
        }
    );

    async function runPreview(targetPath: string, token: number): Promise<void> {
        try {
            const identity = await projectsIpc.projectPreviewIdentity(targetPath);
            if (!props.visible || token !== previewRequestToken) {
                return;
            }
            detectedStack.value = identity.stack;
            if (!titleTouched.value) {
                titleInput.value = deriveDefaultTitleFromPath(targetPath);
            }
            if (!iconTouched.value) {
                iconInput.value = identity.icon;
            }
            if (!colorTouched.value) {
                colorInput.value = identity.color;
            }
            errorMessage.value = null;
        } catch (err: unknown) {
            if (!props.visible || token !== previewRequestToken) {
                return;
            }
            errorMessage.value = ipcError.message(err);
        }
    }

    function schedulePreview(rawPath: string): void {
        clearPreviewTimer();
        const trimmed = rawPath.trim();
        const token = ++previewRequestToken;
        if (!trimmed) {
            detectedStack.value = null;
            return;
        }
        if (!titleTouched.value) {
            titleInput.value = deriveDefaultTitleFromPath(trimmed);
        }
        previewTimer = setTimeout(() => {
            previewTimer = null;
            void runPreview(trimmed, token);
        }, PREVIEW_DEBOUNCE_MS);
    }

    function onPathInputChanged(): void {
        errorMessage.value = null;
        schedulePreview(pathInput.value);
    }

    function onTitleManualEdit(): void {
        titleTouched.value = true;
        errorMessage.value = null;
    }

    function onIconManualEdit(): void {
        iconTouched.value = true;
        errorMessage.value = null;
    }

    function onColorManualEdit(): void {
        colorTouched.value = true;
        errorMessage.value = null;
    }

    async function handlePickFolder(): Promise<void> {
        const chosen = await projectsIpc.pickFolder('Escolher pasta do projeto');
        if (!chosen || !props.visible) {
            return;
        }
        pathInput.value = chosen;
        errorMessage.value = null;
        schedulePreview(chosen);
    }

    function handleClose(): void {
        clearPreviewTimer();
        previewRequestToken += 1;
        emit('update:visible', false);
    }

    async function handleSubmit(): Promise<void> {
        if (submitting.value) {
            return;
        }
        const trimmedPath = pathInput.value.trim();
        if (!trimmedPath) {
            errorMessage.value = 'Informe o caminho da pasta do projeto.';
            return;
        }

        const trimmedTitle = titleInput.value.trim();
        if (trimmedTitle.length > 0 && !isValidProjectTitle(trimmedTitle)) {
            errorMessage.value = 'O título do projeto deve ter entre 1 e 120 caracteres válidos.';
            return;
        }

        if (iconTouched.value && !isValidProjectIcon(iconInput.value)) {
            errorMessage.value = 'O identificador de ícone informado é inválido (use o formato prefixo:nome).';
            return;
        }

        if (colorTouched.value && !isValidProjectColor(colorInput.value)) {
            errorMessage.value = 'A cor do projeto deve estar no formato hexadecimal #RRGGBB.';
            return;
        }

        submitting.value = true;
        errorMessage.value = null;

        try {
            const manualIdentity = iconTouched.value || colorTouched.value;
            const created = await projectsStore.addProject({
                path: trimmedPath,
                title: trimmedTitle.length > 0 ? trimmedTitle : null,
                icon: manualIdentity ? iconInput.value.trim() : null,
                color: manualIdentity ? normalizeProjectColor(colorInput.value) : null
            });
            emit('created', created);
            emit('update:visible', false);
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
        }
    }

    onScopeDispose(() => {
        clearPreviewTimer();
        previewRequestToken += 1;
    });
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    :deep(.project-add-modal.max-modal) {
        background-color: var(--mt-ui-surface, var(--background-800, var(--background-0)));
        border-color: var(--mt-ui-border, var(--background-600, var(--background-100)));
        color: var(--mt-ui-text, var(--background-100, var(--background-800)));

        .max-modal-content {
            padding: 1.125rem;
        }
    }

    .project-add-modal__form {
        display: flex;
        flex-direction: column;
        gap: 0.875rem;
    }

    .project-add-modal__field {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .project-add-modal__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text, var(--background-800));
    }

    .project-add-modal__path-row {
        display: flex;
        gap: 0.5rem;
    }

    .project-add-modal__input {
        flex: 1;
        min-width: 0;
        padding: 0.4375rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.8125rem;

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-add-modal__pick-btn {
        padding: 0.4375rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
        white-space: nowrap;

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }

        &:focus-visible {
            @include mt-focus-ring;
        }
    }
    .project-add-modal__identity-grid {
        display: grid;
        grid-template-columns: 1fr 1fr;
        gap: 0.75rem;
    }

    .project-add-modal__icon-row,
    .project-add-modal__color-row {
        display: flex;
        align-items: center;
        gap: 0.5rem;
    }

    .project-add-modal__icon-preview {
        width: 2rem;
        height: 2rem;
        border-radius: 0.375rem;
        display: inline-flex;
        align-items: center;
        justify-content: center;
        background: color-mix(in srgb, var(--mt-project-color, #14B8A6) 15%, transparent);
        color: var(--mt-project-color, #14B8A6);
        border: 1px solid color-mix(in srgb, var(--mt-project-color, #14B8A6) 35%, transparent);
        flex-shrink: 0;
    }

    .project-add-modal__color-picker {
        width: 2rem;
        height: 2rem;
        padding: 0;
        border: 1px solid var(--background-600);
        border-radius: 0.375rem;
        background: transparent;
        cursor: pointer;
        flex-shrink: 0;
    }

    .project-add-modal__detected {
        font-size: 0.75rem;
        color: var(--max-info-400);
    }

    .project-add-modal__error {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: color-mix(in srgb, var(--max-danger-500) 15%, var(--mt-ui-surface, var(--background-0)));
        border: 1px solid var(--max-danger-500);
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .project-add-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
        padding-top: 0.375rem;
    }

    .project-add-modal__btn {
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

    .project-add-modal__btn--cancel {
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: transparent;
        color: var(--mt-ui-text, var(--background-800));

        &:hover {
            background: var(--mt-ui-surface-hover, var(--background-75));
        }
    }

    .project-add-modal__btn--submit {
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--max-primary-content);

        &:hover:not(:disabled) {
            background: var(--max-primary-500);
        }
    }
</style>
