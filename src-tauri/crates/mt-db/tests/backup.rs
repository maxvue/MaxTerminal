//! Testes de integração de backup online, rotação, detecção de corrupção e recuperação (`mt-db`).

#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use mt_db::{
    BackupKind, Db, DbConfig, DbEventSink, DbHealthStateDto, DbOpenOptions, DbRecovered,
    DbRecoveryAction, SettingChangeOrigin, list_backups,
};
use serde_json::json;
use tempfile::TempDir;

#[derive(Default)]
struct RecoveryCollector {
    recovered: Mutex<Vec<DbRecovered>>,
}

impl DbEventSink for RecoveryCollector {
    fn on_recovered(&self, event: DbRecovered) {
        self.recovered.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn online_backup_and_rotation() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg.clone()).unwrap();

    // Configura retenção diária para manter apenas 2 backups diários.
    db.settings()
        .unwrap()
        .set(
            "db.retention.backup_daily_keep",
            json!(2),
            None,
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap();

    for idx in 0..4 {
        db.settings()
            .unwrap()
            .set(
                "ui.backup_marker",
                json!(format!("step-{idx}")),
                None,
                SettingChangeOrigin::Ui,
            )
            .await
            .unwrap();
        let entry = db
            .maintenance()
            .create_backup(BackupKind::Daily)
            .await
            .unwrap();
        let mode = fs::metadata(&entry.path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    let backups = list_backups(&cfg.backup_dir).unwrap();
    let daily_backups: Vec<_> = backups.iter().filter(|b| b.kind == "daily").collect();
    assert_eq!(daily_backups.len(), 2);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn corrupted_db_enters_recovery_required_and_restores_from_backup() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));

    // 1. Abre banco saudável, grava um dado e cria backup.
    let backup_file_name = {
        let db = Db::open(cfg.clone()).unwrap();
        db.settings()
            .unwrap()
            .set(
                "ui.important_state",
                json!("valor-salvo-no-backup"),
                None,
                SettingChangeOrigin::Ui,
            )
            .await
            .unwrap();

        let entry = db
            .maintenance()
            .create_backup(BackupKind::Manual)
            .await
            .unwrap();
        db.shutdown().await.unwrap();
        entry.file_name
    };

    // 2. Corrompe os bytes do arquivo maxterminal.db em disco.
    fs::write(&cfg.db_path, b"CORRUPTED_SQLITE_HEADER_BYTES_INVALID").unwrap();

    // 3. Reabre: não deve apagar o arquivo corrompido automaticamente; deve entrar em RecoveryRequired.
    let collector = Arc::new(RecoveryCollector::default());
    let options = DbOpenOptions {
        event_sink: collector.clone(),
        ..Default::default()
    };
    let db = Db::open_with(cfg.clone(), options).unwrap();

    let bad_status = db.status().await;
    assert!(matches!(
        bad_status.health,
        DbHealthStateDto::RecoveryRequired { .. }
    ));
    assert!(cfg.db_path.exists());

    // 4. Executa recuperação restaurando do backup.
    let recovered_status = db
        .recover(DbRecoveryAction::RestoreFromBackup {
            backup_file_name: backup_file_name.clone(),
        })
        .await
        .unwrap();

    assert!(matches!(
        recovered_status.health,
        DbHealthStateDto::Ready { .. }
    ));

    // O dado salvo no backup deve estar presente!
    let restored = db
        .settings()
        .unwrap()
        .get_snapshot("ui.important_state")
        .unwrap();
    assert_eq!(restored.value, json!("valor-salvo-no-backup"));

    {
        let events = collector.recovered.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].strategy, "restore_from_backup");
        assert!(events[0].quarantined_path.is_some());
    }

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn corrupted_db_reset_database_action() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));

    {
        let db = Db::open(cfg.clone()).unwrap();
        db.shutdown().await.unwrap();
    }

    fs::write(&cfg.db_path, b"NOT_A_VALID_SQLITE_DB").unwrap();

    let db = Db::open(cfg.clone()).unwrap();
    assert!(matches!(
        db.status().await.health,
        DbHealthStateDto::RecoveryRequired { .. }
    ));

    let expected_ver = mt_db::MigrationRegistry::builtin().unwrap().max_version();
    let status = db.recover(DbRecoveryAction::ResetDatabase).await.unwrap();
    assert!(matches!(
        status.health,
        DbHealthStateDto::Ready {
            schema_version,
            ..
        } if schema_version == expected_ver
    ));

    db.shutdown().await.unwrap();
}
