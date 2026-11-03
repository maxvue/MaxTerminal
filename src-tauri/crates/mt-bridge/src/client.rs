//! Serviço central da ponte `MaxTerminal` ⇄ `MaxCode` (`BridgeService`), gerenciando o servidor
//! loopback, a descoberta de `maxcode.endpoint.json`, o envio de comandos com ACK e os fallbacks de URI.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use specta::Type;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::auth::generate_token_hex;
use crate::config::{
    BridgeSettings, BridgeSettingsPatch, BridgeSettingsStore, InspectFallbackMode,
};
use crate::dedup::RecentRequestSet;
use crate::discovery::{
    DiscoveryEndpoint, candidate_maxcode_endpoint_paths, read_verified, resolve_discovery_dir,
};
use crate::error::{BridgeError, BridgeErrorCode};
use crate::handler::{
    BridgeEventSink, BridgeHost, BridgeProjectSyncedEvent, BridgeTerminalOpenedEvent,
    InboundContext, dispatch_inbound,
};
use crate::issues::{
    BugCycleProgressPayload, BugsCycleAckPayload, BugsCycleStartPayload,
    IssueActionDispatchOutcome, IssueCommandPayload, IssueProtocolSupport,
};
use crate::paths::validate_file_path_for_open;
use crate::prompt::prepare_inspect_payload;
use crate::protocol::{
    BridgeConnectionState, BridgeStatusSnapshot, CAP_BUGS_CYCLE_V1, CAP_ISSUES_PROTOCOL_V1,
    HelloPayload, InspectAckPayload, InspectCommandErrorPayload, InspectErrorCode, OpenFilePayload,
    PROTOCOL_VERSION, PeerRole, PingPayload, RpcEnvelope, RpcPayload, current_unix_ms,
    default_maxterminal_capabilities,
};
use crate::server::{
    LoopbackServerConfig, LoopbackServerHandle, send_http_rpc_envelope, start_loopback_server,
};
use crate::session::{DEFAULT_ACK_TIMEOUT, PeerInfo, SessionRegistry};
use crate::sync::{
    InboundSyncOutcome, OutboundSyncDecision, ProjectSyncDecider, apply_inbound_project_opened,
};
use crate::uri::{
    ParsedMaxTerminalUri, build_vscode_inspect_uri, build_vscode_issue_action_uri,
    build_vscode_open_file_uri, parse_maxterminal_uri,
};

/// Resultado do despacho de `bridge_inspect_command_error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "transport", rename_all = "snake_case")]
pub enum InspectDispatchResult {
    /// Entregue em tempo real pelo canal loopback com ACK confirmado pelo `MaxCode`.
    Loopback {
        /// ID da requisição.
        #[serde(rename = "requestId")]
        request_id: Uuid,
        /// Identificador opcional da conversa aberta em Plan Mode no `MaxCode`.
        #[serde(rename = "conversationId")]
        conversation_id: Option<String>,
        /// Prompt canônico enviado.
        #[serde(rename = "formattedPrompt")]
        formatted_prompt: String,
    },
    /// Entregue via fallback de URI `vscode://johnattas.maxcode/inspect?...` para acordar o `MaxCode`.
    UriFallback {
        /// ID da requisição.
        #[serde(rename = "requestId")]
        request_id: Uuid,
        /// URI disparada no sistema operacional.
        uri: String,
        /// Prompt canônico preparado.
        #[serde(rename = "formattedPrompt")]
        formatted_prompt: String,
    },
    ///Modo `ClipboardOnly`: retorna o prompt canônico para cópia na área de transferência.
    ClipboardOnly {
        /// ID da requisição.
        #[serde(rename = "requestId")]
        request_id: Uuid,
        /// Prompt canônico preparado para cópia.
        #[serde(rename = "formattedPrompt")]
        formatted_prompt: String,
    },
}

/// Repositório em memória de `BridgeSettings` para uso em testes.
#[derive(Default)]
pub struct InMemoryBridgeSettingsStore {
    inner: RwLock<BridgeSettings>,
}

impl InMemoryBridgeSettingsStore {
    /// Cria um store em memória com as configurações informadas.
    #[must_use]
    pub fn new(initial: BridgeSettings) -> Self {
        Self {
            inner: RwLock::new(initial),
        }
    }
}

