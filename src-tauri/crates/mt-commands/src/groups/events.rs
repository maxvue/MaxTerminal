//! Eventos globais Tauri (`command-group://*`) e frames de saída (`GroupOutputFrame`) da Fase 15.

use mt_core::{CommandBlockId, CommandGroupId, PaneId, ProjectId, PtyId, SessionId, TabId};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::groups::model::{
    GroupRun, GroupRunCounts, GroupRunId, GroupRunItem, GroupRunItemId, GroupScope, ItemStatus,
    OutputSubscriptionId, RunStatus,
};

/// Nome do evento global emitido após mutações de catálogo de grupos.
pub const EVENT_GROUP_CHANGED: &str = "command-group://changed";
/// Nome do evento global emitido quando uma execução é aceita e iniciada.
pub const EVENT_RUN_STARTED: &str = "command-group://run-started";
/// Nome do evento global emitido a cada transição de estado de item.
pub const EVENT_RUN_ITEM_CHANGED: &str = "command-group://run-item-changed";
/// Nome do evento global emitido quando uma execução atinge estado terminal.
pub const EVENT_RUN_FINISHED: &str = "command-group://run-finished";

/// Operação de catálogo comunicada em `GroupChangedEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GroupChangeOperation {
    /// Novo grupo criado.
    Created,
    /// Grupo existente atualizado.
    Updated,
    /// Grupo removido.
    Deleted,
    /// Grupos do escopo reordenados.
    Reordered,
}

/// Payload emitido em `command-group://changed` (sem comandos, env ou output).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupChangedEvent {
    /// Identificador do grupo alterado (`None` quando `operation == Reordered`).
    pub group_id: Option<CommandGroupId>,
    /// Escopo afetado (`local` ou `global`).
    pub scope: GroupScope,
    /// Projeto afetado quando `scope == Local`.
    pub project_id: Option<ProjectId>,
    /// Revisão resultante após a mutação.
    pub revision: u64,
    /// Tipo de operação realizada.
    pub operation: GroupChangeOperation,
}

/// Delta compacto de um item incluído em `command-group://run-item-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunItemEventDelta {
    /// Identificador do item na execução.
    pub id: GroupRunItemId,
    /// Posição 0-based.
    pub position: u32,
    /// Novo estado do item.
    pub status: ItemStatus,
    /// Versão monotônica do item.
    pub version: u64,
    /// PTY associado.
    pub pty_id: Option<PtyId>,
    /// Aba associada.
    pub tab_id: Option<TabId>,
    /// Painel associado.
    pub pane_id: Option<PaneId>,
    /// Sessão associada.
    pub session_id: Option<SessionId>,
    /// Bloco OSC 133 correlacionado.
    pub command_block_id: Option<CommandBlockId>,
    /// Código de saída.
    pub exit_code: Option<i32>,
    /// Sinal de encerramento.
    pub signal: Option<i32>,
    /// Código de erro estruturado.
    pub error_code: Option<String>,
    /// Mensagem de erro sanitizada.
    pub error_message: Option<String>,
    /// Timestamp de início (epoch ms).
    pub started_at: Option<i64>,
    /// Timestamp de término (epoch ms).
    pub finished_at: Option<i64>,
    /// Flag indicando truncamento da cauda em 256 KiB.
    pub output_truncated: bool,
}

impl From<&GroupRunItem> for GroupRunItemEventDelta {
    fn from(item: &GroupRunItem) -> Self {
        Self {
            id: item.id.clone(),
            position: item.position,
            status: item.status,
            version: item.version,
            pty_id: item.pty_id.clone(),
            tab_id: item.tab_id.clone(),
            pane_id: item.pane_id.clone(),
            session_id: item.session_id.clone(),
            command_block_id: item.command_block_id.clone(),
            exit_code: item.exit_code,
            signal: item.signal,
            error_code: item.error_code.clone(),
            error_message: item.error_message.clone(),
            started_at: item.started_at,
            finished_at: item.finished_at,
            output_truncated: item.output_truncated,
        }
    }
}

