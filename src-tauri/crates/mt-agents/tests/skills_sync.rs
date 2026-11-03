//! Testes de integração do Sincronizador `MaxSkills` (`catalog`, `targets`, `conflict`, `sync`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use mt_agents::error::codes;
use mt_agents::ports::{ProjectRecord, WorkspacePort};
use mt_agents::registry::AgentKind;
use mt_agents::settings::SkillsSettings;
use mt_agents::skills::catalog::{SKILL_MANIFEST_FILENAME, compute_skill_tree_hash};
use mt_agents::skills::{
    ConflictKind, ConflictResolution, SkillRelevance, SkillSelection, SkillSyncTarget,
    SkillsService, SyncAction, SyncMode, SyncScope,
};
use mt_core::{AppPaths, MtResult, ProjectId, TabId};
use parking_lot::RwLock;

struct MockWorkspacePort {
    project: ProjectRecord,
}

#[async_trait]
impl WorkspacePort for MockWorkspacePort {
    async fn project(&self, _id: ProjectId) -> MtResult<ProjectRecord> {
        Ok(self.project.clone())
    }

    fn project_of_tab(&self, _tab: TabId) -> Option<ProjectId> {
        Some(self.project.id)
    }

    fn tabs_using_cwd(&self, _dir: &Path) -> Vec<TabId> {
        Vec::new()
    }

    async fn ensure_agent_templates(&self, _project: ProjectId) -> MtResult<u32> {
        Ok(4)
    }
}

async fn setup_test_env() -> (
    tempfile::TempDir,
    mt_db::Db,
    ProjectRecord,
    PathBuf,
    Arc<SkillsService>,
) {
    let dir = tempfile::tempdir().unwrap();
    let db_path = dir.path().join("test.sqlite");
    let db_cfg = mt_db::DbConfig::for_path(&db_path);
    let db = mt_db::Db::open(db_cfg).unwrap();

    let project_dir = dir.path().join("my-vue-app");
    std::fs::create_dir_all(&project_dir).unwrap();
    let project_id = ProjectId::new();
    let pid_str = project_id.to_string();
    let ppath_str = project_dir.to_string_lossy().to_string();

    db.write(move |conn| {
        conn.execute(
            "INSERT INTO projects (id, title, path, icon, color, stack, sort_order, created_at, updated_at)
             VALUES (?1, 'My Vue App', ?2, 'logos:vue', '#10B981', 'vue', 0, 1000, 1000);",
            rusqlite::params![pid_str, ppath_str],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "insert_project"))?;
        Ok(())
    })
    .await
    .unwrap();

    let source_dir = dir.path().join("MaxSkills");
    std::fs::create_dir_all(source_dir.join("vue-best-practices")).unwrap();
    std::fs::write(
        source_dir.join("vue-best-practices/SKILL.md"),
        "---\nname: vue-best-practices\ndescription: Padrões Vue 3\nstacks: [vue]\n---\n# Vue 3\n",
    )
    .unwrap();

    std::fs::create_dir_all(source_dir.join("laravel-octane")).unwrap();
    std::fs::write(
        source_dir.join("laravel-octane/SKILL.md"),
        "---\nname: laravel-octane\ndescription: Laravel Octane\nstacks: [laravel]\n---\n# Laravel\n",
    )
    .unwrap();

    let project_rec = ProjectRecord {
        id: project_id,
        title: "My Vue App".to_string(),
        path: project_dir,
        stack: "vue".to_string(),
    };

    let paths = AppPaths {
        config_dir: dir.path().join("config"),
        data_dir: dir.path().join("data"),
        state_dir: dir.path().join("state"),
        cache_dir: dir.path().join("cache"),
        runtime_dir: Some(dir.path().join("run")),
        log_dir: dir.path().join("logs"),
        db_path,
    };

    let settings = Arc::new(RwLock::new(SkillsSettings {
        source_dir: source_dir.to_string_lossy().to_string(),
        default_mode: "symlink".to_string(),
    }));

    let ws: Arc<dyn WorkspacePort> = Arc::new(MockWorkspacePort {
        project: project_rec.clone(),
    });
    let svc = SkillsService::new(db.clone(), ws, paths, dir.path().join("home"), settings);

    (dir, db, project_rec, source_dir, svc)
}

