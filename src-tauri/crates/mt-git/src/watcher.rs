//! Watcher de metadados `.git` não-recursivo com debounce e filtro anti-tempestade (`*.lock`).

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};

use crate::discover::DiscoveredRepo;
use crate::error::{GitError, GitResult};
use crate::types::RepoKey;

/// Janela de debounce para eventos de filesystem em metadados `.git` (`150 ms`).
pub const GIT_WATCH_DEBOUNCE: Duration = Duration::from_millis(150);

/// Nomes de arquivos relevantes na raiz de `git_dir` / `common_dir`.
const RELEVANT_ROOT_FILES: &[&str] = &[
    "HEAD",
    "index",
    "packed-refs",
    "MERGE_HEAD",
    "REBASE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "BISECT_LOG",
    "ORIG_HEAD",
    "FETCH_HEAD",
    "locked",
];

/// Verifica se um caminho modificado dentro de `.git` é relevante para atualizar o status Git
/// (e ignora arquivos temporários `*.lock` para evitar loops infinitos).
#[must_use]
pub fn is_relevant_git_metadata_path(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };

    if Path::new(file_name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("lock"))
        || file_name.starts_with(".#")
        || file_name.ends_with('~')
    {
        return false;
    }

    if RELEVANT_ROOT_FILES.contains(&file_name) {
        return true;
    }

    // Mudanças em refs/heads/*, refs/remotes/* ou worktrees/*
    path.components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("heads" | "remotes" | "worktrees")
        )
    })
}

/// Retorna a lista de diretórios internos do `.git` que devem ser monitorados em modo `NonRecursive`.
///
/// Jamais inclui a working tree do usuário (`worktree_root`) nem `objects/` ou `logs/`.
#[must_use]
pub fn metadata_watch_dirs(discovered: &DiscoveredRepo) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let mut push_if_exists = |dir: PathBuf| {
        if dir.is_dir() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };

    push_if_exists(discovered.git_dir.clone());
    push_if_exists(discovered.git_dir.join("refs").join("heads"));

    if discovered.common_dir != discovered.git_dir {
        push_if_exists(discovered.common_dir.clone());
    }
    push_if_exists(discovered.common_dir.join("refs").join("heads"));
    push_if_exists(discovered.common_dir.join("refs").join("remotes"));
    push_if_exists(discovered.common_dir.join("worktrees"));

    dirs
}

/// Handle RAII que mantém o watcher `notify` ativo para um [`RepoKey`].
pub struct RepoMetadataWatcher {
    repo_key: RepoKey,
    watched_dirs: Vec<PathBuf>,
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
}

impl std::fmt::Debug for RepoMetadataWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RepoMetadataWatcher")
            .field("repo_key", &self.repo_key)
            .field("watched_dirs", &self.watched_dirs)
            .finish_non_exhaustive()
    }
}

impl RepoMetadataWatcher {
    /// Inicia um watcher não-recursivo sobre os diretórios de metadados `.git` de `discovered`.
    pub fn start<F>(discovered: &DiscoveredRepo, on_relevant_change: F) -> GitResult<Self>
    where
        F: Fn(RepoKey) + Send + Sync + 'static,
    {
        let repo_key = discovered.repo_key.clone();
        let cb_key = repo_key.clone();

        let mut debouncer =
            new_debouncer(GIT_WATCH_DEBOUNCE, None, move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    let has_relevant = events
                        .iter()
                        .flat_map(|ev| ev.paths.iter())
                        .any(|p| is_relevant_git_metadata_path(p));
                    if has_relevant {
                        on_relevant_change(cb_key.clone());
                    }
                }
            })
            .map_err(|err| GitError::WatcherFailed {
                message: err.to_string(),
            })?;

        let dirs = metadata_watch_dirs(discovered);
        for dir in &dirs {
            let _ = debouncer.watch(dir, RecursiveMode::NonRecursive);
        }

        Ok(Self {
            repo_key,
            watched_dirs: dirs,
            _debouncer: debouncer,
        })
    }

    /// Retorna os diretórios monitorados (sempre restritos a metadados `.git`).
    #[must_use]
    pub fn watched_dirs(&self) -> &[PathBuf] {
        &self.watched_dirs
    }
}
