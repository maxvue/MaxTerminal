//! Adaptador seguro para o `gh` CLI (`tokio::process::Command`, sem `sh -c`, com `kill_on_drop(true)`).

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::client::{
    RemoteFetchOutcome, RemoteIssueDetailBundle, RemoteIssueRecord, parse_github_timestamp_ms,
};
use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::labels::to_repo_label;
use crate::model::{
    GhCliStatus, GithubRepoIdentity, GithubTransportKind, IssueComment, IssueStateFilter, RepoLabel,
};

const MAX_STDOUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;

/// Saída bruta da execução de um subprocesso `gh`.
#[derive(Debug, Clone)]
pub struct GhCommandOutput {
    /// Código de saída (`0` para sucesso).
    pub exit_code: i32,
    /// `stdout` UTF-8 truncado ao limite seguro.
    pub stdout: String,
    /// `stderr` UTF-8 truncado ao limite seguro.
    pub stderr: String,
}

/// Executor abstrato de subprocesso `gh` (permite injeção de binário falso ou mock em testes).
#[async_trait]
pub trait GhProcessRunner: Send + Sync {
    /// Executa `gh` com os argumentos informados, sem shell, respeitando `timeout`.
    async fn run_gh(&self, args: &[String], timeout: Duration) -> GithubResult<GhCommandOutput>;
}

/// Executor real de `gh` usando `tokio::process::Command` com ambiente limpo (`GH_PAGER=cat`, `GH_PROMPT_DISABLED=1`).
#[derive(Debug, Clone)]
pub struct TokioGhProcessRunner {
    binary_path: PathBuf,
}

impl Default for TokioGhProcessRunner {
    fn default() -> Self {
        Self::new(PathBuf::from("gh"))
    }
}

impl TokioGhProcessRunner {
    /// Cria um runner apontando para um caminho específico do binário `gh`.
    #[must_use]
    pub const fn new(binary_path: PathBuf) -> Self {
        Self { binary_path }
    }
}

#[async_trait]
impl GhProcessRunner for TokioGhProcessRunner {
    async fn run_gh(&self, args: &[String], timeout: Duration) -> GithubResult<GhCommandOutput> {
        let mut cmd = Command::new(&self.binary_path);
        cmd.args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .env("NO_COLOR", "1")
            .env("GH_PAGER", "cat")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                GithubError::new(
                    GithubErrorKind::AuthRequired,
                    "O utilitário 'gh' (GitHub CLI) não foi encontrado no PATH.",
                )
            } else {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "Falha ao iniciar o processo 'gh'.",
                )
                .with_details(e.to_string())
            }
        })?;

        let mut stdout_pipe = child.stdout.take();
        let mut stderr_pipe = child.stderr.take();

        let fut = async move {
            let stdout_task = tokio::spawn(async move {
                let mut buf = Vec::new();
                if let Some(ref mut pipe) = stdout_pipe {
                    let mut take = pipe.take((MAX_STDOUT_BYTES + 1) as u64);
                    let _ = take.read_to_end(&mut buf).await;
                }
                buf
            });
            let stderr_task = tokio::spawn(async move {
                let mut buf = Vec::new();
                if let Some(ref mut pipe) = stderr_pipe {
                    let mut take = pipe.take((MAX_STDERR_BYTES + 1) as u64);
                    let _ = take.read_to_end(&mut buf).await;
                }
                buf
            });

            let status = child.wait().await.map_err(|e| {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "Falha ao aguardar término do 'gh'.",
                )
                .with_details(e.to_string())
            })?;
            let out_bytes = stdout_task.await.unwrap_or_default();
            let err_bytes = stderr_task.await.unwrap_or_default();
            if out_bytes.len() > MAX_STDOUT_BYTES {
                return Err(GithubError::new(
                    GithubErrorKind::Internal,
                    "A saída do 'gh' excedeu o limite máximo permitido de 16 MiB.",
                ));
            }
            Ok(GhCommandOutput {
                exit_code: status.code().unwrap_or(-1),
                stdout: String::from_utf8_lossy(&out_bytes).into_owned(),
                stderr: String::from_utf8_lossy(
                    &err_bytes[..err_bytes.len().min(MAX_STDERR_BYTES)],
                )
                .into_owned(),
            })
        };

        tokio::time::timeout(timeout, fut).await.map_err(|_| {
            GithubError::new(
                GithubErrorKind::NetworkUnavailable,
                "Tempo limite esgotado ao executar o comando 'gh'.",
            )
        })?
    }
}

