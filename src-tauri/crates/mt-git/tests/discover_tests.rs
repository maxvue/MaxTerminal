//! Testes unitários de descoberta de repositório e worktrees (`mt-git::discover`).
#![allow(clippy::unwrap_used)]

use std::fs;

use mt_git::{
    WorktreeKind, discover_repository, is_relevant_git_metadata_path, metadata_watch_dirs,
};
use tempfile::TempDir;

#[test]
fn discovers_main_repository_from_nested_directory() {
    let tmp = TempDir::new().unwrap();
    let repo_root = tmp.path().join("my-project");
    let dot_git = repo_root.join(".git");
    let nested = repo_root.join("src").join("components");
    fs::create_dir_all(dot_git.join("refs").join("heads")).unwrap();
    fs::create_dir_all(&nested).unwrap();

    let disc = discover_repository(&nested, Some(tmp.path()))
        .unwrap()
        .unwrap();

    assert_eq!(disc.worktree_root, fs::canonicalize(&repo_root).unwrap());
    assert_eq!(disc.worktree_kind, WorktreeKind::Main);
    assert_eq!(disc.worktree_name, "my-project");
    assert!(!disc.is_linked);
    assert_eq!(disc.git_dir, disc.common_dir);

    let watch_dirs = metadata_watch_dirs(&disc);
    assert!(watch_dirs.iter().all(|d| d.starts_with(&disc.git_dir)));
    assert!(!watch_dirs.contains(&disc.worktree_root));
}

#[test]
fn discovers_linked_worktrees_for_maxterminal_and_maxcode() {
    let tmp = TempDir::new().unwrap();
    let main_repo = tmp.path().join("repo");
    let common_git = main_repo.join(".git");
    let wt_git_1 = common_git.join("worktrees").join("wt-feat");
    let wt_git_2 = common_git.join("worktrees").join("wt-8f33ab43");
    fs::create_dir_all(&wt_git_1).unwrap();
    fs::create_dir_all(&wt_git_2).unwrap();

    fs::write(wt_git_1.join("commondir"), "../..\n").unwrap();
    fs::write(wt_git_2.join("commondir"), "../..\n").unwrap();
    fs::write(wt_git_2.join("locked"), "locked\n").unwrap();

    let wt_dir_1 = main_repo.join(".worktrees").join("wt-feat");
    let wt_dir_2 = main_repo.join(".max-code-worktrees").join("wt-8f33ab43");
    fs::create_dir_all(&wt_dir_1).unwrap();
    fs::create_dir_all(&wt_dir_2).unwrap();

    fs::write(
        wt_dir_1.join(".git"),
        format!("gitdir: {}\n", wt_git_1.display()),
    )
    .unwrap();
    fs::write(
        wt_dir_2.join(".git"),
        format!("gitdir: {}\n", wt_git_2.display()),
    )
    .unwrap();

    let d1 = discover_repository(&wt_dir_1, None).unwrap().unwrap();
    assert_eq!(d1.worktree_kind, WorktreeKind::MaxTerminal);
    assert_eq!(d1.worktree_name, "wt-feat");
    assert!(d1.is_linked);
    assert!(!d1.locked);
    assert_eq!(d1.main_worktree_path, fs::canonicalize(&main_repo).unwrap());

    let d2 = discover_repository(&wt_dir_2, None).unwrap().unwrap();
    assert_eq!(d2.worktree_kind, WorktreeKind::MaxCode);
    assert_eq!(d2.worktree_name, "wt-8f33ab43");
    assert!(d2.is_linked);
    assert!(d2.locked);
}

#[test]
fn returns_none_when_stopping_at_home_without_git() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    let sub = home.join("downloads").join("folder");
    fs::create_dir_all(&sub).unwrap();

    let found = discover_repository(&sub, Some(&home)).unwrap();
    assert!(found.is_none());
}

#[test]
fn filters_lock_files_in_watcher_predicate() {
    assert!(!is_relevant_git_metadata_path(std::path::Path::new(
        "/repo/.git/index.lock"
    )));
    assert!(!is_relevant_git_metadata_path(std::path::Path::new(
        "/repo/.git/HEAD.lock"
    )));
    assert!(is_relevant_git_metadata_path(std::path::Path::new(
        "/repo/.git/HEAD"
    )));
    assert!(is_relevant_git_metadata_path(std::path::Path::new(
        "/repo/.git/index"
    )));
    assert!(is_relevant_git_metadata_path(std::path::Path::new(
        "/repo/.git/refs/heads/main"
    )));
}
