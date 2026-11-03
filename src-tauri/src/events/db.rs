//! Eventos tipados do domínio `db://` e implementação de `DbEventSink` acoplada ao `AppHandle`.

use mt_db::models::{
    DbEventSink, DbMaintenanceFinished, DbRecovered, DbWriterLag, SettingsChanged,
};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_specta::Event;

/// Evento emitido quando uma configuração é alterada (`db://settings-changed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "db://settings-changed")]
pub struct SettingsChangedEvent(pub SettingsChanged);

/// Evento emitido quando o banco é recuperado de corrupção ou backup (`db://recovered`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "db://recovered")]
pub struct DbRecoveredEvent(pub DbRecovered);

/// Evento emitido ao concluir um ciclo de manutenção (`db://maintenance-finished`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "db://maintenance-finished")]
pub struct DbMaintenanceFinishedEvent(pub DbMaintenanceFinished);

/// Evento emitido quando uma escrita ou lote demora além do limiar (`db://writer-lag`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "db://writer-lag")]
pub struct DbWriterLagEvent(pub DbWriterLag);

/// Implementação de `DbEventSink` que publica eventos Tauri para a janela principal.
pub struct TauriDbEventSink {
    app: AppHandle,
}

impl TauriDbEventSink {
    /// Cria um novo sink vinculado ao `AppHandle` da aplicação.
    #[must_use]
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl DbEventSink for TauriDbEventSink {
    fn on_settings_changed(&self, ev: SettingsChanged) {
        if let Err(err) = SettingsChangedEvent(ev).emit(&self.app) {
            tracing::warn!(
                target: "mt_db",
                error = %err,
                "Falha ao emitir evento db://settings-changed"
            );
        }
    }

    fn on_recovered(&self, ev: DbRecovered) {
        if let Err(err) = DbRecoveredEvent(ev).emit(&self.app) {
            tracing::warn!(
                target: "mt_db",
                error = %err,
                "Falha ao emitir evento db://recovered"
            );
        }
    }

    fn on_maintenance_finished(&self, ev: DbMaintenanceFinished) {
        if let Err(err) = DbMaintenanceFinishedEvent(ev).emit(&self.app) {
            tracing::warn!(
                target: "mt_db",
                error = %err,
                "Falha ao emitir evento db://maintenance-finished"
            );
        }
    }

    fn on_writer_lag(&self, ev: DbWriterLag) {
        if let Err(err) = DbWriterLagEvent(ev).emit(&self.app) {
            tracing::warn!(
                target: "mt_db",
                error = %err,
                "Falha ao emitir evento db://writer-lag"
            );
        }
    }
}