/// Cliente de alto nível baseado no `gh` CLI.
#[derive(Clone)]
pub struct GhCliClient {
    runner: Arc<dyn GhProcessRunner>,
    timeout: Duration,
}

impl GhCliClient {
    /// Cria um novo `GhCliClient` com runner customizado.
    #[must_use]
    pub fn new(runner: Arc<dyn GhProcessRunner>) -> Self {
        Self {
            runner,
            timeout: Duration::from_secs(15),
        }
    }

    /// Define o timeout padrão de chamadas `gh`.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Verifica se o `gh` está instalado e autenticado em `github.com`.
    pub async fn probe_status(&self) -> GhCliStatus {
        let ver_args = vec![String::from("--version")];
        let version_out = match self.runner.run_gh(&ver_args, Duration::from_secs(3)).await {
            Ok(out) if out.exit_code == 0 => out,
            _ => {
                return GhCliStatus {
                    installed: false,
                    version: None,
                    authenticated: false,
                    account_login: None,
                    scopes: Vec::new(),
                    message: Some(String::from("GitHub CLI ('gh') não encontrado no sistema.")),
                };
            }
        };

        let version = version_out
            .stdout
            .lines()
            .next()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty());

        let auth_args = vec![
            String::from("auth"),
            String::from("status"),
            String::from("--hostname"),
            String::from("github.com"),
        ];
        match self.runner.run_gh(&auth_args, Duration::from_secs(5)).await {
            Ok(out) if out.exit_code == 0 => {
                let combined = format!("{}\n{}", out.stdout, out.stderr);
                let account_login = extract_login_from_auth_status(&combined);
                let scopes = extract_scopes_from_auth_status(&combined);
                GhCliStatus {
                    installed: true,
                    version,
                    authenticated: true,
                    account_login,
                    scopes,
                    message: None,
                }
            }
            Ok(out) => GhCliStatus {
                installed: true,
                version,
                authenticated: false,
                account_login: None,
                scopes: Vec::new(),
                message: Some(mt_db::sanitizer::sanitize(out.stderr.trim()).into_owned()),
            },
            Err(err) => GhCliStatus {
                installed: true,
                version,
                authenticated: false,
                account_login: None,
                scopes: Vec::new(),
                message: Some(err.message),
            },
        }
    }

    /// Lista issues via `gh api repos/{owner}/{repo}/issues`.
    pub async fn fetch_issues(
        &self,
        repo: &GithubRepoIdentity,
        state: IssueStateFilter,
        per_page: u32,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RemoteIssueRecord>>> {
        let state_param = match state {
            IssueStateFilter::Open => "open",
            IssueStateFilter::Closed => "closed",
            IssueStateFilter::All => "all",
        };
        let endpoint = format!(
            "repos/{}/{}/issues?state={state_param}&per_page={}&sort=updated&direction=desc",
            repo.owner,
            repo.repo,
            per_page.clamp(1, 100)
        );
        let args = vec![
            String::from("api"),
            String::from("--method"),
            String::from("GET"),
            endpoint,
        ];
        let out = self.runner.run_gh(&args, self.timeout).await?;
        Self::check_exit(&out)?;

        let raw_items: Vec<Value> = serde_json::from_str(&out.stdout).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Resposta JSON inválida do 'gh api' ao listar issues.",
            )
            .with_details(e.to_string())
        })?;

        let mut issues = Vec::new();
        for item in raw_items {
            // Exclui Pull Requests que a API /issues retorna junto
            if item.get("pull_request").is_some() {
                continue;
            }
            if let Some(parsed) = parse_rest_or_gh_issue_json(&item) {
                issues.push(parsed);
            }
        }

        Ok(RemoteFetchOutcome::Fresh {
            data: issues,
            etag: None,
            transport: GithubTransportKind::GhCli,
            rate_limit: None,
        })
    }

    /// Busca detalhe e comentários de uma issue via `gh api`.
    pub async fn fetch_issue_detail(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
    ) -> GithubResult<RemoteIssueDetailBundle> {
        let issue_ep = format!("repos/{}/{}/issues/{issue_number}", repo.owner, repo.repo);
        let issue_out = self
            .runner
            .run_gh(
                &[
                    String::from("api"),
                    String::from("--method"),
                    String::from("GET"),
                    issue_ep,
                ],
                self.timeout,
            )
            .await?;
        Self::check_exit(&issue_out)?;

        let issue_val: Value = serde_json::from_str(&issue_out.stdout).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "JSON inválido ao carregar detalhe da issue via 'gh'.",
            )
            .with_details(e.to_string())
        })?;
        let issue = parse_rest_or_gh_issue_json(&issue_val).ok_or_else(|| {
            GithubError::new(
                GithubErrorKind::NotFound,
                format!("Issue #{issue_number} não encontrada."),
            )
        })?;

        let comments_ep = format!(
            "repos/{}/{}/issues/{issue_number}/comments?per_page=100",
            repo.owner, repo.repo
        );
        let comments_out = self
            .runner
            .run_gh(
                &[
                    String::from("api"),
                    String::from("--method"),
                    String::from("GET"),
                    comments_ep,
                ],
                self.timeout,
            )
            .await?;
        Self::check_exit(&comments_out)?;

        let raw_comments: Vec<Value> = serde_json::from_str(&comments_out.stdout).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "JSON inválido ao carregar comentários da issue via 'gh'.",
            )
            .with_details(e.to_string())
        })?;

        let mut comments = Vec::with_capacity(raw_comments.len());
        for c in &raw_comments {
            if let Some(parsed) = parse_issue_comment_json(c, issue_number) {
                comments.push(parsed);
            }
        }
        let comments_complete = (comments.len() as u32) >= issue.comments_count;

        Ok(RemoteIssueDetailBundle {
            issue,
            comments,
            comments_complete,
            transport: GithubTransportKind::GhCli,
        })
    }

    /// Lista labels do repositório via `gh api repos/{owner}/{repo}/labels`.
    pub async fn fetch_repo_labels(
        &self,
        repo: &GithubRepoIdentity,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RepoLabel>>> {
        let ep = format!("repos/{}/{}/labels?per_page=100", repo.owner, repo.repo);
        let out = self
            .runner
            .run_gh(
                &[
                    String::from("api"),
                    String::from("--method"),
                    String::from("GET"),
                    ep,
                ],
                self.timeout,
            )
            .await?;
        Self::check_exit(&out)?;

        let raw_labels: Vec<GhLabelWire> = serde_json::from_str(&out.stdout).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "JSON inválido ao listar labels via 'gh'.",
            )
            .with_details(e.to_string())
        })?;

        let labels = raw_labels
            .into_iter()
            .map(|l| to_repo_label(&l.name, l.color.as_deref(), l.description.as_deref()))
            .collect();

        Ok(RemoteFetchOutcome::Fresh {
            data: labels,
            etag: None,
            transport: GithubTransportKind::GhCli,
            rate_limit: None,
        })
    }

    /// Cria label no repositório via `gh api`.
    pub async fn create_repo_label(
        &self,
        repo: &GithubRepoIdentity,
        name: &str,
        color_hex: &str,
        description: &str,
    ) -> GithubResult<RepoLabel> {
        let ep = format!("repos/{}/{}/labels", repo.owner, repo.repo);
        let clean_color = color_hex.trim_start_matches('#');
        let args = vec![
            String::from("api"),
            String::from("--method"),
            String::from("POST"),
            ep,
            String::from("-f"),
            format!("name={name}"),
            String::from("-f"),
            format!("color={clean_color}"),
            String::from("-f"),
            format!("description={description}"),
        ];
        let out = self.runner.run_gh(&args, self.timeout).await?;
        Self::check_exit(&out)?;
        Ok(to_repo_label(name, Some(clean_color), Some(description)))
    }

    /// Cria uma nova issue via `gh api repos/{owner}/{repo}/issues`.
    pub async fn create_issue(
        &self,
        repo: &GithubRepoIdentity,
        title: &str,
        body: &str,
        labels: &[String],
        assignees: &[String],
    ) -> GithubResult<RemoteIssueRecord> {
        let ep = format!("repos/{}/{}/issues", repo.owner, repo.repo);
        let mut args = vec![
            String::from("api"),
            String::from("--method"),
            String::from("POST"),
            ep,
            String::from("-f"),
            format!("title={title}"),
            String::from("-f"),
            format!("body={body}"),
        ];
        for label in labels {
            args.push(String::from("-f"));
            args.push(format!("labels[]={label}"));
        }
        for assignee in assignees {
            args.push(String::from("-f"));
            args.push(format!("assignees[]={assignee}"));
        }
        let out = self.runner.run_gh(&args, self.timeout).await?;
        Self::check_exit(&out)?;

        let val: Value = serde_json::from_str(&out.stdout).map_err(|e| {
            GithubError::new(
                GithubErrorKind::Internal,
                "JSON inválido ao criar issue via 'gh'.",
            )
            .with_details(e.to_string())
        })?;
        parse_rest_or_gh_issue_json(&val).ok_or_else(|| {
            GithubError::new(
                GithubErrorKind::Internal,
                "Resposta de criação de issue não pôde ser interpretada.",
            )
        })
    }

    /// Substitui atomicamente as labels de uma issue via `gh api PUT`.
    pub async fn replace_issue_labels(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
        labels: &[String],
    ) -> GithubResult<Vec<RepoLabel>> {
        let ep = format!(
            "repos/{}/{}/issues/{issue_number}/labels",
            repo.owner, repo.repo
        );
        let mut args = vec![
            String::from("api"),
            String::from("--method"),
            String::from("PUT"),
            ep,
        ];
        for label in labels {
            args.push(String::from("-f"));
            args.push(format!("labels[]={label}"));
        }
        let out = self.runner.run_gh(&args, self.timeout).await?;
        Self::check_exit(&out)?;

        let raw_labels: Vec<GhLabelWire> = serde_json::from_str(&out.stdout).unwrap_or_else(|_| {
            labels
                .iter()
                .map(|n| GhLabelWire {
                    name: n.clone(),
                    color: None,
                    description: None,
                })
                .collect()
        });
        Ok(raw_labels
            .into_iter()
            .map(|l| to_repo_label(&l.name, l.color.as_deref(), l.description.as_deref()))
            .collect())
    }

    fn check_exit(out: &GhCommandOutput) -> GithubResult<()> {
        if out.exit_code == 0 {
            return Ok(());
        }
        let stderr_lower = out.stderr.to_ascii_lowercase();
        let kind = if stderr_lower.contains("http 401")
            || stderr_lower.contains("not logged into")
            || stderr_lower.contains("authentication")
        {
            GithubErrorKind::AuthInvalid
        } else if stderr_lower.contains("rate limit") || stderr_lower.contains("http 429") {
            GithubErrorKind::RateLimited
        } else if stderr_lower.contains("http 404") || stderr_lower.contains("not found") {
            GithubErrorKind::NotFound
        } else if stderr_lower.contains("http 403") {
            GithubErrorKind::PermissionDenied
        } else {
            GithubErrorKind::NetworkUnavailable
        };
        Err(
            GithubError::new(kind, "Falha ao executar operação no GitHub CLI ('gh').")
                .with_details(out.stderr.trim()),
        )
    }
}

