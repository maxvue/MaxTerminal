//! Testes de integração das 9 fontes da Busca Global (`SearchEngine`), filtros e ordenação (`mt-db` — Fase 17).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use mt_core::ids::{
    CommandBlockId, CommandGroupId, PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId,
    TemplateId,
};
use mt_core::shell::{
    BlockEndReason, BlockStatus, CommandTextQuality, IntegrationMode, ObservationConfidence,
    ObservationSource, OutputKind, ShellKind,
};
use mt_core::workspace::{ScrollbackChunkKind, TabTemplateInput, TemplateEnvironment};
use mt_db::{
    Db, DbConfig, FinishCommandBlock, NewSession, ProjectMutation, ScrollbackRecord,
    StoredNewProject, StoredProject,
    sanitizer::SanitizedText,
    search::{
        ExitFilter, ProjectionSource, SanitizedProjection, SearchConfig, SearchEngine,
        SearchFilters, SearchOwner, SearchRequest, SearchSort, SearchSourceKind, SearchTarget,
        upsert_projection,
    },
};
use std::collections::BTreeMap;
use tempfile::TempDir;

async fn setup_seeded_engine() -> (TempDir, Db, SearchEngine, ProjectId, TabId, SessionId) {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    let proj_id = ProjectId::new();
    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let ts_id = TabSessionId::new();
    let sess_id = SessionId::new();
    let pty_id = PtyId::new();

    // 1. Projeto (sincroniza automaticamente 'projects' e o template default 'templates')
    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj_id,
                title: SanitizedText::new("Alpha Nebula App"),
                icon: String::from("logos:laravel"),
                color: String::from("#FF2D20"),
                path: String::from("/workspace/alpha-nebula"),
                root_id: None,
                stack: String::from("laravel"),
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

    // 2. Template adicional contendo o termo "nebula"
    let _tpl = db
        .templates()
        .create(
            proj_id,
            TabTemplateInput {
                name: String::from("Nebula Horizon Worker"),
                icon: String::from("mdi:rocket"),
                cwd: Some(String::from("/workspace/alpha-nebula")),
                initial_command: Some(String::from("php artisan nebula:work")),
                add_to_workspace: true,
                continuous: true,
                environment: TemplateEnvironment::default(),
                env_vars: BTreeMap::new(),
                make_default: false,
            },
            1_500,
        )
        .await
        .unwrap();

    // 3. Sessão contenant "nebula"
    db.sessions()
        .prepare(NewSession {
            id: sess_id,
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
            project_id: Some(proj_id),
            tab_session_id: Some(ts_id),
            pty_id,
            shell_kind: ShellKind::Zsh,
            shell_executable: SanitizedText::new("/bin/zsh"),
            initial_cwd: SanitizedText::new("/workspace/alpha-nebula"),
            integration_mode: IntegrationMode::Osc133,
            started_at: 2_000,
        })
        .await
        .unwrap();

    // 4. Bloco de comando com falha contendo "nebula" e segredo redigido
    let block_fail = CommandBlockId::new();
    db.command_blocks()
        .finish(FinishCommandBlock {
            id: block_fail,
            session_id: sess_id,
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
            pty_id,
            command_seq: 1,
            command_text: Some(SanitizedText::new("php artisan nebula:sync --force")),
            command_text_quality: CommandTextQuality::Exact,
            cwd: Some(String::from("/workspace/alpha-nebula")).map(|s| SanitizedText::new(&s)),
            started_at: 2_100,
            ended_at: 2_500,
            duration_ms: 400,
            exit_code: Some(1),
            signal: None,
            status: BlockStatus::Failed,
            end_reason: BlockEndReason::ExitNonZero,
            is_continuous: false,
            source: ObservationSource::Osc133,
            confidence: ObservationConfidence::Exact,
            output_text: Some(SanitizedText::new(
                "Falha fatal nebula com token Bearer ghp_1234567890abcdefghijklmnopqrstuv",
            )),
            output_kind: OutputKind::Full,
            output_bytes: 64,
            output_lines: 1,
            error_excerpt: Some(SanitizedText::new("Falha fatal nebula")),
            inspectable: true,
            start_pty_seq: 1,
            start_byte_offset: 0,
            output_start_pty_seq: Some(2),
            output_start_byte_offset: Some(10),
            end_pty_seq: 3,
            end_byte_offset: 74,
        })
        .await
        .unwrap();

    // 5. Bloco de comando com sucesso contendo "nebula"
    let block_ok = CommandBlockId::new();
    db.command_blocks()
        .finish(FinishCommandBlock {
            id: block_ok,
            session_id: sess_id,
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
            pty_id,
            command_seq: 2,
            command_text: Some(SanitizedText::new("php artisan nebula:status")),
            command_text_quality: CommandTextQuality::Exact,
            cwd: Some(SanitizedText::new("/workspace/alpha-nebula")),
            started_at: 2_600,
            ended_at: 2_700,
            duration_ms: 100,
            exit_code: Some(0),
            signal: None,
            status: BlockStatus::Succeeded,
            end_reason: BlockEndReason::ExitZero,
            is_continuous: false,
            source: ObservationSource::Osc133,
            confidence: ObservationConfidence::Exact,
            output_text: Some(SanitizedText::new("Nebula cluster operacional")),
            output_kind: OutputKind::Full,
            output_bytes: 26,
            output_lines: 1,
            error_excerpt: None,
            inspectable: false,
            start_pty_seq: 4,
            start_byte_offset: 75,
            output_start_pty_seq: Some(5),
            output_start_byte_offset: Some(90),
            end_pty_seq: 6,
            end_byte_offset: 116,
        })
        .await
        .unwrap();

    // 6. Scrollback chunk contendo "nebula"
    db.scrollback()
        .append_chunk(ScrollbackRecord::new(
            sess_id,
            1,
            3_000,
            SanitizedText::new("Nebula telemetry stream online em localhost:9000"),
            ScrollbackChunkKind::Output,
        ))
        .await
        .unwrap();

    // 7, 8, 9. Projeções de Groups, GroupRuns, Issues e Comments contendo "nebula"
    let group_id = CommandGroupId::new();
    let tpl_dummy = TemplateId::new();
    let _ = tpl_dummy;
    db.write(move |tx| {
        upsert_projection(
            tx,
            &SanitizedProjection {
                source: ProjectionSource::Groups,
                entity_key: group_id.to_string(),
                project_id: Some(proj_id),
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: None,
                source_truncated: false,
                fetched_at: None,
                title: SanitizedText::new("Deploy Nebula Stack"),
                body: SanitizedText::new("composer install && php artisan nebula:deploy"),
                target: SearchTarget::Group {
                    group_id,
                    project_id: Some(proj_id),
                },
                occurred_at: Some(3_500),
                source_revision: String::from("1"),
            },
        )?;

        upsert_projection(
            tx,
            &SanitizedProjection {
                source: ProjectionSource::GroupRuns,
                entity_key: String::from("run_nebula_01"),
                project_id: Some(proj_id),
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: Some(1),
                source_truncated: true,
                fetched_at: None,
                title: SanitizedText::new("Execução Deploy Nebula Stack"),
                body: SanitizedText::new("Erro ao iniciar container nebula-redis"),
                target: SearchTarget::GroupRun {
                    run_key: String::from("run_nebula_01"),
                    item_key: Some(String::from("item_01")),
                    project_id: Some(proj_id),
                },
                occurred_at: Some(4_000),
                source_revision: String::from("1"),
            },
        )?;

        upsert_projection(
            tx,
            &SanitizedProjection {
                source: ProjectionSource::Issues,
                entity_key: format!("{proj_id}:owner/alpha#42"),
                project_id: Some(proj_id),
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: None,
                source_truncated: false,
                fetched_at: Some(4_500),
                title: SanitizedText::new("#42 Corrigir vazamento no módulo Nebula"),
                body: SanitizedText::new("O worker nebula falha ao reconectar"),
                target: SearchTarget::Issue {
                    repo_key: String::from("owner/alpha"),
                    number: 42,
                    comment_key: None,
                    project_id: proj_id,
                },
                occurred_at: Some(4_400),
                source_revision: String::from("4400"),
            },
        )?;

        upsert_projection(
            tx,
            &SanitizedProjection {
                source: ProjectionSource::Comments,
                entity_key: format!("{proj_id}:owner/alpha#42:c99"),
                project_id: Some(proj_id),
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: None,
                source_truncated: false,
                fetched_at: Some(4_600),
                title: SanitizedText::new("#42 Corrigir vazamento — @dev"),
                body: SanitizedText::new("Reproduzi o bug no ambiente nebula staging"),
                target: SearchTarget::Issue {
                    repo_key: String::from("owner/alpha"),
                    number: 42,
                    comment_key: Some(String::from("99")),
                    project_id: proj_id,
                },
                occurred_at: Some(4_550),
                source_revision: String::from("4550"),
            },
        )?;

        Ok(())
    })
    .await
    .unwrap();

    let engine = SearchEngine::new(db.clone(), SearchConfig::default());
    (tmp, db, engine, proj_id, tab_id, sess_id)
}

