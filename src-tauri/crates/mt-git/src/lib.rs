//! Subsistema de inspeção Git, cache por `RepoKey`, watcher `.git` e gerenciamento de worktrees (`mt-git` — Fase 08).

pub mod cache;
pub mod discover;
pub mod error;
pub mod porcelain;
pub mod runner;
pub mod types;
pub mod watcher;
pub mod worktree;

pub use cache::{GitEventSink, GitService, NoopGitEventSink};
pub use discover::{
    DiscoveredRepo, classify_worktree_kind, discover_repository, extract_worktree_name,
};
pub use error::{GitError, GitResult};
pub use porcelain::{ParsedStatus, parse_status_porcelain_v2, parse_worktree_list_porcelain};
pub use runner::{CliGitRunner, GitRunner};
pub use types::{
    DegradedReason, GitBranch, GitChanges, GitContext, GitStatusChanged, GitUpstream, GitWorktree,
    RefreshReason, RepoKey, WorktreeCreateRequest, WorktreeEntry, WorktreeKind, WorktreeLocation,
};
pub use watcher::{RepoMetadataWatcher, is_relevant_git_metadata_path, metadata_watch_dirs};
pub use worktree::{
    ActiveWorktreeGuard, CliWorktreeManager, NoopActiveWorktreeGuard, WorktreeManager,
    validate_branch_or_ref, validate_worktree_name,
};
