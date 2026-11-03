//! Comandos IPC de verificação e instalação manual confirmada de atualizações (`CA-40` — Fase 17).

use mt_core::error::MtResult;

use crate::updater::{
    UpdateCapabilityDto, UpdateCheckResultDto, check_for_updates, install_confirmed_update,
};

/// Verifica manualmente se há atualização disponível para a build atual (`CA-40`).
#[tauri::command]
#[specta::specta]
pub async fn app_update_check() -> MtResult<UpdateCheckResultDto> {
    check_for_updates()
}

/// Aplica uma atualização após confirmação explícita do usuário (`CA-40`).
#[tauri::command]
#[specta::specta]
pub async fn app_update_install_confirmed(
    expected_version: String,
) -> MtResult<UpdateCapabilityDto> {
    install_confirmed_update(&expected_version)
}
