//! Inversão de dependência (`BridgeHost` e `BridgeEventSink`) e despachante de mensagens recebidas (`dispatch_inbound`).

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::config::BridgeSettings;
use crate::dedup::RecentRequestSet;
use crate::error::{BridgeError, BridgeErrorCode};
use crate::issues::BugCycleProgressPayload;
use crate::paths::normalize_existing_project_path;
use crate::protocol::{
    AgentKind, BridgeStatusSnapshot, ErrorPayload, HelloPayload, OpenTerminalForIssuePayload,
    PROTOCOL_VERSION, PeerRole, PingPayload, PongPayload, RpcEnvelope, RpcPayload,
    TerminalOpenedAckPayload, WelcomePayload, default_maxterminal_capabilities,
};
use crate::sync::{InboundSyncOutcome, ProjectSyncDecider, apply_inbound_project_opened};

/// Resumo de projeto retornado pelo `BridgeHost`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BridgeProjectSummary {
    /// ID do projeto no `MaxTerminal`.
    pub id: String,
    /// Título do projeto.
    pub title: String,
    /// Caminho absoluto canonicalizado da pasta do projeto.
    pub path: String,
}

/// Dados solicitados ao abrir uma aba de terminal ou agente CLI posicionada em uma issue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenTerminalRequest {
    /// ID do projeto no `MaxTerminal`.
    pub project_id: String,
    /// Diretório de trabalho (worktree ou raiz do projeto).
    pub cwd: String,
    /// Número da issue.
    pub issue_number: u64,
    /// Título opcional da issue.
    pub issue_title: Option<String>,
    /// Branch opcional da issue.
    pub branch: Option<String>,
    /// Agente CLI opcional.
    pub agent: Option<AgentKind>,
    /// Comando inicial opcional.
    pub initial_command: Option<String>,
}

/// Resultado da abertura de aba no `MaxTerminal`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedTerminalInfo {
    /// ID do projeto.
    pub project_id: String,
    /// ID da aba aberta/focada.
    pub tab_id: String,
}

/// Erro de comando carregado pelo `BridgeHost` para o fluxo `inspect.command_error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InspectableBlockContext {
    /// ID do bloco de comando.
    pub block_id: String,
    /// ID da sessão.
    pub session_id: String,
    /// ID da aba.
    pub tab_id: String,
    /// Caminho da raiz do projeto.
    pub project_path: String,
    /// CWD onde o comando rodou.
    pub cwd: String,
    /// Comando já sanitizado.
    pub command: String,
    /// Saída de erro já sanitizada.
    pub error_output: String,
    /// Código de saída (`exit != 0`) ou `None` se encerrado por sinal.
    pub exit_code: Option<i32>,
    /// Sinal de término, se houver.
    pub signal: Option<String>,
    /// Duração da execução em ms.
    pub duration_ms: Option<u64>,
}

/// Trait de integração com os subsistemas hospedeiros do `MaxTerminal` (`mt-projects`, `WorkspaceManager`, `mt-shell`, `mt-os`).
#[async_trait]
pub trait BridgeHost: Send + Sync + 'static {
    /// Retorna o caminho do projeto atualmente selecionado no `MaxTerminal`, se houver.
    async fn active_project_path(&self) -> Option<String>;

    /// Procura um projeto cadastrado pelo caminho absoluto canonicalizado.
    async fn find_project_by_path(
        &self,
        canonical_path: &Path,
    ) -> Result<Option<BridgeProjectSummary>, BridgeError>;

    /// Cria um novo projeto para o caminho absoluto informado.
    async fn create_project_for_path(
        &self,
        canonical_path: &Path,
        preferred_name: Option<&str>,
    ) -> Result<BridgeProjectSummary, BridgeError>;

    /// Seleciona/foca o projeto na interface do `MaxTerminal`.
    async fn focus_project(&self, project_id: &str) -> Result<(), BridgeError>;

    /// Abre uma nova aba de terminal ou agente CLI posicionada na worktree/branch de uma issue.
    async fn open_terminal_for_issue(
        &self,
        request: OpenTerminalRequest,
    ) -> Result<OpenedTerminalInfo, BridgeError>;

    /// Carrega e sanitiza o contexto de erro de um bloco de comando (`command_block_error_context`).
    async fn load_inspectable_block(
        &self,
        block_id: &str,
    ) -> Result<InspectableBlockContext, BridgeError>;

    /// Traz a janela principal do `MaxTerminal` para frente (integração com `mt-os`).
    async fn bring_window_to_front(&self) -> Result<(), BridgeError>;

    /// Dispara a abertura de uma URI externa (`vscode://johnattas.maxcode/...`) no sistema operacional sem shell.
    async fn launch_external_uri(&self, uri: &str) -> Result<(), BridgeError>;
}

/// Evento emitido para o frontend quando um projeto é sincronizado a partir do `MaxCode` (`bridge://project-synced`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BridgeProjectSyncedEvent {
    /// ID do projeto focado no `MaxTerminal`.
    pub project_id: String,
    /// Caminho absoluto do projeto.
    pub project_path: String,
    /// Indica se o projeto precisou ser cadastrado automaticamente.
    pub created: bool,
}

