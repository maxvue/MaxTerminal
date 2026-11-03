//! Crate `mt-bridge` — Integração bidirecional em tempo real entre `MaxTerminal` e a extensão `MaxCode`.
//!
//! Fornece:
//! - Servidor e cliente HTTP/1.1 + WebSocket RFC 6455 em loopback (`127.0.0.1`) autenticados por token CSPRNG de 256 bits (`Authorization: Bearer`);
//! - Descoberta atômica de endpoints (`maxterminal.endpoint.json` e `maxcode.endpoint.json`) com permissões `0700`/`0600`, verificação de UID e PID vivo;
//! - Parser e construtor estrito das URIs `maxterminal://...` e `vscode://johnattas.maxcode/...` com TTL de 60 s e fallback em arquivo `0600`;
//! - Sincronização bidirecional de projeto ativo com máquina anti-eco determinística;
//! - Inspeção de erros de comando (`exit != 0`) abrindo conversa em **Plan Mode** no `MaxCode` com prompt canônico sanitizado;
//! - Contrato completo do protocolo de Issues e Ciclo Automatizado de Bugs (`issues_protocol_v1` e `bugs_cycle_v1`).

pub mod auth;
pub mod client;
pub mod config;
pub mod dedup;
pub mod discovery;
pub mod error;
pub mod handler;
pub mod issues;
pub mod paths;
pub mod prompt;
pub mod protocol;
pub mod server;
pub mod session;
pub mod sync;
pub mod uri;

pub use auth::{
    generate_token_hex, is_allowed_origin, verify_request_headers, verify_token_constant_time,
};
pub use client::{BridgeService, InMemoryBridgeSettingsStore, InspectDispatchResult};
pub use config::{BridgeSettings, BridgeSettingsPatch, BridgeSettingsStore, InspectFallbackMode};
pub use dedup::RecentRequestSet;
pub use discovery::{
    BRIDGE_SUBDIR, DiscoveryEndpoint, MAXCODE_ENDPOINT_FILE, MAXTERMINAL_ENDPOINT_FILE,
    candidate_maxcode_endpoint_paths, ensure_secure_dir, is_pid_alive, read_verified,
    remove_if_owned, resolve_discovery_dir, write_atomic,
};
pub use error::{BridgeError, BridgeErrorCode};
pub use handler::{
    BridgeEventSink, BridgeHost, BridgeProjectSummary, BridgeProjectSyncedEvent,
    BridgeTerminalOpenedEvent, InboundContext, InspectableBlockContext, NoopBridgeEventSink,
    OpenTerminalRequest, OpenedTerminalInfo, dispatch_inbound, validate_hello,
};
pub use issues::{
    BugCycleItemStage, BugCycleOverallStatus, BugCycleProgressPayload, BugCycleStepStatus,
    BugsCycleAckPayload, BugsCycleFilters, BugsCycleSkipEntry, BugsCycleStartPayload,
    IssueAckStatus, IssueActionDispatchOutcome, IssueCommandAckPayload, IssueCommandKind,
    IssueCommandPayload, IssueProtocolSupport, IssueStage,
};
pub use paths::{normalize_existing_project_path, validate_file_path_for_open};
pub use prompt::{
    MAX_PROMPT_COMMAND_BYTES, MAX_PROMPT_ERROR_BYTES, PreparedInspectPrompt, TRUNCATION_SUFFIX,
    format_plan_mode_inspect_prompt, prepare_inspect_payload,
};
pub use protocol::{
    AgentKind, BridgeConnectionState, BridgeStatusSnapshot, CAP_BUGS_CYCLE_V1, CAP_INSPECT_V1,
    CAP_ISSUES_PROTOCOL_V1, CAP_OPEN_FILE_V1, CAP_PING_V1, CAP_SYNC_PROJECT_V1, ErrorPayload,
    HelloPayload, InspectAckPayload, InspectCommandErrorPayload, InspectErrorCode,
    MAX_ENVELOPE_BYTES, OpenFilePayload, OpenTerminalForIssuePayload, PROTOCOL_VERSION, PeerRole,
    PingPayload, PongPayload, ProjectOpenedPayload, RpcEnvelope, RpcPayload,
    TerminalOpenedAckPayload, WelcomePayload, current_unix_ms, default_maxterminal_capabilities,
};
pub use server::{
    LoopbackServerConfig, LoopbackServerHandle, WsFrame, compute_ws_accept_key, read_ws_frame,
    send_http_rpc_envelope, start_loopback_server, write_ws_close, write_ws_frame, write_ws_text,
};
pub use session::{DEFAULT_ACK_TIMEOUT, PeerInfo, SessionRegistry};
pub use sync::{
    InboundSyncOutcome, OutboundSyncDecision, ProjectSyncDecider, apply_inbound_project_opened,
};
pub use uri::{
    InspectFallbackArtifact, MAX_PROMPT_INLINE_BYTES, MAX_URI_TOTAL_LEN, ParsedMaxTerminalUri,
    ParsedVsCodeMaxCodeUri, URI_TTL_MS, build_vscode_inspect_uri, build_vscode_issue_action_uri,
    build_vscode_open_file_uri, build_vscode_project_sync_uri, parse_maxterminal_uri,
    parse_vscode_maxcode_uri, write_inspect_artifact_file,
};
pub use url::Url;
