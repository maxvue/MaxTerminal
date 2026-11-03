//! Descoberta pura em Rust de repositórios e linked worktrees Git sem invocar subprocessos.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::error::{GitError, GitResult};
use crate::types::{GitWorktree, RepoKey, WorktreeKind};

/// Limite máximo de níveis ancestrais percorridos durante a descoberta de repositório.
pub const MAX_ANCESTOR_DEPTH: usize = 64;

/// Tamanho máximo permitido para leitura de um arquivo `.git` (`gitdir:` pointer) ou `commondir`.
pub const MAX_GITFILE_BYTES: u64 = 4096;

/// Metadados estruturais descobertos em disco para uma worktree Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredRepo {
    /// Chave canônica (`worktree_root` canonicalizado).
    pub repo_key: RepoKey,
    /// Caminho canônico da raiz da worktree.
    pub worktree_root: PathBuf,
    /// Diretório `.git` efetivo desta worktree (`<root>/.git` ou `<common>/worktrees/<id>`).
    pub git_dir: PathBuf,
    /// Diretório `.git` comum compartilhado do repositório.
    pub common_dir: PathBuf,
    /// Caminho canônico da worktree principal do repositório.
    pub main_worktree_path: PathBuf,
    /// Classificação da worktree (`Main`, `MaxTerminal`, `MaxCode`, `External`).
    pub worktree_kind: WorktreeKind,
    /// Nome curto da worktree.
    pub worktree_name: String,
    /// `true` quando `git_dir != common_dir`.
    pub is_linked: bool,
    /// `true` quando existe o arquivo `locked` em `git_dir`.
    pub locked: bool,
    /// `true` quando o diretório da worktree não existe mais ou está marcado como podável.
    pub prunable: bool,
}

impl DiscoveredRepo {
    /// Converte os metadados descobertos em [`GitWorktree`].
    #[must_use]
    pub fn to_git_worktree(&self) -> GitWorktree {
        GitWorktree {
            kind: self.worktree_kind,
            name: self.worktree_name.clone(),
            path: self.worktree_root.clone(),
            main_worktree_path: self.main_worktree_path.clone(),
            is_linked: self.is_linked,
            locked: self.locked,
            prunable: self.prunable,
        }
    }
}

/// Classifica o [`WorktreeKind`] de `worktree_root` em relação à `main_worktree_path`.
#[must_use]
pub fn classify_worktree_kind(
    worktree_root: &Path,
    main_worktree_path: &Path,
    is_linked: bool,
) -> WorktreeKind {
    if !is_linked || worktree_root == main_worktree_path {
        return WorktreeKind::Main;
    }

    if let Some(parent) = worktree_root.parent() {
        if parent == main_worktree_path.join(".worktrees") {
            return WorktreeKind::MaxTerminal;
        }
        if parent == main_worktree_path.join(".max-code-worktrees") {
            return WorktreeKind::MaxCode;
        }
    }

    WorktreeKind::External
}

/// Extrai o nome curto de um caminho de worktree.
#[must_use]
pub fn extract_worktree_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("root")
        .to_string()
}

