//! Testes unitários do subsistema Laravel Horizon em `mt-commands`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mt_commands::horizon::{
    HorizonAvailability, HorizonDomainService, HorizonStatus, RawHorizonProbeOutput, RawQueueScope,
    build_snapshot_from_probe, compute_capabilities, has_static_horizon_manifest,
};
use mt_commands::ports::{Clock, ProjectLookup};
use mt_core::ProjectId;

struct MockLookup {
    path: PathBuf,
}

impl ProjectLookup for MockLookup {
    fn owning_project(&self, path: &Path) -> Option<(ProjectId, PathBuf)> {
        if path.starts_with(&self.path) {
            Some((ProjectId::new(), self.path.clone()))
        } else {
            None
        }
    }

    fn project_by_id(&self, _id: ProjectId) -> Option<PathBuf> {
        Some(self.path.clone())
    }
}

struct MockClock {
    time: i64,
}

impl Clock for MockClock {
    fn now_ms(&self) -> i64 {
        self.time
    }
}

#[test]
fn test_compute_capabilities_external_status_never_allows_start_or_stop() {
    let (can_start, can_stop, can_clear) =
        compute_capabilities(HorizonAvailability::Available, HorizonStatus::External, true, Some(10));

    // Processo externo não deve conceder iniciar nem parar supervisionado
    assert!(!can_start);
    assert!(!can_stop);
    assert!(can_clear);
}

#[test]
fn test_compute_capabilities_stopped_and_running() {
    let (can_start, can_stop, _) =
        compute_capabilities(HorizonAvailability::Available, HorizonStatus::Stopped, true, None);
    assert!(can_start);
    assert!(!can_stop);

    let (can_start_run, can_stop_run, _) =
        compute_capabilities(HorizonAvailability::Available, HorizonStatus::Running, true, None);
    assert!(!can_start_run);
    assert!(can_stop_run);
}

#[test]
fn test_build_snapshot_preserves_zero_metrics() {
    let project_id = ProjectId::new();
    let root = Path::new("/var/www/laravel-app");

    let probe = RawHorizonProbeOutput {
        ok: true,
        installed: true,
        horizon_version: Some("5.48.3".to_string()),
        environment: Some("local".to_string()),
        scope_fingerprint: Some("scope-123".to_string()),
        masters_count: 1,
        queues: vec![RawQueueScope {
            connection: "redis".to_string(),
            queue: "default".to_string(),
        }],
        pending: Some(0),
        running: Some(0),
        failed: Some(0),
        error: None,
    };

    let snapshot = build_snapshot_from_probe(
        project_id,
        root,
        Some(&probe),
        Some(HorizonStatus::Running),
        None,
        1,
        1_700_000_000,
    );

    assert_eq!(snapshot.availability, HorizonAvailability::Available);
    assert_eq!(snapshot.status, HorizonStatus::Running);
    assert_eq!(snapshot.pending, Some(0));
    assert_eq!(snapshot.running, Some(0));
    assert_eq!(snapshot.failed, Some(0));
    assert_eq!(snapshot.horizon_version.as_deref(), Some("5.48.3"));
    assert_eq!(snapshot.environment.as_deref(), Some("local"));
    assert_eq!(snapshot.queues.len(), 1);
    assert!(snapshot.can_stop);
    assert!(!snapshot.can_start);
}

#[test]
fn test_build_snapshot_handles_unavailable() {
    let project_id = ProjectId::new();
    let root = Path::new("/var/www/generic-app");

    let probe = RawHorizonProbeOutput {
        ok: true,
        installed: false,
        horizon_version: None,
        environment: None,
        scope_fingerprint: None,
        masters_count: 0,
        queues: Vec::new(),
        pending: None,
        running: None,
        failed: None,
        error: None,
    };

    let snapshot =
        build_snapshot_from_probe(project_id, root, Some(&probe), None, None, 1, 1_700_000_000);

    assert_eq!(snapshot.availability, HorizonAvailability::Unavailable);
    assert_eq!(snapshot.status, HorizonStatus::Stopped);
    assert_eq!(snapshot.pending, None);
    assert_eq!(snapshot.running, None);
    assert_eq!(snapshot.failed, None);
    assert!(!snapshot.can_start);
    assert!(!snapshot.can_clear_pending);
}

#[test]
fn test_static_manifest_check() {
    let tmp = tempfile::tempdir().expect("tempdir");
    assert!(!has_static_horizon_manifest(tmp.path()));

    let composer_json = tmp.path().join("composer.json");
    std::fs::write(
        &composer_json,
        r#"{"require": {"laravel/horizon": "^5.0"}}"#,
    )
    .expect("write composer.json");
    assert!(has_static_horizon_manifest(tmp.path()));
}

#[tokio::test]
async fn test_clear_preview_and_token_lifecycle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let composer_json = tmp.path().join("composer.json");
    std::fs::write(
        &composer_json,
        r#"{"require": {"laravel/horizon": "^5.0"}}"#,
    )
    .expect("write composer.json");

    let lookup = Arc::new(MockLookup {
        path: tmp.path().to_path_buf(),
    });
    let clock = Arc::new(MockClock { time: 10_000 });
    let service = HorizonDomainService::new(lookup, clock);

    let proj_id = ProjectId::new();

    // Tentar preview para projeto indisponível retorna erro
    let err = service
        .clear_pending_preview(proj_id)
        .await
        .expect_err("deve falhar para projeto indisponivel");
    assert_eq!(err.code.0, "horizon.unavailable");

    // Tentativa com token inválido
    let err_token = service
        .clear_pending(proj_id, "invalid-token")
        .await
        .expect_err("deve falhar com token invalido");
    assert_eq!(err_token.code.0, "horizon.token_invalid_or_expired");
}
