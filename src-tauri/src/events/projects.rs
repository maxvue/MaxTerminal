//! Eventos tipados do domínio `project://` e implementação de [`ProjectEventSink`] acoplada ao `AppHandle`.

use mt_projects::{
    ProjectChangedPayload, ProjectDiscoveredPayload, ProjectEvent, ProjectEventSink,
    ProjectRemovedPayload, ProjectRootChangedPayload,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Runtime};
use tauri_specta::Event;

/// Evento emitido quando novas subpastas são sugeridas ou autoimportadas (`project://discovered`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "project://discovered")]
pub struct ProjectDiscoveredTauriEvent(pub ProjectEvent<ProjectDiscoveredPayload>);

/// Evento emitido quando um projeto fica ausente no disco ou uma sugestão desaparece (`project://removed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "project://removed")]
pub struct ProjectRemovedTauriEvent(pub ProjectEvent<ProjectRemovedPayload>);

/// Evento emitido quando o catálogo de projetos sofre criação, edição, exclusão ou reordenação (`project://changed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "project://changed")]
pub struct ProjectChangedTauriEvent(pub ProjectEvent<ProjectChangedPayload>);

/// Evento emitido quando uma Pasta de Projetos é criada, atualizada ou removida (`project://root-changed`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, Event)]
#[serde(transparent)]
#[tauri_specta(event_name = "project://root-changed")]
pub struct ProjectRootChangedTauriEvent(pub ProjectEvent<ProjectRootChangedPayload>);

/// Implementação de [`ProjectEventSink`] que publica eventos Tauri para a janela principal.
pub struct TauriProjectEventSink<R: Runtime = tauri::Wry> {
    app: AppHandle<R>,
}

impl<R: Runtime> TauriProjectEventSink<R> {
    /// Cria um novo sink vinculado ao [`AppHandle`] da aplicação.
    #[must_use]
    pub const fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> ProjectEventSink for TauriProjectEventSink<R> {
    fn discovered(&self, event: ProjectEvent<ProjectDiscoveredPayload>) {
        if let Err(err) = ProjectDiscoveredTauriEvent(event).emit(&self.app) {
            tracing::warn!(
                target: "mt_projects",
                error = %err,
                "Falha ao emitir evento project://discovered"
            );
        }
    }

    fn removed(&self, event: ProjectEvent<ProjectRemovedPayload>) {
        if let Err(err) = ProjectRemovedTauriEvent(event).emit(&self.app) {
            tracing::warn!(
                target: "mt_projects",
                error = %err,
                "Falha ao emitir evento project://removed"
            );
        }
    }

    fn changed(&self, event: ProjectEvent<ProjectChangedPayload>) {
        if let Err(err) = ProjectChangedTauriEvent(event).emit(&self.app) {
            tracing::warn!(
                target: "mt_projects",
                error = %err,
                "Falha ao emitir evento project://changed"
            );
        }
    }

    fn root_changed(&self, event: ProjectEvent<ProjectRootChangedPayload>) {
        if let Err(err) = ProjectRootChangedTauriEvent(event).emit(&self.app) {
            tracing::warn!(
                target: "mt_projects",
                error = %err,
                "Falha ao emitir evento project://root-changed"
            );
        }
    }
}
