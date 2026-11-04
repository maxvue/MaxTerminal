<template>
    <div class="prefs-appearance">
        <MaxAlert v-if="errorMsg" :message="errorMsg" severity="danger" class="prefs-appearance__error" />

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
                    <MaxButton type="button" label="Novo Tema" icon="lucide:plus" @click="onCreateThemeClick" />
                    <MaxInputText v-model="importPathInput" placeholder="Caminho de arquivo (.yaml/.json) ou pasta..." aria-label="Caminho para importar tema" class="prefs-appearance__input" />
                    <MaxButton type="button" label="Importar Arquivo" variant="outlined" :disabled="!importPathInput.trim()" @click="onImportFileClick" />
                    <MaxButton type="button" label="Importar Pasta" variant="outlined" :disabled="!importPathInput.trim()" @click="onImportDirClick" />
                </div>
            </div>

            <ThemeImportReport :report="themeStore.lastImportReport" @close="themeStore.lastImportReport = null" />

            <ThemePicker :themes="themeStore.themes" :active-theme-id="themeStore.appearance.activeThemeId" :preview-suspended="editorStore.isOpen || Boolean(pendingDeleteTheme)" @select="onSelectTheme" @edit="onEditTheme" @duplicate="onDuplicateTheme" @delete="onRequestDeleteTheme" @export="onExportTheme" />

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

        <ThemeEditorDialog @saved="onThemeSaved" />

        <MaxModal :visible="Boolean(pendingDeleteTheme)" title="Confirmar Exclusão de Tema" :no-button="true" width="30rem" :closable="!isDeleting" @update:visible="onDeleteModalVisibilityChange">
            <div v-if="pendingDeleteTheme" class="prefs-appearance__delete-confirm">
                <p class="prefs-appearance__delete-text">{{ deleteImpactSummary }}</p>
                <footer class="prefs-appearance__delete-footer">
                    <MaxButton type="button" label="Cancelar" variant="outlined" :disabled="isDeleting" @click="pendingDeleteTheme = null" />
                    <MaxButton type="button" label="Confirmar exclusão" severity="danger" :loading="isDeleting" :disabled="isDeleting" @click="onConfirmDeleteTheme" />
                </footer>
            </div>
        </MaxModal>
    </div>
</template>

