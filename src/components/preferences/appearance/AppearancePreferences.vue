<template>
    <div class="prefs-appearance">
        <div v-if="errorMsg" class="prefs-appearance__error" role="alert">
            {{ errorMsg }}
        </div>

        <section class="prefs-appearance__group">
            <div class="prefs-appearance__header">
                <h3 class="prefs-appearance__heading">Pré-visualização ao Vivo</h3>
                <label class="prefs-appearance__checkbox">
                    <input type="checkbox" :checked="themeStore.appearance.followSystem" @change="onFollowSystemToggle">
                    <span>Sincronizar modo claro/escuro com o sistema operacional</span>
                </label>
            </div>
            <ThemePreview :resolved="themeStore.effectiveResolved" :appearance="themeStore.appearance" />
        </section>

        <section class="prefs-appearance__group">
            <div class="prefs-appearance__header">
                <h3 class="prefs-appearance__heading">Catálogo de Temas e Importador Tabby</h3>
                <div class="prefs-appearance__import-bar">
                    <input v-model="importPathInput" type="text" class="prefs-appearance__input" placeholder="Caminho de arquivo (.yaml/.json) ou pasta..." aria-label="Caminho para importar tema">
                    <button type="button" class="prefs-appearance__btn" :disabled="!importPathInput.trim()" @click="onImportFileClick">
                        Importar Arquivo
                    </button>
                    <button type="button" class="prefs-appearance__btn prefs-appearance__btn--secondary" :disabled="!importPathInput.trim()" @click="onImportDirClick">
                        Importar Pasta
                    </button>
                </div>
            </div>

            <ThemeImportReport :report="themeStore.lastImportReport" @close="themeStore.lastImportReport = null" />

            <ThemePicker :themes="themeStore.themes" :active-theme-id="themeStore.appearance.activeThemeId" @select="onSelectTheme" @delete="onDeleteTheme" @export="onExportTheme" />

            <div v-if="exportedSnippet" class="prefs-appearance__exported">
                <div class="prefs-appearance__exported-header">
                    <span>YAML Exportado (Tabby)</span>
                    <button type="button" class="prefs-appearance__btn prefs-appearance__btn--secondary" @click="exportedSnippet = null">
                        Fechar
                    </button>
                </div>
                <textarea class="prefs-appearance__textarea" readonly :value="exportedSnippet" rows="6" aria-label="Conteúdo YAML exportado" />
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

    async function onFollowSystemToggle(ev: Event): Promise<void> {
        const followSystem = (ev.target as HTMLInputElement).checked;
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
        border: 1px solid var(--background-800);
        background: var(--background-900);
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
        color: var(--background-50);
    }

    .prefs-appearance__checkbox {
        display: inline-flex;
        align-items: center;
        gap: 0.5rem;
        font-size: 0.75rem;
        color: var(--background-200);
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
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-100);
        font-size: 0.75rem;
    }

    .prefs-appearance__btn {
        height: 2rem;
        padding: 0 0.75rem;
        border-radius: 0.375rem;
        border: 1px solid var(--max-primary-500);
        background: var(--max-primary-600);
        color: var(--background-50);
        font-size: 0.75rem;
        font-weight: 600;
        cursor: pointer;

        &:disabled {
            opacity: 0.45;
            cursor: not-allowed;
        }

        &--secondary {
            border-color: var(--background-700);
            background: var(--background-800);
            color: var(--background-100);
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
        color: var(--background-200);
    }

    .prefs-appearance__textarea {
        width: 100%;
        padding: 0.5rem;
        border-radius: 0.375rem;
        border: 1px solid var(--background-700);
        background: var(--background-950);
        color: var(--background-100);
        font-family: var(--mt-font-mono);
        font-size: 0.75rem;
        resize: vertical;
    }
</style>