/// Payload emitido em `command-group://run-started`, `run-item-changed` e `run-finished`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupRunEvent {
    /// Identificador da execução.
    pub run_id: GroupRunId,
    /// Projeto onde a execução ocorre.
    pub project_id: ProjectId,
    /// Sequência monotônica (`run.version`).
    pub seq: u64,
    /// Estado agregado da execução.
    pub status: RunStatus,
    /// Contadores agregados dos itens.
    pub counts: GroupRunCounts,
    /// Último código de saída observado.
    pub last_exit_code: Option<i32>,
    /// Timestamp de início da execução.
    pub started_at: Option<i64>,
    /// Timestamp de solicitação de cancelamento.
    pub cancel_requested_at: Option<i64>,
    /// Timestamp de término da execução.
    pub finished_at: Option<i64>,
    /// Delta opcional do item que motivou a transição.
    pub item: Option<GroupRunItemEventDelta>,
}

impl GroupRunEvent {
    /// Constrói um `GroupRunEvent` a partir de um `GroupRun` e um item opcional.
    #[must_use]
    pub fn from_run(run: &GroupRun, item: Option<&GroupRunItem>) -> Self {
        Self {
            run_id: run.id.clone(),
            project_id: run.project_id.clone(),
            seq: run.version,
            status: run.status,
            counts: run.counts,
            last_exit_code: run.last_exit_code,
            started_at: run.started_at,
            cancel_requested_at: run.cancel_requested_at,
            finished_at: run.finished_at,
            item: item.map(GroupRunItemEventDelta::from),
        }
    }
}

/// Tipo de frame de saída transmitido via `Channel<GroupOutputFrame>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "snake_case")]
pub enum GroupOutputFrameKind {
    /// Replay do ring buffer existente no momento do attach.
    Replay,
    /// Chunk incremental ao vivo.
    Data,
    /// Indicador de bytes descartados por truncamento de ring buffer ou consumidor lento.
    Gap,
    /// Fim definitivo da saída do item.
    End,
}

/// Frame enviado exclusivamente via `tauri::ipc::Channel<GroupOutputFrame>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupOutputFrame {
    /// Identificador da assinatura.
    pub subscription_id: OutputSubscriptionId,
    /// Item ao qual o frame pertence.
    pub item_id: GroupRunItemId,
    /// Offset monotônico em bytes sanitizados ao final deste frame.
    pub seq: u64,
    /// Tipo do frame.
    pub kind: GroupOutputFrameKind,
    /// Conteúdo textual UTF-8 sanitizado (`Some` em `Replay` e `Data`).
    pub text: Option<String>,
    /// Quantidade de bytes omitidos (`Some` em `Gap`).
    pub dropped_bytes: Option<u64>,
}

/// Resposta do comando `command_group_run_item_attach`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct OutputAttachReceipt {
    /// Identificador único da assinatura criada.
    pub subscription_id: OutputSubscriptionId,
    /// Offset inicial disponível no replay.
    pub replay_start_seq: u64,
    /// Offset final do replay entregue antes do streaming ao vivo.
    pub replay_end_seq: u64,
    /// Indica se houve truncamento na janela de 256 KiB.
    pub truncated: bool,
}

/// Emissor abstrato de eventos globais para desacoplar `mt-commands` do `tauri::AppHandle`.
pub trait GroupEventSink: Send + Sync {
    /// Emite `command-group://changed`.
    fn emit_group_changed(&self, event: GroupChangedEvent);
    /// Emite `command-group://run-started`.
    fn emit_run_started(&self, event: GroupRunEvent);
    /// Emite `command-group://run-item-changed`.
    fn emit_run_item_changed(&self, event: GroupRunEvent);
    /// Emite `command-group://run-finished`.
    fn emit_run_finished(&self, event: GroupRunEvent);
}

/// Assinante de frames de saída (`Channel<GroupOutputFrame>`).
pub trait OutputFrameSubscriber: Send + Sync {
    /// Envia um frame ao assinante; retorna `false` se o canal tiver sido fechado.
    fn send_frame(&self, frame: GroupOutputFrame) -> bool;
}
