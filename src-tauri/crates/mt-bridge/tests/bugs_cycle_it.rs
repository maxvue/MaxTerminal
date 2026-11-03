//! Testes de integração de `send_issue_command`, `bugs_cycle_preview`, `bugs_cycle_start` e `BugCycleProgress` (`bugs_cycle_it.rs`).
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
    BridgeError, BridgeErrorCode, BridgeHost, BridgeProjectSummary, BridgeService, BridgeSettings,
    BugCycleItemStage, BugCycleOverallStatus, BugCycleProgressPayload, BugCycleStepStatus,
    BugsCycleAckPayload, BugsCycleFilters, BugsCycleSkipEntry, BugsCycleStartPayload,
    DiscoveryEndpoint, InMemoryBridgeSettingsStore, InspectableBlockContext, IssueAckStatus,
    IssueActionDispatchOutcome, IssueCommandAckPayload, IssueCommandKind, IssueCommandPayload,
    IssueStage, MAXCODE_ENDPOINT_FILE, NoopBridgeEventSink, OpenTerminalRequest,
    OpenedTerminalInfo, PROTOCOL_VERSION, PeerRole, RpcEnvelope, RpcPayload, current_unix_ms,
    default_maxterminal_capabilities, generate_token_hex, send_http_rpc_envelope, write_atomic,
};
use parking_lot::Mutex;
use tempfile::TempDir;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

struct DummyHost {
    launched_uris: Mutex<Vec<String>>,
}

#[async_trait]
impl BridgeHost for DummyHost {
    async fn active_project_path(&self) -> Option<String> {
        None
    }

    async fn find_project_by_path(
        &self,
        _canonical_path: &Path,
    ) -> Result<Option<BridgeProjectSummary>, BridgeError> {
        Ok(None)
    }

    async fn create_project_for_path(
        &self,
        canonical_path: &Path,
        _preferred_name: Option<&str>,
    ) -> Result<BridgeProjectSummary, BridgeError> {
        Ok(BridgeProjectSummary {
            id: "p1".to_string(),
            title: "P1".to_string(),
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
            tab_id: "t1".to_string(),
        })
    }

