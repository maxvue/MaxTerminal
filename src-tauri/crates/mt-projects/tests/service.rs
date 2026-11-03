//! Testes de integração de `ProjectService` cobrindo regras de negócio, deduplicação,
//! hooks de remoção, leases de atividade e segurança (`mt-projects`).

#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use mt_core::error::codes;
use mt_core::ids::ProjectId;
use mt_core::{MtError, MtResult};
use mt_db::{Db, DbConfig};
use mt_projects::{
    DiscoveryMode, IdentitySource, MemoryProjectEventSink, NewProject, NewProjectRoot,
    ProjectEventSink, ProjectPatch, ProjectRemovalHook, ProjectService, SqliteProjectStore,
    StackKind,
};
use tempfile::TempDir;

struct TestBlockingHook {
    should_fail: AtomicBool,
}

#[async_trait]
impl ProjectRemovalHook for TestBlockingHook {
    async fn before_remove(&self, _id: ProjectId) -> MtResult<()> {
        if self.should_fail.load(Ordering::SeqCst) {
            return Err(MtError::internal("Abas ativas impediram o encerramento."));
        }
        Ok(())
    }
}

async fn setup_service() -> (
    TempDir,
    Db,
    Arc<MemoryProjectEventSink>,
    Arc<ProjectService>,
) {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();
    let store = Arc::new(SqliteProjectStore::from_db(&db));
    let sink = Arc::new(MemoryProjectEventSink::new());
    let sink_dyn: Arc<dyn ProjectEventSink> = sink.clone();
    let svc = ProjectService::new(store, sink_dyn);
    svc.start().await.unwrap();
    (tmp, db, sink, svc)
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn project_crud_symlinks_identity_and_removal_hooks() {
    let (tmp, db, _sink, svc) = setup_service().await;

    let real_dir = tmp.path().join("real-laravel-app");
    fs::create_dir_all(&real_dir).unwrap();
    fs::write(real_dir.join("artisan"), "#!/usr/bin/env php\n").unwrap();
    fs::write(real_dir.join("composer.json"), "{}").unwrap();

    let symlink_dir = tmp.path().join("link-to-laravel");
    symlink(&real_dir, &symlink_dir).unwrap();

    // 1. Adicionar pelo symlink deve armazenar o caminho canônico real e detectar Laravel
    let added = svc
        .add(NewProject {
            path: symlink_dir.to_string_lossy().into_owned(),
            title: None,
            icon: None,
            color: None,
        })
        .await
        .unwrap();

    let canon_real = fs::canonicalize(&real_dir)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(added.path, canon_real);
    assert_eq!(added.title, "real-laravel-app");
    assert_eq!(added.stack, StackKind::Laravel);
    assert_eq!(added.icon, "logos:laravel");
    assert_eq!(added.color, "#FF2D20");
    assert_eq!(added.identity_source, IdentitySource::Auto);

    // 2. Adicionar novamente pelo caminho real deve falhar com project.duplicate_path
    let dup = svc
        .add(NewProject {
            path: real_dir.to_string_lossy().into_owned(),
            title: Some(String::from("Outro")),
            icon: None,
            color: None,
        })
        .await
        .unwrap_err();
    assert_eq!(dup.code, codes::PROJECT_DUPLICATE_PATH);

    // 3. Editar cor manualmente define identity_source = manual e não é sobrescrita por redetect
    let updated = svc
        .update(
            added.id,
            ProjectPatch {
                title: Some(String::from("Laravel Custom")),
                icon: Some(String::from("mdi:folder-star")),
                color: Some(String::from("#14b8a6")),
                reset_identity: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.title, "Laravel Custom");
    assert_eq!(updated.icon, "mdi:folder-star");
    assert_eq!(updated.color, "#14B883".replace("883", "8A6"));
    assert_eq!(updated.identity_source, IdentitySource::Manual);

    // redetect atualiza stack mas preserva ícone e cor manuais
    let redetected = svc.redetect(added.id).await.unwrap();
    assert_eq!(redetected.icon, "mdi:folder-star");
    assert_eq!(redetected.color, "#14B8A6");
    assert_eq!(redetected.identity_source, IdentitySource::Manual);

    // reset_identity junto com icon/color é rejeitado com project.invalid_patch
    let bad_patch = svc
        .update(
            added.id,
            ProjectPatch {
                title: None,
                icon: Some(String::from("mdi:folder")),
                color: None,
                reset_identity: Some(true),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(bad_patch.code, codes::PROJECT_INVALID_PATCH);

    // reset_identity=true restaura auto e re-detecta
    let reset = svc
        .update(
            added.id,
            ProjectPatch {
                title: None,
                icon: None,
                color: None,
                reset_identity: Some(true),
            },
        )
        .await
        .unwrap();
    assert_eq!(reset.identity_source, IdentitySource::Auto);
    assert_eq!(reset.icon, "logos:laravel");
    assert_eq!(reset.color, "#FF2D20");

    // 4. Hook de remoção bloqueando e depois permitindo
    let hook = Arc::new(TestBlockingHook {
        should_fail: AtomicBool::new(true),
    });
    svc.register_removal_hook(hook.clone());

    let blocked = svc.remove(added.id).await.unwrap_err();
    assert_eq!(blocked.code, codes::PROJECT_REMOVAL_BLOCKED);
    assert_eq!(svc.list().await.unwrap().len(), 1);

    hook.should_fail.store(false, Ordering::SeqCst);
    svc.remove(added.id).await.unwrap();
    assert_eq!(svc.list().await.unwrap().len(), 0);

    svc.shutdown().await;
    db.shutdown().await.unwrap();
}

#[tokio::test]
#[allow(clippy::too_many_lines)]
async fn root_discovery_accept_dismiss_and_missing_lifecycle() {
    let (tmp, db, _sink, svc) = setup_service().await;

    let root_dir = tmp.path().join("workspace-root");
    fs::create_dir_all(&root_dir).unwrap();

    // Cria 2 pastas válidas, 1 oculta, 1 arquivo solto e 1 symlink duplicado para proj-alpha
    let alpha = root_dir.join("proj-alpha");
    let beta = root_dir.join("proj-beta");
    let hidden = root_dir.join(".hidden-dir");
    fs::create_dir_all(&alpha).unwrap();
    fs::create_dir_all(&beta).unwrap();
    fs::create_dir_all(&hidden).unwrap();
    fs::write(alpha.join("Cargo.toml"), "[package]\nname=\"alpha\"\n").unwrap();
    fs::write(root_dir.join("README.md"), "hello").unwrap();
    symlink(&alpha, root_dir.join("proj-alpha-alias")).unwrap();

    // Cadastra raiz em modo Suggest com import_existing = false
    let report = svc
        .add_root(NewProjectRoot {
            path: root_dir.to_string_lossy().into_owned(),
            discovery_mode: DiscoveryMode::Suggest,
            import_existing: false,
            watch_enabled: false,
        })
        .await
        .unwrap();

    assert!(report.complete);
    assert_eq!(report.imported.len(), 0);
    // alpha (deduplicado com alias) e beta -> 2 sugestões
    assert_eq!(report.suggested.len(), 2);
    // .hidden-dir + README.md = 2 skipped
    assert_eq!(report.skipped, 2);

    let alpha_canon = fs::canonicalize(&alpha)
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let beta_canon = fs::canonicalize(&beta)
        .unwrap()
        .to_string_lossy()
        .into_owned();

    // Aceita alpha e dispensa beta
    let accepted = svc
        .accept_discovered(vec![alpha_canon.clone()])
        .await
        .unwrap();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].stack, StackKind::Rust);

    svc.dismiss_discovered(vec![beta_canon.clone()])
        .await
        .unwrap();
    assert_eq!(svc.list_discovered().await.unwrap().len(), 0);

    // Rescan não deve ressuscitar beta (dispensado) nem sugerir alpha (já cadastrado)
    let rescan = svc.rescan_root(report.root.id).await.unwrap();
    assert_eq!(rescan.suggested.len(), 0);
    assert_eq!(svc.list_discovered().await.unwrap().len(), 0);

    // Remove fisicamente a pasta alpha -> refresh_missing marca missing=true sem apagar do cadastro
    fs::remove_dir_all(&alpha).unwrap();
    let changed_missing = svc.refresh_missing().await.unwrap();
    assert_eq!(changed_missing.len(), 1);
    assert!(changed_missing[0].missing);

    // Recria a pasta alpha -> refresh_missing restaura missing=false
    fs::create_dir_all(&alpha).unwrap();
    let restored = svc.refresh_missing().await.unwrap();
    assert_eq!(restored.len(), 1);
    assert!(!restored[0].missing);

    svc.shutdown().await;
    db.shutdown().await.unwrap();
}

#[test]
fn security_no_process_command_in_mt_projects_sources() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for entry in fs::read_dir(&manifest_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            let content = fs::read_to_string(&path).unwrap();
            let forbidden_a = ["std::process", "::Command"].concat();
            let forbidden_b = ["process", "::Command"].concat();
            assert!(
                !content.contains(&forbidden_a) && !content.contains(&forbidden_b),
                "Arquivo {path:?} não pode conter chamada a process::Command"
            );
        }
    }
}
