import { computed, ref } from 'vue';
import { defineStore } from 'pinia';
import type { UnlistenFn } from '@tauri-apps/api/event';
import {
    onThemeChanged,
    onThemeLibraryChanged,
    themeAppearanceGet,
    themeAppearanceUpdate,
    themeDelete,
    themeExport,
    themeGet,
    themeImportDir,
    themeImportFile,
    themeList,
    themeSetActive
} from '@/ipc/themes';
import { saveThemeBootstrapSnapshot } from '@/theme/bootstrap';
import { applyMaxUiTokensToRoot } from '@/theme/cssTokens';
import { applyThemeAndTypographyToTerminals } from '@/theme/terminalBridge';
import {
    DEFAULT_DARK_THEME_ID,
    DEFAULT_LIGHT_THEME_ID,
    type AppearanceSettings,
    type AppearanceSettingsPatch,
    type ExportFormat,
    type ImportDirOptions,
    type ImportFileOptions,
    type ImportReport,
    type ResolvedTheme,
    type ThemeId,
    type ThemeScope,
    type ThemeSummary
} from '@/types/theme';

const DEFAULT_APPEARANCE: AppearanceSettings = {
    activeThemeId: DEFAULT_DARK_THEME_ID,
    followSystem: false,
    preferredDarkThemeId: DEFAULT_DARK_THEME_ID,
    preferredLightThemeId: DEFAULT_LIGHT_THEME_ID,
    fontFamily: 'JetBrains Mono, Fira Code, Cascadia Code, monospace',
    fontSize: 13,
    lineHeight: 1.2,
    fontLigatures: true,
    cursorBlink: true,
    cursorStyleOverride: null,
    minimumContrastRatio: 1.0,
    projectOverrides: {}
};