/// Evento emitido quando uma aba é aberta por comando do `MaxCode` (`bridge://terminal-opened`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BridgeTerminalOpenedEvent {
    /// ID do projeto.
    pub project_id: String,
    /// ID da aba criada.
    pub tab_id: String,
    /// Número da issue associada.
    pub issue_number: u64,
}

/// Sink de eventos Tauri da ponte (`bridge://*`).
pub trait BridgeEventSink: Send + Sync + 'static {
    /// Emite `bridge://status-changed`.
    fn emit_status_changed(&self, status: &BridgeStatusSnapshot);
    /// Emite `bridge://project-synced`.
    fn emit_project_synced(&self, event: &BridgeProjectSyncedEvent);
    /// Emite `bridge://terminal-opened`.
    fn emit_terminal_opened(&self, event: &BridgeTerminalOpenedEvent);
    /// Emite `bridge://bug-cycle-progress`.
    fn emit_bug_cycle_progress(&self, event: &BugCycleProgressPayload);
    /// Emite `bridge://error`.
    fn emit_error(&self, error: &BridgeError);
}

/// Implementação no-op de `BridgeEventSink` para testes.
#[derive(Debug, Default)]
pub struct NoopBridgeEventSink;

impl BridgeEventSink for NoopBridgeEventSink {
    fn emit_status_changed(&self, _status: &BridgeStatusSnapshot) {}
    fn emit_project_synced(&self, _event: &BridgeProjectSyncedEvent) {}
    fn emit_terminal_opened(&self, _event: &BridgeTerminalOpenedEvent) {}
    fn emit_bug_cycle_progress(&self, _event: &BugCycleProgressPayload) {}
    fn emit_error(&self, _error: &BridgeError) {}
}

/// Contexto compartilhado passado ao despachante de envelopes recebidos.
pub struct InboundContext {
    /// Identificador único desta instância do `MaxTerminal`.
    pub instance_id: String,
    /// Versão do `MaxTerminal`.
    pub app_version: String,
    /// Host de integração.
    pub host: Arc<dyn BridgeHost>,
    /// Sink de eventos.
    pub sink: Arc<dyn BridgeEventSink>,
    /// Deduplicador de `request_id`.
    pub dedup: Arc<RecentRequestSet>,
    /// Máquina anti-eco de sincronização de projetos.
    pub sync_decider: Arc<ProjectSyncDecider>,
}

/// Processa um `RpcEnvelope` recebido do `MaxCode` e retorna um `RpcEnvelope` de resposta quando aplicável.
pub async fn dispatch_inbound(
    ctx: &InboundContext,
    settings: &BridgeSettings,
    envelope: RpcEnvelope,
) -> Result<Option<RpcEnvelope>, BridgeError> {
    if envelope.v != PROTOCOL_VERSION {
        return Ok(Some(RpcEnvelope::new(RpcPayload::Error(ErrorPayload {
            request_id: Some(envelope.request_id),
            code: BridgeErrorCode::UnsupportedProtocolVersion
                .as_str()
                .to_string(),
            message: format!(
                "Versão de protocolo incompatível: recebido {}, esperado {PROTOCOL_VERSION}.",
                envelope.v
            ),
        }))));
    }

    match envelope.message {
        RpcPayload::Hello(hello) => {
            if hello.protocol_version != PROTOCOL_VERSION {
                return Err(BridgeError::new(
                    BridgeErrorCode::UnsupportedProtocolVersion,
                    format!(
                        "Handshake hello com protocol_version={} não suportado.",
                        hello.protocol_version
                    ),
                ));
            }
            let active_project_path = ctx.host.active_project_path().await;
            let welcome = WelcomePayload {
                protocol_version: PROTOCOL_VERSION,
                role: PeerRole::MaxTerminal,
                instance_id: ctx.instance_id.clone(),
                pid: std::process::id(),
                version: ctx.app_version.clone(),
                active_project_path,
                capabilities: default_maxterminal_capabilities(),
            };
            Ok(Some(RpcEnvelope::with_id(
                envelope.request_id,
                RpcPayload::Welcome(welcome),
            )))
        }
        RpcPayload::Ping(PingPayload { seq }) => Ok(Some(RpcEnvelope::with_id(
            envelope.request_id,
            RpcPayload::Pong(PongPayload { seq }),
        ))),
        RpcPayload::ProjectOpened(payload) => {
            if !ctx.dedup.check_and_insert(envelope.request_id) {
                return Ok(None);
            }
            let outcome = apply_inbound_project_opened(
                &ctx.sync_decider,
                settings,
                ctx.host.as_ref(),
                &payload,
            )
            .await?;
            match outcome {
                InboundSyncOutcome::FocusedExisting(proj) => {
                    ctx.sink.emit_project_synced(&BridgeProjectSyncedEvent {
                        project_id: proj.id,
                        project_path: proj.path,
                        created: false,
                    });
                }
                InboundSyncOutcome::CreatedAndFocused(proj) => {
                    ctx.sink.emit_project_synced(&BridgeProjectSyncedEvent {
                        project_id: proj.id,
                        project_path: proj.path,
                        created: true,
                    });
                }
                InboundSyncOutcome::Ignored(_) => {}
            }
            Ok(None)
        }
        RpcPayload::OpenTerminalForIssue(payload) => {
            if !ctx.dedup.check_and_insert(envelope.request_id) {
                return Ok(None);
            }
            let ack =
                handle_open_terminal_for_issue(ctx, settings, envelope.request_id, payload).await;
            Ok(Some(RpcEnvelope::new(RpcPayload::TerminalOpenedAck(ack))))
        }
        RpcPayload::BugCycleProgress(progress) => {
            ctx.sink.emit_bug_cycle_progress(&progress);
            Ok(None)
        }
        RpcPayload::Welcome(_)
        | RpcPayload::Pong(_)
        | RpcPayload::InspectAck(_)
        | RpcPayload::TerminalOpenedAck(_)
        | RpcPayload::IssueCommandAck(_)
        | RpcPayload::BugsCycleAck(_)
        | RpcPayload::Error(_)
        | RpcPayload::OpenFile(_)
        | RpcPayload::InspectCommandError(_)
        | RpcPayload::IssueCommand(_)
        | RpcPayload::BugsCycleStart(_) => Ok(None),
    }
}

