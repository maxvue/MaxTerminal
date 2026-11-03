//! Modelos de visão operacional de Abas (`TabView`), Painéis (`PaneView`) e Snapshots do Workspace (Fase 07).

use serde::{Deserialize, Serialize};

use crate::error::MtError;
use crate::ids::{PaneId, ProjectId, PtyId, TabId, TabSessionId, TemplateId};
use crate::workspace::layout::PaneLayoutNode;

/// Estado operacional do runtime de um painel individual.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PaneRuntimeState {
    /// Em processo de criação/spawn do PTY.
    #[default]
    Starting,
    /// PTY vivo e supervisionado.
    Live,
    /// PTY encerrou (código de saída ou sinal disponível em `exit`).
    Exited,
    /// Falha ao iniciar ou encerrar o painel (`error` disponível).
    Failed,
    /// Em processo de encerramento gracioso/forçado.
    Stopping,
}

/// Resumo serializável do encerramento de um PTY exibido em [`PaneView`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PaneExitInfo {
    /// Código de saída (`None` quando terminado por sinal).
    pub code: Option<i32>,
    /// Número do sinal POSIX se aplicável.
    pub signal: Option<i32>,
    /// Nome textual do sinal (`ex.: "SIGKILL"`).
    pub signal_name: Option<String>,
    /// Motivo semântico (`exited`, `signaled`, `killedByUser`, `shutdown`, `readerFailed`).
    pub reason: String,
    /// Timestamp UNIX em ms do término.
    pub at_ms: i64,
}

/// Estado completo de um painel dentro de [`TabView`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PaneView {
    /// Identificador persistente da folha do painel.
    pub id: PaneId,
    /// Identificador do PTY ativo na geração corrente (`None` quando `Failed` sem PTY).
    pub pty_id: Option<PtyId>,
    /// Geração monotônica de execução do painel (`>= 1`).
    pub generation: i64,
    /// Diretório de trabalho atual conhecido do painel.
    pub cwd: String,
    /// Estado operacional do painel.
    pub state: PaneRuntimeState,
    /// Informações de saída quando `state == Exited`.
    pub exit: Option<PaneExitInfo>,
    /// Erro estruturado quando `state == Failed` ou bloqueado.
    pub error: Option<MtError>,
}

/// Estado operacional do ciclo de vida da aba.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TabLifecycle {
    /// Aba sendo criada/restaurada.
    #[default]
    Opening,
    /// Aba pronta para uso.
    Ready,
    /// Aba reiniciando sessão (`session_new`).
    Restarting,
    /// Aba encerrando seus painéis (`tab_close`).
    Closing,
    /// Encerramento bloqueado por árvore de processos sobrevivente ou falha de persistência.
    Blocked,
}

/// Origem de criação da aba (`manual` ou `workspace`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum TabOrigin {
    /// Aberta manualmente pelo usuário ou comando IPC.
    #[default]
    Manual,
    /// Aberta automaticamente por template marcado com `add_to_workspace`.
    Workspace,
}

impl TabOrigin {
    /// Representação textual persistida em `tabs.origin`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Workspace => "workspace",
        }
    }

    /// Reconstrói [`TabOrigin`] a partir do valor persistido no SQLite.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "manual" => Some(Self::Manual),
            "workspace" => Some(Self::Workspace),
            _ => None,
        }
    }
}

/// Estado do despacho automático do comando inicial no painel primário da sessão ativa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InitialDispatchState {
    /// Não há comando inicial configurado para este lançamento.
    #[default]
    None,
    /// Aguardando primeiro prompt confiável dentro da janela de 5 s.
    Pending,
    /// Janela de 5 s expirou sem prompt confiável; aguarda confirmação explícita do usuário.
    Waiting,
    /// Reservado no banco antes da escrita no PTY (impede reenvio automático duplicado).
    Claimed,
    /// Escrito com sucesso no PTY.
    Sent,
}

impl InitialDispatchState {
    /// Representação textual persistida em `tab_session_panes.initial_dispatch`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Pending => "pending",
            Self::Waiting => "waiting",
            Self::Claimed => "claimed",
            Self::Sent => "sent",
        }
    }

    /// Reconstrói [`InitialDispatchState`] a partir do SQLite.
    #[must_use]
    pub fn from_db_str(raw: &str) -> Option<Self> {
        match raw {
            "none" => Some(Self::None),
            "pending" => Some(Self::Pending),
            "waiting" => Some(Self::Waiting),
            "claimed" => Some(Self::Claimed),
            "sent" => Some(Self::Sent),
            _ => None,
        }
    }
}