#[tokio::test]
async fn searches_across_all_nine_sources_in_canonical_order() {
    let (_tmp, db, engine, _proj_id, _tab_id, _sess_id) = setup_seeded_engine().await;

    let resp = engine
        .search(
            SearchOwner::new("main"),
            SearchRequest {
                request_id: String::from("req-all-9"),
                generation: 1,
                query: String::from("nebula"),
                filters: SearchFilters::default(),
                sort: SearchSort::Relevance,
                page_size: 25,
                cursor: None,
            },
        )
        .await
        .unwrap();

    let returned_sources: Vec<SearchSourceKind> = resp.sections.iter().map(|s| s.source).collect();
    assert_eq!(returned_sources, SearchSourceKind::ALL_ORDERED.to_vec());

    // Verifica que nenhum hit expõe o token ghp_ em texto puro
    for sec in &resp.sections {
        for item in &sec.items {
            for seg in &item.title_factor_check() {
                assert!(!seg.contains("ghp_123456"));
            }
        }
    }

    db.shutdown().await.unwrap();
}

trait HitTextCheck {
    fn title_factor_check(&self) -> Vec<&str>;
}

impl HitTextCheck for mt_db::search::SearchHit {
    fn title_factor_check(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for s in &self.title {
            out.push(s.text.as_str());
        }
        for s in &self.excerpt {
            out.push(s.text.as_str());
        }
        out
    }
}

