//! Testes de integração de snapshots materializados, cursores HMAC-SHA256, TTL e limites de memória (`mt-db` — Fase 17).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use mt_core::ids::{CommandBlockId, PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandTextQuality, IntegrationMode, ObservationConfidence,
    ObservationSource, OutputKind, ShellKind,
};
use mt_db::{
    Db, DbConfig, FinishCommandBlock, NewSession,
    sanitizer::SanitizedText,
    search::{
        SearchConfig, SearchEngine, SearchFilters, SearchOwner, SearchRequest, SearchSort,
        SearchSourceKind,
    },
};
use tempfile::TempDir;

#[tokio::test]
async fn snapshot_pagination_is_isolated_from_concurrent_inserts_and_validates_hmac() {
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
            shell_kind: ShellKind::Bash,
            shell_executable: SanitizedText::new("/bin/bash"),
            initial_cwd: SanitizedText::new("/tmp"),
            integration_mode: IntegrationMode::Osc133,
            started_at: 1_000,
        })
        .await
        .unwrap();

    for seq in 1..=5_u32 {
        db.command_blocks()
            .finish(FinishCommandBlock {
                id: CommandBlockId::new(),
                session_id: sess_id,
                tab_id: Some(tab_id),
                pane_id: Some(pane_id),
                pty_id,
                command_seq: seq,
                command_text: Some(SanitizedText::new(&format!("echo snapshot_item_{seq}"))),
                command_text_quality: CommandTextQuality::Exact,
                cwd: Some(SanitizedText::new("/tmp")),
                started_at: 1_000 + i64::from(seq) * 100,
                ended_at: 1_050 + i64::from(seq) * 100,
                duration_ms: 50,
                exit_code: Some(0),
                signal: None,
                status: BlockStatus::Succeeded,
                end_reason: BlockEndReason::ExitZero,
                is_continuous: false,
                source: ObservationSource::Osc133,
                confidence: ObservationConfidence::Exact,
                output_text: Some(SanitizedText::new("ok")),
                output_kind: OutputKind::Full,
                output_bytes: 2,
                output_lines: 1,
                error_excerpt: None,
                inspectable: false,
                start_pty_seq: u64::from(seq),
                start_byte_offset: 0,
                output_start_pty_seq: None,
                output_start_byte_offset: None,
                end_pty_seq: u64::from(seq),
                end_byte_offset: 2,
            })
            .await
            .unwrap();
    }

    let engine = SearchEngine::new(db.clone(), SearchConfig::default());
    let owner = SearchOwner::new("main");

    let page1 = engine
        .search(
            owner.clone(),
            SearchRequest {
                request_id: String::from("snap-1"),
                generation: 1,
                query: String::from("snapshot_item"),
                filters: SearchFilters {
                    sources: vec![SearchSourceKind::Commands],
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 2,
                cursor: None,
            },
        )
        .await
        .unwrap();

    assert_eq!(page1.sections.len(), 1);
    assert_eq!(page1.sections[0].items.len(), 2);
    let cursor1 = page1.sections[0].next_cursor.clone().unwrap();

    // Insere um 6º comando APÓS o snapshot ter sido materializado
    db.command_blocks()
        .finish(FinishCommandBlock {
            id: CommandBlockId::new(),
            session_id: sess_id,
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
            pty_id,
            command_seq: 6,
            command_text: Some(SanitizedText::new("echo snapshot_item_6_new")),
            command_text_quality: CommandTextQuality::Exact,
            cwd: Some(SanitizedText::new("/tmp")),
            started_at: 9_000,
            ended_at: 9_050,
            duration_ms: 50,
            exit_code: Some(0),
            signal: None,
            status: BlockStatus::Succeeded,
            end_reason: BlockEndReason::ExitZero,
            is_continuous: false,
            source: ObservationSource::Osc133,
            confidence: ObservationConfidence::Exact,
            output_text: Some(SanitizedText::new("ok")),
            output_kind: OutputKind::Full,
            output_bytes: 2,
            output_lines: 1,
            error_excerpt: None,
            inspectable: false,
            start_pty_seq: 6,
            start_byte_offset: 0,
            output_start_pty_seq: None,
            output_start_byte_offset: None,
            end_pty_seq: 6,
            end_byte_offset: 2,
        })
        .await
        .unwrap();

    // Pagina página 2 e página 3: devem refletir exatamente os 5 itens originais sem duplicação nem deslocamento
    let page2 = engine
        .search(
            owner.clone(),
            SearchRequest {
                request_id: String::from("snap-2"),
                generation: 1,
                query: String::from("snapshot_item"),
                filters: SearchFilters {
                    sources: vec![SearchSourceKind::Commands],
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 2,
                cursor: Some(cursor1.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(page2.sections[0].items.len(), 2);
    let cursor2 = page2.sections[0].next_cursor.clone().unwrap();

    let page3 = engine
        .search(
            owner.clone(),
            SearchRequest {
                request_id: String::from("snap-3"),
                generation: 1,
                query: String::from("snapshot_item"),
                filters: SearchFilters {
                    sources: vec![SearchSourceKind::Commands],
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 2,
                cursor: Some(cursor2),
            },
        )
        .await
        .unwrap();
    assert_eq!(page3.sections[0].items.len(), 1);
    assert!(page3.sections[0].next_cursor.is_none());

    // Cursor adulterado deve falhar com search.invalid_cursor
    let mut tampered = cursor1.clone();
    tampered.push('A');
    let err_tampered = engine
        .search(
            owner.clone(),
            SearchRequest {
                request_id: String::from("snap-tampered"),
                generation: 1,
                query: String::from("snapshot_item"),
                filters: SearchFilters {
                    sources: vec![SearchSourceKind::Commands],
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 2,
                cursor: Some(tampered),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err_tampered.code().0, "search.invalid_cursor");

    // Cursor reutilizado com query diferente deve falhar com search.invalid_cursor
    let err_mismatch = engine
        .search(
            owner.clone(),
            SearchRequest {
                request_id: String::from("snap-mismatch"),
                generation: 1,
                query: String::from("outra_query"),
                filters: SearchFilters {
                    sources: vec![SearchSourceKind::Commands],
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 2,
                cursor: Some(cursor1.clone()),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err_mismatch.code().0, "search.invalid_cursor");

    // Após release explícito do snapshot, o cursor deve retornar search.cursor_expired
    engine.release(&owner, &page1.snapshot_id);
    let err_expired = engine
        .search(
            owner,
            SearchRequest {
                request_id: String::from("snap-expired"),
                generation: 1,
                query: String::from("snapshot_item"),
                filters: SearchFilters {
                    sources: vec![SearchSourceKind::Commands],
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 2,
                cursor: Some(cursor1),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err_expired.code().0, "search.cursor_expired");

    db.shutdown().await.unwrap();
}