#[tokio::test]
async fn scan_fallback_classifies_relevance_and_computes_deterministic_hash() {
    let (_dir, _db, proj, source_dir, svc) = setup_test_env().await;

    let catalog = svc.scan(Some(proj.id)).await.unwrap();
    assert!(!catalog.from_index);
    assert_eq!(catalog.skills.len(), 2);

    let vue_skill = catalog
        .skills
        .iter()
        .find(|s| s.name == "vue-best-practices")
        .unwrap();
    assert_eq!(vue_skill.relevance, SkillRelevance::Recommended);

    let laravel_skill = catalog
        .skills
        .iter()
        .find(|s| s.name == "laravel-octane")
        .unwrap();
    assert_eq!(laravel_skill.relevance, SkillRelevance::Neutral);

    let h1 = compute_skill_tree_hash(&source_dir.join("vue-best-practices")).unwrap();
    let h2 = compute_skill_tree_hash(&source_dir.join("vue-best-practices")).unwrap();
    assert_eq!(h1, h2);
    assert_eq!(vue_skill.hash, h1);
}

#[tokio::test]
async fn dry_run_plan_does_not_write_and_apply_handles_symlink_copy_conflicts_and_backups() {
    let (_dir, _db, proj, source_dir, svc) = setup_test_env().await;

    // 1. Agy sem destOverride deve falhar com skills.capability_unverified
    let agy_err = svc
        .upsert_target(SkillSyncTarget {
            id: None,
            agent: AgentKind::Agy,
            scope: SyncScope::Project,
            project_id: Some(proj.id),
            mode: SyncMode::Symlink,
            auto_sync: false,
            enabled: true,
            selection: SkillSelection::Recommended,
            project_root: None,
            dest_override: None,
            dest_dir: None,
        })
        .await
        .unwrap_err();
    assert_eq!(agy_err.code.0, codes::SKILLS_CAPABILITY_UNVERIFIED);

    // 2. Cria alvo para Claude Code (modo Copy, seleção All)
    let target = svc
        .upsert_target(SkillSyncTarget {
            id: None,
            agent: AgentKind::Claude,
            scope: SyncScope::Project,
            project_id: Some(proj.id),
            mode: SyncMode::Copy,
            auto_sync: false,
            enabled: true,
            selection: SkillSelection::All,
            project_root: None,
            dest_override: None,
            dest_dir: None,
        })
        .await
        .unwrap();
    let target_id = target.id.unwrap();
    let dest_dir = proj.path.join(".claude/skills");

    // 3. Dry-run (plan) não deve criar a pasta .claude/skills no disco (CA-27)
    let plan1 = svc.plan(target_id).await.unwrap();
    assert!(!dest_dir.exists());
    assert_eq!(plan1.actions.len(), 2);

    // 4. Apply cria as cópias e grava .maxterminal-skill.json apenas no destino (nunca na fonte)
    let res1 = svc.apply(&plan1.plan_id, BTreeMap::new()).await.unwrap();
    assert_eq!(res1.applied, 2);
    assert!(
        dest_dir
            .join("vue-best-practices")
            .join(SKILL_MANIFEST_FILENAME)
            .is_file()
    );
    assert!(
        !source_dir
            .join("vue-best-practices")
            .join(SKILL_MANIFEST_FILENAME)
            .exists()
    );

    // 5. Modifica localmente a cópia instalada -> próximo plan deve acusar Conflict::LocallyModified
    std::fs::write(
        dest_dir.join("vue-best-practices/SKILL.md"),
        "modified locally",
    )
    .unwrap();
    let plan2 = svc.plan(target_id).await.unwrap();
    let has_local_mod = plan2.actions.iter().any(|a| {
        matches!(
            a,
            SyncAction::Conflict {
                skill,
                conflict: ConflictKind::LocallyModified { .. },
                ..
            } if skill == "vue-best-practices"
        )
    });
    assert!(has_local_mod);

    // 6. Se o disco mudar entre plan e apply -> retorna skills.plan_stale (CA-35)
    std::fs::write(dest_dir.join("extra-unmanaged-file"), "x").unwrap();
    let stale_err = svc
        .apply(&plan2.plan_id, BTreeMap::new())
        .await
        .unwrap_err();
    assert_eq!(stale_err.code.0, codes::SKILLS_PLAN_STALE);
    std::fs::remove_file(dest_dir.join("extra-unmanaged-file")).unwrap();

    // 7. Replaneja e aplica Overwrite -> gera backup completo antes de sobrescrever (CA-29)
    let plan3 = svc.plan(target_id).await.unwrap();
    let mut resolutions = BTreeMap::new();
    resolutions.insert(
        "vue-best-practices".to_string(),
        ConflictResolution::Overwrite,
    );
    let res3 = svc.apply(&plan3.plan_id, resolutions).await.unwrap();
    assert_eq!(res3.applied, 1);
    assert_eq!(res3.backups.len(), 1);
    assert!(Path::new(&res3.backups[0]).exists());
}

