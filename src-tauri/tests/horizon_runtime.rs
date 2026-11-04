//! Testes de integração do runtime nativo do Laravel Horizon em `src-tauri`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use maxterminal_lib::horizon::HorizonRuntimeService;
use mt_commands::horizon::HorizonDomainService;
use mt_commands::ports::{Clock, ProjectLookup};
use mt_core::ProjectId;
use mt_pty::{PtyConfig, PtyManager};

struct DummyLookup {
    path: PathBuf,
}

impl ProjectLookup for DummyLookup {
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

struct DummyClock;
impl Clock for DummyClock {
    fn now_ms(&self) -> i64 {
        1_700_000_000
    }
}

#[tokio::test]
async fn test_horizon_runtime_service_basic_lifecycle() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pty_manager = PtyManager::new(PtyConfig::default()).expect("pty manager");
    let lookup = Arc::new(DummyLookup {
        path: tmp.path().to_path_buf(),
    });
    let clock = Arc::new(DummyClock);
    let domain = Arc::new(HorizonDomainService::new(lookup, clock));
    let service = HorizonRuntimeService::new(pty_manager, domain);

    assert!(!service.has_active_daemon());

    let proj_id = ProjectId::new();
    let snap = service
        .snapshot(proj_id)
        .await
        .expect("snapshot deve responder");
    assert_eq!(
        snap.availability,
        mt_commands::horizon::HorizonAvailability::Unavailable
    );
    assert_eq!(snap.status, mt_commands::horizon::HorizonStatus::Stopped);
}
