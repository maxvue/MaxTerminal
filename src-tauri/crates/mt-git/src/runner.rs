//! Executor assíncrono isolado de subprocessos `git` (`GitRunner` e `CliGitRunner`).

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mt_db::sanitizer::sanitize;
use tokio::process::Command;
use tokio::sync::Semaphore;

use crate::error::{GitError, GitResult};
use crate::porcelain::{ParsedStatus, parse_status_porcelain_v2, parse_worktree_list_porcelain};
use crate::types::WorktreeEntry;

/// Timeout padrão para consultas rápidas de status/listagem (`1500 ms`).
pub const DEFAULT_STATUS_TIMEOUT: Duration = Duration::from_millis(1500);

/// Timeout padrão para mutações de worktree (`10_000 ms`).
pub const DEFAULT_MUTATION_TIMEOUT: Duration = Duration::from_millis(10_000);

/// Limite global de subprocessos `git` concorrentes no aplicativo.
pub const MAX_CONCURRENT_GIT_PROCS: usize = 4;

/// Abstração de execução de comandos Git para permitir mocks determinísticos em testes.
#[async_trait]
pub trait GitRunner: Send + Sync + 'static {
    /// Coleta `git status --porcelain=v2 --branch -z` para a worktree em `worktree_root`.
    async fn status(&self, worktree_root: &Path) -> GitResult<ParsedStatus>;

    /// Lista todas as worktrees do repositório via `git worktree list --porcelain -z`.
    async fn worktree_list(&self, main_worktree_path: &Path) -> GitResult<Vec<WorktreeEntry>>;

    /// Verifica se uma referência Git existe (`git show-ref --verify --quiet <full_ref>`).
    async fn ref_exists(&self, repo_root: &Path, full_ref: &str) -> GitResult<bool>;

    /// Executa `git worktree add` para criar uma nova worktree gerenciada.
    async fn worktree_add(
        &self,
        main_worktree_path: &Path,
        target_path: &Path,
        branch_name: &str,
        base_ref: &str,
        create_branch: bool,
    ) -> GitResult<()>;

    /// Executa `git worktree remove [--force] <path>`.
    async fn worktree_remove(
        &self,
        main_worktree_path: &Path,
        worktree_path: &Path,
        force: bool,
    ) -> GitResult<()>;
}

/// Implementação real de [`GitRunner`] invocando o binário `git` do sistema com flags seguras.
#[derive(Debug, Clone)]
pub struct CliGitRunner {
    git_bin: String,
    semaphore: Arc<Semaphore>,
    status_timeout: Duration,
    mutation_timeout: Duration,
}

impl Default for CliGitRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl CliGitRunner {
    /// Cria um [`CliGitRunner`] padrão usando o binário `"git"` no `$PATH`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            git_bin: "git".to_string(),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_GIT_PROCS)),
            status_timeout: DEFAULT_STATUS_TIMEOUT,
            mutation_timeout: DEFAULT_MUTATION_TIMEOUT,
        }
    }

    /// Cria um [`CliGitRunner`] customizado (útil para testes de timeout ou binário ausente).
    #[must_use]
    pub fn with_config(
        git_bin: impl Into<String>,
        status_timeout: Duration,
        mutation_timeout: Duration,
    ) -> Self {
        Self {
            git_bin: git_bin.into(),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_GIT_PROCS)),
            status_timeout,
            mutation_timeout,
        }
    }

    async fn run_git(
        &self,
        cwd: &Path,
        subcommand_label: &str,
        args: &[&str],
        timeout: Duration,
    ) -> GitResult<std::process::Output> {
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| GitError::CommandFailed {
                subcommand: subcommand_label.to_string(),
                exit_code: None,
                stderr: "Semáforo de execução Git fechado".to_string(),
            })?;

        let mut cmd = Command::new(&self.git_bin);
        cmd.current_dir(cwd)
            .arg("--no-optional-locks")
            .args(["-c", "core.quotepath=off"])
            .args(["-c", "color.ui=never"])
            .args(["-c", "gc.auto=0"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let child_fut = cmd.output();
        match tokio::time::timeout(timeout, child_fut).await {
            Ok(Ok(output)) => Ok(output),
            Ok(Err(io_err)) => {
                if io_err.kind() == std::io::ErrorKind::NotFound {
                    Err(GitError::BinaryMissing)
                } else {
                    Err(GitError::CommandFailed {
                        subcommand: subcommand_label.to_string(),
                        exit_code: None,
                        stderr: sanitize(&io_err.to_string()).into_owned(),
                    })
                }
            }
            Err(_) => Err(GitError::CommandTimeout {
                subcommand: subcommand_label.to_string(),
                timeout_ms: u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX),
            }),
        }
    }
}

