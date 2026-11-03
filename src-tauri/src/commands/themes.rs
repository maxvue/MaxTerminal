//! Comandos IPC Tauri (`theme_*`) da Fase 10.

use std::path::PathBuf;
use std::sync::Arc;

use mt_core::MtError;
use mt_themes::{
    AppearanceSettings, AppearanceSettingsPatch, ExportFormat, ImportDirOptions, ImportFileOptions,
    ImportReport, ResolvedTheme, ThemeId, ThemeScope, ThemeService, ThemeSummary,
};
use tauri::State;

/// Lista o resumo (`ThemeSummary`) de todos os esquemas de cores disponíveis.
#[tauri::command]
#[specta::specta]
pub async fn theme_list(svc: State<'_, Arc<ThemeService>>) -> Result<Vec<ThemeSummary>, MtError> {
    Ok(svc.list_summaries())
}

/// Retorna o `ResolvedTheme` completo (esquema + payload xterm + tokens CSS da UI) pelo `ThemeId`.
#[tauri::command]
#[specta::specta]
pub async fn theme_get(
    svc: State<'_, Arc<ThemeService>>,
    id: ThemeId,
) -> Result<ResolvedTheme, MtError> {
    svc.get_resolved(&id).map_err(MtError::from)
}

/// Retorna as configurações atuais de aparência (`AppearanceSettings`).
#[tauri::command]
#[specta::specta]
pub async fn theme_appearance_get(
    svc: State<'_, Arc<ThemeService>>,
) -> Result<AppearanceSettings, MtError> {
    Ok(svc.appearance_get())
}

/// Atualiza as configurações globais de aparência e tipografia do terminal.
#[tauri::command]
#[specta::specta]
pub async fn theme_appearance_update(
    svc: State<'_, Arc<ThemeService>>,
    patch: AppearanceSettingsPatch,
) -> Result<AppearanceSettings, MtError> {
    let svc_clone = Arc::clone(&svc);
    tokio::task::spawn_blocking(move || svc_clone.appearance_update(patch))
        .await
        .map_err(|e| MtError::internal(e.to_string()))?
        .map_err(MtError::from)
}

/// Importa um arquivo de esquema de cores (Tabby YAML/JSON, Windows Terminal JSON ou Xterm JSON).
#[tauri::command]
#[specta::specta]
pub async fn theme_import_file(
    svc: State<'_, Arc<ThemeService>>,
    path: String,
    options: ImportFileOptions,
) -> Result<ImportReport, MtError> {
    let svc_clone = Arc::clone(&svc);
    let path_buf = PathBuf::from(path);
    tokio::task::spawn_blocking(move || svc_clone.import_file(&path_buf, options))
        .await
        .map_err(|e| MtError::internal(e.to_string()))?
        .map_err(MtError::from)
}

/// Importa todos os arquivos de esquema válidos encontrados em um diretório.
#[tauri::command]
#[specta::specta]
pub async fn theme_import_dir(
    svc: State<'_, Arc<ThemeService>>,
    dir_path: String,
    options: ImportDirOptions,
) -> Result<ImportReport, MtError> {
    let svc_clone = Arc::clone(&svc);
    let path_buf = PathBuf::from(dir_path);
    tokio::task::spawn_blocking(move || svc_clone.import_dir(&path_buf, options))
        .await
        .map_err(|e| MtError::internal(e.to_string()))?
        .map_err(MtError::from)
}

/// Define o tema ativo globalmente ou para um projeto específico (`ThemeScope`).
#[tauri::command]
#[specta::specta]
pub async fn theme_set_active(
    svc: State<'_, Arc<ThemeService>>,
    id: ThemeId,
    scope: ThemeScope,
) -> Result<ResolvedTheme, MtError> {
    let svc_clone = Arc::clone(&svc);
    tokio::task::spawn_blocking(move || svc_clone.set_active(id, scope))
        .await
        .map_err(|e| MtError::internal(e.to_string()))?
        .map_err(MtError::from)
}

/// Remove um tema `imported` ou `custom` (com fallback automático caso estivesse ativo).
#[tauri::command]
#[specta::specta]
pub async fn theme_delete(
    svc: State<'_, Arc<ThemeService>>,
    id: ThemeId,
    fallback_id: Option<ThemeId>,
) -> Result<(), MtError> {
    let svc_clone = Arc::clone(&svc);
    tokio::task::spawn_blocking(move || svc_clone.delete_theme(&id, fallback_id))
        .await
        .map_err(|e| MtError::internal(e.to_string()))?
        .map_err(MtError::from)
}

/// Exporta um tema existente para YAML ou JSON no formato solicitado.
#[tauri::command]
#[specta::specta]
pub async fn theme_export(
    svc: State<'_, Arc<ThemeService>>,
    id: ThemeId,
    format: ExportFormat,
) -> Result<String, MtError> {
    svc.export_theme(&id, format).map_err(MtError::from)
}
