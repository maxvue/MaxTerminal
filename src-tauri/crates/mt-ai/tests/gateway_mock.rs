//! Testes de integração do `GatewayClient` e processador SSE contra servidor HTTP local real (`CA-05`, `CA-08`, `CA-36`).

#![allow(clippy::unwrap_used)]

use mt_ai::client::dialect::{ChatMessage, ChatRequest, Role};
use mt_ai::client::http::{GatewayClient, GatewayStatus, Op};
use mt_ai::config::{AiSelection, AiSettings};
use mt_ai::error::codes;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn sends_x_agent_name_header_and_handles_sse_chunk_boundary_secret() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let observed_agent_header = Arc::new(Mutex::new(None::<String>));
    let observed_clone = Arc::clone(&observed_agent_header);

    tokio::spawn(async move {
        if let Ok((stream, _)) = listener.accept().await {
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    break;
                }
                if let Some((k, v)) = trimmed.split_once(':')
                    && k.trim().eq_ignore_ascii_case("X-Agent-Name")
                {
                    *observed_clone.lock() = Some(v.trim().to_string());
                }
                line.clear();
            }

            // Envia SSE com segredo sk-proj-... dividido exatamente na fronteira de 2 eventos SSE
            let sse_payload = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Connection: close\r\n\r\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"echo OPENAI_API_KEY=sk-proj-123456\"}}]}\n\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"7890abcdefghijklmnop\"}}]}\n\n",
                "data: [DONE]\n\n"
            );
            let _ = write_half.write_all(sse_payload.as_bytes()).await;
            let _ = write_half.flush().await;
        }
    });

    let settings = AiSettings {
        base_url: Some(format!("http://{addr}")),
        ..AiSettings::default()
    };
    let client = GatewayClient::new(&settings).unwrap();
    let mut emitted_deltas = String::new();
    let mut on_delta = |d: &str| {
        emitted_deltas.push_str(d);
    };

    let req = ChatRequest {
        selection: AiSelection {
            harness_id: "claude-code".to_string(),
            model_id: "sonnet".to_string(),
            reasoning_id: None,
        },
        messages: vec![ChatMessage {
            role: Role::User,
            content: "teste".to_string(),
        }],
        json_mode: false,
        stream: true,
        max_tokens: None,
    };

    let cancel = CancellationToken::new();
    let resp = client
        .chat(&req, Op::Suggest, &cancel, &mut on_delta)
        .await
        .unwrap();

    assert_eq!(observed_agent_header.lock().as_deref(), Some("MaxTerminal"));
    assert!(
        !resp.content.contains("sk-proj-1234567890abcdefghijklmnop"),
        "Segredo dividido entre chunks SSE deve ser sanitizado: {}",
        resp.content
    );
    assert!(
        !emitted_deltas.contains("sk-proj-1234567890abcdefghijklmnop"),
        "Deltas emitidos nunca podem vazar segredo dividido entre chunks: {emitted_deltas}"
    );
}

#[tokio::test]
async fn premature_sse_close_without_done_fails_with_bad_response() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let sse_incomplete = concat!(
                "HTTP/1.1 200 OK\r\n",
                "Content-Type: text/event-stream\r\n",
                "Connection: close\r\n\r\n",
                "data: {\"choices\":[{\"delta\":{\"content\":\"{\"command\":\"ls\"\"}}]}\n\n"
            );
            let _ = stream.write_all(sse_incomplete.as_bytes()).await;
            let _ = stream.flush().await;
        }
    });

    let settings = AiSettings {
        base_url: Some(format!("http://{addr}")),
        ..AiSettings::default()
    };
    let client = GatewayClient::new(&settings).unwrap();
    let req = ChatRequest {
        selection: AiSelection {
            harness_id: "claude-code".to_string(),
            model_id: "sonnet".to_string(),
            reasoning_id: None,
        },
        messages: vec![ChatMessage {
            role: Role::User,
            content: "listar".to_string(),
        }],
        json_mode: true,
        stream: true,
        max_tokens: None,
    };

    let cancel = CancellationToken::new();
    let mut noop = |_: &str| {};
    let err = client
        .chat(&req, Op::Suggest, &cancel, &mut noop)
        .await
        .unwrap_err();
    assert_eq!(err.code, codes::AI_BAD_RESPONSE);
}

#[tokio::test]
async fn health_check_reflects_online_unconfigured_and_disabled() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = listener.accept().await {
            let body = r#"{"status":"ok","version":"1.4.2"}"#;
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(resp.as_bytes()).await;
        }
    });

    let client = GatewayClient::new(&AiSettings {
        base_url: Some(format!("http://{addr}")),
        ..AiSettings::default()
    })
    .unwrap();

    let h_online = client.health().await.unwrap();
    assert_eq!(h_online.status, GatewayStatus::Online);
    assert_eq!(h_online.version.as_deref(), Some("1.4.2"));

    client
        .reconfigure(&AiSettings {
            base_url: None,
            ..AiSettings::default()
        })
        .unwrap();
    let h_unconf = client.health().await.unwrap();
    assert_eq!(h_unconf.status, GatewayStatus::Unconfigured);

    client
        .reconfigure(&AiSettings {
            enabled: false,
            base_url: Some(format!("http://{addr}")),
            ..AiSettings::default()
        })
        .unwrap();
    let h_dis = client.health().await.unwrap();
    assert_eq!(h_dis.status, GatewayStatus::Disabled);
}
