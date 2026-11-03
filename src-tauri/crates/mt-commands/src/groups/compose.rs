//! Compositor de submissão unitária de item para o modo Sequencial na mesma aba (`SequentialComposer`).

use std::collections::BTreeMap;
use std::path::Path;

use mt_core::MtResult;

use crate::groups::model::{GroupErrorCode, ShellDialect};
use crate::groups::validate::validate_env_key;

/// Compositor de submissão unitária de item para o modo Sequencial na mesma aba.
pub struct SequentialComposer;

impl SequentialComposer {
    /// Aplica quoting de um argumento literal para o dialeto de shell alvo:
    /// - `Bash` / `Zsh` (POSIX): envolve em `'...'` e substitui cada `'` interno por `'\''`.
    /// - `Fish`: envolve em `'...'` e substitui cada `\` por `\\` e cada `'` por `\'`.
    #[must_use]
    pub fn quote_arg(dialect: ShellDialect, raw: &str) -> String {
        match dialect {
            ShellDialect::Bash | ShellDialect::Zsh => {
                let mut out = String::with_capacity(raw.len() + 2);
                out.push('\'');
                for ch in raw.chars() {
                    if ch == '\'' {
                        out.push_str("'\\''");
                    } else {
                        out.push(ch);
                    }
                }
                out.push('\'');
                out
            }
            ShellDialect::Fish => {
                let mut out = String::with_capacity(raw.len() + 2);
                out.push('\'');
                for ch in raw.chars() {
                    match ch {
                        '\\' => out.push_str("\\\\"),
                        '\'' => out.push_str("\\'"),
                        _ => out.push(ch),
                    }
                }
                out.push('\'');
                out
            }
        }
    }

    /// Prepara os bytes de uma única submissão de item para escrita no PTY do painel:
    /// - Sem overrides (`resolved_cwd_override == None` e `env_overrides.is_empty()`): envia exatamente
    ///   `format!("{command}\n")` sem modificar a sintaxe original do usuário (preservando `cd`,
    ///   `export`/`set -gx`, aliases e funções no shell ativo).
    /// - Com overrides (`resolved_cwd_override.is_some()` ou `!env_overrides.is_empty()`):
    ///   - Exige `env_chdir_supported == true` quando `resolved_cwd_override.is_some()` (senão retorna
    ///     `Err(GroupErrorCode::EnvCwdUnavailable)`).
    ///   - Compõe `env [-C <cwd_quoted>] [<K=V_quoted>...] <shell_path_quoted> -c <command_quoted>\n`
    ///     onde cada argumento é protegido por `quote_arg(dialect, ...)`, isolando `cwd` e `env`
    ///     em um processo shell filho sem vazar para o shell pai do painel.
    pub fn compose_submission(
        dialect: ShellDialect,
        shell_path: &Path,
        command: &str,
        resolved_cwd_override: Option<&Path>,
        env_overrides: &BTreeMap<String, String>,
        env_chdir_supported: bool,
    ) -> MtResult<Vec<u8>> {
        let trimmed = command.trim();
        if trimmed.is_empty() {
            return Err(
                GroupErrorCode::Validation.into_error("O comando do item não pode ser vazio.")
            );
        }
        if command.contains('\n') || command.contains('\r') || command.contains('\0') {
            return Err(GroupErrorCode::Validation.into_error(
                "O comando do item não pode conter quebras de linha (CR/LF) ou caractere NUL.",
            ));
        }

        if resolved_cwd_override.is_none() && env_overrides.is_empty() {
            let mut line = String::with_capacity(command.len() + 1);
            line.push_str(command);
            line.push('\n');
            return Ok(line.into_bytes());
        }

        if resolved_cwd_override.is_some() && !env_chdir_supported {
            return Err(GroupErrorCode::EnvCwdUnavailable.into_error(
                "O binário 'env' do sistema não suporta a opção '-C' necessária para isolar o diretório de trabalho no modo Sequencial.",
            ));
        }

        let mut parts: Vec<String> = Vec::new();
        parts.push("env".to_string());

        if let Some(cwd_path) = resolved_cwd_override {
            let cwd_str = cwd_path.to_str().ok_or_else(|| {
                GroupErrorCode::Validation
                    .into_error("O caminho do diretório de trabalho contém bytes UTF-8 inválidos.")
            })?;
            parts.push("-C".to_string());
            parts.push(Self::quote_arg(dialect, cwd_str));
        }

        for (k, v) in env_overrides {
            if let Err(reason) = validate_env_key(k) {
                return Err(GroupErrorCode::Validation.into_error(reason));
            }
            if v.contains('\0') {
                return Err(GroupErrorCode::Validation.into_error(
                    "O valor da variável de ambiente não pode conter o caractere NUL.",
                ));
            }
            let kv = format!("{k}={v}");
            parts.push(Self::quote_arg(dialect, &kv));
        }

        let shell_str = shell_path.to_str().ok_or_else(|| {
            GroupErrorCode::UnsupportedShell
                .into_error("Caminho do executável do shell contém UTF-8 inválido.")
        })?;
        parts.push(Self::quote_arg(dialect, shell_str));
        parts.push("-c".to_string());
        parts.push(Self::quote_arg(dialect, command));

        let mut composed = parts.join(" ");
        composed.push('\n');
        Ok(composed.into_bytes())
    }
}