#[derive(Debug, Deserialize)]
struct GhLabelWire {
    name: String,
    color: Option<String>,
    description: Option<String>,
}

fn extract_login_from_auth_status(text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(idx) = trimmed.find("account ") {
            let after = &trimmed[(idx + "account ".len())..];
            let token = after.split_whitespace().next().unwrap_or("");
            let clean =
                token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_');
            if !clean.is_empty() {
                return Some(clean.to_string());
            }
        }
    }
    None
}

fn extract_scopes_from_auth_status(text: &str) -> Vec<String> {
    for line in text.lines() {
        if let Some(idx) = line.find("Token scopes:") {
            let after = &line[(idx + "Token scopes:".len())..];
            return after
                .split(',')
                .map(|s| s.trim().trim_matches('\'').trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

/// Converte um objeto JSON de issue (formato REST v3 / `gh api`) em `RemoteIssueRecord`.
#[must_use]
pub fn parse_rest_or_gh_issue_json(v: &Value) -> Option<RemoteIssueRecord> {
    let number = v.get("number")?.as_u64()?;
    let title = v.get("title")?.as_str()?.to_string();
    let body = v
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let state = v
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or("open")
        .to_ascii_lowercase();
    let state_reason = v
        .get("state_reason")
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let node_id = v
        .get("node_id")
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let user = v.get("user").or_else(|| v.get("author"));
    let author_login = user
        .and_then(|u| u.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("ghost")
        .to_string();
    let author_avatar_url = user
        .and_then(|u| u.get("avatar_url"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let assignees: Vec<String> = v
        .get("assignees")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    a.get("login")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })
                .collect()
        })
        .unwrap_or_default();

    let milestone_title = v
        .get("milestone")
        .and_then(|m| m.get("title"))
        .and_then(Value::as_str)
        .map(ToString::to_string);

    let labels: Vec<RepoLabel> = v
        .get("labels")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    if let Some(s) = item.as_str() {
                        Some(to_repo_label(s, None, None))
                    } else {
                        let name = item.get("name")?.as_str()?;
                        let color = item.get("color").and_then(Value::as_str);
                        let desc = item.get("description").and_then(Value::as_str);
                        Some(to_repo_label(name, color, desc))
                    }
                })
                .collect()
        })
        .unwrap_or_default();

    let comments_count = v
        .get("comments")
        .and_then(Value::as_u64)
        .and_then(|n| u32::try_from(n).ok())
        .unwrap_or(0);

    let html_url = v
        .get("html_url")
        .or_else(|| v.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    let created_at_ms = v
        .get("created_at")
        .or_else(|| v.get("createdAt"))
        .and_then(Value::as_str)
        .map_or(0, parse_github_timestamp_ms);
    let updated_at_ms = v
        .get("updated_at")
        .or_else(|| v.get("updatedAt"))
        .and_then(Value::as_str)
        .map_or(created_at_ms, parse_github_timestamp_ms);
    let closed_at_ms = v
        .get("closed_at")
        .or_else(|| v.get("closedAt"))
        .and_then(Value::as_str)
        .map(parse_github_timestamp_ms)
        .filter(|&ms| ms > 0);

    Some(RemoteIssueRecord {
        number,
        node_id,
        title,
        body,
        state,
        state_reason,
        author_login,
        author_avatar_url,
        assignees,
        milestone_title,
        labels,
        comments_count,
        html_url,
        created_at_ms,
        updated_at_ms,
        closed_at_ms,
    })
}