/// Inspeciona os ancestrais de `start_path` (até `$HOME` opcional ou `/`, limitado a 64 níveis)
/// para descobrir se pertence a uma worktree Git.
///
/// Retorna:
/// - `Ok(Some(DiscoveredRepo))` quando encontra um `.git` (diretório ou arquivo `gitdir:`) válido.
/// - `Ok(None)` quando atinge o limite ancestral sem encontrar `.git`.
/// - `Err(GitError)` se `start_path` não existir ou se um arquivo `.git` encontrado estiver corrompido.
pub fn discover_repository(
    start_path: &Path,
    stop_at_home: Option<&Path>,
) -> GitResult<Option<DiscoveredRepo>> {
    if start_path.as_os_str().is_empty() {
        return Err(GitError::InvalidPath {
            path: start_path.to_path_buf(),
            reason: "Caminho vazio".to_string(),
        });
    }

    let canonical_start = fs::canonicalize(start_path).map_err(|err| GitError::InvalidPath {
        path: start_path.to_path_buf(),
        reason: err.to_string(),
    })?;

    let mut current: &Path = if canonical_start.is_file() {
        canonical_start.parent().unwrap_or(&canonical_start)
    } else {
        &canonical_start
    };

    let canonical_home = stop_at_home.and_then(|h| fs::canonicalize(h).ok());

    for _ in 0..MAX_ANCESTOR_DEPTH {
        let dot_git = current.join(".git");
        if let Ok(meta) = fs::symlink_metadata(&dot_git) {
            if meta.is_dir() {
                let worktree_root =
                    fs::canonicalize(current).map_err(|err| GitError::InvalidPath {
                        path: current.to_path_buf(),
                        reason: err.to_string(),
                    })?;
                let git_dir =
                    fs::canonicalize(&dot_git).unwrap_or_else(|_| worktree_root.join(".git"));
                let common_dir = resolve_common_dir(&git_dir)?;
                let is_linked = git_dir != common_dir;
                let main_worktree_path = if is_linked {
                    common_dir
                        .parent()
                        .and_then(|p| fs::canonicalize(p).ok())
                        .unwrap_or_else(|| worktree_root.clone())
                } else {
                    worktree_root.clone()
                };
                let worktree_name = extract_worktree_name(&worktree_root);
                let worktree_kind =
                    classify_worktree_kind(&worktree_root, &main_worktree_path, is_linked);
                let locked = is_linked && git_dir.join("locked").exists();

                return Ok(Some(DiscoveredRepo {
                    repo_key: RepoKey(worktree_root.clone()),
                    worktree_root,
                    git_dir,
                    common_dir,
                    main_worktree_path,
                    worktree_kind,
                    worktree_name,
                    is_linked,
                    locked,
                    prunable: false,
                }));
            } else if meta.is_file() {
                let worktree_root =
                    fs::canonicalize(current).map_err(|err| GitError::InvalidPath {
                        path: current.to_path_buf(),
                        reason: err.to_string(),
                    })?;
                let git_dir = parse_gitfile_pointer(&dot_git, &worktree_root)?;
                let common_dir = resolve_common_dir(&git_dir)?;
                let is_linked = git_dir != common_dir;
                let main_worktree_path = common_dir
                    .parent()
                    .and_then(|p| fs::canonicalize(p).ok())
                    .unwrap_or_else(|| worktree_root.clone());
                let worktree_kind =
                    classify_worktree_kind(&worktree_root, &main_worktree_path, is_linked);
                let worktree_name = extract_worktree_name(&worktree_root);
                let locked = git_dir.join("locked").exists();

                return Ok(Some(DiscoveredRepo {
                    repo_key: RepoKey(worktree_root.clone()),
                    worktree_root,
                    git_dir,
                    common_dir,
                    main_worktree_path,
                    worktree_kind,
                    worktree_name,
                    is_linked,
                    locked,
                    prunable: false,
                }));
            }
        }

        if let Some(home) = &canonical_home
            && current == home.as_path()
        {
            break;
        }

        let Some(parent) = current.parent() else {
            break;
        };
        if parent == current {
            break;
        }
        current = parent;
    }

    Ok(None)
}

fn read_small_text_file(path: &Path) -> GitResult<String> {
    let meta = fs::metadata(path).map_err(|err| GitError::InvalidPath {
        path: path.to_path_buf(),
        reason: err.to_string(),
    })?;
    if meta.len() > MAX_GITFILE_BYTES {
        return Err(GitError::InvalidPath {
            path: path.to_path_buf(),
            reason: format!("Arquivo excede o limite de {MAX_GITFILE_BYTES} bytes"),
        });
    }
    let mut file = fs::File::open(path).map_err(|err| GitError::InvalidPath {
        path: path.to_path_buf(),
        reason: err.to_string(),
    })?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)
        .map_err(|err| GitError::InvalidPath {
            path: path.to_path_buf(),
            reason: err.to_string(),
        })?;
    Ok(buf)
}

fn parse_gitfile_pointer(dot_git_file: &Path, worktree_root: &Path) -> GitResult<PathBuf> {
    let content = read_small_text_file(dot_git_file)?;
    let first_line = content.lines().next().unwrap_or("").trim();
    let Some(raw_target) = first_line.strip_prefix("gitdir:") else {
        return Err(GitError::InvalidPath {
            path: dot_git_file.to_path_buf(),
            reason: "Arquivo .git não contém prefixo 'gitdir:' válido".to_string(),
        });
    };
    let target_trimmed = raw_target.trim();
    if target_trimmed.is_empty() {
        return Err(GitError::InvalidPath {
            path: dot_git_file.to_path_buf(),
            reason: "Alvo 'gitdir:' vazio".to_string(),
        });
    }
    let candidate = Path::new(target_trimmed);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        worktree_root.join(candidate)
    };
    fs::canonicalize(&resolved).map_err(|err| GitError::InvalidPath {
        path: resolved,
        reason: format!("Diretório gitdir inexistente ou inválido: {err}"),
    })
}

fn resolve_common_dir(git_dir: &Path) -> GitResult<PathBuf> {
    let commondir_file = git_dir.join("commondir");
    if !commondir_file.exists() {
        return Ok(git_dir.to_path_buf());
    }
    let content = read_small_text_file(&commondir_file)?;
    let rel = content.lines().next().unwrap_or("").trim();
    if rel.is_empty() {
        return Ok(git_dir.to_path_buf());
    }
    let candidate = Path::new(rel);
    let resolved = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        git_dir.join(candidate)
    };
    fs::canonicalize(&resolved).map_err(|err| GitError::InvalidPath {
        path: resolved,
        reason: format!("Diretório commondir inválido: {err}"),
    })
}