#[async_trait]
impl GitRunner for CliGitRunner {
    async fn status(&self, worktree_root: &Path) -> GitResult<ParsedStatus> {
        let output = self
            .run_git(
                worktree_root,
                "status",
                &[
                    "status",
                    "--porcelain=v2",
                    "--branch",
                    "-z",
                    "--untracked-files=normal",
                ],
                self.status_timeout,
            )
            .await?;

        if !output.status.success() {
            let raw_err = String::from_utf8_lossy(&output.stderr);
            let sanitized = sanitize(raw_err.trim()).into_owned();
            return Err(GitError::CommandFailed {
                subcommand: "status".to_string(),
                exit_code: output.status.code(),
                stderr: sanitized,
            });
        }

        parse_status_porcelain_v2(&output.stdout)
    }

    async fn worktree_list(&self, main_worktree_path: &Path) -> GitResult<Vec<WorktreeEntry>> {
        let output = self
            .run_git(
                main_worktree_path,
                "worktree list",
                &["worktree", "list", "--porcelain", "-z"],
                self.status_timeout,
            )
            .await?;

        if !output.status.success() {
            let raw_err = String::from_utf8_lossy(&output.stderr);
            let sanitized = sanitize(raw_err.trim()).into_owned();
            return Err(GitError::CommandFailed {
                subcommand: "worktree list".to_string(),
                exit_code: output.status.code(),
                stderr: sanitized,
            });
        }

        parse_worktree_list_porcelain(&output.stdout, main_worktree_path)
    }

    async fn ref_exists(&self, repo_root: &Path, full_ref: &str) -> GitResult<bool> {
        let output = self
            .run_git(
                repo_root,
                "show-ref",
                &["show-ref", "--verify", "--quiet", full_ref],
                self.status_timeout,
            )
            .await?;
        Ok(output.status.success())
    }

    async fn worktree_add(
        &self,
        main_worktree_path: &Path,
        target_path: &Path,
        branch_name: &str,
        base_ref: &str,
        create_branch: bool,
    ) -> GitResult<()> {
        let target_str = target_path.to_str().ok_or_else(|| GitError::InvalidPath {
            path: target_path.to_path_buf(),
            reason: "Caminho de worktree não é UTF-8 válido".to_string(),
        })?;

        let args: Vec<&str> = if create_branch {
            vec![
                "worktree",
                "add",
                "-b",
                branch_name,
                "--",
                target_str,
                base_ref,
            ]
        } else {
            vec!["worktree", "add", "--", target_str, branch_name]
        };

        let output = self
            .run_git(
                main_worktree_path,
                "worktree add",
                &args,
                self.mutation_timeout,
            )
            .await?;

        if !output.status.success() {
            let raw_err = String::from_utf8_lossy(&output.stderr);
            let sanitized = sanitize(raw_err.trim()).into_owned();
            return Err(GitError::CommandFailed {
                subcommand: "worktree add".to_string(),
                exit_code: output.status.code(),
                stderr: sanitized,
            });
        }

        Ok(())
    }

    async fn worktree_remove(
        &self,
        main_worktree_path: &Path,
        worktree_path: &Path,
        force: bool,
    ) -> GitResult<()> {
        let target_str = worktree_path
            .to_str()
            .ok_or_else(|| GitError::InvalidPath {
                path: worktree_path.to_path_buf(),
                reason: "Caminho de worktree não é UTF-8 válido".to_string(),
            })?;

        let args: Vec<&str> = if force {
            vec!["worktree", "remove", "--force", "--", target_str]
        } else {
            vec!["worktree", "remove", "--", target_str]
        };

        let output = self
            .run_git(
                main_worktree_path,
                "worktree remove",
                &args,
                self.mutation_timeout,
            )
            .await?;

        if !output.status.success() {
            let raw_err = String::from_utf8_lossy(&output.stderr);
            let sanitized = sanitize(raw_err.trim()).into_owned();
            return Err(GitError::CommandFailed {
                subcommand: "worktree remove".to_string(),
                exit_code: output.status.code(),
                stderr: sanitized,
            });
        }

        Ok(())
    }
}
