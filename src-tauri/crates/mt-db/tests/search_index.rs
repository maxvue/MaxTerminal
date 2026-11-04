//! Testes de integração de estatísticas (`search_index_stats`) e reconstrução (`search_index_rebuild`) dos índices FTS5 (`mt-db` — Fase 17).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use std::collections::BTreeMap;

use mt_core::ids::ProjectId;
use mt_core::workspace::{TabTemplateInput, TemplateConnectionInput, TemplateEnvironment};
use mt_db::{
    Db, DbConfig, ProjectMutation, StoredNewProject, StoredProject,
    sanitizer::SanitizedText,
    search::{SearchConfig, SearchEngine, SearchSourceKind},
};
use tempfile::TempDir;

#[tokio::test]
async fn stats_and_rebuild_keep_counts_aligned_and_sanitized() {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();

    let proj_id = ProjectId::new();
    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj_id,
                title: SanitizedText::new("IndexProj"),
                icon: String::from("lucide:folder"),
                color: String::from("#00BCD4"),
                path: String::from("/workspace/index-proj"),
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

    let created_tpl = db
        .templates()
        .create(
            proj_id,
            TabTemplateInput {
                name: String::from("Dev Server"),
                icon: String::from("lucide:play"),
                cwd: None,
                initial_command: Some(String::from("php artisan serve")),
                add_to_workspace: false,
                continuous: true,
                environment: TemplateEnvironment::default(),
                env_vars: BTreeMap::new(),
                make_default: false,
                connection: TemplateConnectionInput::default(),
            },
            1_500,
        )
        .await
        .unwrap();

    // Simula dado legado no banco contendo APP_KEY antes do rebuild
    let tpl_id_str = created_tpl.id.to_string();
    db.write(move |tx| {
        tx.execute(
            "UPDATE tab_templates SET initial_command = 'APP_KEY=base64:abcdefghijklmnopqrstuvwx1234567890== php artisan serve' WHERE id = ?1;",
            [&tpl_id_str],
        )
        .unwrap();
        Ok(())
    })
    .await
    .unwrap();

    let engine = SearchEngine::new(db.clone(), SearchConfig::default());
    let stats_before = engine.index_stats().await.unwrap();
    assert_eq!(stats_before.len(), 9);

    let proj_stat = stats_before
        .iter()
        .find(|s| s.source == SearchSourceKind::Projects)
        .unwrap();
    assert!(proj_stat.ready);
    assert_eq!(proj_stat.row_count, 1);

    // Apaga deliberadamente as linhas de search_metadata e reconstrói via rebuild_indexes
    db.write(|tx| {
        tx.execute("DELETE FROM search_metadata;", []).unwrap();
        Ok(())
    })
    .await
    .unwrap();

    let (tx_prog, mut rx_prog) = tokio::sync::mpsc::unbounded_channel();
    let handle = engine
        .rebuild_indexes(Vec::new(), Some(tx_prog))
        .await
        .unwrap();
    assert!(handle.job_id.starts_with("rebuild_"));

    let mut events = Vec::new();
    while let Ok(ev) = rx_prog.try_recv() {
        events.push(ev);
    }
    assert_eq!(events.len(), 9);
    assert!(events.last().unwrap().done);

    let stats_after = engine.index_stats().await.unwrap();
    let proj_after = stats_after
        .iter()
        .find(|s| s.source == SearchSourceKind::Projects)
        .unwrap();
    assert_eq!(proj_after.row_count, 1);

    // Verifica que o segredo APP_KEY foi sanitizado na projeção reconstruída
    let body_stored: String = db
        .read_blocking(|conn| {
            let b: String = conn
                .query_row(
                    "SELECT body FROM search_metadata WHERE source = 'templates' AND title = 'Dev Server';",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            Ok(b)
        })
        .unwrap();
    assert!(
        !body_stored.contains("abcdefghijklmnopqrstuvwx1234567890"),
        "Projeção não deve conter APP_KEY em claro: {body_stored}"
    );

    db.shutdown().await.unwrap();
}
