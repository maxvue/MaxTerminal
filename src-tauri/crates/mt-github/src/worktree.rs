//! Convenções de nomenclatura e correspondência de branches e worktrees para issues (`MaxCode` / `MaxTerminal`).

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;

use crate::model::IssueWorktreeResolution;

const MAX_SLUG_CHARS: usize = 38;

/// Converte um título em `pt-BR` para um slug ASCII hifenizado determinístico.
#[must_use]
pub fn slugify_issue_title(title: &str) -> String {
    let mut mapped = String::with_capacity(title.len());
    for ch in title.chars() {
        let lower = match ch {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'Á' | 'À' | 'Â' | 'Ã' | 'Ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' | 'Ê' | 'Ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'Í' | 'Ì' | 'Î' | 'Ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'Ó' | 'Ò' | 'Ô' | 'Õ' | 'Ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' | 'Ú' | 'Ù' | 'Û' | 'Ü' => 'u',
            'ç' | 'Ç' => 'c',
            'ñ' | 'Ñ' => 'n',
            c if c.is_ascii_alphanumeric() => c.to_ascii_lowercase(),
            _ => '-',
        };
        mapped.push(lower);
    }

    let collapsed = mapped
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    if collapsed.is_empty() {
        return String::from("issue");
    }

    if collapsed.len() <= MAX_SLUG_CHARS {
        collapsed
    } else {
        collapsed[..MAX_SLUG_CHARS]
            .trim_end_matches('-')
            .to_string()
    }
}

/// Gera o nome canônico padrão da branch de correção para uma issue (`fix/issue-{number}-{slug}`).
#[must_use]
pub fn default_issue_branch_name(issue_number: u64, issue_title: &str) -> String {
    let slug = slugify_issue_title(issue_title);
    format!("fix/issue-{issue_number}-{slug}")
}

/// Gera o caminho sugerido de worktree para uma issue sob `.worktrees/issue-{number}`.
#[must_use]
pub fn suggested_issue_worktree_path(project_path: &Path, issue_number: u64) -> String {
    let maxcode_dir = project_path.join(".max-code-worktrees");
    let base_dir = if maxcode_dir.is_dir() {
        maxcode_dir
    } else {
        project_path.join(".worktrees")
    };
    base_dir
        .join(format!("issue-{issue_number}"))
        .to_string_lossy()
        .into_owned()
}

/// Verifica se um nome de branch ou caminho de worktree corresponde ao número da issue.
#[must_use]
pub fn matches_issue_number(branch_or_path: &str, issue_number: u64) -> bool {
    static TOKEN_SPLIT_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"[^a-zA-Z0-9]+").unwrap_or_else(|e| panic!("Regex inválida: {e}"))
    });

    let lower = branch_or_path.to_ascii_lowercase();
    let num_str = issue_number.to_string();
    let tokens: Vec<&str> = TOKEN_SPLIT_RE
        .split(&lower)
        .filter(|t| !t.is_empty())
        .collect();

    for (idx, &tok) in tokens.iter().enumerate() {
        if tok == num_str {
            if idx == 0 {
                return true;
            }
            let prev = tokens[idx - 1];
            if matches!(
                prev,
                "issue" | "fix" | "bugfix" | "feat" | "hotfix" | "gh" | "wt"
            ) {
                return true;
            }
        }
    }
    false
}

/// Resolve a melhor correspondência entre worktrees ativas e branches locais para uma issue.
#[must_use]
pub fn resolve_worktree_from_inventory(
    project_path: &Path,
    issue_number: u64,
    issue_title: &str,
    default_branch: Option<&str>,
    existing_worktrees: &[(String, Option<String>)],
    local_branches: &[String],
) -> IssueWorktreeResolution {
    // 1. Verifica worktrees já existentes (exceto a pasta raiz principal)
    let root_canon = project_path.to_string_lossy();
    for (wt_path, wt_branch) in existing_worktrees {
        if wt_path == root_canon.as_ref() {
            continue;
        }
        let branch_matches = wt_branch
            .as_deref()
            .is_some_and(|b| matches_issue_number(b, issue_number));
        let path_matches = matches_issue_number(wt_path, issue_number);
        if branch_matches || path_matches {
            return IssueWorktreeResolution::ExistingWorktree {
                path: wt_path.clone(),
                branch: wt_branch
                    .clone()
                    .unwrap_or_else(|| default_issue_branch_name(issue_number, issue_title)),
            };
        }
    }

    let suggested_path = suggested_issue_worktree_path(project_path, issue_number);

    // 2. Verifica branches locais existentes
    for branch in local_branches {
        if matches_issue_number(branch, issue_number) {
            return IssueWorktreeResolution::ExistingBranch {
                branch: branch.clone(),
                suggested_worktree_path: suggested_path,
            };
        }
    }

    // 3. Sugere criação de nova branch e worktree
    IssueWorktreeResolution::NeedsCreation {
        suggested_branch: default_issue_branch_name(issue_number, issue_title),
        suggested_worktree_path: suggested_path,
        base_branch: default_branch.unwrap_or("main").to_string(),
    }
}
