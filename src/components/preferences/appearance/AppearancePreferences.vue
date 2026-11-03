<template>
    <div class="prefs-appearance">
        <MaxAlert v-if="errorMsg" :message="errorMsg" severity="error" class="prefs-appearance__error" />

        <section class="prefs-appearance__group">
            <div class="prefs-appearance__header">
                <h3 class="prefs-appearance__heading">Pré-visualização ao Vivo</h3>
                <MaxInputCheckbox :model-value="themeStore.appearance.followSystem" label="Sincronizar modo claro/escuro com o sistema operacional" @update:model-value="onFollowSystemToggle" />
            </div>
            <ThemePreview :resolved="themeStore.effectiveResolved" :appearance="themeStore.appearance" />
        </section>

        <section class="prefs-appearance__group">
            <div class="prefs-appearance__header">
                <h3 class="prefs-appearance__heading">Catálogo de Temas e Importador Tabby</h3>
                <div class="prefs-appearance__import-bar">
                    <MaxInputText v-model="importPathInput" placeholder="Caminho de arquivo (.yaml/.json) ou pasta..." aria-label="Caminho para importar tema" class="prefs-appearance__input" />
                    <MaxButton type="button" label="Importar Arquivo" :disabled="!importPathInput.trim()" @click="onImportFileClick" />
                    <MaxButton type="button" label="Importar Pasta" variant="outlined" :disabled="!importPathInput.trim()" @click="onImportDirClick" />
                </div>
            </div>

            <ThemeImportReport :report="themeStore.lastImportReport" @close="themeStore.lastImportReport = null" />

            <ThemePicker :themes="themeStore.themes" :active-theme-id="themeStore.appearance.activeThemeId" @select="onSelectTheme" @delete="onDeleteTheme" @export="onExportTheme" />

            <div v-if="exportedSnippet" class="prefs-appearance__exported">
                <div class="prefs-appearance__exported-header">
                    <span>YAML Exportado (Tabby)</span>
                    <MaxButton type="button" label="Fechar" variant="outlined" size="small" @click="exportedSnippet = null" />
                </div>
                <MaxCodeBlock :code="exportedSnippet" language="yaml" aria-label="Conteúdo YAML exportado" />
            </div>
        </section>

        <section class="prefs-appearance__group">
            <h3 class="prefs-appearance__heading">Tipografia e Cursor do Terminal</h3>
            <TerminalFontSettings :appearance="themeStore.appearance" @update="onAppearanceUpdate" />
        </section>

        <section class="prefs-appearance__group">
            <h3 class="prefs-appearance__heading">Temas por Projeto</h3>
            <ProjectThemeOverrides :projects="projectsStore.projects" :themes="themeStore.themes" :overrides="themeStore.appearance.projectOverrides" @change="onProjectOverrideChange" />
        </section>
    </div>
</template>

