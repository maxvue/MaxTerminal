<template>
    <MaxModal v-if="targetProject" :visible="isOpen" :title="dialogTitle" :sub-title="targetProject.path" :no-button="true" max-width="44rem" class="project-settings-dialog" :dismissable="!isBusy" :close-on-escape="!isBusy && !childDropdownOpen" @before-close="handleBeforeClose" @update:visible="handleVisibleUpdate">
        <div class="project-settings-dialog__body">
            <section class="project-settings-dialog__section" aria-labelledby="proj-settings-identity-heading">
                <header class="project-settings-dialog__section-header">
                    <h3 id="proj-settings-identity-heading" class="project-settings-dialog__section-title">Identidade visual</h3>
                    <p class="project-settings-dialog__section-desc">Personalize o título, ícone e cor de identificação do projeto.</p>
                </header>
                <ProjectIdentityForm :project="targetProject" :active="isOpen" @saved="handleIdentitySaved" @cancel="handleCancel" @busy-change="identityBusy = $event" />
            </section>

            <hr class="project-settings-dialog__divider">

            <section class="project-settings-dialog__section" aria-labelledby="proj-settings-appearance-heading">
                <header class="project-settings-dialog__section-header">
                    <h3 id="proj-settings-appearance-heading" class="project-settings-dialog__section-title">Aparência & Esquema de cores</h3>
                    <p class="project-settings-dialog__section-desc">Defina um tema específico para este projeto ou herde as configurações globais de aparência.</p>
                </header>
                <div v-if="themeError" class="project-settings-dialog__theme-error" role="alert">
                    {{ themeError }}
                </div>
                <ProjectThemeOverrides :projects="[targetProject]" :themes="themeStore.themes" :overrides="themeStore.appearance.projectOverrides" :disabled="themeBusy" @dropdown-change="childDropdownOpen = $event" @change="handleThemeChange" />
            </section>
        </div>
    </MaxModal>
</template>

<script setup lang="ts">
    import { computed, ref, watch } from 'vue';
    import MaxModal from '@maxvue/max-components-ui/components/MaxModal';
    import ProjectIdentityForm from '@/components/projects/ProjectIdentityForm.vue';
    import ProjectThemeOverrides from '@/components/preferences/appearance/ProjectThemeOverrides.vue';
    import { useIpcError } from '@/composables/useIpcError';
    import { useProjectsStore } from '@/stores/projects';
    import { useThemeStore } from '@/stores/theme';
    import { useWindowStore } from '@/stores/window';
    import type { Project_Serialize } from '@/types/bindings';
    import type { ThemeId, ThemeScope } from '@/types/theme';

    const windowStore = useWindowStore();
    const projectsStore = useProjectsStore();
    const themeStore = useThemeStore();
    const ipcError = useIpcError();

    const identityBusy = ref<boolean>(false);
    const themeBusy = ref<boolean>(false);
    const themeError = ref<string | null>(null);
    const childDropdownOpen = ref<boolean>(false);

    const isBusy = computed<boolean>(() => identityBusy.value || themeBusy.value);

    function handleBeforeClose(event: { preventDefault: () => void }): void {
        if (isBusy.value || childDropdownOpen.value) {
            event.preventDefault();
        }
    }

    const targetProject = computed<Project_Serialize | null>(() => {
        const id = windowStore.projectSettingsProjectId;
        if (!id) {
            return null;
        }
        return projectsStore.projectById(id) ?? null;
    });

    const isOpen = computed<boolean>(() => windowStore.projectSettingsProjectId !== null && targetProject.value !== null);

    const dialogTitle = computed<string>(() => {
        if (!targetProject.value) {
            return 'Configurações do projeto';
        }
        return `Configurações do projeto — ${targetProject.value.title}`;
    });

    watch(
        () => [windowStore.projectSettingsProjectId, projectsStore.projects] as const,
        ([currentId]) => {
            if (currentId && !projectsStore.projectById(currentId)) {
                windowStore.closeProjectSettings();
            }
        }
    );

    function handleVisibleUpdate(val: boolean): void {
        if (!val && !isBusy.value) {
            windowStore.closeProjectSettings();
        }
    }

    function handleCancel(): void {
        if (!isBusy.value) {
            windowStore.closeProjectSettings();
        }
    }

    function handleIdentitySaved(_updated: Project_Serialize): void {
        // Identidade salva com sucesso; mantém o painel aberto com feedback visível no formulário.
    }

    async function handleThemeChange(id: ThemeId, scope: ThemeScope): Promise<void> {
        const capturedId = targetProject.value?.id;
        if (!capturedId || themeBusy.value) {
            return;
        }
        themeBusy.value = true;
        themeError.value = null;
        try {
            await themeStore.setActiveTheme(id, scope);
        } catch (err: unknown) {
            if (targetProject.value?.id === capturedId) {
                themeError.value = ipcError.message(err);
            }
        } finally {
            themeBusy.value = false;
        }
    }
</script>

<style lang="scss" scoped>
    :deep(.project-settings-dialog.max-modal) {
        background-color: var(--background-900);
        border-color: var(--background-700);
        color: var(--background-100);
        max-height: 85vh;
        display: flex;
        flex-direction: column;

        .max-modal-content {
            padding: 1.25rem;
            flex: 1;
            min-height: 0;
            overflow-y: auto;
        }
    }

    .project-settings-dialog__body {
        display: flex;
        flex-direction: column;
        gap: 1.25rem;
    }

    .project-settings-dialog__section {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
    }

    .project-settings-dialog__section-header {
        display: flex;
        flex-direction: column;
        gap: 0.25rem;
    }

    .project-settings-dialog__section-title {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--background-100);
    }

    .project-settings-dialog__section-desc {
        margin: 0;
        font-size: 0.75rem;
        color: var(--background-400);
    }

    .project-settings-dialog__divider {
        margin: 0.25rem 0;
        border: none;
        border-top: 1px solid var(--background-750, var(--background-700));
    }

    .project-settings-dialog__theme-error {
        padding: 0.5rem 0.75rem;
        border-radius: 0.375rem;
        background: color-mix(in srgb, var(--max-danger-500) 15%, var(--background-900));
        border: 1px solid var(--max-danger-500);
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }
</style>