/// Converte um objeto JSON de comentário de issue em `IssueComment` sanitizado.
#[must_use]
pub fn parse_issue_comment_json(v: &Value, issue_number: u64) -> Option<IssueComment> {
    let comment_id = v.get("id")?.as_i64()?;
    let user = v.get("user").or_else(|| v.get("author"));
    let author_login = user
        .and_then(|u| u.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("ghost")
        .to_string();
    let author_avatar_url = user
        .and_then(|u| u.get("avatar_url"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let raw_body = v.get("body").and_then(Value::as_str).unwrap_or_default();
    let body_markdown = mt_db::sanitizer::sanitize(raw_body).into_owned();
    let html_url = v
        .get("html_url")
        .or_else(|| v.get("url"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let created_at_ms = v
        .get("created_at")
        .or_else(|| v.get("createdAt"))
        .and_then(Value::as_str)
        .map_or(0, parse_github_timestamp_ms);
    let updated_at_ms = v
        .get("updated_at")
        .or_else(|| v.get("updatedAt"))
        .and_then(Value::as_str)
        .map_or(created_at_ms, parse_github_timestamp_ms);

    Some(IssueComment {
        comment_id,
        issue_number,
        author_login,
        author_avatar_url,
        body_markdown,
        html_url,
        created_at_ms,
        updated_at_ms,
        contains_active_plan: false,
    })
}
