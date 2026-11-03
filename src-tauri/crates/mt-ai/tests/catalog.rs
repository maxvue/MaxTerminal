//! Testes do `CatalogService` (4 harnesses canônicos obrigatórios, merge sem inventar modelos/raciocínio
//! e revalidação `ETag` / `304 Not Modified` — `CA-11`, `CA-12`).

#![allow(clippy::unwrap_used)]

use mt_ai::catalog::{CatalogService, CatalogSource};
use mt_ai::client::http::GatewayClient;
use mt_ai::config::{AiSelection, AiSettings};
use mt_ai::error::codes;
use mt_ai::repo::AiRepo;
use mt_db::{Db, DbConfig};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

#[tokio::test]
async fn fallback_contains_4_canonical_harnesses_with_empty_models() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("ai.db"))).unwrap();
    let repo = Arc::new(AiRepo::new(db.clone()));
    let client = Arc::new(GatewayClient::new(&AiSettings::default()).unwrap());
    let catalog_svc = CatalogService::new(client, repo);

    let cat = catalog_svc.get().await;
    assert_eq!(cat.source, CatalogSource::Fallback);
    let ids: Vec<&str> = cat.harnesses.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids, vec!["claude-code", "codex", "agy", "opencode"]);
    for h in &cat.harnesses {
        assert!(
            h.models.is_empty(),
            "Fallback nunca deve inventar modelos fictícios"
        );
    }

    // Resolver seleção contra fallback sem modelos retorna ai.unknown_selection
    let err = catalog_svc
        .resolve(
            &cat,
            &AiSelection {
                harness_id: "claude-code".to_string(),
                model_id: "inventado".to_string(),
                reasoning_id: None,
            },
        )
        .unwrap_err();
    assert_eq!(err.code, codes::AI_UNKNOWN_SELECTION);

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn catalog_fetches_merges_and_honors_304_not_modified() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_clone = Arc::clone(&hits);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let call_num = hits_clone.fetch_add(1, Ordering::SeqCst);
            let (read_half, mut write_half) = stream.into_split();
            let mut reader = BufReader::new(read_half);
            let mut if_none_match = None;
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                let t = line.trim();
                if t.is_empty() {
                    break;
                }
                if let Some((k, v)) = t.split_once(':')
                    && k.trim().eq_ignore_ascii_case("If-None-Match")
                {
                    if_none_match = Some(v.trim().to_string());
                }
                line.clear();
            }

            if call_num > 0 && if_none_match.as_deref() == Some("\"cat-v1\"") {
                let resp =
                    "HTTP/1.1 304 Not Modified\r\nETag: \"cat-v1\"\r\nConnection: close\r\n\r\n";
                let _ = write_half.write_all(resp.as_bytes()).await;
            } else {
                let body = r#"{"harnesses":[
                    {"id":"claude-code","label":"Claude Code","icon":"simple-icons:claude","models":[
                        {"id":"sonnet-4","label":"Claude Sonnet 4","reasoningLevels":[
                            {"id":"low","label":"Baixo"},
                            {"id":"high","label":"Alto"}
                        ],"defaultReasoning":"low","contextWindow":200000},
                        {"id":"haiku","label":"Claude Haiku","reasoningLevels":[]}
                    ]}
                ]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nETag: \"cat-v1\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
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
    let client = Arc::new(
        GatewayClient::new(&AiSettings {
            base_url: Some(format!("http://{addr}")),
            ..AiSettings::default()
        })
        .unwrap(),
    );
    let catalog_svc = CatalogService::new(client, repo);

    // 1ª chamada vai ao gateway (200 OK)
    let cat1 = catalog_svc.get().await;
    assert_eq!(cat1.source, CatalogSource::Gateway);
    assert_eq!(cat1.harnesses.len(), 4);
    assert_eq!(cat1.harnesses[0].models.len(), 2);
    assert_eq!(cat1.harnesses[0].models[0].reasoning_levels[0].rank, 0);
    assert_eq!(cat1.harnesses[0].models[0].reasoning_levels[1].rank, 1);

    // 2ª chamada usa memória (0 HTTP)
    let _cat_mem = catalog_svc.get().await;
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    // Refresh envia If-None-Match e recebe 304 Not Modified preservando os modelos
    let cat_refreshed = catalog_svc.refresh().await.unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(cat_refreshed.source, CatalogSource::Gateway);
    assert_eq!(cat_refreshed.harnesses[0].models.len(), 2);

    // Modelo sem reasoningLevels aceita apenas reasoning_id == None
    let ok_haiku = catalog_svc
        .resolve(
            &cat_refreshed,
            &AiSelection {
                harness_id: "claude-code".to_string(),
                model_id: "haiku".to_string(),
                reasoning_id: None,
            },
        )
        .unwrap();
    assert!(ok_haiku.selection.reasoning_id.is_none());

    let bad_haiku = catalog_svc
        .resolve(
            &cat_refreshed,
            &AiSelection {
                harness_id: "claude-code".to_string(),
                model_id: "haiku".to_string(),
                reasoning_id: Some("high".to_string()),
            },
        )
        .unwrap_err();
    assert_eq!(bad_haiku.code, codes::AI_UNKNOWN_SELECTION);

    db.shutdown().await.unwrap();
}
