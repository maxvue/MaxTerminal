//! Testes de integração do servidor loopback HTTP/WebSocket e do `BridgeService` (`server_it.rs`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use mt_bridge::{
    BridgeConnectionState, BridgeError, BridgeErrorCode, BridgeHost, BridgeProjectSummary,
    BridgeService, BridgeSettings, DiscoveryEndpoint, HelloPayload, InMemoryBridgeSettingsStore,
    InspectAckPayload, InspectDispatchResult, InspectFallbackMode, InspectableBlockContext,
    MAXCODE_ENDPOINT_FILE, NoopBridgeEventSink, OpenTerminalForIssuePayload, OpenTerminalRequest,
    OpenedTerminalInfo, PROTOCOL_VERSION, PeerRole, PingPayload, ProjectOpenedPayload, RpcEnvelope,
    RpcPayload, WelcomePayload, WsFrame, current_unix_ms, default_maxterminal_capabilities,
    generate_token_hex, read_ws_frame, send_http_rpc_envelope, write_atomic, write_ws_text,
};
use parking_lot::Mutex;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct MockBridgeHost {
    projects: Mutex<Vec<BridgeProjectSummary>>,
    focused_project: Mutex<Option<String>>,
    opened_terminals: Mutex<Vec<OpenTerminalRequest>>,
    launched_uris: Mutex<Vec<String>>,
    block_ctx: InspectableBlockContext,
}

impl MockBridgeHost {
    fn new(project_path: &Path) -> Self {
        let canon = std::fs::canonicalize(project_path)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        Self {
            projects: Mutex::new(vec![BridgeProjectSummary {
                id: "proj-1".to_string(),
                title: "MaxTerminal".to_string(),
                path: canon.clone(),
            }]),
            focused_project: Mutex::new(None),
            opened_terminals: Mutex::new(Vec::new()),
            launched_uris: Mutex::new(Vec::new()),
            block_ctx: InspectableBlockContext {
                block_id: "blk-err-1".to_string(),
                session_id: "sess-1".to_string(),
                tab_id: "tab-1".to_string(),
                project_path: canon.clone(),
                cwd: canon,
                command: "php artisan migrate".to_string(),
                error_output: "SQLSTATE[HY000] [2002] Connection refused".to_string(),
                exit_code: Some(1),
                signal: None,
                duration_ms: Some(180),
            },
        }
    }
}

#[async_trait]
impl BridgeHost for MockBridgeHost {
    async fn active_project_path(&self) -> Option<String> {
        self.focused_project.lock().clone()
    }

    async fn find_project_by_path(
        &self,
        canonical_path: &Path,
    ) -> Result<Option<BridgeProjectSummary>, BridgeError> {
        let target = canonical_path.to_string_lossy();
        Ok(self
            .projects
            .lock()
            .iter()
            .find(|p| p.path == target)
            .cloned())
    }

    async fn create_project_for_path(
        &self,
        canonical_path: &Path,
        preferred_name: Option<&str>,
    ) -> Result<BridgeProjectSummary, BridgeError> {
        let mut guard = self.projects.lock();
        let summary = BridgeProjectSummary {
            id: format!("proj-{}", guard.len() + 1),
            title: preferred_name.unwrap_or("Novo").to_string(),
            path: canonical_path.to_string_lossy().into_owned(),
        };
        guard.push(summary.clone());
        Ok(summary)
    }

    async fn focus_project(&self, project_id: &str) -> Result<(), BridgeError> {
        let path = self
            .projects
            .lock()
            .iter()
            .find(|p| p.id == project_id)
            .map(|p| p.path.clone());
        *self.focused_project.lock() = path;
        Ok(())
    }

    async fn open_terminal_for_issue(
        &self,
        request: OpenTerminalRequest,
    ) -> Result<OpenedTerminalInfo, BridgeError> {
        self.opened_terminals.lock().push(request.clone());
        Ok(OpenedTerminalInfo {
            project_id: request.project_id,
            tab_id: "tab-issue-42".to_string(),
        })
    }