#[async_trait]
impl BridgeSettingsStore for InMemoryBridgeSettingsStore {
    async fn load(&self) -> Result<BridgeSettings, BridgeError> {
        Ok(self.inner.read().clone())
    }

    async fn save(&self, settings: &BridgeSettings) -> Result<(), BridgeError> {
        *self.inner.write() = settings.clone();
        Ok(())
    }
}

/// Sink interno que encaminha eventos ao `BridgeEventSink` externo e também alimenta o broadcast de progresso de ciclo de bugs.
struct ForwardingEventSink {
    inner: Arc<dyn BridgeEventSink>,
    progress_tx: broadcast::Sender<BugCycleProgressPayload>,
    cycle_snapshots: Arc<RwLock<HashMap<Uuid, BugCycleProgressPayload>>>,
}

impl BridgeEventSink for ForwardingEventSink {
    fn emit_status_changed(&self, status: &BridgeStatusSnapshot) {
        self.inner.emit_status_changed(status);
    }

    fn emit_project_synced(&self, event: &BridgeProjectSyncedEvent) {
        self.inner.emit_project_synced(event);
    }

    fn emit_terminal_opened(&self, event: &BridgeTerminalOpenedEvent) {
        self.inner.emit_terminal_opened(event);
    }

    fn emit_bug_cycle_progress(&self, event: &BugCycleProgressPayload) {
        self.cycle_snapshots
            .write()
            .insert(event.cycle_id, event.clone());
        let _ = self.progress_tx.send(event.clone());
        self.inner.emit_bug_cycle_progress(event);
    }

    fn emit_error(&self, error: &BridgeError) {
        self.inner.emit_error(error);
    }
}

/// Serviço principal da ponte `MaxTerminal` ⇄ `MaxCode`.
pub struct BridgeService {
    instance_id: String,
    app_version: String,
    discovery_dir_override: Option<PathBuf>,
    settings_store: Arc<dyn BridgeSettingsStore>,
    settings: Arc<RwLock<BridgeSettings>>,
    sessions: Arc<SessionRegistry>,
    host: Arc<dyn BridgeHost>,
    sink: Arc<dyn BridgeEventSink>,
    inbound_ctx: Arc<InboundContext>,
    server_handle: tokio::sync::Mutex<Option<LoopbackServerHandle>>,
    progress_tx: broadcast::Sender<BugCycleProgressPayload>,
    cycle_snapshots: Arc<RwLock<HashMap<Uuid, BugCycleProgressPayload>>>,
    ping_seq: Mutex<u64>,
}

impl BridgeService {
    /// Cria e inicializa o `BridgeService`, subindo o servidor loopback se `settings.enabled == true`.
    pub async fn new(
        app_version: impl Into<String>,
        settings_store: Arc<dyn BridgeSettingsStore>,
        host: Arc<dyn BridgeHost>,
        sink: Arc<dyn BridgeEventSink>,
        discovery_dir_override: Option<PathBuf>,
    ) -> Result<Arc<Self>, BridgeError> {
        let loaded_settings = settings_store.load().await.unwrap_or_default();
        let settings = Arc::new(RwLock::new(loaded_settings.clone()));
        let sessions = Arc::new(SessionRegistry::new());
        let dedup = Arc::new(RecentRequestSet::default());
        let sync_decider = Arc::new(ProjectSyncDecider::default());
        let instance_id = Uuid::new_v4().to_string();
        let app_version_str = app_version.into();

        let (progress_tx, _) = broadcast::channel(64);
        let cycle_snapshots = Arc::new(RwLock::new(HashMap::new()));
        let forwarding_sink: Arc<dyn BridgeEventSink> = Arc::new(ForwardingEventSink {
            inner: sink,
            progress_tx: progress_tx.clone(),
            cycle_snapshots: Arc::clone(&cycle_snapshots),
        });

        let inbound_ctx = Arc::new(InboundContext {
            instance_id: instance_id.clone(),
            app_version: app_version_str.clone(),
            host: Arc::clone(&host),
            sink: Arc::clone(&forwarding_sink),
            dedup,
            sync_decider,
        });

        let svc = Arc::new(Self {
            instance_id,
            app_version: app_version_str,
            discovery_dir_override,
            settings_store,
            settings,
            sessions,
            host,
            sink: forwarding_sink,
            inbound_ctx,
            server_handle: tokio::sync::Mutex::new(None),
            progress_tx,
            cycle_snapshots,
            ping_seq: Mutex::new(0),
        });

        if loaded_settings.enabled {
            svc.ensure_server_running().await?;
            let _ = svc.probe_and_handshake_peer().await;
        } else {
            svc.sessions.set_state(BridgeConnectionState::Disabled);
        }

        Ok(svc)
    }

