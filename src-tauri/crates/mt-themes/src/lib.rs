//! Crate `mt-themes` — Motor de Temas, Conversão OKLCH/WCAG, Mapeamento Semântico `MaxComponentsUi`
//! e Importador/Exportador Tabby Terminal para o `MaxTerminal`.

pub mod builtin;
pub mod cache;
pub mod color;
pub mod contrast;
pub mod error;
pub mod export;
pub mod formats;
pub mod ids;
pub mod import;
pub mod mapping;
pub mod normalize;
pub mod oklch;
pub mod scale;
pub mod scheme;
pub mod service;

pub use builtin::{
    BUILTIN_THEME_IDS, builtin_resolved_themes, builtin_schemes, find_builtin_resolved,
    is_builtin_theme_id,
};
pub use cache::ThemeCache;
pub use color::{HexColor, RgbaColor, parse_color_with_bg};
pub use contrast::{
    MIN_FG_BG_CONTRAST, POLARITY_LUMINANCE_THRESHOLD, RECOMMENDED_FG_BG_CONTRAST,
    classify_polarity, contrast_ratio, ensure_min_contrast, pick_content_color, relative_luminance,
};
pub use error::ThemeError;
pub use export::{ExportFormat, export_scheme};
pub use formats::{RawColorScheme, SchemeFileFormat, parse_schemes_by_format, validate_safe_yaml};
pub use ids::{ThemeId, ThemeOrigin, ThemePolarity};
pub use import::{
    ImportDirOptions, ImportFailure, ImportFileOptions, ImportReport, MAX_DIR_DEPTH,
    MAX_FILES_PER_DIR_IMPORT, MAX_SCHEMES_PER_FILE, MAX_THEME_FILE_BYTES, OverwritePolicy,
    SkipReason, SkippedEntry, ThemeSummary,
};
pub use mapping::{
    ResolvedTheme,
    tokens::{MaxUiTokens, TerminalUiTokens, derive_max_ui_tokens},
    xterm::{XtermThemePayload, derive_xterm_theme},
};
pub use normalize::{
    ImportWarning, NormalizedSchemeOutput, WarningCode, normalize_draft_input, normalize_raw_scheme,
};
pub use oklch::{Oklch, hsl_to_srgb, linear_to_srgb_u8, srgb_u8_to_linear};
pub use scale::{BackgroundRamp, ColorScale11, generate_background_ramp, generate_scale};
pub use scheme::{
    AnsiPalette, AnsiPaletteInput, ColorScheme, CursorStyle, ThemeDraftInput, ThemeMetadata,
};
pub use service::{
    AppearanceSettings, AppearanceSettingsPatch, LibraryChangeReason, NoopThemeEventSink,
    ThemeChangedEvent, ThemeEventSink, ThemeLibraryChangedEvent, ThemeScope, ThemeService,
    validate_font_family,
};
