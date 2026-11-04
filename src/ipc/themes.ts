import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type AppearanceSettings,
    type AppearanceSettingsPatch,
    type ExportFormat,
    type ImportDirOptions,
    type ImportFileOptions,
    type ImportReport,
    type ResolvedTheme,
    type ThemeChangedEvent,
    type ThemeDraftInput,
    type ThemeId,
    type ThemeLibraryChangedEvent,
    type ThemeScope,
    type ThemeSummary
} from '@/types/bindings';

/** Lista o resumo de todos os temas disponíveis (`theme_list`). */
export function themeList(): Promise<ThemeSummary[]> {
    return unwrap(commands.themeList(), 'theme_list');
}

/** Obtém um `ResolvedTheme` completo pelo ID (`theme_get`). */
export function themeGet(id: ThemeId): Promise<ResolvedTheme> {
    return unwrap(commands.themeGet(id), 'theme_get');
}

/** Obtém as configurações globais de aparência e overrides por projeto (`theme_appearance_get`). */
export function themeAppearanceGet(): Promise<AppearanceSettings> {
    return unwrap(commands.themeAppearanceGet(), 'theme_appearance_get');
}

/** Atualiza parcialmente as configurações de aparência (`theme_appearance_update`). */
export function themeAppearanceUpdate(
    patch: AppearanceSettingsPatch
): Promise<AppearanceSettings> {
    return unwrap(commands.themeAppearanceUpdate(patch), 'theme_appearance_update');
}

/** Importa um arquivo de esquema de cores (`theme_import_file`). */
export function themeImportFile(
    path: string,
    options: ImportFileOptions
): Promise<ImportReport> {
    return unwrap(commands.themeImportFile(path, options), 'theme_import_file');
}

/** Importa todos os esquemas válidos de um diretório (`theme_import_dir`). */
export function themeImportDir(
    dirPath: string,
    options: ImportDirOptions
): Promise<ImportReport> {
    return unwrap(commands.themeImportDir(dirPath, options), 'theme_import_dir');
}

/** Define o tema ativo globalmente ou para um projeto (`theme_set_active`). */
export function themeSetActive(
    id: ThemeId,
    scope: ThemeScope
): Promise<ResolvedTheme> {
    return unwrap(commands.themeSetActive(id, scope), 'theme_set_active');
}

/** Remove um tema importado/customizado (`theme_delete`). */
export function themeDelete(
    id: ThemeId,
    fallbackId: ThemeId | null = null
): Promise<null> {
    return unwrap(commands.themeDelete(id, fallbackId), 'theme_delete');
}

/** Exporta um tema no formato solicitado (`theme_export`). */
export function themeExport(
    id: ThemeId,
    format: ExportFormat
): Promise<string> {
    return unwrap(commands.themeExport(id, format), 'theme_export');
}

/** Cria um novo tema customizado (`theme_create`). */
export function themeCreate(input: ThemeDraftInput): Promise<ResolvedTheme> {
    return unwrap(commands.themeCreate(input), 'theme_create');
}

/** Atualiza um tema customizado/importado existente (`theme_update`). */
export function themeUpdate(
    id: ThemeId,
    input: ThemeDraftInput
): Promise<ResolvedTheme> {
    return unwrap(commands.themeUpdate(id, input), 'theme_update');
}

/** Duplica qualquer tema existente (`theme_duplicate`). */
export function themeDuplicate(id: ThemeId): Promise<ResolvedTheme> {
    return unwrap(commands.themeDuplicate(id), 'theme_duplicate');
}

/** Escuta o evento `theme://changed`. */
export function onThemeChanged(
    handler: (payload: ThemeChangedEvent) => void
): Promise<UnlistenFn> {
    return listen<ThemeChangedEvent>('theme://changed', (ev) => {
        handler(ev.payload);
    });
}

/** Escuta o evento `theme://library-changed`. */
export function onThemeLibraryChanged(
    handler: (payload: ThemeLibraryChangedEvent) => void
): Promise<UnlistenFn> {
    return listen<ThemeLibraryChangedEvent>('theme://library-changed', (ev) => {
        handler(ev.payload);
    });
}