    async fn load_inspectable_block(
        &self,
        block_id: &str,
    ) -> Result<InspectableBlockContext, BridgeError> {
        if block_id == self.block_ctx.block_id {
            Ok(self.block_ctx.clone())
        } else {
            Err(BridgeError::new(
                BridgeErrorCode::BlockNotFound,
                "Bloco não encontrado.",
            ))
        }
    }

    async fn bring_window_to_front(&self) -> Result<(), BridgeError> {
        Ok(())
    }

    async fn launch_external_uri(&self, uri: &str) -> Result<(), BridgeError> {
        self.launched_uris.lock().push(uri.to_string());
        Ok(())
    }
}

#[tokio::test]
async fn server_rejects_unauthorized_and_evil_origin_and_accepts_valid_rpc() {
    let tmp = TempDir::new().unwrap();
    let proj_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&proj_dir).unwrap();

    let host = Arc::new(MockBridgeHost::new(&proj_dir));
    let store = Arc::new(InMemoryBridgeSettingsStore::new(BridgeSettings::default()));
    let sink = Arc::new(NoopBridgeEventSink);
    let disc_dir = tmp.path().join("max-bridge");

    let svc = BridgeService::new("0.1.0", store, host.clone(), sink, Some(disc_dir.clone()))
        .await
        .expect("start service");

    let status = svc.status();
    let port = status.server_port.expect("porta loopback");

    // 1. Token inválido deve retornar 401
    let ping = RpcEnvelope::new(RpcPayload::Ping(PingPayload { seq: 1 }));
    let (code_unauth, _) = send_http_rpc_envelope(port, "token-invalido", &ping)
        .await
        .unwrap();
    assert_eq!(code_unauth, 401);

    // 2. Origin de navegador malicioso deve retornar 403 mesmo em /v1/health
    let mut raw_stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    raw_stream
        .write_all(
            b"GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1\r\nOrigin: https://evil.example.com\r\nConnection: close\r\n\r\n",
        )
        .await
        .unwrap();
    let mut resp = String::new();
    raw_stream.read_to_string(&mut resp).await.unwrap();
    assert!(resp.starts_with("HTTP/1.1 403"));

    // 3. Lê o token real publicado no arquivo de descoberta e envia Hello + ProjectOpened + OpenTerminalForIssue
    let ep_file = disc_dir.join("maxterminal.endpoint.json");
    let ep = mt_bridge::read_verified(&ep_file).unwrap();

    let hello = RpcEnvelope::new(RpcPayload::Hello(HelloPayload {
        protocol_version: PROTOCOL_VERSION,
        role: PeerRole::MaxCode,
        instance_id: "maxcode-1".to_string(),
        pid: std::process::id(),
        version: "1.2.3".to_string(),
        active_project_path: None,
        capabilities: default_maxterminal_capabilities(),
    }));
    let (code_ok, welcome_opt) = send_http_rpc_envelope(port, &ep.token, &hello)
        .await
        .unwrap();
    assert_eq!(code_ok, 200);
    assert!(matches!(
        welcome_opt.map(|e| e.message),
        Some(RpcPayload::Welcome(_))
    ));
    assert_eq!(svc.status().state, BridgeConnectionState::Connected);

    // Envia OpenTerminalForIssue e recebe TerminalOpenedAck
    let open_term = RpcEnvelope::new(RpcPayload::OpenTerminalForIssue(
        OpenTerminalForIssuePayload {
            project_path: proj_dir.to_string_lossy().into_owned(),
            worktree_path: None,
            branch: Some("fix/42".to_string()),
            issue_number: 42,
            issue_title: Some("Corrigir bug".to_string()),
            agent: None,
            initial_command: None,
        },
    ));
    let (code_term, ack_opt) = send_http_rpc_envelope(port, &ep.token, &open_term)
        .await
        .unwrap();
    assert_eq!(code_term, 200);
    match ack_opt.map(|e| e.message) {
        Some(RpcPayload::TerminalOpenedAck(ack)) => {
            assert!(ack.accepted);
            assert_eq!(ack.tab_id.as_deref(), Some("tab-issue-42"));
        }
        other => panic!("esperado TerminalOpenedAck, recebido: {other:?}"),
    }

    // Envia ProjectOpened
    let proj_open = RpcEnvelope::new(RpcPayload::ProjectOpened(ProjectOpenedPayload {
        project_path: proj_dir.to_string_lossy().into_owned(),
        project_name: Some("MaxTerminal".to_string()),
        git_branch: Some("main".to_string()),
        origin_id: "maxcode".to_string(),
    }));
    let (code_proj, _) = send_http_rpc_envelope(port, &ep.token, &proj_open)
        .await
        .unwrap();
    assert_eq!(code_proj, 200);
    assert!(host.active_project_path().await.is_some());
}