export const useThemeStore = defineStore('theme', () => {
    const themes = ref<ThemeSummary[]>([]);
    const appearance = ref<AppearanceSettings>({ ...DEFAULT_APPEARANCE });
    const activeResolved = ref<ResolvedTheme | null>(null);
    const previewResolved = ref<ResolvedTheme | null>(null);
    const currentProjectId = ref<string | null>(null);
    const loading = ref(false);
    const initialized = ref(false);
    const lastImportReport = ref<ImportReport | null>(null);

    const resolvedCache = new Map<string, ResolvedTheme>();
    let unlistenChanged: UnlistenFn | null = null;
    let unlistenLibrary: UnlistenFn | null = null;

    const effectiveThemeId = computed<string>(() => {
        if (previewResolved.value?.scheme?.id) {
            return previewResolved.value.scheme.id;
        }
        const pid = currentProjectId.value;
        const overrides = appearance.value?.projectOverrides;
        if (pid && overrides && overrides[pid]) {
            return overrides[pid] as string;
        }
        return appearance.value?.activeThemeId ?? DEFAULT_DARK_THEME_ID;
    });

    const effectiveResolved = computed<ResolvedTheme | null>(
        () => previewResolved.value ?? activeResolved.value
    );

    const builtinThemes = computed(() =>
        themes.value.filter((t) => t.origin === 'builtin')
    );

    const customThemes = computed(() =>
        themes.value.filter((t) => t.origin !== 'builtin')
    );

    function applyResolvedToUiAndTerminals(
        resolved: ResolvedTheme,
        persistBootstrap: boolean
    ): void {
        const context = { scheme: resolved.scheme };
        applyMaxUiTokensToRoot(resolved.uiTokens, null, context);
        applyThemeAndTypographyToTerminals(resolved, appearance.value);
        if (persistBootstrap) {
            saveThemeBootstrapSnapshot(resolved.scheme.id, resolved.uiTokens, context);
        }
    }

    async function fetchAndCacheResolved(id: ThemeId): Promise<ResolvedTheme | null> {
        const cached = resolvedCache.get(id);
        if (cached) {
            return cached;
        }
        const resolved = await themeGet(id);
        if (!resolved || !resolved.scheme || !resolved.uiTokens) {
            return null;
        }
        resolvedCache.set(id, resolved);
        return resolved;
    }

    async function syncEffectiveTheme(persistBootstrap = true): Promise<void> {
        const targetId = effectiveThemeId.value;
        try {
            const resolved = await fetchAndCacheResolved(targetId);
            if (!resolved) {
                return;
            }
            activeResolved.value = resolved;
            if (!previewResolved.value) {
                applyResolvedToUiAndTerminals(resolved, persistBootstrap);
            }
        } catch {
            if (targetId !== DEFAULT_DARK_THEME_ID) {
                try {
                    const fallback = await fetchAndCacheResolved(DEFAULT_DARK_THEME_ID);
                    if (fallback) {
                        activeResolved.value = fallback;
                        if (!previewResolved.value) {
                            applyResolvedToUiAndTerminals(fallback, persistBootstrap);
                        }
                    }
                } catch {
                    // Mantém tokens de bootstrap quando IPC não está disponível
                }
            }
        }
    }

    async function init(): Promise<void> {
        if (initialized.value) {
            return;
        }
        loading.value = true;
        try {
            const [list, appSettings] = await Promise.all([
                themeList().catch(() => [] as ThemeSummary[]),
                themeAppearanceGet().catch(() => null)
            ]);
            themes.value = Array.isArray(list) ? list : [];
            if (appSettings && typeof appSettings === 'object' && 'activeThemeId' in appSettings) {
                appearance.value = appSettings;
            }
            await syncEffectiveTheme(true);

            unlistenChanged = await onThemeChanged((ev) => {
                if (!ev?.resolved?.scheme) {
                    return;
                }
                resolvedCache.set(ev.resolved.scheme.id, ev.resolved);
                if (ev.appearance) {
                    appearance.value = ev.appearance;
                }
                activeResolved.value = ev.resolved;
                if (!previewResolved.value) {
                    applyResolvedToUiAndTerminals(ev.resolved, true);
                }
            }).catch(() => null);

            unlistenLibrary = await onThemeLibraryChanged((ev) => {
                if (Array.isArray(ev?.themes)) {
                    themes.value = ev.themes;
                }
                resolvedCache.clear();
            }).catch(() => null);

            initialized.value = true;
        } finally {
            loading.value = false;
        }
    }

    function dispose(): void {
        if (unlistenChanged) {
            unlistenChanged();
            unlistenChanged = null;
        }
        if (unlistenLibrary) {
            unlistenLibrary();
            unlistenLibrary = null;
        }
        initialized.value = false;
    }

    async function setActiveTheme(
        id: ThemeId,
        scope: ThemeScope = { kind: 'global' }
    ): Promise<ResolvedTheme> {
        previewResolved.value = null;
        const resolved = await themeSetActive(id, scope);
        resolvedCache.set(resolved.scheme.id, resolved);
        activeResolved.value = resolved;
        if (scope.kind === 'global') {
            appearance.value = {
                ...appearance.value,
                activeThemeId: id
            };
        } else if (scope.kind === 'project') {
            appearance.value = {
                ...appearance.value,
                projectOverrides: {
                    ...appearance.value.projectOverrides,
                    [scope.projectId]: id
                }
            };
        } else if (scope.kind === 'clear_project') {
            const nextOverrides = { ...appearance.value.projectOverrides };
            delete nextOverrides[scope.projectId];
            appearance.value = {
                ...appearance.value,
                projectOverrides: nextOverrides
            };
        }
        applyResolvedToUiAndTerminals(resolved, scope.kind === 'global');
        return resolved;
    }

    async function updateAppearance(
        patch: AppearanceSettingsPatch
    ): Promise<AppearanceSettings> {
        const updated = await themeAppearanceUpdate(patch);
        appearance.value = updated;
        if (activeResolved.value) {
            applyResolvedToUiAndTerminals(activeResolved.value, true);
        }
        return updated;
    }

    async function setCurrentProjectContext(projectId: string | null): Promise<void> {
        if (currentProjectId.value === projectId) {
            return;
        }
        currentProjectId.value = projectId;
        if (initialized.value) {
            await syncEffectiveTheme(projectId === null);
        }
    }

    async function startPreview(id: ThemeId): Promise<void> {
        const resolved = await fetchAndCacheResolved(id);
        if (!resolved) {
            return;
        }
        previewResolved.value = resolved;
        applyResolvedToUiAndTerminals(resolved, false);
    }

    function cancelPreview(): void {
        if (!previewResolved.value) {
            return;
        }
        previewResolved.value = null;
        if (activeResolved.value) {
            applyResolvedToUiAndTerminals(activeResolved.value, false);
        }
    }

    async function importFile(
        path: string,
        options: ImportFileOptions
    ): Promise<ImportReport> {
        const report = await themeImportFile(path, options);
        lastImportReport.value = report;
        if (!options.dryRun && report.imported.length > 0) {
            themes.value = await themeList();
        }
        return report;
    }

    async function importDir(
        dirPath: string,
        options: ImportDirOptions
    ): Promise<ImportReport> {
        const report = await themeImportDir(dirPath, options);
        lastImportReport.value = report;
        if (!options.dryRun && report.imported.length > 0) {
            themes.value = await themeList();
        }
        return report;
    }

    async function deleteTheme(
        id: ThemeId,
        fallbackId: ThemeId | null = null
    ): Promise<void> {
        await themeDelete(id, fallbackId);
        resolvedCache.delete(id);
        themes.value = await themeList();
        appearance.value = await themeAppearanceGet();
        await syncEffectiveTheme(true);
    }

    async function exportTheme(
        id: ThemeId,
        format: ExportFormat
    ): Promise<string> {
        return themeExport(id, format);
    }

    return {
        themes,
        builtinThemes,
        customThemes,
        appearance,
        activeResolved,
        previewResolved,
        effectiveThemeId,
        effectiveResolved,
        currentProjectId,
        loading,
        initialized,
        lastImportReport,
        init,
        dispose,
        setActiveTheme,
        updateAppearance,
        setCurrentProjectContext,
        startPreview,
        cancelPreview,
        importFile,
        importDir,
        deleteTheme,
        exportTheme,
        fetchAndCacheResolved
    };
});