/// Snapshot completo de uma aba aberta no workspace.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct TabView {
    /// Identificador único persistente da aba.
    pub id: TabId,
    /// Projeto proprietário.
    pub project_id: ProjectId,
    /// Template associado (pode ser `None` se o template foi excluído posteriormente).
    pub template_id: Option<TemplateId>,
    /// Título visível da aba.
    pub title: String,
    /// Ícone Iconify da aba.
    pub icon: String,
    /// Posição de ordenação dentro do projeto (`0..N-1`).
    pub position: i64,
    /// Origem da aba (`manual` ou `workspace`).
    pub origin: TabOrigin,
    /// Indica se o template/lançamento foi configurado como execução contínua.
    pub continuous: bool,
    /// Estado operacional atual da aba.
    pub lifecycle: TabLifecycle,
    /// Revisão monotônica geral da aba (`>= 1`).
    pub revision: i64,
    /// Revisão monotônica da árvore de layout (`>= 1`).
    pub layout_revision: i64,
    /// Árvore binária de layout dos painéis abertos.
    pub layout: PaneLayoutNode,
    /// Lista de painéis abertos da aba.
    pub panes: Vec<PaneView>,
    /// Painel atualmente focado dentro da aba.
    pub active_pane_id: PaneId,
    /// Painel primário que recebe o comando inicial em novas sessões.
    pub primary_pane_id: PaneId,
    /// Estado do comando inicial no painel primário.
    pub initial_dispatch: InitialDispatchState,
    /// Identificador do agrupamento de sessão ativo da aba (`TabSessionId`).
    pub tab_session_id: TabSessionId,
}

/// Resultado retornado por `pane_close`: indica se apenas o painel fechou ou se era o último painel e fechou a aba inteira.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneCloseOutcome {
    /// O painel foi fechado e a aba permaneceu aberta com os demais painéis.
    #[serde(rename_all = "camelCase")]
    PaneClosed {
        /// Visão atualizada da aba após colapsar a folha removida.
        tab: Box<TabView>,
    },
    /// Era o último painel da aba, portanto a aba inteira foi fechada.
    #[serde(rename_all = "camelCase")]
    TabClosed {
        /// Identificador da aba fechada.
        tab_id: TabId,
    },
}

/// Falha individual registrada durante a restauração automática do workspace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRestoreFailure {
    /// Projeto associado à falha.
    pub project_id: ProjectId,
    /// Template associado, quando aplicável.
    pub template_id: Option<TemplateId>,
    /// Aba associada, quando aplicável.
    pub tab_id: Option<TabId>,
    /// Código estável do erro.
    pub code: String,
    /// Mensagem descritiva em `pt-BR`.
    pub message: String,
}

/// Relatório consolidado da recuperação de crash e restauração de abas automáticas no boot atual.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceRestoreReport {
    /// Identificador único do boot atual.
    pub boot_id: String,
    /// Quantidade de abas automáticas (`add_to_workspace = true`) abertas/restauradas neste boot.
    pub opened_tabs: usize,
    /// Quantidade de agrupamentos de sessão anteriores recuperados e arquivados como `crash_recovered`.
    pub recovered_sessions: usize,
    /// Quantidade de operações em andamento de boots anteriores marcadas como `interrupted`.
    pub interrupted_operations: usize,
    /// Quantidade de sessões legadas da Fase 05 conservadas sem vínculo de aba/projeto.
    pub unlinked_legacy_sessions: usize,
    /// Falhas individuais ocorridas durante o restore.
    pub failures: Vec<WorkspaceRestoreFailure>,
}

/// Snapshot global somente leitura do workspace retornado por `workspace_snapshot`.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    /// Identificador único do boot atual.
    pub boot_id: String,
    /// Último número de sequência de evento emitido (`0` se nenhum).
    #[specta(type = u32)]
    pub event_seq: u64,
    /// Indica se o `restore()` inicial do boot já concluiu.
    pub restored: bool,
    /// Todas as abas abertas ordenadas por `(project_id, position, id)`.
    pub tabs: Vec<TabView>,
    /// Relatório do restore inicial quando concluído.
    pub restore_report: Option<WorkspaceRestoreReport>,
}
