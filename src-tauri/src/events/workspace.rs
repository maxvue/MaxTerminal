//! Eventos Tauri tipados emitidos pelo subsistema de workspace (Fase 07).

use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId};
use mt_core::workspace::{PaneExitInfo, SessionSummary, TabView, WorkspaceRestoreReport};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// Envelope sequencial e versionado por `boot_id` para todos os eventos de workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct WorkspaceEvent<T> {
    /// Identificador único de boot da instância atual.
    pub boot_id: String,
    /// Número de sequência monotônico do evento.
    pub event_seq: u64,
    /// Carga útil específica do evento.
    pub payload: T,
}

/// Payload do evento `tab://closed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TabClosedPayload {
    /// Identificador da aba fechada.
    pub tab_id: TabId,
    /// Identificador do projeto proprietário.
    pub project_id: ProjectId,
    /// Revisão final da aba ao ser encerrada.
    pub revision: i64,
}

/// Payload do evento `tab://reordered`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TabReorderedPayload {
    /// Identificador do projeto reordenado.
    pub project_id: ProjectId,
    /// Lista ordenada de identificadores das abas abertas.
    pub ordered_tab_ids: Vec<TabId>,
}

/// Payload do evento `pane://pty-replaced`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PanePtyReplacedPayload {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel cujo PTY foi substituído.
    pub pane_id: PaneId,
    /// Nova sessão de painel criada.
    pub session_id: SessionId,
    /// Nova geração do painel.
    pub generation: i64,
    /// Identificador do PTY anterior (se existia).
    pub old_pty_id: Option<PtyId>,
    /// Identificador do novo PTY criado.
    pub new_pty_id: PtyId,
}

/// Payload do evento `pane://exited`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct PaneExitedPayload {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel cujo processo encerrou.
    pub pane_id: PaneId,
    /// Identificador da sessão de painel.
    pub session_id: SessionId,
    /// Geração do painel no momento do encerramento.
    pub generation: i64,
    /// Identificador do PTY encerrado.
    pub pty_id: PtyId,
    /// Dados do encerramento (código, sinal, motivo e timestamp).
    pub exit: PaneExitInfo,
}

/// Payload do evento `template://changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct TemplateChangedPayload {
    /// Identificador do projeto afetado.
    pub project_id: ProjectId,
    /// Identificadores dos templates afetados.
    pub template_ids: Vec<TemplateId>,
}

/// Evento emitido quando uma aba é aberta ou reaberta (`tab://opened`).
#[derive(Debug, Clone, PartialEq, Serialize, Type, Event)]
#[tauri_specta(event_name = "tab://opened")]
pub struct TabOpenedEvent(pub WorkspaceEvent<TabView>);

/// Evento emitido quando uma aba é fechada (`tab://closed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "tab://closed")]
pub struct TabClosedEvent(pub WorkspaceEvent<TabClosedPayload>);

/// Evento emitido quando a ordem das abas de um projeto muda (`tab://reordered`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "tab://reordered")]
pub struct TabReorderedEvent(pub WorkspaceEvent<TabReorderedPayload>);

/// Evento emitido quando o layout, foco ou estado de painéis de uma aba muda (`tab://layout-changed`).
#[derive(Debug, Clone, PartialEq, Serialize, Type, Event)]
#[tauri_specta(event_name = "tab://layout-changed")]
pub struct TabLayoutChangedEvent(pub WorkspaceEvent<TabView>);

/// Evento emitido quando o PTY de um painel é substituído por restart ou nova sessão (`pane://pty-replaced`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "pane://pty-replaced")]
pub struct PanePtyReplacedEvent(pub WorkspaceEvent<PanePtyReplacedPayload>);

/// Evento emitido quando o PTY de um painel encerra (`pane://exited`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "pane://exited")]
pub struct PaneExitedEvent(pub WorkspaceEvent<PaneExitedPayload>);

/// Evento emitido quando uma nova `tab_session` é iniciada (`session://started`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "session://started")]
pub struct SessionStartedEvent(pub WorkspaceEvent<SessionSummary>);

/// Evento emitido quando uma `tab_session` é arquivada (`session://archived`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "session://archived")]
pub struct SessionArchivedEvent(pub WorkspaceEvent<SessionSummary>);

/// Evento emitido quando os templates de um projeto sofrem alteração (`template://changed`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "template://changed")]
pub struct TemplateChangedEvent(pub WorkspaceEvent<TemplateChangedPayload>);

/// Evento emitido ao concluir a reconciliação e restauração inicial do workspace (`workspace://restored`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type, Event)]
#[tauri_specta(event_name = "workspace://restored")]
pub struct WorkspaceRestoredEvent(pub WorkspaceEvent<WorkspaceRestoreReport>);