    async fn load_inspectable_block(
        &self,
        _block_id: &str,
    ) -> Result<InspectableBlockContext, BridgeError> {
        Err(BridgeError::new(
            BridgeErrorCode::BlockNotFound,
            "Não usado.",
        ))
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
async fn send_issue_command_falls_back_to_uri_and_bugs_cycle_streams_progress() {
    let tmp = TempDir::new().unwrap();
    let disc_dir = tmp.path().join("max-bridge");
    let host = Arc::new(DummyHost {
        launched_uris: Mutex::new(Vec::new()),
    });
    let store = Arc::new(InMemoryBridgeSettingsStore::new(BridgeSettings::default()));
    let svc = BridgeService::new(
        "0.1.0",
        store,
        host.clone(),
        Arc::new(NoopBridgeEventSink),
        Some(disc_dir.clone()),
    )
    .await
    .unwrap();

    // 1. Sem MaxCode online -> send_issue_command cai para vscode://johnattas.maxcode/issue/plan?...
    let cmd = IssueCommandPayload {
        kind: IssueCommandKind::Plan,
        project_path: "/home/dev/maxterminal".to_string(),
        repo_full_name: "johnattas/MaxTerminal".to_string(),
        issue_number: 17,
        issue_title: "Melhorar performance".to_string(),
        issue_url: None,
        labels: vec!["Bug".to_string()],
        stage: IssueStage::BugPlan,
        focus_editor: true,
    };
    let outcome = svc.send_issue_command(cmd.clone()).await.unwrap();
    match outcome {
        IssueActionDispatchOutcome::UriFallback { uri } => {
            assert!(uri.starts_with("vscode://johnattas.maxcode/issue/plan?"));
        }
        other @ IssueActionDispatchOutcome::Loopback { .. } => {
            panic!("esperado UriFallback, recebido {other:?}")
        }
    }

    // 2. Com MaxCode online -> send_issue_command e bugs_cycle_start recebem ACK via loopback
    let maxcode_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let maxcode_port = maxcode_listener.local_addr().unwrap().port();
    write_atomic(
        &disc_dir.join(MAXCODE_ENDPOINT_FILE),
        &DiscoveryEndpoint {
            protocol_version: PROTOCOL_VERSION,
            role: PeerRole::MaxCode,
            instance_id: "maxcode-issues".to_string(),
            pid: std::process::id(),
            host: "127.0.0.1".to_string(),
            port: maxcode_port,
            ws_path: "/v1/ws".to_string(),
            http_rpc_path: "/v1/rpc".to_string(),
            token: generate_token_hex(),
            started_at_ms: current_unix_ms(),
            capabilities: default_maxterminal_capabilities(),
        },
    )
    .unwrap();

    let cycle_id = Uuid::new_v4();
    tokio::spawn(async move {
        for _ in 0..2 {
            if let Ok((mut s, _)) = maxcode_listener.accept().await {
                let mut buf = vec![0u8; 8192];
                let n = s.read(&mut buf).await.unwrap_or(0);
                let raw = std::str::from_utf8(&buf[..n]).unwrap_or_default();
                if let Some(body_str) = raw.split("\r\n\r\n").nth(1)
                    && let Ok(env) = serde_json::from_str::<RpcEnvelope>(body_str)
                {
                    let reply_payload = match env.message {
                        RpcPayload::IssueCommand(_) => {
                            RpcPayload::IssueCommandAck(IssueCommandAckPayload {
                                request_id: env.request_id,
                                status: IssueAckStatus::Accepted,
                                conversation_id: Some("conv-17".to_string()),
                                worktree_path: None,
                                error: None,
                            })
                        }
                        RpcPayload::BugsCycleStart(start) => {
                            RpcPayload::BugsCycleAck(BugsCycleAckPayload {
                                request_id: env.request_id,
                                cycle_id: start.cycle_id,
                                accepted: true,
                                queued_issues: vec![17, 18],
                                skipped_issues: vec![BugsCycleSkipEntry {
                                    issue_number: 19,
                                    reason: "Já aprovada".to_string(),
                                }],
                                error: None,
                            })
                        }
                        _ => continue,
                    };
                    let ack_env = RpcEnvelope::with_id(env.request_id, reply_payload);
                    let body = serde_json::to_vec(&ack_env).unwrap();
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = s.write_all(resp.as_bytes()).await;
                    let _ = s.write_all(&body).await;
                }
            }
        }
    });

    let outcome_lb = svc.send_issue_command(cmd).await.unwrap();
    assert!(matches!(
        outcome_lb,
        IssueActionDispatchOutcome::Loopback { .. }
    ));

    let cycle_ack = svc
        .bugs_cycle_start(BugsCycleStartPayload {
            cycle_id,
            project_path: "/home/dev/maxterminal".to_string(),
            repo_full_name: "johnattas/MaxTerminal".to_string(),
            issue_numbers: vec![17, 18, 19],
            filters: BugsCycleFilters::default(),
            dry_run: false,
        })
        .await
        .unwrap();
    assert!(cycle_ack.accepted);
    assert_eq!(cycle_ack.queued_issues, vec![17, 18]);

    // 3. MaxCode envia BugCycleProgress para o servidor loopback do MaxTerminal
    let mut progress_rx = svc.subscribe_bug_cycle_progress();
    let mt_ep = mt_bridge::read_verified(&disc_dir.join("maxterminal.endpoint.json")).unwrap();
    let prog_payload = BugCycleProgressPayload {
        cycle_id,
        project_path: "/home/dev/maxterminal".to_string(),
        overall_status: BugCycleOverallStatus::Running,
        current_issue_number: Some(17),
        stage: BugCycleItemStage::Executing,
        step_status: BugCycleStepStatus::InProgress,
        completed_count: 1,
        total_count: 2,
        updated_label: Some("Waiting Execute".to_string()),
        message: Some("Executando correção da issue #17".to_string()),
    };
    let prog_env = RpcEnvelope::new(RpcPayload::BugCycleProgress(prog_payload.clone()));
    let (code, _) = send_http_rpc_envelope(mt_ep.port, &mt_ep.token, &prog_env)
        .await
        .unwrap();
    assert_eq!(code, 200);

    let received = progress_rx.recv().await.unwrap();
    assert_eq!(received, prog_payload);
    assert_eq!(svc.bugs_cycle_status(cycle_id), Some(prog_payload));
}
