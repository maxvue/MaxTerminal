//! Comandos IPC Tauri para gerenciamento do cofre de credenciais e senhas mestras.

use mt_core::error::MtError;
use tauri::State;

use crate::credentials::{VaultService, VaultStatus};

/// Consulta o estado operacional do cofre (`unconfigured`, `locked` ou `unlocked`).
#[tauri::command]
#[specta::specta]
pub async fn vault_get_state(service: State<'_, VaultService>) -> Result<VaultStatus, MtError> {
    service.status().await
}

/// Configura a senha mestra inicial do cofre.
#[tauri::command]
#[specta::specta]
pub async fn vault_setup(
    service: State<'_, VaultService>,
    master_password: String,
) -> Result<VaultStatus, MtError> {
    service.setup(&master_password).await?;
    service.status().await
}

/// Desbloqueia o cofre com a senha mestra.
#[tauri::command]
#[specta::specta]
pub async fn vault_unlock(
    service: State<'_, VaultService>,
    master_password: String,
) -> Result<VaultStatus, MtError> {
    service.unlock(&master_password).await?;
    service.status().await
}

/// Bloqueia o cofre, removendo a chave DEK da memória.
#[tauri::command]
#[specta::specta]
pub fn vault_lock(service: State<'_, VaultService>) -> Result<VaultStatus, MtError> {
    service.lock();
    Ok(VaultStatus::Locked)
}

/// Troca a senha mestra do cofre.
#[tauri::command]
#[specta::specta]
pub async fn vault_change_password(
    service: State<'_, VaultService>,
    current_password: String,
    new_password: String,
) -> Result<VaultStatus, MtError> {
    service
        .change_master_password(&current_password, &new_password)
        .await?;
    service.status().await
}
