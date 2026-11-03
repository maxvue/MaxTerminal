//! Testes de integração para `RelayServer` e protocolo Unix domain socket (`02/spec.md` §3.1).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use mt_agents::launch::PendingRunBinding;
use mt_agents::ports::RunBinding;
use mt_agents::registry::AgentKind;
use mt_agents::relay::{
    MAX_RELAY_MESSAGE_BYTES, RelayAck, RelayEnvelope, RelayMessage, RelayServer,
};
use mt_core::{PaneId, ProjectId, PtyId, SessionId, TabId};
use serde_json::json;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

fn make_pending_binding(run_id: &str, tab_id: TabId) -> PendingRunBinding {
    PendingRunBinding {
        tab_id,
        pane_id: PaneId::new(),
        session_id: SessionId::new(),
        generation: 1,
        project_id: ProjectId::new(),
        run_id: run_id.to_string(),
        agent: AgentKind::Claude,
        cwd: PathBuf::from("/tmp/project"),
        started_at_ms: 1_000,
    }
}

fn promote_binding(pending: &PendingRunBinding) -> RunBinding {
    RunBinding {
        tab_id: pending.tab_id,
        pane_id: pending.pane_id,
        pty_id: PtyId::new(),
        session_id: pending.session_id,
        generation: pending.generation,
        project_id: pending.project_id,
        run_id: pending.run_id.clone(),
        agent: pending.agent,
        cwd: pending.cwd.clone(),
        started_at_ms: pending.started_at_ms,
    }
}

async fn send_raw_line(sock_path: &std::path::Path, line: &[u8]) -> RelayAck {
    let mut stream = UnixStream::connect(sock_path).await.unwrap();
    stream.write_all(line).await.unwrap();
    stream.flush().await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut resp = String::new();
    reader.read_line(&mut resp).await.unwrap();
    serde_json::from_str(&resp).unwrap()
}

#[tokio::test]
async fn relay_server_authenticates_queues_pending_and_revokes_tokens() {
    let tmp = TempDir::new().unwrap();
    let run_dir = tmp.path().join("rt");

    let (tx, mut rx) = mpsc::channel(RelayServer::CHANNEL_CAPACITY);
    let server = RelayServer::bind_in_dir(&run_dir, tx).await.unwrap();

    // Verifica permissões 0700 no diretório e 0600 no socket
    let dir_mode = std::fs::metadata(&run_dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700);
    let sock_mode = std::fs::metadata(server.socket_path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(sock_mode, 0o600);

    let tab_id = TabId::new();
    let pending = make_pending_binding("run-1", tab_id);
    let token = server.issue(pending.clone());
    assert_eq!(format!("{token:?}"), "RelayToken([REDACTED])");

    // 1. Mensagem enviada antes de activate fica retida na fila da reserva
    let env_pending = RelayEnvelope {
        v: 1,
        token: token.as_str().to_string(),
        msg: RelayMessage::Hook {
            event: "Notification".to_string(),
            payload: json!({ "notification_type": "permission_prompt" }),
        },
    };
    let mut bytes = serde_json::to_vec(&env_pending).unwrap();
    bytes.push(b'\n');
    let ack = send_raw_line(server.socket_path(), &bytes).await;
    assert!(ack.ok);

    // Ainda não deve ter chegado no canal antes do activate
    assert!(rx.try_recv().is_err());

    // 2. Ativa com RunBinding real -> drena evento enfileirado
    let active_binding = promote_binding(&pending);
    server.activate(active_binding.clone()).unwrap();

    let ev = tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ev.binding.run_id, "run-1");
    assert!(!ev.overflow_reconcile);
    assert!(matches!(ev.message, RelayMessage::Hook { ref event, .. } if event == "Notification"));

    // 3. Token inválido deve retornar ok: false sem vazar erro detalhado
    let bad_env = RelayEnvelope {
        v: 1,
        token: "token_invalido_12345".to_string(),
        msg: RelayMessage::Statusline { payload: json!({}) },
    };
    let mut bad_bytes = serde_json::to_vec(&bad_env).unwrap();
    bad_bytes.push(b'\n');
    let bad_ack = send_raw_line(server.socket_path(), &bad_bytes).await;
    assert!(!bad_ack.ok);
    assert!(bad_ack.error.is_none());

    // 4. Payload > 64 KiB deve ser rejeitado com payload_too_large
    let huge = vec![b'a'; MAX_RELAY_MESSAGE_BYTES + 128];
    let mut huge_line = huge;
    huge_line.push(b'\n');
    let huge_ack = send_raw_line(server.socket_path(), &huge_line).await;
    assert!(!huge_ack.ok);
    assert_eq!(huge_ack.error.as_deref(), Some("payload_too_large"));

    // 5. Revogação por run_id invalida o token imediatamente
    assert!(server.revoke_run("run-1"));
    let ack_after_revoke = send_raw_line(server.socket_path(), &bytes).await;
    assert!(!ack_after_revoke.ok);

    server.shutdown().await;
}
