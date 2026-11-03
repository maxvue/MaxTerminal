//! Testes de integração de `SettingsRegistry` e `SettingsRepo` (`mt-db`).

#![allow(clippy::unwrap_used)]

use std::sync::{Arc, Mutex};

use mt_db::{
    Db, DbConfig, DbError, DbEventSink, DbOpenOptions, SettingChangeOrigin, SettingsChanged,
};
use serde_json::json;
use tempfile::TempDir;

#[derive(Default)]
struct SettingsEventCollector {
    changes: Mutex<Vec<SettingsChanged>>,
}

impl DbEventSink for SettingsEventCollector {
    fn on_settings_changed(&self, event: SettingsChanged) {
        self.changes.lock().unwrap().push(event);
    }
}

#[tokio::test]
async fn phase_03_defaults_and_typed_read() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();
    let settings = db.settings().unwrap();

    let snap = settings
        .get_snapshot("db.retention.backup_daily_keep")
        .unwrap();
    assert_eq!(snap.key, "db.retention.backup_daily_keep");
    assert_eq!(snap.value, json!(7));
    assert_eq!(snap.revision, 0);
    assert_eq!(snap.updated_at, 0);

    let keep_typed: u32 = settings
        .get_typed("db.retention.backup_daily_keep")
        .unwrap()
        .unwrap();
    assert_eq!(keep_typed, 7);

    let all = settings.get_many(None).unwrap();
    assert!(all.len() >= 4);
    assert_eq!(all.get("db.retention.backup_daily_keep"), Some(&json!(7)));
    assert_eq!(all.get("ui.theme"), Some(&json!("max-navy-teal")));

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn validation_unknown_key_ui_namespace_and_64kib_limit() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();
    let settings = db.settings().unwrap();

    // Valor fora da faixa [1..=60] deve retornar SettingInvalidValue
    let err_zero = settings
        .set(
            "db.retention.backup_daily_keep",
            json!(0),
            None,
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap_err();
    assert!(matches!(err_zero, DbError::SettingInvalidValue { .. }));

    let err_high = settings
        .set(
            "db.retention.backup_daily_keep",
            json!(61),
            None,
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap_err();
    assert!(matches!(err_high, DbError::SettingInvalidValue { .. }));

    // Chave desconhecida fora de ui.* deve retornar SettingUnknownKey
    let err_unknown = settings
        .set(
            "nonexistent.key",
            json!(true),
            None,
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap_err();
    assert!(matches!(err_unknown, DbError::SettingUnknownKey { .. }));

    // Chave dinâmica em ui.* é permitida
    let ui_snap = settings
        .set(
            "ui.sidebar.collapsed",
            json!(true),
            None,
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap();
    assert_eq!(ui_snap.key, "ui.sidebar.collapsed");
    assert_eq!(ui_snap.value, json!(true));
    assert_eq!(ui_snap.revision, 1);

    // Payload acima de 64 KiB deve ser rejeitado
    let huge = "a".repeat(70 * 1024);
    let err_huge = settings
        .set("ui.huge_blob", json!(huge), None, SettingChangeOrigin::Ui)
        .await
        .unwrap_err();
    assert!(matches!(err_huge, DbError::SettingInvalidValue { .. }));

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn secret_rejected_in_settings() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();
    let settings = db.settings().unwrap();

    let jwt = "Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U";
    let err_secret = settings
        .set(
            "ui.leaky_config",
            json!({ "nested": { "auth": jwt } }),
            None,
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap_err();
    assert!(
        matches!(err_secret, DbError::SettingSecretRejected { .. }),
        "esperava SettingSecretRejected, obteve: {err_secret:?}"
    );

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn optimistic_concurrency_and_event_emission() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let collector = Arc::new(SettingsEventCollector::default());

    let options = DbOpenOptions {
        event_sink: collector.clone(),
        ..Default::default()
    };
    let db = Db::open_with(cfg, options).unwrap();
    let settings = db.settings().unwrap();

    // Primeira escrita (revision 0 -> 1)
    let snap1 = settings
        .set(
            "db.retention.backup_daily_keep",
            json!(14),
            Some(0),
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap();
    assert_eq!(snap1.revision, 1);
    assert_eq!(snap1.value, json!(14));
    assert_eq!(collector.changes.lock().unwrap().len(), 1);

    // Escrita repetida com o MESMO valor não deve incrementar revision nem emitir evento
    let snap_same = settings
        .set(
            "db.retention.backup_daily_keep",
            json!(14),
            Some(1),
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap();
    assert_eq!(snap_same.revision, 1);
    assert_eq!(collector.changes.lock().unwrap().len(), 1);

    // Conflito de revisão (expected_revision = 0, mas atual = 1)
    let conflict = settings
        .set(
            "db.retention.backup_daily_keep",
            json!(21),
            Some(0),
            SettingChangeOrigin::Ui,
        )
        .await
        .unwrap_err();
    match conflict {
        DbError::SettingConflict {
            key,
            expected_revision,
            current_revision,
        } => {
            assert_eq!(key, "db.retention.backup_daily_keep");
            assert_eq!(expected_revision, 0);
            assert_eq!(current_revision, 1);
        }
        other => panic!("esperava SettingConflict, obteve: {other:?}"),
    }

    db.shutdown().await.unwrap();
}