<script setup lang="ts">
    import { computed, onMounted, ref } from 'vue';
    import {
        MaxAlert,
        MaxButton,
        MaxCodeBlock,
        MaxInputCheckbox,
        MaxInputText,
        MaxModal
    } from '@maxvue/max-components-ui';
    import ProjectThemeOverrides from '@/components/preferences/appearance/ProjectThemeOverrides.vue';
    import TerminalFontSettings from '@/components/preferences/appearance/TerminalFontSettings.vue';
    import ThemeEditorDialog from '@/components/preferences/appearance/ThemeEditorDialog.vue';
    import ThemeImportReport from '@/components/preferences/appearance/ThemeImportReport.vue';
    import ThemePicker from '@/components/preferences/appearance/ThemePicker.vue';
    import ThemePreview from '@/components/preferences/appearance/ThemePreview.vue';
    import { useIpcError } from '@/composables/useIpcError';
    import { useProjectsStore } from '@/stores/projects';
    import { useThemeStore } from '@/stores/theme';
    import { useThemeEditorStore } from '@/stores/themeEditor';
    import {
        DEFAULT_DARK_THEME_ID,
        type AppearanceSettingsPatch,
        type ResolvedTheme,
        type ThemeId,
        type ThemeScope,
        type ThemeSummary
    } from '@/types/theme';

    const themeStore = useThemeStore();
    const editorStore = useThemeEditorStore();
    const projectsStore = useProjectsStore();
    const ipcError = useIpcError();

    const importPathInput = ref('');
    const errorMsg = ref<string | null>(null);
    const exportedSnippet = ref<string | null>(null);
    const pendingDeleteTheme = ref<ThemeSummary | null>(null);
    const isDeleting = ref(false);
    const isDuplicating = ref(false);

    const deleteImpactSummary = computed(() => {
        const target = pendingDeleteTheme.value;
        if (!target) {
            return '';
        }
        const impacts: string[] = [];
        if (themeStore.appearance.activeThemeId === target.id) {
            impacts.push(
                'ele está ativo globalmente e será revertido para o tema padrão Max Navy/Teal'
            );
        }
        if (themeStore.appearance.preferredDarkThemeId === target.id) {
            impacts.push('é o tema escuro preferido do sistema');
        }
        if (themeStore.appearance.preferredLightThemeId === target.id) {
            impacts.push('é o tema claro preferido do sistema');
        }
        const affectedProjects = Object.values(
            themeStore.appearance.projectOverrides ?? {}
        ).filter((tid) => tid === target.id).length;
        if (affectedProjects > 0) {
            impacts.push(
                `${affectedProjects} projeto(s) com override próprio passarão a seguir o tema global`
            );
        }
        if (impacts.length === 0) {
            return `Deseja realmente excluir o tema "${target.name}"? Esta ação não pode ser desfeita.`;
        }
        return `Deseja excluir o tema "${target.name}"? Atenção: ${impacts.join('; ')}.`;
    });

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

    function onCreateThemeClick(): void {
        errorMsg.value = null;
        themeStore.cancelPreview();
        editorStore.openCreate(themeStore.effectiveResolved?.scheme ?? null);
    }

    async function onEditTheme(id: ThemeId): Promise<void> {
        errorMsg.value = null;
        themeStore.cancelPreview();
        const found = themeStore.themes.find((t) => t.id === id);
        if (found?.origin === 'builtin') {
            errorMsg.value = 'Temas nativos são somente leitura. Duplique o tema para editá-lo.';
            return;
        }
        try {
            const resolved = await themeStore.fetchAndCacheResolved(id);
            if (resolved) {
                editorStore.openEdit(resolved);
            }
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        }
    }

    async function onDuplicateTheme(id: ThemeId): Promise<void> {
        if (isDuplicating.value) {
            return;
        }
        isDuplicating.value = true;
        errorMsg.value = null;
        themeStore.cancelPreview();
        try {
            await themeStore.duplicateTheme(id);
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        } finally {
            isDuplicating.value = false;
        }
    }

    function onRequestDeleteTheme(id: ThemeId): void {
        errorMsg.value = null;
        themeStore.cancelPreview();
        const found = themeStore.themes.find((t) => t.id === id);
        if (found?.origin === 'builtin') {
            errorMsg.value = 'Temas nativos não podem ser excluídos.';
            return;
        }
        if (!found) {
            return;
        }
        pendingDeleteTheme.value = found;
    }

    function onDeleteModalVisibilityChange(visible: boolean): void {
        if (!visible && !isDeleting.value) {
            pendingDeleteTheme.value = null;
        }
    }

    async function onConfirmDeleteTheme(): Promise<void> {
        const target = pendingDeleteTheme.value;
        if (!target || isDeleting.value) {
            return;
        }
        isDeleting.value = true;
        errorMsg.value = null;
        try {
            await themeStore.deleteTheme(target.id, DEFAULT_DARK_THEME_ID);
            pendingDeleteTheme.value = null;
        } catch (err: unknown) {
            errorMsg.value = ipcError.message(err);
        } finally {
            isDeleting.value = false;
        }
    }

    async function onThemeSaved(resolved: ResolvedTheme): Promise<void> {
        errorMsg.value = null;
        themeStore.applySavedThemeFromEditor(resolved);
        try {
            await themeStore.refreshCatalog();
        } catch (err: unknown) {
            errorMsg.value = `Tema salvo com sucesso, mas ocorreu uma falha ao atualizar o catálogo: ${ipcError.message(err)}`;
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
        const followSystem =
            typeof val === 'boolean'
                ? val
                : Boolean((val as { target?: HTMLInputElement })?.target?.checked);
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

    .prefs-appearance__delete-confirm {
        display: flex;
        flex-direction: column;
        gap: 0.5rem;
    }

    .prefs-appearance__delete-text {
        margin: 0;
        font-size: 0.8125rem;
        color: var(--mt-ui-text, var(--background-800));
    }

    .prefs-appearance__delete-footer {
        display: flex;
        align-items: center;
        justify-content: flex-end;
        gap: 0.5rem;
    }
</style>