#[tokio::test]
async fn websocket_upgrade_handshake_and_oversized_frame_rejection() {
    let tmp = TempDir::new().unwrap();
    let proj_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&proj_dir).unwrap();

    let host = Arc::new(MockBridgeHost::new(&proj_dir));
    let store = Arc::new(InMemoryBridgeSettingsStore::new(BridgeSettings::default()));
    let sink = Arc::new(NoopBridgeEventSink);
    let disc_dir = tmp.path().join("max-bridge");

    let svc = BridgeService::new("0.1.0", store, host, sink, Some(disc_dir.clone()))
        .await
        .unwrap();
    let ep = mt_bridge::read_verified(&disc_dir.join("maxterminal.endpoint.json")).unwrap();

    let mut stream = TcpStream::connect(("127.0.0.1", ep.port)).await.unwrap();
    let upgrade_req = format!(
        "GET /v1/ws HTTP/1.1\r\n\
         Host: 127.0.0.1:{}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Authorization: Bearer {}\r\n\
         Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
         Sec-WebSocket-Version: 13\r\n\r\n",
        ep.port, ep.token
    );
    stream.write_all(upgrade_req.as_bytes()).await.unwrap();

    let mut head_buf = [0u8; 512];
    let n = stream.read(&mut head_buf).await.unwrap();
    let head_str = std::str::from_utf8(&head_buf[..n]).unwrap();
    assert!(head_str.starts_with("HTTP/1.1 101 Switching Protocols"));
    assert!(head_str.contains("s3pPLMBiTxaQ9kYGzzhZRbK+xOo="));

    // Envia frame Hello mascarado
    let hello = RpcEnvelope::new(RpcPayload::Hello(HelloPayload {
        protocol_version: PROTOCOL_VERSION,
        role: PeerRole::MaxCode,
        instance_id: "ws-peer-1".to_string(),
        pid: std::process::id(),
        version: "2.0.0".to_string(),
        active_project_path: None,
        capabilities: default_maxterminal_capabilities(),
    }));
    write_ws_text(&mut stream, &serde_json::to_string(&hello).unwrap(), true)
        .await
        .unwrap();

    let welcome_frame = read_ws_frame(&mut stream).await.unwrap();
    match welcome_frame {
        WsFrame::Text(txt) => {
            let env: RpcEnvelope = serde_json::from_str(&txt).unwrap();
            assert!(matches!(env.message, RpcPayload::Welcome(_)));
        }
        other => panic!("esperado Text Welcome, recebido: {other:?}"),
    }
    assert_eq!(svc.status().state, BridgeConnectionState::Connected);
}

