/**
 * Tipos e contratos de domínio de Temas e Aparência do MaxTerminal (Fase 10).
 */
export type {
    AnsiPalette,
    AppearanceSettings,
    AppearanceSettingsPatch,
    BackgroundRamp,
    ColorScale11,
    ColorScheme,
    CursorStyle,
    ExportFormat,
    ImportDirOptions,
    ImportFailure,
    ImportFileOptions,
    ImportReport,
    ImportWarning,
    LibraryChangeReason,
    MaxUiTokens,
    OverwritePolicy,
    ResolvedTheme,
    SchemeFileFormat,
    SkipReason,
    SkippedEntry,
    TerminalUiTokens,
    ThemeChangedEvent,
    ThemeError,
    ThemeId,
    ThemeLibraryChangedEvent,
    ThemeMetadata,
    ThemeOrigin,
    ThemePolarity,
    ThemeScope,
    ThemeSummary,
    WarningCode,
    XtermThemePayload
} from '@/types/bindings';
import type { AnsiPalette } from '@/types/bindings';

/** Contexto suplementar opcional para geração de variáveis CSS (ex.: paleta ANSI do tema). */
export interface CssVarsContext {
    scheme?: {
        ansi?: AnsiPalette;
    };
    ansi?: AnsiPalette;
}

/** ID canônico do tema escuro padrão (Max Navy/Teal). */
export const DEFAULT_DARK_THEME_ID = 'max-navy-teal' as const;

/** ID canônico do tema claro padrão (Max Light). */
export const DEFAULT_LIGHT_THEME_ID = 'max-light' as const;

/** Chave do `localStorage` para bootstrap síncrono sem FOUC. */
export const BOOTSTRAP_STORAGE_KEY = 'maxterminal.theme.bootstrap.v2' as const;

/** Estrutura mínima persistida no `localStorage` para hidratação síncrona antes de `app.mount()`. */
export interface ThemeBootstrapSnapshot {
    version: 2;
    themeId: string;
    polarity: 'dark' | 'light';
    cssVars: Record<string, string>;
}