#[tokio::test]
async fn filters_by_exit_code_time_range_and_sources() {
    let (_tmp, db, engine, proj_id, _tab_id, _sess_id) = setup_seeded_engine().await;

    // 1. ExitFilter::Nonzero retorna apenas Commands (exit_code=1) e GroupRuns (exit_code=1)
    let resp_nonzero = engine
        .search(
            SearchOwner::new("main"),
            SearchRequest {
                request_id: String::from("req-nonzero"),
                generation: 2,
                query: String::from("nebula"),
                filters: SearchFilters {
                    project_id: Some(proj_id),
                    exit: ExitFilter::Nonzero,
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 25,
                cursor: None,
            },
        )
        .await
        .unwrap();

    let kinds: Vec<SearchSourceKind> = resp_nonzero.sections.iter().map(|s| s.source).collect();
    assert_eq!(
        kinds,
        vec![SearchSourceKind::Commands, SearchSourceKind::GroupRuns]
    );
    assert_eq!(resp_nonzero.sections[0].items.len(), 1);
    assert!(resp_nonzero.sections[1].items[0].source_truncated);

    // 2. ExitFilter::Zero retorna apenas o comando com exit_code = 0
    let resp_ok = engine
        .search(
            SearchOwner::new("main"),
            SearchRequest {
                request_id: String::from("req-ok"),
                generation: 3,
                query: String::from("nebula"),
                filters: SearchFilters {
                    exit: ExitFilter::Zero,
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 25,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(resp_ok.sections.len(), 1);
    assert_eq!(resp_ok.sections[0].source, SearchSourceKind::Commands);
    assert_eq!(resp_ok.sections[0].items.len(), 1);

    // 3. Intervalo temporal inválido (from_ms > until_ms) é rejeitado com InvalidFilters
    let err = engine
        .search(
            SearchOwner::new("main"),
            SearchRequest {
                request_id: String::from("req-bad-range"),
                generation: 4,
                query: String::from("nebula"),
                filters: SearchFilters {
                    from_ms: Some(5_000),
                    until_ms: Some(1_000),
                    ..Default::default()
                },
                sort: SearchSort::Recent,
                page_size: 25,
                cursor: None,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(err.code().0, "search.invalid_filters");

    db.shutdown().await.unwrap();
}