#[tokio::test]
async fn inspect_command_error_delivers_via_loopback_and_falls_back_to_uri_when_offline() {
    let tmp = TempDir::new().unwrap();
    let proj_dir = tmp.path().join("repo");
    std::fs::create_dir_all(&proj_dir).unwrap();

    let host = Arc::new(MockBridgeHost::new(&proj_dir));
    let store = Arc::new(InMemoryBridgeSettingsStore::new(BridgeSettings {
        inspect_fallback_mode: InspectFallbackMode::UriScheme,
        ..BridgeSettings::default()
    }));
    let sink = Arc::new(NoopBridgeEventSink);
    let disc_dir = tmp.path().join("max-bridge");

    let svc = BridgeService::new("0.1.0", store, host.clone(), sink, Some(disc_dir.clone()))
        .await
        .unwrap();

    // 1. Sem MaxCode rodando -> deve cair no fallback de URI vscode://johnattas.maxcode/inspect
    let res_fallback = svc
        .inspect_command_error("blk-err-1")
        .await
        .expect("inspect fallback");
    match res_fallback {
        InspectDispatchResult::UriFallback {
            uri,
            formatted_prompt,
            ..
        } => {
            assert!(uri.starts_with("vscode://johnattas.maxcode/inspect?"));
            assert_eq!(
                formatted_prompt,
                "Ao executar o comando \"php artisan migrate\" tive o seguinte erro: \"SQLSTATE[HY000] [2002] Connection refused\""
            );
        }
        other => panic!("esperado UriFallback, recebido {other:?}"),
    }
    assert_eq!(host.launched_uris.lock().len(), 1);

    // 2. Simula um servidor loopback do MaxCode publicado em maxcode.endpoint.json
    let maxcode_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let maxcode_port = maxcode_listener.local_addr().unwrap().port();
    let maxcode_token = generate_token_hex();
    write_atomic(
        &disc_dir.join(MAXCODE_ENDPOINT_FILE),
        &DiscoveryEndpoint {
            protocol_version: PROTOCOL_VERSION,
            role: PeerRole::MaxCode,
            instance_id: "maxcode-sim".to_string(),
            pid: std::process::id(),
            host: "127.0.0.1".to_string(),
            port: maxcode_port,
            ws_path: "/v1/ws".to_string(),
            http_rpc_path: "/v1/rpc".to_string(),
            token: maxcode_token,
            started_at_ms: current_unix_ms(),
            capabilities: default_maxterminal_capabilities(),
        },
    )
    .unwrap();

    tokio::spawn(async move {
        if let Ok((mut s, _)) = maxcode_listener.accept().await {
            let mut buf = vec![0u8; 8192];
            let n = s.read(&mut buf).await.unwrap_or(0);
            let raw = std::str::from_utf8(&buf[..n]).unwrap_or_default();
            if let Some(body_str) = raw.split("\r\n\r\n").nth(1)
                && let Ok(env) = serde_json::from_str::<RpcEnvelope>(body_str)
            {
                let ack = RpcEnvelope::with_id(
                    env.request_id,
                    RpcPayload::InspectAck(InspectAckPayload {
                        request_id: env.request_id,
                        accepted: true,
                        conversation_id: Some("conv-plan-99".to_string()),
                        error: None,
                    }),
                );
                let body = serde_json::to_vec(&ack).unwrap();
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = s.write_all(resp.as_bytes()).await;
                let _ = s.write_all(&body).await;
            }
        }
    });

    let res_loopback = svc
        .inspect_command_error("blk-err-1")
        .await
        .expect("inspect loopback");
    match res_loopback {
        InspectDispatchResult::Loopback {
            conversation_id,
            formatted_prompt,
            ..
        } => {
            assert_eq!(conversation_id.as_deref(), Some("conv-plan-99"));
            assert_eq!(
                formatted_prompt,
                "Ao executar o comando \"php artisan migrate\" tive o seguinte erro: \"SQLSTATE[HY000] [2002] Connection refused\""
            );
        }
        other => panic!("esperado Loopback, recebido {other:?}"),
    }
    let _ = WelcomePayload {
        protocol_version: PROTOCOL_VERSION,
        role: PeerRole::MaxCode,
        instance_id: String::new(),
        pid: 1,
        version: String::new(),
        active_project_path: None,
        capabilities: Vec::new(),
    };
}
