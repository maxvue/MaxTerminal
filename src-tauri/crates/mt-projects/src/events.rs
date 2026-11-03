//! Eventos de domínio emitidos pelo `ProjectService` e trait `ProjectEventSink` (sem dependência de Tauri).

use mt_core::ids::ProjectId;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::model::{DiscoveredFolder, DiscoveryMode, Project, ProjectRoot, ProjectRootId};

/// Payload do evento `project://discovered`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectDiscoveredPayload {
    /// Identificador da Pasta de Projetos onde as subpastas foram descobertas.
    pub root_id: ProjectRootId,
    /// Modo de descoberta aplicado (`suggest` preenche `folders`; `auto_import` preenche `imported`).
    pub mode: DiscoveryMode,
    /// Subpastas sugeridas aguardando decisão do usuário.
    pub folders: Vec<DiscoveredFolder>,
    /// Projetos importados automaticamente.
    pub imported: Vec<Project>,
}

/// Motivo de emissão do evento `project://removed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum RemovedKind {
    /// O diretório de um projeto cadastrado deixou de existir no disco (`missing = true`).
    ProjectMissing,
    /// Uma sugestão de descoberta deixou de existir, foi aceita ou foi dispensada.
    SuggestionGone,
}

/// Payload do evento `project://removed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRemovedPayload {
    /// Pasta de Projetos relacionada, quando aplicável.
    pub root_id: Option<ProjectRootId>,
    /// Tipo de remoção observada.
    pub kind: RemovedKind,
    /// Caminho absoluto canônico afetado.
    pub path: String,
    /// Identificador do projeto marcado como ausente, quando `kind == ProjectMissing`.
    pub project_id: Option<ProjectId>,
}

/// Payload do evento `project://changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectChangedPayload {
    /// Projetos inseridos ou atualizados (incluindo restauração de `missing`).
    Upserted {
        /// Lista de projetos criados ou atualizados.
        projects: Vec<Project>,
    },
    /// Projetos removidos definitivamente do cadastro.
    Deleted {
        /// Identificadores dos projetos excluídos.
        ids: Vec<ProjectId>,
    },
    /// Nova ordem completa de identificadores após `project_reorder`.
    Reordered {
        /// Identificadores na nova ordem `0..N-1`.
        ids: Vec<ProjectId>,
    },
}

/// Payload do evento `project://root-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProjectRootChangedPayload {
    /// Pasta de Projetos criada, editada ou com `watch_status` alterado.
    Upserted {
        /// Estado atualizado da Pasta de Projetos.
        root: ProjectRoot,
    },
    /// Pasta de Projetos removida do cadastro.
    Deleted {
        /// Identificador da Pasta de Projetos removida.
        id: ProjectRootId,
    },
}

/// Envelope versionado para todos os eventos `project://*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
#[specta(inline)]
pub struct ProjectEvent<T> {
    /// Época do processo em execução.
    pub epoch: String,
    /// Revisão monotônica associada à alteração.
    pub revision: u32,
    /// Carga útil específica do evento.
    pub payload: T,
}

/// Sink abstrato para emissão dos 4 eventos do domínio de projetos.
pub trait ProjectEventSink: Send + Sync + 'static {
    /// Emite `project://discovered`.
    fn discovered(&self, event: ProjectEvent<ProjectDiscoveredPayload>);
    /// Emite `project://removed`.
    fn removed(&self, event: ProjectEvent<ProjectRemovedPayload>);
    /// Emite `project://changed`.
    fn changed(&self, event: ProjectEvent<ProjectChangedPayload>);
    /// Emite `project://root-changed`.
    fn root_changed(&self, event: ProjectEvent<ProjectRootChangedPayload>);
}

/// Implementação nula de [`ProjectEventSink`].
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopProjectEventSink;

impl ProjectEventSink for NoopProjectEventSink {
    fn discovered(&self, _event: ProjectEvent<ProjectDiscoveredPayload>) {}
    fn removed(&self, _event: ProjectEvent<ProjectRemovedPayload>) {}
    fn changed(&self, _event: ProjectEvent<ProjectChangedPayload>) {}
    fn root_changed(&self, _event: ProjectEvent<ProjectRootChangedPayload>) {}
}

/// Sink em memória para inspeção determinística de eventos em testes automatizados.
#[derive(Debug, Default)]
pub struct MemoryProjectEventSink {
    /// Eventos `discovered` capturados.
    pub discovered_events: Mutex<Vec<ProjectEvent<ProjectDiscoveredPayload>>>,
    /// Eventos `removed` capturados.
    pub removed_events: Mutex<Vec<ProjectEvent<ProjectRemovedPayload>>>,
    /// Eventos `changed` capturados.
    pub changed_events: Mutex<Vec<ProjectEvent<ProjectChangedPayload>>>,
    /// Eventos `root_changed` capturados.
    pub root_changed_events: Mutex<Vec<ProjectEvent<ProjectRootChangedPayload>>>,
}

impl MemoryProjectEventSink {
    /// Cria um novo coletor em memória.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl ProjectEventSink for MemoryProjectEventSink {
    fn discovered(&self, event: ProjectEvent<ProjectDiscoveredPayload>) {
        self.discovered_events.lock().push(event);
    }

    fn removed(&self, event: ProjectEvent<ProjectRemovedPayload>) {
        self.removed_events.lock().push(event);
    }

    fn changed(&self, event: ProjectEvent<ProjectChangedPayload>) {
        self.changed_events.lock().push(event);
    }

    fn root_changed(&self, event: ProjectEvent<ProjectRootChangedPayload>) {
        self.root_changed_events.lock().push(event);
    }
}
