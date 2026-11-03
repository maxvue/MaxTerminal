//! Testes de descoberta Artisan, single-flight por raiz, fingerprint, cache compatível com MaxCode e resolução de raiz.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mt_commands::artisan::model::{
    CatalogSource, CatalogUpdatedPayload, CommandCreatedPayload, DiscoveryFailedPayload,
    LaravelLayout, RunnerKind,
};
use mt_commands::artisan::parser::parse_artisan_list;
use mt_commands::artisan::root::RootResolver;
use mt_commands::artisan::runner::RunnerSelector;
use mt_commands::artisan::service::ArtisanService;
use mt_commands::ports::{ArtisanRunner, Clock, CommandsEventSink, ProjectLookup, RunLimits};
use mt_core::{MtResult, ProjectId};
use tempfile::TempDir;

struct FakeRunner {
    calls: Arc<AtomicUsize>,
    payload: Vec<u8>,
    delay: Duration,
}

#[async_trait::async_trait]
impl ArtisanRunner for FakeRunner {
    fn kind(&self) -> RunnerKind {
        RunnerKind::Local
    }

    fn invoker(&self, root: &Path, cwd: &Path) -> String {
        let rel = mt_commands::artisan::runner::relative_path(cwd, &root.join("artisan"));
        format!("php {}", rel.display())
    }

    fn invoker_tokens(&self, root: &Path, cwd: &Path) -> Vec<String> {
        let rel = mt_commands::artisan::runner::relative_path(cwd, &root.join("artisan"));
        vec!["php".to_string(), rel.display().to_string()]
    }

    async fn list_json(&self, _root: &Path, _limits: RunLimits) -> MtResult<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        Ok(self.payload.clone())
    }
}

#[derive(Default)]
struct RecordingSink {
    updated: Mutex<Vec<CatalogUpdatedPayload>>,
    failed: Mutex<Vec<DiscoveryFailedPayload>>,
}

impl CommandsEventSink for RecordingSink {
    fn catalog_updated(&self, payload: CatalogUpdatedPayload) {
        self.updated.lock().expect("lock").push(payload);
    }

    fn discovery_failed(&self, payload: DiscoveryFailedPayload) {
        self.failed.lock().expect("lock").push(payload);
    }

    fn command_created(&self, _payload: CommandCreatedPayload) {}
}

struct FixedClock(i64);
impl Clock for FixedClock {
    fn now_ms(&self) -> i64 {
        self.0
    }
}

struct SingleProjectLookup {
    id: ProjectId,
    path: PathBuf,
}

impl ProjectLookup for SingleProjectLookup {
    fn owning_project(&self, candidate: &Path) -> Option<(ProjectId, PathBuf)> {
        let canon_proj = fs::canonicalize(&self.path).unwrap_or_else(|_| self.path.clone());
        if candidate.starts_with(&canon_proj) {
            Some((self.id, canon_proj))
        } else {
            None
        }
    }

    fn project_by_id(&self, id: ProjectId) -> Option<PathBuf> {
        (id == self.id).then(|| self.path.clone())
    }
}

fn create_laravel_skeleton(dir: &Path) {
    fs::create_dir_all(dir.join("app/Console/Commands")).expect("mkdir");
    fs::create_dir_all(dir.join("bootstrap")).expect("mkdir");
    fs::create_dir_all(dir.join("routes")).expect("mkdir");
    fs::write(dir.join("artisan"), "#!/usr/bin/env php\n<?php\n").expect("write artisan");
    fs::write(
        dir.join("composer.json"),
        r#"{
            "name": "laravel/laravel",
            "require": { "php": "^8.2", "laravel/framework": "^11.0" },
            "autoload": { "psr-4": { "App\\": "app/" } }
        }"#,
    )
    .expect("write composer.json");
    fs::write(
        dir.join("composer.lock"),
        r#"{
            "packages": [
                { "name": "laravel/framework", "version": "v11.31.0" }
            ]
        }"#,
    )
    .expect("write composer.lock");
    fs::write(
        dir.join("bootstrap/app.php"),
        "<?php\nuse Illuminate\\Foundation\\Application;\nreturn Application::configure(basePath: dirname(__DIR__))->withRouting(commands: __DIR__.'/../routes/console.php')->create();\n",
    )
    .expect("write bootstrap");
}

