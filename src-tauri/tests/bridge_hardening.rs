//! Testes de endurecimento do servidor loopback HTTP/WebSocket da ponte `MaxCode` (`CA-33` — Fase 17).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines
)]

use std::fs;
use std::net::Ipv4Addr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use mt_bridge::{
    BridgeError, BridgeErrorCode, BridgeHost, BridgeProjectSummary, BridgeService, BridgeSettings,
    InMemoryBridgeSettingsStore, InspectableBlockContext, NoopBridgeEventSink, OpenTerminalRequest,
    OpenedTerminalInfo, PingPayload, RpcEnvelope, RpcPayload, send_http_rpc_envelope,
};
use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;

struct TempTestDir(PathBuf);

impl TempTestDir {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!("mt-bridge-hard-{}", Uuid::now_v7()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct DummyBridgeHost {
    project: BridgeProjectSummary,
}

impl DummyBridgeHost {
    fn new(project_path: &Path) -> Self {
        let canon = fs::canonicalize(project_path)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        Self {
            project: BridgeProjectSummary {
                id: "proj-1".to_string(),
                title: "MaxTerminal".to_string(),
                path: canon,
            },
        }
    }
}

#[async_trait]
impl BridgeHost for DummyBridgeHost {
    async fn active_project_path(&self) -> Option<String> {
        Some(self.project.path.clone())
    }

    async fn find_project_by_path(
        &self,
        canonical_path: &Path,
    ) -> Result<Option<BridgeProjectSummary>, BridgeError> {
        if canonical_path.to_string_lossy() == self.project.path {
            Ok(Some(self.project.clone()))
        } else {
            Ok(None)
        }
    }

    async fn create_project_for_path(
        &self,
        canonical_path: &Path,
        preferred_name: Option<&str>,
    ) -> Result<BridgeProjectSummary, BridgeError> {
        Ok(BridgeProjectSummary {
            id: "proj-created".to_string(),
            title: preferred_name.unwrap_or("Novo").to_string(),
            path: canonical_path.to_string_lossy().into_owned(),
        })
    }

    async fn focus_project(&self, _project_id: &str) -> Result<(), BridgeError> {
        Ok(())
    }

    async fn open_terminal_for_issue(
        &self,
        request: OpenTerminalRequest,
    ) -> Result<OpenedTerminalInfo, BridgeError> {
        Ok(OpenedTerminalInfo {
            project_id: request.project_id,
            tab_id: "tab-1".to_string(),
        })
    }

    async fn load_inspectable_block(
        &self,
        _block_id: &str,
    ) -> Result<InspectableBlockContext, BridgeError> {
        Err(BridgeError::new(
            BridgeErrorCode::BlockNotFound,
            "Bloco inexistente.",
        ))
    }

    async fn bring_window_to_front(&self) -> Result<(), BridgeError> {
        Ok(())
    }

    async fn launch_external_uri(&self, _uri: &str) -> Result<(), BridgeError> {
        Ok(())
    }
}

async fn send_raw_http(port: u16, raw_request: &[u8]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    stream.write_all(raw_request).await.unwrap();
    let mut resp = String::new();
    stream.read_to_string(&mut resp).await.unwrap();
    resp
}

#[tokio::test]
async fn bridge_loopback_hardening_enforces_bind_bearer_host_origin_and_payload_limits() {
    let tmp = TempTestDir::new();
    let proj_dir = tmp.path().join("repo");
    fs::create_dir_all(&proj_dir).unwrap();

    let host = Arc::new(DummyBridgeHost::new(&proj_dir));
    let store = Arc::new(InMemoryBridgeSettingsStore::new(BridgeSettings::default()));
    let sink = Arc::new(NoopBridgeEventSink);
    let disc_dir = tmp.path().join("max-bridge");

    let svc = BridgeService::new("0.1.0", store, host, sink, Some(disc_dir.clone()))
        .await
        .expect("start bridge service");
    let status = svc.status();
    let port = status.server_port.expect("porta ativa");

    // 1. Endpoint publicado deve estar vinculado exclusivamente a 127.0.0.1 (nunca 0.0.0.0)
    let ep = mt_bridge::read_verified(&disc_dir.join("maxterminal.endpoint.json")).unwrap();
    assert_eq!(ep.host, "127.0.0.1");
    assert_ne!(ep.host, Ipv4Addr::UNSPECIFIED.to_string());
    assert_eq!(ep.port, port);

    let rebind = TcpListener::bind(("127.0.0.1", port)).await;
    assert!(rebind.is_err());

    // 2. /v1/health funciona sem Authorization: Bearer quando Host é 127.0.0.1:<port> ou localhost:<port>
    let health_ip = send_raw_http(
        port,
        format!("GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await;
    assert!(
        health_ip.starts_with("HTTP/1.1 200"),
        "esperado 200 em /v1/health com 127.0.0.1:<port>, obtido: {health_ip}"
    );

    let health_localhost = send_raw_http(
        port,
        format!("GET /v1/health HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await;
    assert!(
        health_localhost.starts_with("HTTP/1.1 200"),
        "esperado 200 em /v1/health com localhost:<port>, obtido: {health_localhost}"
    );

    // 3. /v1/rpc, /v1/messages e /v1/ws sem Authorization ou com Bearer inválido retornam 401
    let ping = RpcEnvelope::new(RpcPayload::Ping(PingPayload { seq: 1 }));
    let (status_bad_token, _) = send_http_rpc_envelope(port, "token-incorreto", &ping)
        .await
        .unwrap();
    assert_eq!(status_bad_token, 401);

    for route in ["POST /v1/rpc", "POST /v1/messages", "GET /v1/ws"] {
        let req_no_auth = format!(
            "{route} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
        );
        let resp = send_raw_http(port, req_no_auth.as_bytes()).await;
        assert!(
            resp.starts_with("HTTP/1.1 401"),
            "rota {route} sem Bearer deve retornar 401, obtido: {resp}"
        );
    }

    // Com Bearer válido, /v1/messages aceita o envelope RPC igual a /v1/rpc
    let ping_bytes = serde_json::to_vec(&ping).unwrap();
    let req_messages_ok = format!(
        "POST /v1/messages HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        ep.token,
        ping_bytes.len()
    );
    let mut raw_msg = req_messages_ok.into_bytes();
    raw_msg.extend_from_slice(&ping_bytes);
    let resp_messages_ok = send_raw_http(port, &raw_msg).await;
    assert!(resp_messages_ok.starts_with("HTTP/1.1 200"));

    // 4. Validação anti-DNS-rebinding do header Host (incluindo /v1/health)
    let invalid_host_requests = [
        "GET /v1/health HTTP/1.1\r\nHost: evil.com\r\nConnection: close\r\n\r\n".to_string(),
        format!(
            "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
            port.wrapping_add(1)
        ),
        "GET /v1/health HTTP/1.1\r\nConnection: close\r\n\r\n".to_string(),
        format!(
            "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: evil.com\r\nConnection: close\r\n\r\n"
        ),
    ];

    for bad_req in &invalid_host_requests {
        let resp = send_raw_http(port, bad_req.as_bytes()).await;
        assert!(
            resp.starts_with("HTTP/1.1 403"),
            "Host inválido deve retornar 403 Forbidden: {bad_req:?} -> {resp}"
        );
        assert!(
            resp.contains("bridge.host_forbidden"),
            "Resposta 403 deve conter código bridge.host_forbidden: {resp}"
        );
    }

    // 5. Qualquer rota com Origin de navegador não autorizada retorna 403 Forbidden
    for evil_origin in [
        "https://evil.com",
        "http://localhost:3000",
        "chrome-extension://abcdef",
        "null",
    ] {
        let req_evil_origin = format!(
            "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: {evil_origin}\r\nConnection: close\r\n\r\n"
        );
        let resp = send_raw_http(port, req_evil_origin.as_bytes()).await;
        assert!(
            resp.starts_with("HTTP/1.1 403"),
            "Origin proibido {evil_origin} deve retornar 403, obtido: {resp}"
        );
    }

    // 6. Corpo acima de 256 KiB retorna 413 Payload Too Large
    let oversized_len = 256 * 1024 + 1024;
    let req_oversized = format!(
        "POST /v1/rpc HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {oversized_len}\r\nConnection: close\r\n\r\n",
        ep.token
    );
    let resp_oversized = send_raw_http(port, req_oversized.as_bytes()).await;
    assert!(
        resp_oversized.starts_with("HTTP/1.1 413"),
        "Payload > 256 KiB deve retornar 413, obtido: {resp_oversized}"
    );
    let _ = Mutex::new(());
}
