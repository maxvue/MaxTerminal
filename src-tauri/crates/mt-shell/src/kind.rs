//! Detecção de família de shell (`ShellKind`) e verificação de elegibilidade para injeção de scripts OSC 133.

use std::path::Path;

use mt_core::shell::ShellKind;

/// Detecta a família do shell a partir do caminho ou nome do executável.
#[must_use]
pub fn detect_shell_kind(program: &Path) -> ShellKind {
    let raw_name = program
        .file_name()
        .and_then(|os| os.to_str())
        .unwrap_or_default();

    // Remove eventual prefixo '-' de login shell em argv[0] (ex.: "-zsh", "-bash")
    let base = raw_name.trim_start_matches('-').trim();
    if base.is_empty() {
        return ShellKind::Custom(String::new());
    }

    let lower = base.to_ascii_lowercase();
    if lower == "bash" || lower.starts_with("bash-") {
        ShellKind::Bash
    } else if lower == "zsh" || lower.starts_with("zsh-") {
        ShellKind::Zsh
    } else if lower == "fish" || lower.starts_with("fish-") {
        ShellKind::Fish
    } else if matches!(lower.as_str(), "sh" | "dash" | "ash" | "ksh" | "mksh") {
        ShellKind::Sh
    } else {
        ShellKind::Custom(base.to_string())
    }
}

/// Verifica se os argumentos fornecidos pelo usuário permitem injeção automática de scripts
/// sem colidir com flags de comando único (`-c`) ou de customização explícita de startup.
#[must_use]
pub fn can_inject_shell_integration(kind: &ShellKind, args: &[String]) -> bool {
    if !matches!(kind, ShellKind::Bash | ShellKind::Zsh | ShellKind::Fish) {
        return false;
    }

    for arg in args {
        let trimmed = arg.trim();
        if trimmed == "--" {
            // Argumentos posicionais após `--` indicam execução de script não-interativo
            return false;
        }
        // Qualquer flag `-c` ou combinada como `-lc` / `-ic` executa comando inline e encerra
        if trimmed == "-c"
            || (trimmed.starts_with('-')
                && !trimmed.starts_with("--")
                && trimmed[1..].contains('c'))
        {
            return false;
        }

        match kind {
            ShellKind::Bash => {
                if matches!(
                    trimmed,
                    "--rcfile" | "--init-file" | "--norc" | "--noprofile" | "--posix"
                ) {
                    return false;
                }
            }
            ShellKind::Zsh => {
                if matches!(trimmed, "--no-rcs" | "--no-globalrcs" | "-f") {
                    return false;
                }
            }
            ShellKind::Fish => {
                if matches!(
                    trimmed,
                    "--no-config" | "-N" | "--init-command" | "-C" | "--command"
                ) {
                    return false;
                }
            }
            ShellKind::Sh | ShellKind::Custom(_) => return false,
        }
    }

    // Se houver um argumento posicional que não começa com '-', trata-se de script (ex.: `bash script.sh`)
    if args.iter().any(|a| !a.is_empty() && !a.starts_with('-')) {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_shells_and_custom_binaries() {
        assert_eq!(
            detect_shell_kind(Path::new("/usr/bin/bash")),
            ShellKind::Bash
        );
        assert_eq!(detect_shell_kind(Path::new("/bin/-zsh")), ShellKind::Zsh);
        assert_eq!(
            detect_shell_kind(Path::new("/usr/local/bin/fish")),
            ShellKind::Fish
        );
        assert_eq!(detect_shell_kind(Path::new("/bin/dash")), ShellKind::Sh);
        assert_eq!(
            detect_shell_kind(Path::new("/usr/bin/claude")),
            ShellKind::Custom("claude".to_string())
        );
    }

    #[test]
    fn rejects_non_interactive_or_conflicting_flags() {
        assert!(can_inject_shell_integration(&ShellKind::Bash, &[]));
        assert!(can_inject_shell_integration(
            &ShellKind::Bash,
            &["-i".to_string()]
        ));
        assert!(!can_inject_shell_integration(
            &ShellKind::Bash,
            &["-c".to_string(), "echo hi".to_string()]
        ));
        assert!(!can_inject_shell_integration(
            &ShellKind::Bash,
            &["--norc".to_string()]
        ));
        assert!(!can_inject_shell_integration(
            &ShellKind::Zsh,
            &["-f".to_string()]
        ));
        assert!(!can_inject_shell_integration(
            &ShellKind::Fish,
            &["--no-config".to_string()]
        ));
        assert!(!can_inject_shell_integration(
            &ShellKind::Bash,
            &["meu_script.sh".to_string()]
        ));
    }
}