    /// Retorna o snapshot atual de status da ponte (`bridge_status`).
    #[must_use]
    pub fn status(&self) -> BridgeStatusSnapshot {
        self.sessions.snapshot()
    }

    /// Retorna as configurações atuais (`bridge_get_settings`).
    #[must_use]
    pub fn get_settings(&self) -> BridgeSettings {
        self.settings.read().clone()
    }

    /// Atualiza e persiste as configurações da ponte (`bridge_update_settings`),
    /// iniciando, parando ou reiniciando o servidor loopback conforme necessário.
    pub async fn update_settings(
        &self,
        patch: BridgeSettingsPatch,
    ) -> Result<BridgeSettings, BridgeError> {
        let (prev, updated) = {
            let mut guard = self.settings.write();
            let before = guard.clone();
            guard.apply_patch(patch)?;
            (before, guard.clone())
        };

        self.settings_store.save(&updated).await?;

        if !updated.enabled {
            self.stop_server().await;
            self.sessions.clear_peer();
            self.sessions.set_server_port(None);
            self.sessions.set_state(BridgeConnectionState::Disabled);
            self.sink.emit_status_changed(&self.sessions.snapshot());
        } else if !prev.enabled || prev.preferred_port != updated.preferred_port {
            self.stop_server().await;
            self.sessions.set_state(BridgeConnectionState::Disconnected);
            self.ensure_server_running().await?;
            let _ = self.probe_and_handshake_peer().await;
            self.sink.emit_status_changed(&self.sessions.snapshot());
        }

        Ok(updated)
    }

    /// Força releitura do arquivo de descoberta `maxcode.endpoint.json` e handshake imediato (`bridge_reconnect_now`).
    pub async fn reconnect_now(&self) -> Result<BridgeStatusSnapshot, BridgeError> {
        if !self.settings.read().enabled {
            return Err(BridgeError::new(
                BridgeErrorCode::BridgeDisabled,
                "A integração com o MaxCode está desabilitada nas preferências.",
            ));
        }

        self.ensure_server_running().await?;
        let _ = self.probe_and_handshake_peer().await;
        let snap = self.sessions.snapshot();
        self.sink.emit_status_changed(&snap);
        Ok(snap)
    }

    /// Envia um comando com erro para inspeção em Plan Mode no `MaxCode` (`bridge_inspect_command_error`).
    pub async fn inspect_command_error(
        &self,
        block_id: &str,
    ) -> Result<InspectDispatchResult, BridgeError> {
        let settings = self.settings.read().clone();
        if !settings.enabled {
            return Err(BridgeError::new(
                BridgeErrorCode::BridgeDisabled,
                "A integração com o MaxCode está desabilitada.",
            ));
        }

        let ctx = self.host.load_inspectable_block(block_id).await?;
        if ctx.exit_code == Some(0) && ctx.signal.is_none() {
            return Err(BridgeError::new(
                BridgeErrorCode::BlockDidNotFail,
                "O comando selecionado terminou com código 0 e não possui erro para inspecionar.",
            ));
        }

        let prepared = prepare_inspect_payload(&ctx.command, &ctx.error_output);
        let exit_status = if let Some(sig_name) = ctx.signal {
            InspectErrorCode::Signal {
                signal: ctx.exit_code.unwrap_or(15),
                name: sig_name,
            }
        } else {
            InspectErrorCode::ExitCode {
                code: ctx.exit_code.unwrap_or(1),
            }
        };

        let payload = InspectCommandErrorPayload {
            block_id: ctx.block_id,
            session_id: ctx.session_id,
            tab_id: ctx.tab_id,
            project_path: ctx.project_path,
            cwd: ctx.cwd,
            command: prepared.command,
            error_output: prepared.error_output,
            exit_status,
            duration_ms: ctx.duration_ms,
            mode: "plan".to_string(),
            formatted_prompt: prepared.formatted_prompt.clone(),
        };

        let envelope = RpcEnvelope::new(RpcPayload::InspectCommandError(payload.clone()));
        let request_id = envelope.request_id;

        if let Ok(reply) = self.send_envelope_and_wait_ack(&envelope).await {
            match reply {
                RpcPayload::InspectAck(InspectAckPayload {
                    accepted: true,
                    conversation_id,
                    ..
                }) => {
                    return Ok(InspectDispatchResult::Loopback {
                        request_id,
                        conversation_id,
                        formatted_prompt: prepared.formatted_prompt,
                    });
                }
                RpcPayload::InspectAck(InspectAckPayload {
                    accepted: false,
                    error,
                    ..
                }) => {
                    return Err(BridgeError::new(
                        BridgeErrorCode::InvalidEnvelope,
                        error.unwrap_or_else(|| {
                            "O MaxCode rejeitou a solicitação de inspeção.".to_string()
                        }),
                    ));
                }
                _ => {}
            }
        }

        // Fallback conforme configuração do usuário
        match settings.inspect_fallback_mode {
            InspectFallbackMode::UriScheme => {
                let artifact_dir = resolve_discovery_dir(self.discovery_dir_override.as_deref())?;
                let uri = build_vscode_inspect_uri(request_id, &payload, &artifact_dir)?;
                self.host.launch_external_uri(&uri).await?;
                Ok(InspectDispatchResult::UriFallback {
                    request_id,
                    uri,
                    formatted_prompt: prepared.formatted_prompt,
                })
            }
            InspectFallbackMode::ClipboardOnly => Ok(InspectDispatchResult::ClipboardOnly {
                request_id,
                formatted_prompt: prepared.formatted_prompt,
            }),
            InspectFallbackMode::Disabled => Err(BridgeError::new(
                BridgeErrorCode::PeerDisconnected,
                "Não foi possível conectar ao MaxCode via loopback e o fallback de URI está desabilitado.",
            )),
        }
    }

