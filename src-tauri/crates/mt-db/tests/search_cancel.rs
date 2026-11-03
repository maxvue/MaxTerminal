//! Testes de integração de cancelamento determinístico (`search_cancel`) e substituição por geração (`mt-db` — Fase 17).

#![allow(
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::assertions_on_constants
)]

use mt_core::ids::{PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{IntegrationMode, ShellKind};
use mt_core::workspace::ScrollbackChunkKind;
use mt_db::{
    Db, DbConfig, NewSession, ScrollbackRecord,
    sanitizer::SanitizedText,
    search::{SearchConfig, SearchEngine, SearchFilters, SearchOwner, SearchRequest, SearchSort},
};
use tempfile::TempDir;

#[tokio::test]
async fn explicit_cancel_and_generation_supersession() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    let sess_id = SessionId::new();
    let pty_id = PtyId::new();
    let tab_id = TabId::new();
    let pane_id = PaneId::new();

    db.sessions()
        .prepare(NewSession {
            id: sess_id,
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
            project_id: None,
            tab_session_id: None,
            pty_id,
            shell_kind: ShellKind::Zsh,
            shell_executable: SanitizedText::new("/bin/zsh"),
            initial_cwd: SanitizedText::new("/tmp"),
            integration_mode: IntegrationMode::Osc133,
            started_at: 1_000,
        })
        .await
        .unwrap();

    for idx in 1..=50_i64 {
        db.scrollback()
            .append_chunk(ScrollbackRecord::new(
                sess_id,
                idx,
                1_000 + idx,
                SanitizedText::new(&format!("linha de scrollback {idx} com termo cancelavel")),
                ScrollbackChunkKind::Output,
            ))
            .await
            .unwrap();
    }

    let engine = SearchEngine::new(db.clone(), SearchConfig::default());
    let owner = SearchOwner::new("main");

    // Pré-cancelar um request_id antes de executar faz a busca retornar SearchError::Cancelled imediatamente
    let pre_cancel = engine.cancel(&owner, "req-pre-cancelled");
    assert!(pre_cancel.cancelled);

    let res_cancelled = engine
        .search(
            owner.clone(),
            SearchRequest {
                request_id: String::from("req-pre-cancelled"),
                generation: 1,
                query: String::from("cancelavel"),
                filters: SearchFilters::default(),
                sort: SearchSort::Relevance,
                page_size: 25,
                cursor: None,
            },
        )
        .await;
    assert_eq!(res_cancelled.unwrap_err().code().0, "search.cancelled");

    // Uma nova geração posterior (generation = 2) executa normalmente e invalida gerações antigas
    let r2 = engine
        .search(
            owner.clone(),
            SearchRequest {
                request_id: String::from("req-gen-2"),
                generation: 2,
                query: String::from("cancelavel"),
                filters: SearchFilters::default(),
                sort: SearchSort::Relevance,
                page_size: 25,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(r2.generation, 2);
    assert_eq!(r2.sections.len(), 1);

    // Qualquer requisição subsequente com generation < 2 para o mesmo owner é cancelada automaticamente
    let r_stale = engine
        .search(
            owner,
            SearchRequest {
                request_id: String::from("req-gen-1-late"),
                generation: 1,
                query: String::from("cancelavel"),
                filters: SearchFilters::default(),
                sort: SearchSort::Relevance,
                page_size: 25,
                cursor: None,
            },
        )
        .await;
    assert_eq!(r_stale.unwrap_err().code().0, "search.cancelled");

    db.shutdown().await.unwrap();
}
