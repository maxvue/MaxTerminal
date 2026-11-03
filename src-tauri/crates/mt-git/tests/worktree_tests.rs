//! Testes de criação, listagem e remoção protegida de worktrees (`mt-git::worktree`).
#![allow(clippy::unwrap_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use mt_core::error::MtError;
use mt_core::error::codes;
use mt_git::{
    ActiveWorktreeGuard, CliGitRunner, CliWorktreeManager, GitError, NoopActiveWorktreeGuard,
    WorktreeCreateRequest, WorktreeKind, WorktreeLocation, WorktreeManager, validate_worktree_name,
};
use tempfile::TempDir;

fn init_real_git_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .current_dir(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Tester")
            .env("GIT_AUTHOR_EMAIL", "tester@example.com")
            .env("GIT_COMMITTER_NAME", "Tester")
            .env("GIT_COMMITTER_EMAIL", "tester@example.com")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} falhou");
    };
    run(&["init", "-b", "main"]);
    fs::write(dir.join("README.md"), "# MaxTerminal\n").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "Commit inicial"]);
}

struct MockInUseGuard {
    busy_path: PathBuf,
}

impl ActiveWorktreeGuard for MockInUseGuard {
    fn is_worktree_in_use(&self, worktree_path: &Path) -> bool {
        let canonical =
            fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
        let busy = fs::canonicalize(&self.busy_path).unwrap_or_else(|_| self.busy_path.clone());
        canonical == busy
    }
}

#[test]
fn validates_worktree_names_strictly() {
    assert!(validate_worktree_name("wt-feat_01.ok").is_ok());
    assert!(validate_worktree_name("").is_err());
    assert!(validate_worktree_name(".").is_err());
    assert!(validate_worktree_name("..").is_err());
    assert!(validate_worktree_name("-flag").is_err());
    assert!(validate_worktree_name("a/b").is_err());
    assert!(validate_worktree_name("nome com espaco").is_err());
}

#[tokio::test]
async fn creates_lists_and_enforces_safe_worktree_removal_with_real_git() {
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path().join("project");
    init_real_git_repo(&repo_dir);

    let runner = Arc::new(CliGitRunner::new());
    let manager = CliWorktreeManager::new(runner.clone(), Arc::new(NoopActiveWorktreeGuard));

    // Cria worktree MaxTerminal (.worktrees/wt-one)
    let created = manager
        .create(WorktreeCreateRequest {
            project_root: repo_dir.clone(),
            name: "wt-one".to_string(),
            branch: None,
            base_ref: None,
            location: WorktreeLocation::MaxTerminal,
        })
        .await
        .unwrap();

    assert_eq!(created.kind, WorktreeKind::MaxTerminal);
    assert_eq!(created.name, "wt-one");
    assert_eq!(created.branch.as_deref(), Some("wt/wt-one"));
    assert!(!created.is_main);

    // Lista worktrees (main + wt-one)
    let list = manager
        .list(&repo_dir)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(list.len(), 2);

    // Recusa remover worktree principal
    let err_main = manager.remove(&repo_dir, true).await.unwrap_err();
    let mt_main: MtError = err_main.into();
    assert_eq!(mt_main.code, codes::GIT_WORKTREE_NOT_MANAGED);

    // Recusa remover worktree em uso por aba ativa
    let busy_manager = CliWorktreeManager::new(
        runner.clone(),
        Arc::new(MockInUseGuard {
            busy_path: created.path.clone(),
        }),
    );
    let err_busy = busy_manager.remove(&created.path, false).await.unwrap_err();
    assert!(matches!(err_busy, GitError::WorktreeInUse { .. }));

    // Suja a worktree e tenta remover com force=false -> git.worktree_dirty
    fs::write(created.path.join("dirty.txt"), "uncommitted").unwrap();
    let err_dirty = manager.remove(&created.path, false).await.unwrap_err();
    let mt_dirty: MtError = err_dirty.into();
    assert_eq!(mt_dirty.code, codes::GIT_WORKTREE_DIRTY);

    // Com force=true remove com sucesso
    manager.remove(&created.path, true).await.unwrap();
    let list_after = manager.list(&repo_dir).await.unwrap();
    assert_eq!(list_after.len(), 1);
}
