//! Comandos IPC Tauri do domínio `db_*` (settings, status de saúde e recuperação controlada).

use std::collections::BTreeMap;

use mt_core::MtError;
use mt_db::models::{DbRecoveryAction, DbStatusDto, SettingChangeOrigin, SettingSnapshot};
use serde_json::Value;
use tauri::State;

use crate::state::DbService;

/// Lê configurações tipadas do banco (`None` retorna todas as chaves registradas mescladas com defaults).
#[tauri::command]
#[specta::specta]
pub async fn db_settings_get(
    state: State<'_, DbService>,
    keys: Option<Vec<String>>,
) -> Result<BTreeMap<String, Value>, MtError> {
    let settings = state.db().settings()?;
    settings.get_many(keys.as_deref()).map_err(MtError::from)
}

/// Atualiza uma chave de configuração validando tipo, bloqueando segredos e verificando revisão otimista.
#[tauri::command]
#[specta::specta]
pub async fn db_settings_set(
    state: State<'_, DbService>,
    key: String,
    value: Value,
    expected_revision: Option<i64>,
) -> Result<SettingSnapshot, MtError> {
    let settings = state.db().settings()?;
    settings
        .set(&key, value, expected_revision, SettingChangeOrigin::Ui)
        .await
        .map_err(MtError::from)
}

/// Lê a fotografia completa (`key`, `value`, `revision`, `updatedAt`) de uma única configuração.
#[tauri::command]
#[specta::specta]
pub async fn db_setting_snapshot(
    state: State<'_, DbService>,
    key: String,
) -> Result<SettingSnapshot, MtError> {
    let settings = state.db().settings()?;
    settings.get_snapshot(&key).map_err(MtError::from)
}

/// Retorna o estado operacional (`Ready` ou `RecoveryRequired`) e métricas da fila do `BatchWriter`.
#[tauri::command]
#[specta::specta]
pub async fn db_status(state: State<'_, DbService>) -> Result<DbStatusDto, MtError> {
    Ok(state.db().status().await)
}

/// Executa uma ação explícita de recuperação (`RetryOpen`, `RestoreFromBackup`, `ResetDatabase`).
#[tauri::command]
#[specta::specta]
pub async fn db_recover(
    state: State<'_, DbService>,
    action: DbRecoveryAction,
) -> Result<DbStatusDto, MtError> {
    state.db().recover(action).await.map_err(MtError::from)
}
