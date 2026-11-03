//! Subsistema de manutenção periódica, retenção em lotes, backup online e recuperação de corrupção.

pub mod backup;
pub mod retention;
pub mod scheduler;

pub use backup::{
    BackupKind, RestoreStage, apply_pending_restore_on_boot, apply_pending_restore_with_injection,
    create_backup_blocking, execute_recovery_action, list_backups, pending_restore_marker_path,
    quarantine_corrupted_files, schedule_restore, schedule_restore_with_dirs,
    validate_candidate_backup,
};
pub use retention::{RetentionRegistry, RetentionRule};
pub use scheduler::{MaintenanceHandle, spawn_maintenance_scheduler};
