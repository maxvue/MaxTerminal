//! Comandos IPC Tauri do domínio `backup_*` (criação online, listagem e agendamento de restauração — Fase 17).

use mt_core::MtError;
use mt_db::{
    BackupEntryDto, BackupKind, DbError, RestoreScheduledDto, create_backup_blocking, list_backups,
    schedule_restore_with_dirs,
};
use tauri::State;

use crate::state::DbService;

/// Cria imediatamente um backup manual consistente usando a Online Backup API do SQLite.
#[tauri::command]
#[specta::specta]
pub async fn backup_create(state: State<'_, DbService>) -> Result<BackupEntryDto, MtError> {
    let cfg = state.db().config().clone();
    tokio::task::spawn_blocking(move || {
        create_backup_blocking(&cfg.db_path, &cfg.backup_dir, BackupKind::Manual, 7)
    })
    .await
    .map_err(|e| {
        MtError::from(DbError::BackupFailed {
            reason: e.to_string(),
        })
    })?
    .map_err(MtError::from)
}

/// Lista todos os backups disponíveis no diretório `backups/`, do mais recente ao mais antigo.
#[tauri::command]
#[specta::specta]
pub async fn backup_list(state: State<'_, DbService>) -> Result<Vec<BackupEntryDto>, MtError> {
    let backup_dir = state.db().config().backup_dir.clone();
    tokio::task::spawn_blocking(move || list_backups(&backup_dir))
        .await
        .map_err(|e| {
            MtError::from(DbError::BackupFailed {
                reason: e.to_string(),
            })
        })?
        .map_err(MtError::from)
}

/// Valida um arquivo de backup e agenda sua restauração atômica (`pending-restore.json`) para o próximo boot.
#[tauri::command]
#[specta::specta]
pub async fn backup_restore_schedule(
    state: State<'_, DbService>,
    backup_file_name: String,
) -> Result<RestoreScheduledDto, MtError> {
    let cfg = state.db().config().clone();
    let max_version = state.db().schema_version().max(11);
    let data_dir = cfg
        .db_path
        .parent()
        .map_or_else(|| cfg.backup_dir.clone(), std::path::Path::to_path_buf);
    let backup_dir = cfg.backup_dir.clone();

    tokio::task::spawn_blocking(move || {
        schedule_restore_with_dirs(&data_dir, &backup_dir, &backup_file_name, max_version)
    })
    .await
    .map_err(|e| {
        MtError::from(DbError::BackupFailed {
            reason: e.to_string(),
        })
    })?
    .map_err(MtError::from)
}
