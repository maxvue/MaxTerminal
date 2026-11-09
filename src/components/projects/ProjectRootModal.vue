<template>
    <MaxModal :visible="props.visible" :title="props.root ? 'Configurar pasta de projetos' : 'Adicionar pasta de projetos'" :no-button="true" max-width="32rem" class="project-root-modal" :dismissable="!submitting" :close-on-escape="!submitting" :before-close="onBeforeClose" @update:visible="onUpdateVisible">
        <form class="project-root-modal__form" @submit.prevent="handleSubmit">
            <div class="project-root-modal__field">
                <label for="project-root-path" class="project-root-modal__label">Caminho da pasta de projetos</label>
                <div class="project-root-modal__path-row">
                    <MaxInputText id="project-root-path" v-model="pathInput" class="project-root-modal__input" placeholder="~/GitHub" :readonly="Boolean(props.root)" autocomplete="off" />
                    <MaxButton v-if="!props.root" type="button" severity="secondary" size="small" class="project-root-modal__pick-btn" label="Escolher pasta" @click="handlePickFolder" />
                </div>
            </div>

            <fieldset class="project-root-modal__fieldset">
                <legend class="project-root-modal__label">Modo de descoberta</legend>
                <MaxInputRadio v-model="discoveryMode" name="project-root-discovery-mode" value="suggest" label="Sugerir novas pastas" />
                <MaxInputRadio v-model="discoveryMode" name="project-root-discovery-mode" value="auto_import" label="Importar automaticamente" />
            </fieldset>

            <MaxInputCheckbox v-if="!props.root" v-model="importExisting" label="Importar subpastas existentes agora" />

            <MaxInputCheckbox v-model="watchEnabled" label="Monitorar alterações automaticamente" />

            <div v-if="props.root" class="project-root-modal__edit-actions">
                <MaxButton type="button" severity="secondary" label="Varrer agora" :disabled="submitting" @click="handleRescan" />
                <MaxButton type="button" severity="danger" label="Remover pasta de projetos" :disabled="submitting" @click="handleRemoveRoot" />
            </div>

            <MaxAlert v-if="errorMessage" severity="danger" role="alert" :message="errorMessage" class="project-root-modal__error" />

            <footer class="project-root-modal__footer">
                <MaxButton type="button" severity="secondary" label="Cancelar" class="project-root-modal__btn project-root-modal__btn--cancel" @click="handleClose" />
                <MaxButton type="submit" class="project-root-modal__btn project-root-modal__btn--submit" :label="submitting ? 'Salvando...' : 'Salvar'" :disabled="submitting" :loading="submitting" />
            </footer>
        </form>
    </MaxModal>
</template>

