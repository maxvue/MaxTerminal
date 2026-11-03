//! Parsers puros para `git status --porcelain=v2 --branch -z` e `git worktree list --porcelain -z`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::discover::{classify_worktree_kind, extract_worktree_name};
use crate::error::{GitError, GitResult};
use crate::types::{GitBranch, GitChanges, GitUpstream, WorktreeEntry, WorktreeKind};

/// Limite de segurança de entradas processadas no `git status` para evitar alocação excessiva.
pub const MAX_STATUS_ENTRIES: usize = 50_000;

/// Resultado estruturado do parser de `git status --porcelain=v2 --branch -z`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedStatus {
    /// Estado da branch / `HEAD`.
    pub branch: GitBranch,
    /// Contadores de alterações.
    pub changes: GitChanges,
    /// Divergência em relação ao upstream, se houver.
    pub upstream: Option<GitUpstream>,
}

/// Faz o parse dos bytes brutos retornados por `git status --porcelain=v2 --branch -z`.
#[allow(clippy::too_many_lines)]
pub fn parse_status_porcelain_v2(bytes: &[u8]) -> GitResult<ParsedStatus> {
    let separator = if bytes.contains(&0) { b'\0' } else { b'\n' };
    let mut tokens = bytes
        .split(|&b| b == separator)
        .filter(|tok| !tok.is_empty());

    let mut branch_oid: Option<String> = None;
    let mut branch_head: Option<String> = None;
    let mut upstream_name: Option<String> = None;
    let mut ahead: u32 = 0;
    let mut behind: u32 = 0;

    let mut staged: u32 = 0;
    let mut unstaged: u32 = 0;
    let mut untracked: u32 = 0;
    let mut conflicted: u32 = 0;
    let mut processed_entries: usize = 0;
    let mut summary_truncated = false;

    while let Some(raw_token) = tokens.next() {
        let line = String::from_utf8_lossy(raw_token);
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }

        if let Some(header) = line.strip_prefix("# ") {
            if let Some(oid_val) = header.strip_prefix("branch.oid ") {
                branch_oid = Some(oid_val.trim().to_string());
            } else if let Some(head_val) = header.strip_prefix("branch.head ") {
                branch_head = Some(head_val.trim().to_string());
            } else if let Some(up_val) = header.strip_prefix("branch.upstream ") {
                upstream_name = Some(up_val.trim().to_string());
            } else if let Some(ab_val) = header.strip_prefix("branch.ab ") {
                for part in ab_val.split_whitespace() {
                    if let Some(a) = part.strip_prefix('+') {
                        ahead = a.parse::<u32>().unwrap_or(0);
                    } else if let Some(b) = part.strip_prefix('-') {
                        behind = b.parse::<u32>().unwrap_or(0);
                    }
                }
            }
            continue;
        }

        if processed_entries >= MAX_STATUS_ENTRIES {
            summary_truncated = true;
            if line.starts_with("2 ") && separator == b'\0' {
                let _ = tokens.next();
            }
            continue;
        }

        let bytes_line = line.as_bytes();
        match bytes_line.first().copied() {
            Some(b'1') if bytes_line.len() >= 4 && bytes_line[1] == b' ' => {
                processed_entries += 1;
                let x = bytes_line[2];
                let y = bytes_line[3];
                if x != b'.' {
                    staged = staged.saturating_add(1);
                }
                if y != b'.' {
                    unstaged = unstaged.saturating_add(1);
                }
            }
            Some(b'2') if bytes_line.len() >= 4 && bytes_line[1] == b' ' => {
                processed_entries += 1;
                let x = bytes_line[2];
                let y = bytes_line[3];
                if x != b'.' {
                    staged = staged.saturating_add(1);
                }
                if y != b'.' {
                    unstaged = unstaged.saturating_add(1);
                }
                // No formato `-z`, entradas tipo `2` (rename/copy) possuem um segundo token NUL com o `orig_path`.
                if separator == b'\0' {
                    let _ = tokens.next();
                }
            }
            Some(b'u') if bytes_line.len() >= 4 && bytes_line[1] == b' ' => {
                processed_entries += 1;
                conflicted = conflicted.saturating_add(1);
            }
            Some(b'?') if bytes_line.len() >= 2 && bytes_line[1] == b' ' => {
                processed_entries += 1;
                untracked = untracked.saturating_add(1);
            }
            _ => {}
        }
    }

    let short_oid = branch_oid
        .as_deref()
        .filter(|oid| *oid != "(initial)" && !oid.is_empty())
        .map(|oid| oid.chars().take(8).collect::<String>());

    let is_unborn = branch_oid.as_deref() == Some("(initial)");
    let head_str = branch_head.as_deref().unwrap_or("(detached)");

    let branch = if head_str == "(detached)" {
        GitBranch::Detached {
            oid: short_oid.unwrap_or_else(|| "HEAD".to_string()),
        }
    } else if is_unborn {
        GitBranch::Unborn {
            name: head_str.to_string(),
        }
    } else {
        GitBranch::Named {
            name: head_str.to_string(),
            oid: short_oid,
        }
    };

    let upstream = upstream_name.map(|name| GitUpstream {
        name,
        ahead,
        behind,
    });

    let dirty = staged > 0 || unstaged > 0 || untracked > 0 || conflicted > 0;

    Ok(ParsedStatus {
        branch,
        changes: GitChanges {
            staged,
            unstaged,
            untracked,
            conflicted,
            dirty,
            summary_truncated,
        },
        upstream,
    })
}