<script setup lang="ts">
    import { onMounted, ref } from 'vue';
    import { MaxAlert, MaxButton, MaxCodeBlock, MaxInputCheckbox, MaxInputText } from '@maxvue/max-components-ui';
    import ProjectThemeOverrides from '@/components/preferences/appearance/ProjectThemeOverrides.vue';
    import TerminalFontSettings from '@/components/preferences/appearance/TerminalFontSettings.vue';
    import ThemeImportReport from '@/components/preferences/appearance/ThemeImportReport.vue';
    import ThemePicker from '@/components/preferences/appearance/ThemePicker.vue';
    import ThemePreview from '@/components/preferences/appearance/ThemePreview.vue';
    import { useIpcError } from '@/composables/useIpcError';
    import { useProjectsStore } from '@/stores/projects';
    import { useThemeStore } from '@/stores/theme';
    import type {
        AppearanceSettingsPatch,
        ThemeId,
        ThemeScope
    } from '@/types/theme';

    const themeStore = useThemeStore();
    const projectsStore = useProjectsStore();
    const ipcError = useIpcError();

    const importPathInput = ref('');
    const errorMsg = ref<string | null>(null);
    const exportedSnippet = ref<string | null>(null);

    onMounted(() => {
        if (!themeStore.initialized) {
            void themeStore.init().catch((err: unknown) => {
                errorMsg.value = ipcError.message(err);
            });
        }
    });

    async function onSelectTheme(id: ThemeId): Promise<void> {
        errorMsg.value = null;
        try {
            await themeStore.setActiveTheme(id, { kind: 'global' });
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onDeleteTheme(id: ThemeId): Promise<void> {
        errorMsg.value = null;
        try {
            await themeStore.deleteTheme(id);
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onExportTheme(id: ThemeId): Promise<void> {
        errorMsg.value = null;
        try {
            exportedSnippet.value = await themeStore.exportTheme(id, 'tabby_yaml');
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onFollowSystemToggle(val: unknown): Promise<void> {
        const followSystem = typeof val === 'boolean' ? val : Boolean((val as { target?: HTMLInputElement })?.target?.checked);
        errorMsg.value = null;
        try {
            await themeStore.updateAppearance({ followSystem });
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onAppearanceUpdate(patch: AppearanceSettingsPatch): Promise<void> {
        errorMsg.value = null;
        try {
            await themeStore.updateAppearance(patch);
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onProjectOverrideChange(
        id: ThemeId,
        scope: ThemeScope
    ): Promise<void> {
        errorMsg.value = null;
        try {
            await themeStore.setActiveTheme(id, scope);
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onImportFileClick(): Promise<void> {
        const path = importPathInput.value.trim();
        if (!path) {
            return;
        }
        errorMsg.value = null;
        try {
            await themeStore.importFile(path, {
                activateIfSingle: false,
                dryRun: false
            });
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onImportDirClick(): Promise<void> {
        const dirPath = importPathInput.value.trim();
        if (!dirPath) {
            return;
        }
        errorMsg.value = null;
        try {
            await themeStore.importDir(dirPath, {
                dryRun: false
            });
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }
</script>

<style lang="scss" scoped>
    .prefs-appearance {
        display: flex;
        flex-direction: column;
        gap: 1.25rem;
    }

    .prefs-appearance__error {
        padding: 0.625rem 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-danger-500);
        background: color-mix(in srgb, var(--max-danger-500) 14%, transparent);
        color: var(--max-danger-400);
        font-size: 0.75rem;
    }

    .prefs-appearance__group {
        display: flex;
        flex-direction: column;
        gap: 0.75rem;
        padding: 0.875rem;
        border-radius: 0.5rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface-panel, var(--background-25));
    }

    .prefs-appearance__header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        gap: 0.75rem;
        flex-wrap: wrap;
    }

    .prefs-appearance__heading {
        margin: 0;
        font-size: 0.875rem;
        font-weight: 600;
        color: var(--mt-ui-text-strong, var(--background-950));
    }

    .prefs-appearance__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.75rem;
        color: var(--mt-ui-text-secondary, var(--background-600));
        cursor: pointer;

        input {
            accent-color: var(--max-primary-500);
        }
    }

    .prefs-appearance__import-bar {
        display: flex;
        align-items: center;
        gap: 0.375rem;
        flex-wrap: wrap;
    }

    .prefs-appearance__input {
        height: 2rem;
        min-width: 15rem;
        padding: 0 0.625rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-size: 0.75rem;
    }

    .prefs-appearance__btn {
        height: 2rem;
        padding: 0 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--max-primary-content, #fff);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }

        &--secondary {
            border-color: var(--mt-ui-border, var(--background-100));
            background: var(--mt-ui-surface-elevated, var(--background-50));
            color: var(--mt-ui-text, var(--background-800));
        }
    }

    .prefs-appearance__exported {
        display: flex;
        flex-direction: column;
        gap: 0.375rem;
    }

    .prefs-appearance__exported-header {
        display: flex;
        align-items: center;
        justify-content: space-between;
        font-size: 0.75rem;
        font-weight: 600;
        color: var(--mt-ui-text-secondary, var(--background-600));
    }

    .prefs-appearance__textarea {
        width: 100%;
        padding: 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--mt-ui-border, var(--background-100));
        background: var(--mt-ui-surface, var(--background-0));
        color: var(--mt-ui-text, var(--background-800));
        font-family: var(--mt-font-mono);
        font-size: 0.75rem;
        resize: vertical;
    }
</style>