<script setup lang="ts">
    import { ref, watch } from 'vue';
    import {
        MaxAlert,
        MaxButton,
        MaxInputCheckbox,
        MaxInputRadio,
        MaxInputText,
        MaxModal
    } from '@maxvue/max-components-ui';
    import { useIpcError } from '@/composables/useIpcError';
    import * as projectsIpc from '@/ipc/projects';
    import { useProjectsStore } from '@/stores/projects';
    import type {
        DiscoveryMode,
        ProjectRoot,
        ProjectRootId,
        RootImportReport_Serialize
    } from '@/types/bindings';

    export interface ProjectRootModalProps {
        visible: boolean;
        root?: ProjectRoot | null;
    }

    const props = withDefaults(defineProps<ProjectRootModalProps>(), {
        root: null
    });

    const emit = defineEmits<{
        'update:visible': [v: boolean];
        saved: [r: RootImportReport_Serialize | ProjectRoot];
        removed: [id: ProjectRootId];
    }>();

    const projectsStore = useProjectsStore();
    const ipcError = useIpcError();

    const pathInput = ref<string>('');
    const discoveryMode = ref<DiscoveryMode>('suggest');
    const importExisting = ref<boolean>(true);
    const watchEnabled = ref<boolean>(true);
    const submitting = ref<boolean>(false);
    const errorMessage = ref<string | null>(null);

    function syncFromProps(): void {
        errorMessage.value = null;
        submitting.value = false;
        if (props.root) {
            pathInput.value = props.root.path;
            discoveryMode.value = props.root.discoveryMode;
            watchEnabled.value = props.root.watchEnabled;
            importExisting.value = false;
        } else {
            pathInput.value = '';
            discoveryMode.value = 'suggest';
            importExisting.value = true;
            watchEnabled.value = true;
        }
    }

    watch(
        () => [props.visible, props.root] as const,
        ([isOpen]) => {
            if (isOpen) {
                syncFromProps();
            }
        },
        { immediate: true }
    );

    async function handlePickFolder(): Promise<void> {
        const chosen = await projectsIpc.pickFolder('Escolher pasta de projetos');
        if (chosen && props.visible) {
            pathInput.value = chosen;
            errorMessage.value = null;
        }
    }

    async function onBeforeClose(event: { preventDefault: () => void }): Promise<boolean> {
        if (submitting.value) {
            event.preventDefault();
            return false;
        }
        return true;
    }

    function handleClose(): void {
        if (submitting.value) {
            return;
        }
        emit('update:visible', false);
    }

    function onUpdateVisible(visible: boolean): void {
        if (!visible) {
            if (submitting.value) {
                return;
            }
            handleClose();
        }
    }

    async function handleRescan(): Promise<void> {
        if (!props.root || submitting.value) {
            return;
        }
        submitting.value = true;
        errorMessage.value = null;
        try {
            const report = await projectsStore.rescanRoot(props.root.id);
            emit('saved', report);
            emit('update:visible', false);
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
        }
    }

    async function handleRemoveRoot(): Promise<void> {
        if (!props.root || submitting.value) {
            return;
        }
        submitting.value = true;
        errorMessage.value = null;
        try {
            const id = props.root.id;
            await projectsStore.removeRoot(id);
            emit('removed', id);
            emit('update:visible', false);
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
        }
    }

    async function handleSubmit(): Promise<void> {
        if (submitting.value) {
            return;
        }
        submitting.value = true;
        errorMessage.value = null;

        try {
            if (props.root) {
                const updated = await projectsStore.updateRoot(props.root.id, {
                    discoveryMode: discoveryMode.value,
                    watchEnabled: watchEnabled.value
                });
                emit('saved', updated);
                emit('update:visible', false);
            } else {
                const trimmedPath = pathInput.value.trim();
                if (!trimmedPath) {
                    errorMessage.value = 'Informe o caminho da pasta de projetos.';
                    return;
                }
                const report = await projectsStore.addRoot({
                    path: trimmedPath,
                    discoveryMode: discoveryMode.value,
                    importExisting: importExisting.value,
                    watchEnabled: watchEnabled.value
                });
                emit('saved', report);
                emit('update:visible', false);
            }
        } catch (err: unknown) {
            errorMessage.value = ipcError.message(err);
        } finally {
            submitting.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    @use '@/styles/params' as *;

    :deep(.project-root-modal.max-modal) {
        background-color: var(--mt-ui-surface, var(--background-800, var(--background-0)));
        border-color: var(--mt-ui-border, var(--background-600, var(--background-100)));
        color: var(--mt-ui-text, var(--background-100, var(--background-800)));

        .max-modal-content {
            padding: 1.125rem;
        }
    }

    .project-root-modal__form {
        display: flex;
        flex-direction: column;
        gap: 0.875rem;
    }

    .project-root-modal__field {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .project-root-modal__label {
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text, var(--background-800));
    }

    .project-root-modal__path-row {
        display: flex;
        gap: 0.5rem;
    }

    .project-root-modal__input {
        flex: 1;
        min-width: 0;
        padding: 0.4375rem 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
        color: var(--mt-ui-text-strong, var(--background-950));
        font-size: 0.8125rem;
    }

    .project-root-modal__pick-btn {
        padding: 0.4375rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;
    }

    .project-root-modal__fieldset {
        border: 1px solid var(--mt-ui-border, var(--background-100));
        border-radius: 0.375rem;
        padding: 0.5rem 0.75rem;
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .project-root-modal__radio,
    .project-root-modal__checkbox {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.8125rem;
        color: var(--mt-ui-text, var(--background-800));
        cursor: pointer;
    }

    .project-root-modal__edit-actions {
        display: flex;
        justify-content: space-between;
        gap: 0.5rem;
    }

    .project-root-modal__error {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: color-mix(in srgb, var(--max-danger-500) 15%, var(--mt-ui-surface, var(--background-0)));
        border: 1px solid var(--max-danger-500);
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .project-root-modal__footer {
        display: flex;
        justify-content: flex-end;
        gap: 0.5rem;
    }

    .project-root-modal__btn {
        padding: 0.4375rem 0.875rem;
        border-radius: 0.375rem;
        font-size: 0.8125rem;
        font-weight: 600;
        cursor: pointer;

        &:focus-visible {
            @include mt-focus-ring;
        }
    }

    .project-root-modal__btn--secondary {
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-elevated, var(--background-50));
        color: var(--mt-ui-text, var(--background-800));
    }

    .project-root-modal__btn--danger {
        border: 1px solid var(--max-danger-500);
        background: transparent;
        color: var(--max-danger-400);
    }

    .project-root-modal__btn--cancel {
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: transparent;
        color: var(--mt-ui-text, var(--background-800));
    }

    .project-root-modal__btn--submit {
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--max-primary-content);
    }
</style>