async fn handle_open_terminal_for_issue(
    ctx: &InboundContext,
    settings: &BridgeSettings,
    request_id: uuid::Uuid,
    payload: OpenTerminalForIssuePayload,
) -> TerminalOpenedAckPayload {
    let project_canon = match normalize_existing_project_path(&payload.project_path) {
        Ok(p) => p,
        Err(err) => {
            return TerminalOpenedAckPayload {
                request_id,
                accepted: false,
                project_id: None,
                tab_id: None,
                error: Some(err.message),
            };
        }
    };

    let target_cwd = if let Some(ref wt) = payload.worktree_path {
        match normalize_existing_project_path(wt) {
            Ok(p) => p,
            Err(err) => {
                return TerminalOpenedAckPayload {
                    request_id,
                    accepted: false,
                    project_id: None,
                    tab_id: None,
                    error: Some(err.message),
                };
            }
        }
    } else {
        project_canon.clone()
    };

    let project = match ctx.host.find_project_by_path(&project_canon).await {
        Ok(Some(p)) => p,
        Ok(None) => match ctx.host.create_project_for_path(&project_canon, None).await {
            Ok(created) => created,
            Err(err) => {
                return TerminalOpenedAckPayload {
                    request_id,
                    accepted: false,
                    project_id: None,
                    tab_id: None,
                    error: Some(err.message),
                };
            }
        },
        Err(err) => {
            return TerminalOpenedAckPayload {
                request_id,
                accepted: false,
                project_id: None,
                tab_id: None,
                error: Some(err.message),
            };
        }
    };

    let open_res = ctx
        .host
        .open_terminal_for_issue(OpenTerminalRequest {
            project_id: project.id.clone(),
            cwd: target_cwd.to_string_lossy().into_owned(),
            issue_number: payload.issue_number,
            issue_title: payload.issue_title,
            branch: payload.branch,
            agent: payload.agent,
            initial_command: payload.initial_command,
        })
        .await;

    match open_res {
        Ok(opened) => {
            if settings.focus_window_on_incoming_command {
                let _ = ctx.host.bring_window_to_front().await;
            }
            ctx.sink.emit_terminal_opened(&BridgeTerminalOpenedEvent {
                project_id: opened.project_id.clone(),
                tab_id: opened.tab_id.clone(),
                issue_number: payload.issue_number,
            });
            TerminalOpenedAckPayload {
                request_id,
                accepted: true,
                project_id: Some(opened.project_id),
                tab_id: Some(opened.tab_id),
                error: None,
            }
        }
        Err(err) => TerminalOpenedAckPayload {
            request_id,
            accepted: false,
            project_id: Some(project.id),
            tab_id: None,
            error: Some(err.message),
        },
    }
}

/// Valida o payload `HelloPayload` recebido.
pub fn validate_hello(hello: &HelloPayload) -> Result<(), BridgeError> {
    if hello.protocol_version != PROTOCOL_VERSION {
        return Err(BridgeError::new(
            BridgeErrorCode::UnsupportedProtocolVersion,
            format!(
                "Versão de protocolo incompatível: {} (esperado {PROTOCOL_VERSION}).",
                hello.protocol_version
            ),
        ));
    }
    if hello.instance_id.trim().is_empty() {
        return Err(BridgeError::new(
            BridgeErrorCode::InvalidEnvelope,
            "O campo instance_id no handshake hello não pode ser vazio.",
        ));
    }
    Ok(())
}
