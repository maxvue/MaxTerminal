//! Execução supervisionada de `php artisan list --format=json` sem shell, com grupo de processos isolado,
//! drenagem concorrente de `stdout`/`stderr`, timeout com `SIGKILL` no grupo e sanitização obrigatória.

use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use mt_core::MtResult;
use mt_db::sanitizer::sanitize;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

use crate::artisan::model::RunnerKind;
use crate::error;
use crate::ports::{ArtisanRunner, RunLimits};

const ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "LANG",
    "LC_ALL",
    "COMPOSER_HOME",
    "XDG_CONFIG_HOME",
    "DOCKER_HOST",
    "DOCKER_CONTEXT",
    "WWWUSER",
    "WWWGROUP",
];

const DIAGNOSTIC_STDERR_MAX_BYTES: usize = 4096;

/// Calcula o caminho relativo de `target` a partir de `base`.
#[must_use]
pub fn relative_path(base: &Path, target: &Path) -> PathBuf {
    let base_comps: Vec<Component<'_>> = base.components().collect();
    let target_comps: Vec<Component<'_>> = target.components().collect();

    let common = base_comps
        .iter()
        .zip(target_comps.iter())
        .take_while(|(a, b)| a == b)
        .count();

    if common == 0 {
        return target.to_path_buf();
    }

    let mut out = PathBuf::new();
    for _ in common..base_comps.len() {
        out.push("..");
    }
    for comp in &target_comps[common..] {
        out.push(comp.as_os_str());
    }
    if out.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        out
    }
}

/// Faz quoting seguro de um token POSIX caso contenha espaços ou metacaracteres.
#[must_use]
pub fn quote_shell_token(token: &str) -> String {
    if !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/' | b':'))
    {
        return token.to_string();
    }
    let escaped = token.replace('\'', "'\\''");
    format!("'{escaped}'")
}

fn rel_artisan_token(root: &Path, cwd: &Path) -> String {
    let rel = relative_path(cwd, &root.join("artisan"));
    rel.to_string_lossy().into_owned()
}

/// Runner que executa o binário `php` local diretamente (sem shell).
#[derive(Debug, Clone)]
pub struct LocalPhpRunner {
    php_bin: PathBuf,
}

impl LocalPhpRunner {
    /// Cria um [`LocalPhpRunner`] usando `ARTISAN_PHP_BIN` (para testes) ou `"php"` no `PATH`.
    #[must_use]
    pub fn new() -> Self {
        let php_bin =
            std::env::var_os("ARTISAN_PHP_BIN").map_or_else(|| PathBuf::from("php"), PathBuf::from);
        Self { php_bin }
    }

    /// Cria um [`LocalPhpRunner`] com caminho explícito para o executável PHP.
    #[must_use]
    pub fn with_bin(php_bin: impl Into<PathBuf>) -> Self {
        Self {
            php_bin: php_bin.into(),
        }
    }
}

impl Default for LocalPhpRunner {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl ArtisanRunner for LocalPhpRunner {
    fn kind(&self) -> RunnerKind {
        RunnerKind::Local
    }

    fn invoker(&self, root: &Path, cwd: &Path) -> String {
        let tok = rel_artisan_token(root, cwd);
        format!("php {}", quote_shell_token(&tok))
    }

    fn invoker_tokens(&self, root: &Path, cwd: &Path) -> Vec<String> {
        vec!["php".to_string(), rel_artisan_token(root, cwd)]
    }

    async fn list_json(&self, root: &Path, limits: RunLimits) -> MtResult<Vec<u8>> {
        let mut cmd = Command::new(&self.php_bin);
        cmd.args([
            "artisan",
            "list",
            "--format=json",
            "--no-interaction",
            "--no-ansi",
        ])
        .current_dir(root)
        .env_clear()
        .envs(
            ENV_ALLOWLIST
                .iter()
                .filter_map(|k| std::env::var_os(k).map(|v| (*k, v))),
        )
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);

        run_bounded(cmd, limits).await
    }
}

/// Runner para projetos com Laravel Sail (desabilitado por padrão quando não há supervisão de contêiner verificada).
#[derive(Debug, Clone, Default)]
pub struct SailRunner;

#[async_trait::async_trait]
impl ArtisanRunner for SailRunner {
    fn kind(&self) -> RunnerKind {
        RunnerKind::Sail
    }

    fn invoker(&self, root: &Path, cwd: &Path) -> String {
        let rel = relative_path(cwd, &root.join("vendor/bin/sail"));
        let bin = if rel == Path::new("vendor/bin/sail") {
            "./vendor/bin/sail".to_string()
        } else {
            rel.to_string_lossy().into_owned()
        };
        format!("{} artisan", quote_shell_token(&bin))
    }

    fn invoker_tokens(&self, root: &Path, cwd: &Path) -> Vec<String> {
        let rel = relative_path(cwd, &root.join("vendor/bin/sail"));
        let bin = if rel == Path::new("vendor/bin/sail") {
            "./vendor/bin/sail".to_string()
        } else {
            rel.to_string_lossy().into_owned()
        };
        vec![bin, "artisan".to_string()]
    }

    async fn list_json(&self, _root: &Path, _limits: RunLimits) -> MtResult<Vec<u8>> {
        Err(error::runner_unsupported(
            "Descoberta via Laravel Sail desabilitada sem identificador supervisionado de contêiner; instale o PHP localmente ou use o cache existente.",
        ))
    }
}

