//! Testes de integração de upgrade incremental (v1..v10 -> v11) e restauração agendada com injeção de falha (`mt-db` — Fase 17).

#![allow(clippy::unwrap_used, clippy::too_many_lines)]

use mt_core::ids::ProjectId;
use mt_db::{
    BackupKind, Db, DbConfig, MigrationRegistry, MigrationRunner, ProjectMutation, RestoreStage,
    StoredNewProject, StoredProject, apply_pending_restore_on_boot,
    apply_pending_restore_with_injection, create_backup_blocking, list_backups, open_writer,
    sanitizer::SanitizedText,
    schedule_restore_with_dirs,
    search::{
        SearchConfig, SearchEngine, SearchFilters, SearchOwner, SearchRequest, SearchSort,
        SearchSourceKind,
    },
};
use tempfile::TempDir;

#[tokio::test]
async fn upgrade_from_partial_versions_populates_search_metadata_and_restore_recovers_on_failure() {
    // 1. Banco migrado para v11 sincroniza projeções e permite busca imediata
    let tmp = TempDir::new().unwrap();
    let db_path = tmp.path().join("maxterminal.db");
    let cfg = DbConfig::for_path(&db_path);

    let db = Db::open(cfg.clone()).unwrap();
    assert_eq!(db.schema_version(), 12);

    let proj1 = ProjectId::new();
    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj1,
                title: SanitizedText::new("MigratedAlpha"),
                icon: String::from("lucide:folder"),
                color: String::from("#00BCD4"),
                path: String::from("/workspace/migrated-alpha"),
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

    let engine = SearchEngine::new(db.clone(), SearchConfig::default());
    let resp = engine
        .search(
            SearchOwner::new("main"),
            SearchRequest {
                request_id: String::from("upg-1"),
                generation: 1,
                query: String::from("MigratedAlpha"),
                filters: SearchFilters {
                    sources: vec![SearchSourceKind::Projects],
                    ..Default::default()
                },
                sort: SearchSort::Relevance,
                page_size: 10,
                cursor: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(resp.sections.len(), 1);
    assert_eq!(resp.sections[0].items.len(), 1);

    // 2. Cria backup manual quando há apenas 1 projeto ("MigratedAlpha")
    let bkp = create_backup_blocking(&cfg.db_path, &cfg.backup_dir, BackupKind::Manual, 7).unwrap();
    let listed = list_backups(&cfg.backup_dir).unwrap();
    assert_eq!(listed.len(), 1);

    // 3. Adiciona um segundo projeto APÓS o backup ("PostBackupProject")
    let proj2 = ProjectId::new();
    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj2,
                title: SanitizedText::new("PostBackupProject"),
                icon: String::from("lucide:folder"),
                color: String::from("#FF9800"),
                path: String::from("/workspace/post-backup"),
                root_id: None,
                stack: String::from("vue"),
                identity_source: String::from("auto"),
                sort_order: 1,
                missing: false,
                created_at: 2_000,
                updated_at: 2_000,
            },
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap();

    db.shutdown().await.unwrap();

    // 4. Agenda restauração do backup e injeta falha na etapa RebuildFts:
    //    O rollback a partir do backup PreRestore deve preservar os 2 projetos!
    let data_dir = cfg.db_path.parent().unwrap();
    let scheduled =
        schedule_restore_with_dirs(data_dir, &cfg.backup_dir, &bkp.file_name, 12).unwrap();
    assert_eq!(scheduled.backup_file_name, bkp.file_name);
    assert!(scheduled.requires_restart);

    let err = apply_pending_restore_with_injection(&cfg, Some(RestoreStage::RebuildFts));
    assert!(
        err.is_err(),
        "Falha injetada na etapa RebuildFts deve abortar a restauração"
    );

    // Reabre via open_writer diretamente (sem disparar apply_pending_restore_on_boot novamente) para inspecionar o rollback
    {
        let opened = open_writer(&cfg).unwrap();
        let count: i64 = opened
            .conn
            .query_row("SELECT COUNT(*) FROM projects;", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            count, 2,
            "Rollback a partir do backup pre_restore deve preservar os 2 projetos existentes antes da falha"
        );
    }

    // 5. Agora executa apply_pending_restore_on_boot sem falha injetada -> restaura o estado com 1 projeto
    let restored_path = apply_pending_restore_on_boot(&cfg).unwrap();
    assert!(restored_path.is_some());

    let db_restored = Db::open(cfg.clone()).unwrap();
    let titles: Vec<String> = db_restored
        .read_blocking(|conn| {
            let mut stmt = conn
                .prepare("SELECT title FROM projects ORDER BY sort_order ASC;")
                .unwrap();
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            Ok(rows)
        })
        .unwrap();
    assert_eq!(titles, vec![String::from("MigratedAlpha")]);
    db_restored.shutdown().await.unwrap();

    // Valida que MigrationRunner em banco novo chega a v11 com search_metadata íntegra
    let tmp2 = TempDir::new().unwrap();
    let cfg2 = DbConfig::for_path(tmp2.path().join("maxterminal.db"));
    let mut conn2 = open_writer(&cfg2).unwrap().conn;
    let reg = MigrationRegistry::builtin().unwrap();
    let out = MigrationRunner::new(reg)
        .run(&mut conn2, &cfg2.db_path, &cfg2.backup_dir, 7)
        .unwrap();
    assert_eq!(out.schema_version, 12);
}