#[test]
fn parser_normalizes_laravel11_and_laravel10_and_handles_leading_warnings() {
    let l11 = include_bytes!("fixtures/artisan_list_laravel11.json");
    let mut with_warning =
        b"PHP Warning: Module 'xdebug' already loaded in Unknown on line 0\n".to_vec();
    with_warning.extend_from_slice(l11);

    let cmds = parse_artisan_list(&with_warning).expect("parse laravel 11");
    // _complete é hidden e deve ser excluído; ordenação alfabética: about, make:model, migrate
    let names: Vec<&str> = cmds.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, vec!["about", "make:model", "migrate"]);

    let make_model = cmds
        .iter()
        .find(|c| c.name == "make:model")
        .expect("make:model");
    let arg_names: Vec<&str> = make_model
        .arguments
        .iter()
        .map(|a| a.name.as_str())
        .collect();
    assert_eq!(arg_names, vec!["name", "extra"]);
    assert_eq!(make_model.aliases, vec!["model:make"]);

    let migrate = cmds.iter().find(|c| c.name == "migrate").expect("migrate");
    let db_opt = migrate
        .options
        .iter()
        .find(|o| o.name == "--database")
        .expect("--database");
    assert_eq!(db_opt.shortcut, None);
    assert_eq!(db_opt.accept_value, Some(true));
    assert_eq!(db_opt.is_global, None);

    let verbose_opt = migrate
        .options
        .iter()
        .find(|o| o.name == "--verbose")
        .expect("--verbose");
    assert_eq!(verbose_opt.is_global, Some(true));

    let l10 = include_bytes!("fixtures/artisan_list_laravel10.json");
    let cmds10 = parse_artisan_list(l10).expect("parse laravel 10");
    let cache_clear = cmds10
        .iter()
        .find(|c| c.name == "cache:clear")
        .expect("cache:clear");
    assert!(cache_clear.options.iter().any(|o| o.name == "--tags"));
}