    /// Notifica o `MaxCode` sobre a troca de projeto ativo no `MaxTerminal` (`bridge_notify_project_selected`).
    pub async fn notify_project_selected(
        &self,
        project_path: &str,
        project_name: Option<String>,
        git_branch: Option<String>,
    ) -> Result<bool, BridgeError> {
        let settings = self.settings.read().clone();
        if !settings.enabled || !settings.auto_sync_active_project {
            return Ok(false);
        }

        let decision = self.inbound_ctx.sync_decider.decide_outbound(
            &settings,
            project_path,
            project_name,
            git_branch,
            Instant::now(),
        )?;

        let OutboundSyncDecision::Send(payload) = decision else {
            return Ok(false);
        };

        let envelope = RpcEnvelope::new(RpcPayload::ProjectOpened(payload));
        match self.send_envelope_fire_and_forget(&envelope).await {
            Ok(()) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Solicita ao `MaxCode` a abertura de um arquivo em linha e coluna (`bridge_open_file_in_maxcode`).
    pub async fn open_file_in_maxcode(
        &self,
        mut payload: OpenFilePayload,
    ) -> Result<bool, BridgeError> {
        let settings = self.settings.read().clone();
        if !settings.enabled {
            return Err(BridgeError::new(
                BridgeErrorCode::BridgeDisabled,
                "A integração com o MaxCode está desabilitada.",
            ));
        }

        let canonical = validate_file_path_for_open(&payload.file_path)?;
        payload.file_path = canonical.to_string_lossy().into_owned();

        let envelope = RpcEnvelope::new(RpcPayload::OpenFile(payload.clone()));
        if self.send_envelope_fire_and_forget(&envelope).await.is_ok() {
            return Ok(true);
        }

        let uri = build_vscode_open_file_uri(envelope.request_id, &payload)?;
        self.host.launch_external_uri(&uri).await?;
        Ok(false)
    }

    /// Processa uma URI `maxterminal://...` recebida via deep-link / single-instance (`mt-os`).
    #[allow(clippy::too_many_lines)]
    pub async fn handle_inbound_uri(&self, raw_uri: &str) -> Result<(), BridgeError> {
        let settings = self.settings.read().clone();
        if !settings.enabled {
            return Err(BridgeError::new(
                BridgeErrorCode::BridgeDisabled,
                "A integração com o MaxCode está desabilitada.",
            ));
        }

        let parsed = parse_maxterminal_uri(raw_uri, current_unix_ms())?;
        match parsed {
            ParsedMaxTerminalUri::OpenProject {
                request_id,
                payload,
            } => {
                if !self.inbound_ctx.dedup.check_and_insert(request_id) {
                    return Ok(());
                }
                let canonical =
                    crate::paths::normalize_existing_project_path(&payload.project_path)?;
                if !settings.auto_register_unknown_project
                    && self.host.find_project_by_path(&canonical).await?.is_none()
                {
                    return Err(BridgeError::new(
                        BridgeErrorCode::ProjectNotFound,
                        "Projeto não encontrado no catálogo do MaxTerminal.",
                    ));
                }
                let outcome = apply_inbound_project_opened(
                    &self.inbound_ctx.sync_decider,
                    &settings,
                    self.host.as_ref(),
                    &payload,
                )
                .await?;
                match outcome {
                    InboundSyncOutcome::FocusedExisting(proj) => {
                        self.sink.emit_project_synced(&BridgeProjectSyncedEvent {
                            project_id: proj.id,
                            project_path: proj.path,
                            created: false,
                        });
                    }
                    InboundSyncOutcome::CreatedAndFocused(proj) => {
                        self.sink.emit_project_synced(&BridgeProjectSyncedEvent {
                            project_id: proj.id,
                            project_path: proj.path,
                            created: true,
                        });
                    }
                    InboundSyncOutcome::Ignored(_) => {}
                }
                if settings.focus_window_on_incoming_command {
                    let _ = self.host.bring_window_to_front().await;
                }
                Ok(())
            }
            ParsedMaxTerminalUri::OpenTerminalForIssue {
                request_id,
                payload,
            } => {
                let project_canon =
                    crate::paths::normalize_existing_project_path(&payload.project_path)?;
                if !settings.auto_register_unknown_project
                    && self
                        .host
                        .find_project_by_path(&project_canon)
                        .await?
                        .is_none()
                {
                    return Err(BridgeError::new(
                        BridgeErrorCode::ProjectNotFound,
                        "Projeto não encontrado no catálogo do MaxTerminal.",
                    ));
                }
                if let Some(ref wt) = payload.worktree_path {
                    let wt_canon = crate::paths::normalize_existing_project_path(wt)?;
                    let inside_project = wt_canon.starts_with(&project_canon);
                    let sibling_worktree = project_canon.parent().is_some_and(|parent| {
                        wt_canon.starts_with(parent.join(".worktrees"))
                            || wt_canon.starts_with(parent.join(".max-code-worktrees"))
                    });
                    if !inside_project && !sibling_worktree {
                        return Err(BridgeError::new(
                            BridgeErrorCode::CwdOutsideProject,
                            "O diretório de worktree informado está fora do projeto autorizado.",
                        ));
                    }
                }
                let env =
                    RpcEnvelope::with_id(request_id, RpcPayload::OpenTerminalForIssue(payload));
                let _ = dispatch_inbound(&self.inbound_ctx, &settings, env).await?;
                Ok(())
            }
            ParsedMaxTerminalUri::Focus { request_id } => {
                if !self.inbound_ctx.dedup.check_and_insert(request_id) {
                    return Ok(());
                }
                self.host.bring_window_to_front().await
            }
        }
    }

    /// Retorna o suporte às capabilities do protocolo de Issues (`issues_protocol_v1` e `bugs_cycle_v1`).
    #[must_use]
    pub fn issue_protocol_support(&self) -> IssueProtocolSupport {
        let caps = self.sessions.peer_capabilities();
        IssueProtocolSupport {
            issues_protocol_v1: caps.iter().any(|c| c == CAP_ISSUES_PROTOCOL_V1),
            bugs_cycle_v1: caps.iter().any(|c| c == CAP_BUGS_CYCLE_V1),
        }
    }

    /// Despacha uma ação unitária de Issue (`Abrir`, `Analisar (BUG PLAN)`, `Corrigir (BUG FIX)`)
    /// para o `MaxCode` com fallback automático para `vscode://johnattas.maxcode/issue/...`.
    pub async fn send_issue_command(
        &self,
        payload: IssueCommandPayload,
    ) -> Result<IssueActionDispatchOutcome, BridgeError> {
        let settings = self.settings.read().clone();
        if !settings.enabled {
            return Err(BridgeError::new(
                BridgeErrorCode::BridgeDisabled,
                "A integração com o MaxCode está desabilitada.",
            ));
        }

        let envelope = RpcEnvelope::new(RpcPayload::IssueCommand(payload.clone()));
        let request_id = envelope.request_id;

        if let Ok(RpcPayload::IssueCommandAck(ack)) =
            self.send_envelope_and_wait_ack(&envelope).await
        {
            return Ok(IssueActionDispatchOutcome::Loopback { ack });
        }

        let uri = build_vscode_issue_action_uri(request_id, &payload)?;
        self.host.launch_external_uri(&uri).await?;
        Ok(IssueActionDispatchOutcome::UriFallback { uri })
    }

    /// Solicita uma prévia (`dry_run = true`) do ciclo automatizado de bugs em lote ao `MaxCode`.
    pub async fn bugs_cycle_preview(
        &self,
        mut payload: BugsCycleStartPayload,
    ) -> Result<BugsCycleAckPayload, BridgeError> {
        payload.dry_run = true;
        self.dispatch_bugs_cycle(payload).await
    }

    /// Inicia (`dry_run = false`) o ciclo automatizado de bugs em lote no `MaxCode`.
    pub async fn bugs_cycle_start(
        &self,
        mut payload: BugsCycleStartPayload,
    ) -> Result<BugsCycleAckPayload, BridgeError> {
        payload.dry_run = false;
        self.dispatch_bugs_cycle(payload).await
    }

    /// Consulta o último evento de progresso recebido para um `cycle_id`.
    #[must_use]
    pub fn bugs_cycle_status(&self, cycle_id: Uuid) -> Option<BugCycleProgressPayload> {
        self.cycle_snapshots.read().get(&cycle_id).cloned()
    }

    /// Cria um receptor `broadcast::Receiver` para acompanhar o progresso do ciclo de bugs em tempo real.
    #[must_use]
    pub fn subscribe_bug_cycle_progress(&self) -> broadcast::Receiver<BugCycleProgressPayload> {
        self.progress_tx.subscribe()
    }

    async fn dispatch_bugs_cycle(
        &self,
        payload: BugsCycleStartPayload,
    ) -> Result<BugsCycleAckPayload, BridgeError> {
        if !self.settings.read().enabled {
            return Err(BridgeError::new(
                BridgeErrorCode::BridgeDisabled,
                "A integração com o MaxCode está desabilitada.",
            ));
        }

        let envelope = RpcEnvelope::new(RpcPayload::BugsCycleStart(payload));
        match self.send_envelope_and_wait_ack(&envelope).await? {
            RpcPayload::BugsCycleAck(ack) => Ok(ack),
            RpcPayload::Error(err) => Err(BridgeError::new(
                BridgeErrorCode::InvalidEnvelope,
                err.message,
            )),
            _ => Err(BridgeError::new(
                BridgeErrorCode::InvalidEnvelope,
                "Resposta inesperada do MaxCode ao iniciar ciclo de bugs.",
            )),
        }
    }

    async fn ensure_server_running(&self) -> Result<(), BridgeError> {
        let mut guard = self.server_handle.lock().await;
        if guard.is_some() {
            return Ok(());
        }

        let preferred_port = self.settings.read().preferred_port;
        let token = generate_token_hex();
        let cfg = LoopbackServerConfig {
            preferred_port,
            discovery_dir_override: self.discovery_dir_override.clone(),
            token,
            instance_id: self.instance_id.clone(),
            app_version: self.app_version.clone(),
            settings: Arc::clone(&self.settings),
            sessions: Arc::clone(&self.sessions),
            inbound_ctx: Arc::clone(&self.inbound_ctx),
        };

        let handle = start_loopback_server(cfg).await?;
        *guard = Some(handle);
        Ok(())
    }

    async fn stop_server(&self) {
        let mut guard = self.server_handle.lock().await;
        if let Some(handle) = guard.take() {
            handle.stop().await;
        }
    }

    fn discover_maxcode_endpoint(&self) -> Result<DiscoveryEndpoint, BridgeError> {
        let candidates = candidate_maxcode_endpoint_paths(self.discovery_dir_override.as_deref());
        let mut last_err = BridgeError::new(
            BridgeErrorCode::DiscoveryNotFound,
            "Nenhum arquivo maxcode.endpoint.json encontrado.",
        );
        for path in candidates {
            match read_verified(&path) {
                Ok(ep) => return Ok(ep),
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }

    async fn probe_and_handshake_peer(&self) -> Result<(), BridgeError> {
        let ep = self.discover_maxcode_endpoint()?;
        let active_project_path = self.host.active_project_path().await;
        let hello = RpcEnvelope::new(RpcPayload::Hello(HelloPayload {
            protocol_version: PROTOCOL_VERSION,
            role: PeerRole::MaxTerminal,
            instance_id: self.instance_id.clone(),
            pid: std::process::id(),
            version: self.app_version.clone(),
            active_project_path,
            capabilities: default_maxterminal_capabilities(),
        }));

        let start = Instant::now();
        let (status, reply_opt) = send_http_rpc_envelope(ep.port, &ep.token, &hello).await?;
        if status == 200
            && let Some(RpcEnvelope {
                message: RpcPayload::Welcome(welcome),
                ..
            }) = reply_opt
        {
            #[allow(clippy::cast_possible_truncation)]
            let rtt = start.elapsed().as_millis() as u64;
            self.sessions.update_peer_metadata(PeerInfo {
                instance_id: welcome.instance_id,
                pid: welcome.pid,
                version: welcome.version,
                capabilities: welcome.capabilities,
            });
            self.sessions.record_rtt(rtt);
            return Ok(());
        }

        // Também realiza um Ping rápido se o peer já estiver vivo
        let seq = {
            let mut g = self.ping_seq.lock();
            *g += 1;
            *g
        };
        let ping_env = RpcEnvelope::new(RpcPayload::Ping(PingPayload { seq }));
        let (st2, pong_opt) = send_http_rpc_envelope(ep.port, &ep.token, &ping_env).await?;
        if st2 == 200
            && let Some(RpcEnvelope {
                message: RpcPayload::Pong(_),
                ..
            }) = pong_opt
        {
            self.sessions.update_peer_metadata(PeerInfo {
                instance_id: ep.instance_id,
                pid: ep.pid,
                version: "1.0.0".to_string(),
                capabilities: ep.capabilities,
            });
            return Ok(());
        }

        Err(BridgeError::new(
            BridgeErrorCode::PeerDisconnected,
            "O endpoint do MaxCode não respondeu ao handshake.",
        ))
    }

    async fn send_envelope_fire_and_forget(
        &self,
        envelope: &RpcEnvelope,
    ) -> Result<(), BridgeError> {
        if self.sessions.try_send_ws(envelope.clone()).await.is_ok() {
            return Ok(());
        }

        let ep = self.discover_maxcode_endpoint()?;
        let (status, _) = send_http_rpc_envelope(ep.port, &ep.token, envelope).await?;
        if (200..300).contains(&status) {
            Ok(())
        } else {
            Err(BridgeError::new(
                BridgeErrorCode::PeerDisconnected,
                format!("O servidor loopback do MaxCode retornou HTTP {status}."),
            ))
        }
    }

    async fn send_envelope_and_wait_ack(
        &self,
        envelope: &RpcEnvelope,
    ) -> Result<RpcPayload, BridgeError> {
        // 1. Tenta enviar pela sessão WebSocket ativa e aguardar o oneshot de ACK
        let rx = self.sessions.register_pending_ack(envelope.request_id);
        if self.sessions.try_send_ws(envelope.clone()).await.is_ok() {
            match tokio::time::timeout(DEFAULT_ACK_TIMEOUT, rx).await {
                Ok(Ok(payload)) => return Ok(payload),
                _ => {
                    self.sessions.remove_pending_ack(&envelope.request_id);
                }
            }
        } else {
            self.sessions.remove_pending_ack(&envelope.request_id);
        }

        // 2. Tenta via HTTP POST /v1/rpc no endpoint descoberto do MaxCode
        let ep = self.discover_maxcode_endpoint()?;
        let rpc_fut = send_http_rpc_envelope(ep.port, &ep.token, envelope);
        let (status, reply_opt) = tokio::time::timeout(DEFAULT_ACK_TIMEOUT, rpc_fut)
            .await
            .map_err(|_| {
                BridgeError::new(
                    BridgeErrorCode::AckTimeout,
                    "Timeout de 3 segundos aguardando confirmação do MaxCode.",
                )
            })??;

        if (200..300).contains(&status)
            && let Some(reply_env) = reply_opt
        {
            return Ok(reply_env.message);
        }

        Err(BridgeError::new(
            BridgeErrorCode::PeerDisconnected,
            "Não foi possível obter confirmação do MaxCode via loopback.",
        ))
    }
}