#[tokio::test]
async fn skills_watcher_ignores_symlinks_worktrees_and_projects_dir_without_blocking() {
    use mt_agents::skills::SyncResult;
    use mt_agents::skills::watch::{SkillsChangedPayload, SkillsEventSink, SkillsWatcher};
    use std::os::unix::fs::symlink;
    use std::time::{Duration, Instant};

    struct TestSkillsSink {
        tx: tokio::sync::mpsc::UnboundedSender<SkillsChangedPayload>,
    }

    impl SkillsEventSink for TestSkillsSink {
        fn emit_changed(&self, payload: SkillsChangedPayload) {
            let _ = self.tx.send(payload);
        }
        fn emit_sync_completed(&self, _result: SyncResult) {}
    }

    let (dir, _db, _proj, source_dir, svc) = setup_test_env().await;

    // Cria diretórios pesados/worktrees e symlinks externos/cíclicos dentro de MaxSkills
    let external_repo = dir.path().join("external-huge-repo");
    std::fs::create_dir_all(external_repo.join("node_modules/pkg")).unwrap();
    symlink(&external_repo, external_repo.join("self_cycle")).unwrap();

    let projects_dir = source_dir.join("projects");
    std::fs::create_dir_all(&projects_dir).unwrap();
    symlink(&external_repo, projects_dir.join("engeapp")).unwrap();

    let worktree_dir = source_dir.join(".max-code-worktrees/wt-test");
    std::fs::create_dir_all(&worktree_dir).unwrap();
    symlink(&external_repo, worktree_dir.join("linked")).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let sink: Arc<dyn SkillsEventSink> = Arc::new(TestSkillsSink { tx });

    let started = Instant::now();
    let watcher = SkillsWatcher::start(Arc::clone(&svc), sink);
    assert!(
        watcher.is_some(),
        "SkillsWatcher deve iniciar na fonte válida"
    );
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "SkillsWatcher::start não deve bloquear seguindo symlinks ou worktrees"
    );

    // Modifica uma skill real e verifica que o evento é emitido normalmente
    std::fs::write(
        source_dir.join("vue-best-practices/SKILL.md"),
        "---\nname: vue-best-practices\ndescription: Atualizado\nstacks: [vue]\n---\n# Vue 3\n",
    )
    .unwrap();

    let ev = tokio::time::timeout(Duration::from_secs(3), rx.recv())
        .await
        .expect("Evento skills://changed deve chegar dentro do timeout")
        .expect("Canal de eventos não deve fechar");
    assert!(
        ev.changed_skills
            .contains(&"vue-best-practices".to_string())
    );
}
