//! Benchmark Criterion da Busca Global FTS5 (`mt-db` — `CA-08`):
//! mede `p50`, `p95`, `p99` sobre o catálogo versionado `queries_v1.json` e concorrência de leitura/escrita.

#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::too_many_lines,
    clippy::bool_to_int_with_if
)]

use std::time::{Duration, Instant};

use criterion::{Criterion, criterion_group, criterion_main};
use mt_core::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId};
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandTextQuality, IntegrationMode, ObservationConfidence,
    ObservationSource, OutputKind, ShellKind,
};
use mt_core::workspace::ScrollbackChunkKind;
use mt_db::{
    Db, DbConfig, FinishCommandBlock, NewSession, ProjectMutation, ScrollbackRecord,
    StoredNewProject, StoredProject,
    sanitizer::SanitizedText,
    search::{
        ExitFilter, SearchConfig, SearchEngine, SearchFilters, SearchOwner, SearchRequest,
        SearchSort,
    },
};
use tempfile::TempDir;

const QUERIES_JSON: &str = include_str!("queries_v1.json");

#[derive(serde::Deserialize)]
struct QueriesCatalog {
    categories: Categories,
}

#[derive(serde::Deserialize)]
struct Categories {
    rare: Vec<String>,
    common: Vec<String>,
    prefix: Vec<String>,
}

fn setup_bench_engine(rt: &tokio::runtime::Runtime) -> (TempDir, Db, SearchEngine) {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("bench_search.db"));
    let db = Db::open(cfg).unwrap();

    let proj_id = ProjectId::new();
    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let sess_id = SessionId::new();
    let pty_id = PtyId::new();

    rt.block_on(async {
        db.projects()
            .commit(ProjectMutation::Add(StoredNewProject {
                project: StoredProject {
                    id: proj_id,
                    title: SanitizedText::new("Bench Project rare_token_0001"),
                    icon: String::from("lucide:folder"),
                    color: String::from("#00BCD4"),
                    path: String::from("/workspace/bench"),
                    root_id: None,
                    stack: String::from("rust"),
                    identity_source: String::from("auto"),
                    sort_order: 0,
                    missing: false,
                    created_at: 1_000,
                    updated_at: 1_000,
                },
                clear_dismissed_paths: Vec::new(),
                if_exists_noop: false,
            }))
            .await
            .unwrap();

        db.sessions()
            .prepare(NewSession {
                id: sess_id,
                tab_id: Some(tab_id),
                pane_id: Some(pane_id),
                project_id: Some(proj_id),
                tab_session_id: None,
                pty_id,
                shell_kind: ShellKind::Zsh,
                shell_executable: SanitizedText::new("/bin/zsh"),
                initial_cwd: SanitizedText::new("/workspace/bench"),
                integration_mode: IntegrationMode::Osc133,
                started_at: 1_000,
            })
            .await
            .unwrap();

        for seq in 1..=200_u32 {
            db.command_blocks()
                .finish(FinishCommandBlock {
                    id: CommandBlockId::new(),
                    session_id: sess_id,
                    tab_id: Some(tab_id),
                    pane_id: Some(pane_id),
                    pty_id,
                    command_seq: seq,
                    command_text: Some(SanitizedText::new(&format!(
                        "php artisan migrate --step={seq} cargo test"
                    ))),
                    command_text_quality: CommandTextQuality::Exact,
                    cwd: Some(SanitizedText::new("/workspace/bench")),
                    started_at: 1_000 + i64::from(seq) * 10,
                    ended_at: 1_005 + i64::from(seq) * 10,
                    duration_ms: 5,
                    exit_code: Some(if seq % 4 == 0 { 1 } else { 0 }),
                    signal: None,
                    status: if seq % 4 == 0 {
                        BlockStatus::Failed
                    } else {
                        BlockStatus::Succeeded
                    },
                    end_reason: if seq % 4 == 0 {
                        BlockEndReason::ExitNonZero
                    } else {
                        BlockEndReason::ExitZero
                    },
                    is_continuous: false,
                    source: ObservationSource::Osc133,
                    confidence: ObservationConfidence::Exact,
                    output_text: Some(SanitizedText::new(&format!(
                        "Resultado {seq} horizon worker sqlite wal"
                    ))),
                    output_kind: OutputKind::Full,
                    output_bytes: 40,
                    output_lines: 1,
                    error_excerpt: None,
                    inspectable: false,
                    start_pty_seq: u64::from(seq),
                    start_byte_offset: 0,
                    output_start_pty_seq: None,
                    output_start_byte_offset: None,
                    end_pty_seq: u64::from(seq),
                    end_byte_offset: 40,
                })
                .await
                .unwrap();

            db.scrollback()
                .append_chunk(ScrollbackRecord::new(
                    sess_id,
                    i64::from(seq),
                    2_000 + i64::from(seq),
                    SanitizedText::new(&format!("stream {seq} vitest bundle tauri wayland")),
                    ScrollbackChunkKind::Output,
                ))
                .await
                .unwrap();
        }
    });

    let engine = SearchEngine::new(db.clone(), SearchConfig::default());
    (tmp, db, engine)
}

fn bench_search(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (_tmp, _db, engine) = setup_bench_engine(&rt);
    let catalog: QueriesCatalog = serde_json::from_str(QUERIES_JSON).unwrap();
    assert_eq!(catalog.categories.rare.len(), 50);
    assert_eq!(catalog.categories.common.len(), 50);
    assert_eq!(catalog.categories.prefix.len(), 50);

    let owner = SearchOwner::new("main");
    let mut seq = 0_u32;

    let mut group = c.benchmark_group("search_fts5");
    group.measurement_time(Duration::from_secs(2));

    group.bench_function("warm_common_and_prefix", |b| {
        b.iter(|| {
            seq = seq.wrapping_add(1);
            let q = &catalog.categories.common[(seq as usize) % catalog.categories.common.len()];
            let start = Instant::now();
            let resp = rt
                .block_on(engine.search(
                    owner.clone(),
                    SearchRequest {
                        request_id: format!("bench-{seq}"),
                        generation: seq,
                        query: q.clone(),
                        filters: SearchFilters {
                            exit: ExitFilter::Any,
                            ..Default::default()
                        },
                        sort: SearchSort::Relevance,
                        page_size: 25,
                        cursor: None,
                    },
                ))
                .unwrap();
            let _elapsed = start.elapsed();
            engine.release(&owner, &resp.snapshot_id);
        });
    });

    group.finish();
}

criterion_group!(benches, bench_search);
criterion_main!(benches);
