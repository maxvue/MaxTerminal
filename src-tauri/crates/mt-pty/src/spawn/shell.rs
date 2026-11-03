//! Resolução de executável de shell e argumentos de login.

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::error::{PtyError, PtyResult};

/// Resultado da resolução de shell e seus argumentos iniciais.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedShell {
    /// Caminho para o executável regular validado.
    pub executable: PathBuf,
    /// Lista de argumentos a passar ao processo (após `argv[0]`).
    pub args: Vec<String>,
}

/// Verifica se `path` aponta para um arquivo regular existente com permissão de execução.
#[must_use]
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    meta.is_file() && (meta.permissions().mode() & 0o111 != 0)
}

fn find_in_path(name: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let path_os = path_env?;
    for dir in std::env::split_paths(path_os) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn resolve_candidate(raw: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    if raw.is_empty() || raw.contains('\0') {
        return None;
    }
    if raw.contains('/') {
        let p = PathBuf::from(raw);
        is_executable_file(&p).then_some(p)
    } else {
        find_in_path(raw, path_env)
    }
}

fn compute_args(
    executable: &Path,
    login: bool,
    explicit_args: &[String],
) -> PtyResult<Vec<String>> {
    for arg in explicit_args {
        if arg.contains('\0') {
            return Err(PtyError::InputInvalid(
                "Argumento de comando contém byte nulo".to_string(),
            ));
        }
    }

    if !explicit_args.is_empty() {
        return Ok(explicit_args.to_vec());
    }

    if !login {
        return Ok(Vec::new());
    }

    let basename = executable
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default();

    let args = match basename {
        "bash" | "zsh" => vec!["-l".to_string()],
        "fish" => vec!["--login".to_string()],
        _ => Vec::new(),
    };

    Ok(args)
}

/// Resolve o shell usando variáveis de ambiente explícitas (ideal para testes determinísticos).
pub fn resolve_shell_with_env(
    requested: Option<&str>,
    login: bool,
    explicit_args: &[String],
    path_env: Option<&OsStr>,
    shell_env: Option<&OsStr>,
    passwd_shell: Option<&Path>,
) -> PtyResult<ResolvedShell> {
    if let Some(req) = requested {
        let executable = resolve_candidate(req, path_env)
            .ok_or_else(|| PtyError::ShellNotFound(req.to_string()))?;
        let args = compute_args(&executable, login, explicit_args)?;
        return Ok(ResolvedShell { executable, args });
    }

    if let Some(sh_os) = shell_env
        && let Some(sh_str) = sh_os.to_str()
        && let Some(executable) = resolve_candidate(sh_str, path_env)
    {
        let args = compute_args(&executable, login, explicit_args)?;
        return Ok(ResolvedShell { executable, args });
    }

    if let Some(pw_path) = passwd_shell
        && is_executable_file(pw_path)
    {
        let executable = pw_path.to_path_buf();
        let args = compute_args(&executable, login, explicit_args)?;
        return Ok(ResolvedShell { executable, args });
    }

    for fallback in ["/bin/bash", "/bin/sh"] {
        let p = Path::new(fallback);
        if is_executable_file(p) {
            let executable = p.to_path_buf();
            let args = compute_args(&executable, login, explicit_args)?;
            return Ok(ResolvedShell { executable, args });
        }
    }

    Err(PtyError::ShellNotFound("<default>".to_string()))
}

/// Resolve o shell de acordo com a hierarquia canônica:
/// `spec.shell` → `$SHELL` → `getpwuid` → `/bin/bash` → `/bin/sh`.
pub fn resolve_shell(
    requested: Option<&str>,
    login: bool,
    explicit_args: &[String],
) -> PtyResult<ResolvedShell> {
    let path_env = std::env::var_os("PATH");
    let shell_env = std::env::var_os("SHELL");
    let passwd_shell = nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|u| u.shell);

    resolve_shell_with_env(
        requested,
        login,
        explicit_args,
        path_env.as_deref(),
        shell_env.as_deref(),
        passwd_shell.as_deref(),
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn explicit_invalid_shell_fails_without_fallback() {
        let err = resolve_shell(Some("/caminho/inexistente/shell"), true, &[]).unwrap_err();
        assert_eq!(err.code(), "pty.shell_not_found");

        let err_empty = resolve_shell(Some(""), true, &[]).unwrap_err();
        assert_eq!(err_empty.code(), "pty.shell_not_found");

        let err_nul = resolve_shell(Some("/bin/sh\0bad"), true, &[]).unwrap_err();
        assert_eq!(err_nul.code(), "pty.shell_not_found");
    }

    #[test]
    fn directory_or_non_executable_fails_shell_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let err_dir = resolve_shell(Some(tmp.path().to_str().unwrap()), false, &[]).unwrap_err();
        assert_eq!(err_dir.code(), "pty.shell_not_found");

        let file_path = tmp.path().join("not_exec.sh");
        let mut f = fs::File::create(&file_path).unwrap();
        writeln!(f, "#!/bin/sh").unwrap();
        fs::set_permissions(&file_path, fs::Permissions::from_mode(0o644)).unwrap();

        let err_no_exec = resolve_shell(Some(file_path.to_str().unwrap()), false, &[]).unwrap_err();
        assert_eq!(err_no_exec.code(), "pty.shell_not_found");
    }

    #[test]
    fn login_flags_per_shell_and_explicit_args_override() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["bash", "zsh", "fish", "sh"] {
            let p = tmp.path().join(name);
            fs::write(&p, b"#!/bin/sh\n").unwrap();
            fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let bash = resolve_shell_with_env(
            Some("bash"),
            true,
            &[],
            Some(tmp.path().as_os_str()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(bash.args, vec!["-l"]);

        let zsh = resolve_shell_with_env(
            Some("zsh"),
            true,
            &[],
            Some(tmp.path().as_os_str()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(zsh.args, vec!["-l"]);

        let fish = resolve_shell_with_env(
            Some("fish"),
            true,
            &[],
            Some(tmp.path().as_os_str()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(fish.args, vec!["--login"]);

        let sh = resolve_shell_with_env(
            Some("sh"),
            true,
            &[],
            Some(tmp.path().as_os_str()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(sh.args, Vec::<String>::new());

        let explicit = vec!["-c".to_string(), "echo oi".to_string()];
        let bash_explicit = resolve_shell_with_env(
            Some("bash"),
            true,
            &explicit,
            Some(tmp.path().as_os_str()),
            None,
            None,
        )
        .unwrap();
        assert_eq!(bash_explicit.args, explicit);
    }

    #[test]
    fn fallback_chain_resolves_env_then_passwd_then_system() {
        let tmp = tempfile::tempdir().unwrap();
        let custom_sh = tmp.path().join("my_custom_shell");
        fs::write(&custom_sh, b"#!/bin/sh\n").unwrap();
        fs::set_permissions(&custom_sh, fs::Permissions::from_mode(0o755)).unwrap();

        let resolved_from_env = resolve_shell_with_env(
            None,
            false,
            &[],
            Some(tmp.path().as_os_str()),
            Some(custom_sh.as_os_str()),
            None,
        )
        .unwrap();
        assert_eq!(resolved_from_env.executable, custom_sh);

        let resolved_from_passwd = resolve_shell_with_env(
            None,
            false,
            &[],
            None,
            Some(OsStr::new("/nao/existe")),
            Some(&custom_sh),
        )
        .unwrap();
        assert_eq!(resolved_from_passwd.executable, custom_sh);
    }
}
