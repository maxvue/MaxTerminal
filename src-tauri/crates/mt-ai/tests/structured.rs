//! Testes do `StructuredGenerator` e `RagService`: validação de JSON Schema, bloqueio de `$ref` externo,
//! 1 tentativa automática de reparo e validação de slugs RAG (`CA-26`, `CA-27`, `CA-36`).

#![allow(clippy::unwrap_used)]

use mt_ai::AiService;
use mt_ai::config::{AiSelection, AiSettings, ContextSettings};
use mt_ai::context::{AiContext, AiContextSource, BlockFailure, OsInfo, PaneBindingSnapshot};
use mt_ai::error::codes;
use mt_ai::ids::AiRequestId;
use mt_ai::rag::{RagSearchRequest, RagService};
use mt_ai::repo::AiRepo;
use mt_ai::structured::{StructuredGenerator, StructuredRequest};
use mt_core::MtResult;
use mt_core::ids::{CommandBlockId, PaneId};
use mt_db::{Db, DbConfig};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

struct DummyContext;

#[async_trait::async_trait]
impl AiContextSource for DummyContext {
    async fn for_pane(&self, _pane: PaneId, _opts: &ContextSettings) -> MtResult<AiContext> {
        Ok(AiContext {
            shell: "zsh".to_string(),
            os: OsInfo {
                family: "linux".to_string(),
                distro: "CachyOS".to_string(),
                kernel: "6.12".to_string(),
            },
            cwd: "~".to_string(),
            project_stack: None,
            project_name: None,
            recent_commands: Vec::new(),
            output_tail: None,
        })
    }

    async fn for_block(
        &self,
        _block: CommandBlockId,
        _tail_bytes: u32,
    ) -> MtResult<(AiContext, BlockFailure)> {
        unreachable!()
    }

    fn pane_snapshot(&self, _pane: PaneId) -> Option<PaneBindingSnapshot> {
        None
    }
}

#[test]
fn rejects_external_schema_refs_and_invalid_rag_slugs() {
    let ext_schema = json!({
        "type": "object",
        "properties": {
            "cmd": { "$ref": "https://evil.example.com/schema.json" }
        }
    });
    let err = StructuredGenerator::validate_schema_security(&ext_schema).unwrap_err();
    assert_eq!(err.code, codes::AI_SCHEMA_VALIDATION);

    let local_schema = json!({
        "type": "object",
        "$defs": { "nameType": { "type": "string" } },
        "properties": {
            "name": { "$ref": "#/$defs/nameType" }
        }
    });
    assert!(StructuredGenerator::validate_schema_security(&local_schema).is_ok());

    assert!(RagService::validate_slug("laravel").is_ok());
    assert!(RagService::validate_slug("vue-3_docs").is_ok());
    assert_eq!(
        RagService::validate_slug("../etc/passwd").unwrap_err().code,
        codes::AI_INVALID_SLUG
    );
    assert_eq!(
        RagService::validate_slug("InvalidUpper").unwrap_err().code,
        codes::AI_INVALID_SLUG
    );
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn structured_generation_repairs_once_and_rag_maps_404() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let chat_attempt = Arc::new(AtomicUsize::new(0));
    let chat_attempt_clone = Arc::clone(&chat_attempt);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut req_line = String::new();
            let _ = reader.read_line(&mut req_line).await;
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                if line.trim().is_empty() {
                    break;
                }
                line.clear();
            }

            if req_line.contains("/v1/harnesses") {
                let body = r#"{"harnesses":[{"id":"claude-code","label":"Claude Code","icon":"simple-icons:claude","models":[{"id":"sonnet","label":"Sonnet","reasoningLevels":[]}]}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = write_half.write_all(resp.as_bytes()).await;
            } else if req_line.contains("/v1/rags/inexistente/search") {
                let body = r#"{"error":"not found"}"#;
                let resp = format!(
                    "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = write_half.write_all(resp.as_bytes()).await;
            } else if req_line.contains("/v1/chat/completions") {
                let n = chat_attempt_clone.fetch_add(1, Ordering::SeqCst);
                let body = if n == 0 {
                    // 1ª resposta falha no schema (falta campo obrigatório signature)
                    r#"{"choices":[{"message":{"content":"{\"className\":\"SyncUsersCommand\"}"}}]}"#
                } else {
                    // 2ª resposta (reparo) cumpre o schema
                    r#"{"choices":[{"message":{"content":"{\"className\":\"SyncUsersCommand\",\"signature\":\"users:sync\"}"}}]}"#
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = write_half.write_all(resp.as_bytes()).await;
            }
        }
    });

    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("ai.db"))).unwrap();
    let repo = Arc::new(AiRepo::new(db.clone()));

    let settings = AiSettings {
        base_url: Some(format!("http://{addr}")),
        default_selection: Some(AiSelection {
            harness_id: "claude-code".to_string(),
            model_id: "sonnet".to_string(),
            reasoning_id: None,
        }),
        ..AiSettings::default()
    };
    repo.save_settings(settings).await.unwrap();

    let svc = AiService::new(repo, Arc::new(DummyContext)).await.unwrap();

    let schema = json!({
        "type": "object",
        "required": ["className", "signature"],
        "properties": {
            "className": { "type": "string", "minLength": 3 },
            "signature": { "type": "string", "minLength": 3 }
        },
        "additionalProperties": false
    });

    let resp = svc
        .generate_structured(StructuredRequest {
            request_id: AiRequestId::new(),
            schema_id: "artisan.command.v1".to_string(),
            schema,
            prompt: "Criar comando para sincronizar usuários".to_string(),
            system: None,
            selection: None,
            pane_id: None,
            project_id: None,
        })
        .await
        .unwrap();

    assert!(resp.repaired);
    assert_eq!(resp.value["className"], "SyncUsersCommand");
    assert_eq!(resp.value["signature"], "users:sync");
    assert_eq!(chat_attempt.load(Ordering::SeqCst), 2);

    // RAG 404 vira ai.rag_not_found (CA-26)
    let rag_err = svc
        .rag_search(RagSearchRequest {
            request_id: AiRequestId::new(),
            slug: "inexistente".to_string(),
            query: "eloquent".to_string(),
            top_k: Some(5),
            pane_id: None,
        })
        .await
        .unwrap_err();
    assert_eq!(rag_err.code, codes::AI_RAG_NOT_FOUND);

    db.shutdown().await.unwrap();
}
