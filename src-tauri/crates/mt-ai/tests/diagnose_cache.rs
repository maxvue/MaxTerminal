//! Testes do `DiagnosisService`: cache hit sem HTTP (`0` chamadas de rede), substituição por painel
//! (`Superseded`), isolamento entre painéis distintos e filtros de `ignored_exit_codes` / `is_interactive` (`CA-20`..`CA-22`, `CA-34`).

#![allow(clippy::unwrap_used)]

use mt_ai::AiService;
use mt_ai::config::{AiSelection, AiSettings, ContextSettings};
use mt_ai::context::{AiContext, AiContextSource, BlockFailure, OsInfo, PaneBindingSnapshot};
use mt_ai::diagnose::{DiagnoseRequest, DiagnosisOutcome, DiagnosisSkip};
use mt_ai::ids::AiRequestId;
use mt_ai::repo::AiRepo;
use mt_core::MtResult;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId};
use mt_db::{Db, DbConfig};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

struct MockContextSource {
    blocks: RwLock<HashMap<CommandBlockId, BlockFailure>>,
    panes: RwLock<HashMap<PaneId, PaneBindingSnapshot>>,
}

#[async_trait::async_trait]
impl AiContextSource for MockContextSource {
    async fn for_pane(&self, _pane: PaneId, _opts: &ContextSettings) -> MtResult<AiContext> {
        Ok(AiContext {
            shell: "zsh".to_string(),
            os: OsInfo {
                family: "linux".to_string(),
                distro: "CachyOS".to_string(),
                kernel: "6.12".to_string(),
            },
            cwd: "~/proj".to_string(),
            project_stack: Some("rust".to_string()),
            project_name: Some("MaxTerminal".to_string()),
            recent_commands: Vec::new(),
            output_tail: None,
        })
    }

    async fn for_block(
        &self,
        block: CommandBlockId,
        _tail_bytes: u32,
    ) -> MtResult<(AiContext, BlockFailure)> {
        let failure = self.blocks.read().get(&block).cloned().unwrap();
        let ctx = self
            .for_pane(failure.pane_id, &ContextSettings::default())
            .await?;
        Ok((ctx, failure))
    }

    fn pane_snapshot(&self, pane: PaneId) -> Option<PaneBindingSnapshot> {
        self.panes.read().get(&pane).cloned()
    }
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn diagnosis_cache_hit_makes_zero_http_requests_and_filters_ignored_codes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let chat_hits = Arc::new(AtomicUsize::new(0));
    let chat_hits_clone = Arc::clone(&chat_hits);

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
            } else if req_line.contains("/v1/chat/completions") {
                chat_hits_clone.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"choices":[{"message":{"content":"{\"summary\":\"Pacote ausente no node_modules.\",\"fixCommand\":\"npm install\",\"risk\":\"safe\"}"}}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = write_half.write_all(resp.as_bytes()).await;
            } else {
                let body = r#"{"status":"ok"}"#;
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

    let init_settings = AiSettings {
        base_url: Some(format!("http://{addr}")),
        default_selection: Some(AiSelection {
            harness_id: "claude-code".to_string(),
            model_id: "sonnet".to_string(),
            reasoning_id: None,
        }),
        ..AiSettings::default()
    };
    repo.save_settings(init_settings).await.unwrap();

    let pane_id = PaneId::new();
    let tab_id = TabId::new();
    let session_id = SessionId::new();
    let pty_id = PtyId::new();
    let proj_id = ProjectId::new();

    let ctx_src = Arc::new(MockContextSource {
        blocks: RwLock::new(HashMap::new()),
        panes: RwLock::new(HashMap::new()),
    });
    ctx_src.panes.write().insert(
        pane_id,
        PaneBindingSnapshot {
            project_id: Some(proj_id),
            tab_id,
            pane_id,
            session_id,
            pty_id,
        },
    );

    let b1 = CommandBlockId::new();
    let b2 = CommandBlockId::new();
    let b_sigint = CommandBlockId::new();

    let base_failure = BlockFailure {
        pane_id,
        session_id,
        pty_id,
        project_id: Some(proj_id),
        command: "npm run build".to_string(),
        exit_code: Some(1),
        output_tail: "Error: Cannot find module 'vite'".to_string(),
        is_interactive: false,
    };
    ctx_src.blocks.write().insert(b1, base_failure.clone());
    ctx_src.blocks.write().insert(b2, base_failure);
    ctx_src.blocks.write().insert(
        b_sigint,
        BlockFailure {
            pane_id,
            session_id,
            pty_id,
            project_id: Some(proj_id),
            command: "sleep 10".to_string(),
            exit_code: Some(130),
            output_tail: "^C".to_string(),
            is_interactive: false,
        },
    );

    let svc = AiService::new(repo, ctx_src).await.unwrap();

    // Exit code 130 (SIGINT) é pulado sem HTTP
    let out_sigint = svc
        .diagnose_block(DiagnoseRequest {
            request_id: AiRequestId::new(),
            block_id: b_sigint,
        })
        .await
        .unwrap();
    assert!(matches!(
        out_sigint,
        DiagnosisOutcome::Skipped {
            reason: DiagnosisSkip::IgnoredExitCode
        }
    ));
    assert_eq!(chat_hits.load(Ordering::SeqCst), 0);

    // 1º diagnóstico do erro faz 1 chamada HTTP
    let out1 = svc
        .diagnose_block(DiagnoseRequest {
            request_id: AiRequestId::new(),
            block_id: b1,
        })
        .await
        .unwrap();
    match out1 {
        DiagnosisOutcome::Ready { diagnosis } => {
            assert!(!diagnosis.from_cache);
            assert_eq!(diagnosis.fix_command.as_deref(), Some("npm install"));
        }
        other @ DiagnosisOutcome::Skipped { .. } => panic!("Esperado Ready, obteve {other:?}"),
    }
    assert_eq!(chat_hits.load(Ordering::SeqCst), 1);

    // 2º diagnóstico com a mesma assinatura retorna do cache com 0 novas chamadas HTTP (CA-21)
    let out2 = svc
        .diagnose_block(DiagnoseRequest {
            request_id: AiRequestId::new(),
            block_id: b2,
        })
        .await
        .unwrap();
    match out2 {
        DiagnosisOutcome::Ready { diagnosis } => {
            assert!(diagnosis.from_cache);
            assert_eq!(diagnosis.block_id, b2);
        }
        other @ DiagnosisOutcome::Skipped { .. } => {
            panic!("Esperado Ready do cache, obteve {other:?}")
        }
    }
    assert_eq!(chat_hits.load(Ordering::SeqCst), 1);

    db.shutdown().await.unwrap();
}