#[tokio::test]
async fn single_flight_deduplicates_10_concurrent_calls_and_writes_cache() {
    let tmp = TempDir::new().expect("tempdir");
    create_laravel_skeleton(tmp.path());

    let calls = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(FakeRunner {
        calls: Arc::clone(&calls),
        payload: include_bytes!("fixtures/artisan_list_laravel11.json").to_vec(),
        delay: Duration::from_millis(40),
    });

    // Preserva .max-code/.gitignore existente
    fs::create_dir_all(tmp.path().join(".max-code")).expect("mkdir .max-code");
    fs::write(tmp.path().join(".max-code/.gitignore"), "# custom\n*\n").expect("write gitignore");

    let sink = Arc::new(RecordingSink::default());
    let svc = ArtisanService::new(
        RunnerSelector::new(runner),
        sink.clone(),
        Arc::new(FixedClock(1_700_000_000_000)),
    );

    let mut handles = Vec::new();
    for _ in 0..10 {
        let s = Arc::clone(&svc);
        let root = tmp.path().to_path_buf();
        handles.push(tokio::spawn(async move { s.get(&root).await }));
    }

    for h in handles {
        let cat = h.await.expect("join").expect("catalog");
        assert_eq!(cat.commands.len(), 3);
    }

    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "deve executar o runner apenas 1 vez (CA-05)"
    );

    // Segunda chamada sem alterações não executa o runner
    let cached = svc.get(tmp.path()).await.expect("cached");
    assert_eq!(cached.source, CatalogSource::Cache);
    assert!(!cached.stale);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Verifica que .max-code/.gitignore não foi sobrescrito
    let gitignore = fs::read_to_string(tmp.path().join(".max-code/.gitignore")).expect("read");
    assert_eq!(gitignore, "# custom\n*\n");

    // Modificar um arquivo em app/Console/Commands invalida o fingerprint e força redescoberta (CA-08)
    fs::write(
        tmp.path().join("app/Console/Commands/FooCommand.php"),
        "<?php class FooCommand {}\n",
    )
    .expect("write cmd");
    let refreshed = svc.get(tmp.path()).await.expect("refreshed");
    assert_eq!(refreshed.source, CatalogSource::Fresh);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn foreign_maxcode_cache_serves_stale_immediately_and_revalidates_in_background() {
    let tmp = TempDir::new().expect("tempdir");
    create_laravel_skeleton(tmp.path());

    let cmd_dir = tmp.path().join(".max-code/commands");
    fs::create_dir_all(&cmd_dir).expect("mkdir");
    fs::write(
        cmd_dir.join("artisan.json"),
        include_bytes!("fixtures/maxcode_cache_legacy.json"),
    )
    .expect("write legacy cache");

    let calls = Arc::new(AtomicUsize::new(0));
    let runner = Arc::new(FakeRunner {
        calls: Arc::clone(&calls),
        payload: include_bytes!("fixtures/artisan_list_laravel11.json").to_vec(),
        delay: Duration::from_millis(20),
    });
    let sink = Arc::new(RecordingSink::default());
    let svc = ArtisanService::new(
        RunnerSelector::new(runner),
        sink.clone(),
        Arc::new(FixedClock(1_700_000_000_000)),
    );

    let first = svc.get(tmp.path()).await.expect("foreign cache");
    assert_eq!(first.source, CatalogSource::ForeignCache);
    assert!(first.stale);
    assert_eq!(first.commands.len(), 2);

    // Aguarda a revalidação em segundo plano terminar
    for _ in 0..30 {
        if calls.load(Ordering::SeqCst) >= 1 && !sink.updated.lock().expect("lock").is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let after = svc.get(tmp.path()).await.expect("fresh after revalidate");
    assert_eq!(after.source, CatalogSource::Cache);
    assert!(!after.stale);
    assert_eq!(after.commands.len(), 3);
}

#[test]
fn root_resolver_prefers_worktree_over_main_project_and_computes_relative_invoker() {
    let tmp = TempDir::new().expect("tempdir");
    create_laravel_skeleton(tmp.path());

    let wt_root = tmp.path().join(".max-code-worktrees/wt-feature-1");
    create_laravel_skeleton(&wt_root);
    fs::write(
        wt_root.join(".git"),
        "gitdir: ../../.git/worktrees/wt-feature-1\n",
    )
    .expect("write .git file");
    let wt_sub = wt_root.join("app/Console");
    fs::create_dir_all(&wt_sub).expect("mkdir sub");

    let proj_id = ProjectId::new();
    let lookup = Arc::new(SingleProjectLookup {
        id: proj_id,
        path: tmp.path().to_path_buf(),
    });
    let resolver = RootResolver::new(lookup, Arc::new(FixedClock(1_000)));
    let runner = mt_commands::artisan::runner::LocalPhpRunner::new();

    let resolved = resolver
        .resolve(&wt_sub, Some(proj_id), &runner)
        .expect("resolve")
        .expect("must find worktree root");

    let canon_wt = fs::canonicalize(&wt_root).expect("canon wt");
    assert_eq!(resolved.root, canon_wt.display().to_string());
    assert!(resolved.is_worktree);
    assert_eq!(resolved.invoker, "php ../../artisan");
    assert_eq!(resolved.layout, LaravelLayout::Modern);
    assert_eq!(resolved.laravel_version.as_deref(), Some("11.31.0"));
}
