//! Testes de cache, deduplicação por `RepoKey` e coalescing de refresh (`mt-git::cache`).
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use mt_core::ids::{PaneId, TabId};
use mt_git::{
    GitBranch, GitChanges, GitContext, GitEventSink, GitResult, GitRunner, GitService,
    GitStatusChanged, ParsedStatus, RefreshReason, WorktreeEntry,
};
use parking_lot::Mutex;
use tempfile::TempDir;

struct CountingRunner {
    calls: AtomicUsize,
    delay: Duration,
    staged_count: AtomicUsize,
}

#[async_trait]
impl GitRunner for CountingRunner {
    async fn status(&self, _worktree_root: &Path) -> GitResult<ParsedStatus> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.delay.is_zero() {
            tokio::time::sleep(self.delay).await;
        }
        let staged = u32::try_from(self.staged_count.load(Ordering::SeqCst)).unwrap_or(0);
        Ok(ParsedStatus {
            branch: GitBranch::Named {
                name: "main".to_string(),
                oid: Some("12345678".to_string()),
            },
            changes: GitChanges {
                staged,
                unstaged: 0,
                untracked: 0,
                conflicted: 0,
                dirty: staged > 0,
                summary_truncated: false,
            },
            upstream: None,
        })
    }

    async fn worktree_list(&self, _main_worktree_path: &Path) -> GitResult<Vec<WorktreeEntry>> {
        Ok(Vec::new())
    }

    async fn ref_exists(&self, _repo_root: &Path, _full_ref: &str) -> GitResult<bool> {
        Ok(false)
    }

    async fn worktree_add(
        &self,
        _main_worktree_path: &Path,
        _target_path: &Path,
        _branch_name: &str,
        _base_ref: &str,
        _create_branch: bool,
    ) -> GitResult<()> {
        Ok(())
    }

    async fn worktree_remove(
        &self,
        _main_worktree_path: &Path,
        _worktree_path: &Path,
        _force: bool,
    ) -> GitResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct CapturingSink {
    events: Mutex<Vec<GitStatusChanged>>,
}

impl GitEventSink for CapturingSink {
    fn emit_status_changed(&self, payload: GitStatusChanged) {
        self.events.lock().push(payload);
    }
}

#[tokio::test]
async fn deduplicates_panes_in_same_repo_and_coalesces_concurrent_refreshes() {
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path().join("project");
    let sub_a = repo_dir.join("src");
    let sub_b = repo_dir.join("tests");
    fs::create_dir_all(repo_dir.join(".git")).unwrap();
    fs::create_dir_all(&sub_a).unwrap();
    fs::create_dir_all(&sub_b).unwrap();

    let runner = Arc::new(CountingRunner {
        calls: AtomicUsize::new(0),
        delay: Duration::from_millis(30),
        staged_count: AtomicUsize::new(0),
    });
    let sink = Arc::new(CapturingSink::default());
    let service = GitService::new(runner.clone(), sink.clone(), false);

    let tab_id = TabId::new();
    let pane_1 = PaneId::new();
    let pane_2 = PaneId::new();

    // Primeiro bind faz 1 chamada ao runner
    let ctx1 = service
        .bind_pane_cwd(tab_id, pane_1, sub_a.clone(), RefreshReason::Initial)
        .await
        .unwrap();
    assert_eq!(ctx1.revision(), 1);
    assert_eq!(runner.calls.load(Ordering::SeqCst), 1);

    // Segundo painel em outra subpasta do mesmo repo reutiliza o cache (0 chamadas extras)
    let ctx2 = service
        .bind_pane_cwd(tab_id, pane_2, sub_b.clone(), RefreshReason::Initial)
        .await
        .unwrap();
    assert_eq!(ctx2.revision(), 1);
    assert_eq!(runner.calls.load(Ordering::SeqCst), 1);

    // Quando o estado semântico muda, a revisão sobe e ambos os painéis aparecem no evento
    runner.staged_count.store(2, Ordering::SeqCst);
    let refreshed = service
        .refresh_path(&repo_dir, RefreshReason::Manual)
        .await
        .unwrap();
    assert_eq!(refreshed.revision(), 2);
    if let GitContext::Repository { changes, .. } = refreshed {
        assert_eq!(changes.staged, 2);
        assert!(changes.dirty);
    } else {
        panic!("Esperado GitContext::Repository");
    }

    let emitted = sink.events.lock();
    assert_eq!(emitted.len(), 2);
    assert_eq!(emitted[1].affected_pane_ids.len(), 2);
    assert_eq!(emitted[1].affected_tab_ids, vec![tab_id]);
}