/// Seletor de [`ArtisanRunner`] por raiz.
#[derive(Clone)]
pub struct RunnerSelector {
    runner: Arc<dyn ArtisanRunner>,
}

impl RunnerSelector {
    /// Cria um seletor com o runner informado.
    #[must_use]
    pub fn new(runner: Arc<dyn ArtisanRunner>) -> Self {
        Self { runner }
    }

    /// Retorna o runner configurado para `root`.
    #[must_use]
    pub fn pick(&self, _root: &Path) -> Arc<dyn ArtisanRunner> {
        Arc::clone(&self.runner)
    }
}

impl Default for RunnerSelector {
    fn default() -> Self {
        Self::new(Arc::new(LocalPhpRunner::new()))
    }
}

fn kill_process_group(pgid: u32) {
    if let Ok(raw) = i32::try_from(pgid) {
        if raw > 0 {
            let _ = kill(Pid::from_raw(-raw), Signal::SIGKILL);
        }
    }
}

/// Executa `cmd` drenando `stdout` e `stderr` concorrentemente com limites de tamanho e timeout.
pub async fn run_bounded(mut cmd: Command, limits: RunLimits) -> MtResult<Vec<u8>> {
    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            error::php_not_found()
        } else {
            error::exit_nonzero(None, format!("Falha ao iniciar processo PHP: {e}"))
        }
    })?;

    let pgid = child.id().unwrap_or(0);
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| error::exit_nonzero(None, "Pipe stdout indisponível.".to_string()))?;
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| error::exit_nonzero(None, "Pipe stderr indisponível.".to_string()))?;

    let max_stdout = limits.max_stdout;
    let max_stderr = limits.max_stderr;

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 16 * 1024];
        loop {
            let n = stdout.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            if buf.len().saturating_add(n) > max_stdout {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::FileTooLarge,
                    "stdout_overflow",
                ));
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        Ok::<Vec<u8>, std::io::Error>(buf)
    });

    let stderr_task = tokio::spawn(async move {
        let mut ring = Vec::<u8>::new();
        let mut overflowed = false;
        let mut chunk = [0u8; 8 * 1024];
        loop {
            let n = stderr.read(&mut chunk).await?;
            if n == 0 {
                break;
            }
            if ring.len().saturating_add(n) > max_stderr {
                overflowed = true;
                let total = ring.len() + n;
                let drop_count = total.saturating_sub(max_stderr);
                if drop_count >= ring.len() {
                    ring.clear();
                    let keep = max_stderr.min(n);
                    ring.extend_from_slice(&chunk[n - keep..n]);
                } else {
                    ring.drain(..drop_count);
                    ring.extend_from_slice(&chunk[..n]);
                }
            } else {
                ring.extend_from_slice(&chunk[..n]);
            }
        }
        Ok::<(Vec<u8>, bool), std::io::Error>((ring, overflowed))
    });

    let wait_res = tokio::time::timeout(limits.timeout, async {
        let stdout_res = stdout_task.await;
        if let Ok(Err(ref e)) = stdout_res {
            if e.kind() == std::io::ErrorKind::FileTooLarge {
                kill_process_group(pgid);
            }
        }
        let status = child.wait().await;
        let stderr_res = stderr_task.await;
        (status, stdout_res, stderr_res)
    })
    .await;

    let Ok((status_res, stdout_res, stderr_res)) = wait_res else {
        kill_process_group(pgid);
        let _ = child.kill().await;
        let _ = child.wait().await;
        return Err(error::timeout_error(limits.timeout.as_secs()));
    };

    // Garante que nenhum processo filho remanescente no grupo sobreviva após o término do líder.
    kill_process_group(pgid);

    let stdout_bytes = match stdout_res {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::FileTooLarge => {
            return Err(error::output_too_large(limits.max_stdout));
        }
        Ok(Err(e)) => {
            return Err(error::exit_nonzero(
                None,
                format!("Falha ao ler stdout: {e}"),
            ));
        }
        Err(e) => {
            return Err(error::exit_nonzero(
                None,
                format!("Tarefa stdout interrompida: {e}"),
            ));
        }
    };

    let (stderr_bytes, stderr_overflowed) = match stderr_res {
        Ok(Ok(pair)) => pair,
        _ => (Vec::new(), false),
    };

    let status = status_res.map_err(|e| error::exit_nonzero(None, e.to_string()))?;
    if !status.success() {
        let tail = format_sanitized_stderr_tail(&stderr_bytes, stderr_overflowed);
        return Err(error::exit_nonzero(status.code(), tail));
    }

    Ok(stdout_bytes)
}

fn format_sanitized_stderr_tail(raw: &[u8], overflowed: bool) -> String {
    let text = String::from_utf8_lossy(raw);
    let trimmed_first_partial = if overflowed {
        text.find('\n')
            .map_or(text.as_ref(), |idx| &text[idx + 1..])
    } else {
        text.as_ref()
    };
    let sanitized = sanitize(trimmed_first_partial);
    if sanitized.len() <= DIAGNOSTIC_STDERR_MAX_BYTES {
        return sanitized.trim().to_string();
    }
    let start = sanitized.len() - DIAGNOSTIC_STDERR_MAX_BYTES;
    let slice = &sanitized[start..];
    let clean = slice.find('\n').map_or(slice, |idx| &slice[idx + 1..]);
    clean.trim().to_string()
}
