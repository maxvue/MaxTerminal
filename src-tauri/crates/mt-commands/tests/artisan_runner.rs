//! Testes do `LocalPhpRunner` e `run_bounded`: timeout + kill do grupo de processos,
//! `php_not_found`, sanitização de segredos em `stderr` e `output_too_large`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::Duration;

use mt_commands::artisan::runner::LocalPhpRunner;
use mt_commands::error::codes;
use mt_commands::ports::{ArtisanRunner, RunLimits};
use nix::sys::signal::kill;
use nix::unistd::Pid;
use tempfile::TempDir;

fn write_executable_script(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).expect("write script");
    let mut perms = fs::metadata(&path).expect("meta").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&path, perms).expect("chmod");
    path
}

#[tokio::test]
async fn runner_reports_php_not_found_when_binary_missing() {
    let tmp = TempDir::new().expect("tempdir");
    let runner = LocalPhpRunner::with_bin(tmp.path().join("non-existent-php-bin"));
    let err = runner
        .list_json(tmp.path(), RunLimits::default())
        .await
        .expect_err("must fail");
    assert_eq!(err.code, codes::PHP_NOT_FOUND);
}

#[tokio::test]
async fn runner_masks_secrets_in_stderr_on_nonzero_exit() {
    let tmp = TempDir::new().expect("tempdir");
    let script = write_executable_script(
        tmp.path(),
        "fake-php-err.sh",
        "#!/usr/bin/env sh\necho 'Fatal error: APP_KEY=base64:SuperSecretKey1234567890==' >&2\nexit 1\n",
    );
    let runner = LocalPhpRunner::with_bin(script);
    let err = runner
        .list_json(tmp.path(), RunLimits::default())
        .await
        .expect_err("must fail");
    assert_eq!(err.code, codes::EXIT_NONZERO);
    let details_str = serde_json::to_string(&err.details).expect("json");
    assert!(
        !details_str.contains("SuperSecretKey1234567890"),
        "Segredo APP_KEY não pode vazar no stderrTail: {details_str}"
    );
}

#[tokio::test]
async fn runner_enforces_stdout_limit() {
    let tmp = TempDir::new().expect("tempdir");
    let script = write_executable_script(
        tmp.path(),
        "fake-php-big.sh",
        "#!/usr/bin/env sh\nhead -c 4096 /dev/zero | tr '\\0' 'a'\n",
    );
    let runner = LocalPhpRunner::with_bin(script);
    let limits = RunLimits {
        timeout: Duration::from_secs(5),
        max_stdout: 1024,
        max_stderr: 1024,
    };
    let err = runner
        .list_json(tmp.path(), limits)
        .await
        .expect_err("must fail with output_too_large");
    assert_eq!(err.code, codes::OUTPUT_TOO_LARGE);
}

#[tokio::test]
async fn runner_kills_entire_process_group_on_timeout() {
    let tmp = TempDir::new().expect("tempdir");
    let pid_file = tmp.path().join("child.pid");
    let script_body = format!(
        "#!/usr/bin/env sh\nsleep 30 &\necho $! > '{}'\nwait\n",
        pid_file.display()
    );
    let script = write_executable_script(tmp.path(), "fake-php-sleep.sh", &script_body);
    let runner = LocalPhpRunner::with_bin(script);
    let limits = RunLimits {
        timeout: Duration::from_millis(250),
        max_stdout: 1024 * 1024,
        max_stderr: 1024 * 1024,
    };

    let err = runner
        .list_json(tmp.path(), limits)
        .await
        .expect_err("must timeout");
    assert_eq!(err.code, codes::TIMEOUT);

    // Verifica que o processo filho criado pelo script não permaneceu vivo (CA-09)
    if let Ok(pid_str) = fs::read_to_string(&pid_file) {
        if let Ok(child_pid) = pid_str.trim().parse::<i32>() {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let alive = kill(Pid::from_raw(child_pid), None).is_ok();
            assert!(
                !alive,
                "Processo filho {child_pid} não deveria sobreviver após o timeout"
            );
        }
    }
}
