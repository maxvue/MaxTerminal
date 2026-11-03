//! Parser e resolvedor de remotes Git para identidade canônica `github.com/{owner}/{repo}`.

use std::sync::LazyLock;

use regex::Regex;
use url::Url;

use crate::model::{GithubRepoIdentity, RepoResolution};
use crate::ports::GitRemoteEntry;

static OWNER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9](?:[A-Za-z0-9-]{0,37}[A-Za-z0-9])?$")
        .unwrap_or_else(|e| panic!("Regex de owner inválida: {e}"))
});

static REPO_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[A-Za-z0-9._-]{1,100}$").unwrap_or_else(|e| panic!("Regex de repo inválida: {e}"))
});

/// Resultado do parsing de uma única URL de remote Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedRemoteUrl {
    /// URL válida apontando para `github.com/{owner}/{repo}`.
    GitHub {
        /// Owner com capitalização original.
        owner: String,
        /// Repo sem sufixo `.git` com capitalização original.
        repo: String,
    },
    /// URL válida apontando para outro host (ex.: `gitlab.com`, `bitbucket.org`, GHE).
    OtherHost {
        /// Host identificado em lowercase.
        host: String,
    },
    /// Formato de URL de remote não reconhecido ou inválido.
    Invalid,
}

/// Analisa uma URL de remote Git (`https://`, `http://`, `ssh://`, `git://` ou SCP `git@host:owner/repo.git`).
#[must_use]
pub fn parse_remote_url(raw: &str) -> ParsedRemoteUrl {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ParsedRemoteUrl::Invalid;
    }

    // 1. Formato URL padrão com scheme (`https://`, `http://`, `ssh://`, `git://`)
    if trimmed.contains("://") {
        let Ok(parsed) = Url::parse(trimmed) else {
            return ParsedRemoteUrl::Invalid;
        };
        let Some(host_raw) = parsed.host_str() else {
            return ParsedRemoteUrl::Invalid;
        };
        let host = host_raw.to_ascii_lowercase();
        if host != "github.com" && host != "www.github.com" {
            return ParsedRemoteUrl::OtherHost { host };
        }
        return extract_owner_repo_from_path(parsed.path());
    }

    // 2. Formato SCP (`git@github.com:owner/repo.git` ou `github.com:owner/repo`)
    if let Some((left, right)) = trimmed.split_once(':') {
        if !right.starts_with("//") {
            let host_part = left.rsplit('@').next().unwrap_or(left).trim();
            let host = host_part.to_ascii_lowercase();
            if host.is_empty() || host.contains('/') {
                return ParsedRemoteUrl::Invalid;
            }
            if host != "github.com" && host != "www.github.com" {
                return ParsedRemoteUrl::OtherHost { host };
            }
            return extract_owner_repo_from_path(right);
        }
    }

    ParsedRemoteUrl::Invalid
}

fn extract_owner_repo_from_path(path: &str) -> ParsedRemoteUrl {
    let clean = path.trim().trim_matches('/');
    let segments: Vec<&str> = clean.split('/').filter(|s| !s.is_empty()).collect();
    if segments.len() != 2 {
        return ParsedRemoteUrl::Invalid;
    }

    let owner = segments[0].trim();
    let mut repo = segments[1].trim();
    if let Some(stripped) = repo.strip_suffix(".git") {
        repo = stripped;
    }

    if !is_valid_owner(owner) || !is_valid_repo(repo) {
        return ParsedRemoteUrl::Invalid;
    }

    ParsedRemoteUrl::GitHub {
        owner: owner.to_string(),
        repo: repo.to_string(),
    }
}

fn is_valid_owner(owner: &str) -> bool {
    !owner.is_empty() && !owner.contains("--") && OWNER_RE.is_match(owner)
}

fn is_valid_repo(repo: &str) -> bool {
    !repo.is_empty() && repo != "." && repo != ".." && REPO_RE.is_match(repo)
}

/// Constrói a identidade canônica `GithubRepoIdentity` a partir de `owner`, `repo` e `remote_name`.
#[must_use]
pub fn build_repo_identity(
    owner: &str,
    repo: &str,
    remote_name: &str,
    default_branch: Option<String>,
) -> GithubRepoIdentity {
    let repo_key = format!(
        "github.com/{}/{}",
        owner.to_ascii_lowercase(),
        repo.to_ascii_lowercase()
    );
    let html_url = format!("https://github.com/{owner}/{repo}");
    GithubRepoIdentity {
        repo_key,
        host: String::from("github.com"),
        owner: owner.to_string(),
        repo: repo.to_string(),
        remote_name: remote_name.to_string(),
        html_url,
        default_branch,
    }
}

/// Resolve a lista de remotes de um repositório Git aplicando a prioridade `origin` -> `upstream` -> ordem alfabética.
#[must_use]
pub fn resolve_from_remotes(
    remotes: Option<&[GitRemoteEntry]>,
    default_branch: Option<String>,
) -> RepoResolution {
    let Some(list) = remotes else {
        return RepoResolution::NotGitRepo;
    };
    if list.is_empty() {
        return RepoResolution::NoRemotes;
    }

    let mut ordered: Vec<&GitRemoteEntry> = list.iter().collect();
    ordered.sort_by(|a, b| {
        remote_priority(&a.name)
            .cmp(&remote_priority(&b.name))
            .then_with(|| a.name.cmp(&b.name))
    });

    let mut first_other_host: Option<(String, String)> = None;

    for entry in ordered {
        match parse_remote_url(&entry.url) {
            ParsedRemoteUrl::GitHub { owner, repo } => {
                return RepoResolution::GitHub {
                    identity: build_repo_identity(
                        &owner,
                        &repo,
                        &entry.name,
                        default_branch.clone(),
                    ),
                };
            }
            ParsedRemoteUrl::OtherHost { host } => {
                if first_other_host.is_none() {
                    first_other_host = Some((entry.name.clone(), host));
                }
            }
            ParsedRemoteUrl::Invalid => {}
        }
    }

    if let Some((remote_name, host)) = first_other_host {
        return RepoResolution::NonGitHubRemote { remote_name, host };
    }

    RepoResolution::NonGitHubRemote {
        remote_name: list[0].name.clone(),
        host: String::from("desconhecido"),
    }
}

fn remote_priority(name: &str) -> u8 {
    match name {
        "origin" => 0,
        "upstream" => 1,
        _ => 2,
    }
}