/// Faz o parse da saída de `git worktree list --porcelain -z`.
#[allow(clippy::too_many_lines)]
pub fn parse_worktree_list_porcelain(
    bytes: &[u8],
    main_worktree_path: &Path,
) -> GitResult<Vec<WorktreeEntry>> {
    let separator = if bytes.contains(&0) { b'\0' } else { b'\n' };
    let tokens: Vec<String> = bytes
        .split(|&b| b == separator)
        .map(|tok| {
            String::from_utf8_lossy(tok)
                .trim_end_matches('\r')
                .to_string()
        })
        .collect();

    let canonical_main =
        fs::canonicalize(main_worktree_path).unwrap_or_else(|_| main_worktree_path.to_path_buf());

    let mut entries: Vec<WorktreeEntry> = Vec::new();
    let mut current_path: Option<PathBuf> = None;
    let mut head_oid = String::new();
    let mut branch: Option<String> = None;
    let mut detached = false;
    let mut bare = false;
    let mut locked: Option<String> = None;
    let mut prunable: Option<String> = None;

    let flush_entry = |entries: &mut Vec<WorktreeEntry>,
                       path_opt: &mut Option<PathBuf>,
                       head_oid: &mut String,
                       branch: &mut Option<String>,
                       detached: &mut bool,
                       bare: &mut bool,
                       locked: &mut Option<String>,
                       prunable: &mut Option<String>,
                       is_first: bool| {
        let Some(raw_path) = path_opt.take() else {
            return;
        };
        let canonical = fs::canonicalize(&raw_path).unwrap_or_else(|_| raw_path.clone());
        let is_main = is_first || canonical == canonical_main;
        let kind = if is_main {
            WorktreeKind::Main
        } else {
            classify_worktree_kind(&canonical, &canonical_main, true)
        };
        let name = extract_worktree_name(&canonical);
        let short_oid: String = head_oid.chars().take(8).collect();

        entries.push(WorktreeEntry {
            path: canonical,
            head_oid: short_oid,
            branch: branch.take(),
            detached: *detached,
            bare: *bare,
            locked: locked.take(),
            prunable: prunable.take(),
            kind,
            name,
            is_main,
        });

        head_oid.clear();
        *detached = false;
        *bare = false;
    };

    for line in tokens {
        if line.is_empty() {
            let is_first = entries.is_empty();
            flush_entry(
                &mut entries,
                &mut current_path,
                &mut head_oid,
                &mut branch,
                &mut detached,
                &mut bare,
                &mut locked,
                &mut prunable,
                is_first,
            );
            continue;
        }

        if let Some(wt_path) = line.strip_prefix("worktree ") {
            if current_path.is_some() {
                let is_first = entries.is_empty();
                flush_entry(
                    &mut entries,
                    &mut current_path,
                    &mut head_oid,
                    &mut branch,
                    &mut detached,
                    &mut bare,
                    &mut locked,
                    &mut prunable,
                    is_first,
                );
            }
            current_path = Some(PathBuf::from(wt_path.trim()));
        } else if let Some(oid) = line.strip_prefix("HEAD ") {
            head_oid = oid.trim().to_string();
        } else if let Some(b_ref) = line.strip_prefix("branch ") {
            let trimmed = b_ref.trim();
            let short = trimmed.strip_prefix("refs/heads/").unwrap_or(trimmed);
            branch = Some(short.to_string());
        } else if line == "detached" {
            detached = true;
        } else if line == "bare" {
            bare = true;
        } else if line == "locked" {
            locked = Some(String::new());
        } else if let Some(reason) = line.strip_prefix("locked ") {
            locked = Some(reason.trim().to_string());
        } else if line == "prunable" {
            prunable = Some(String::new());
        } else if let Some(reason) = line.strip_prefix("prunable ") {
            prunable = Some(reason.trim().to_string());
        }
    }

    if current_path.is_some() {
        let is_first = entries.is_empty();
        flush_entry(
            &mut entries,
            &mut current_path,
            &mut head_oid,
            &mut branch,
            &mut detached,
            &mut bare,
            &mut locked,
            &mut prunable,
            is_first,
        );
    }

    Ok(entries)
}

#[allow(dead_code)]
fn _ensure_git_error_used(_: GitError) {}
