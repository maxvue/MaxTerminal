//! Preparação e validação completa de `SpawnSpec` (shell, `cwd`, `env` e dimensões).

pub mod env;
pub mod shell;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use mt_core::ids::PtyId;
use rustix::fs::{Access, access};

use crate::error::{PtyError, PtyResult};
use crate::types::{PtyOwner, PtySize, PtySummary, SpawnSpec};

pub use env::{
    APPIMAGE_COLON_PATH_VARS, APPIMAGE_DIRECT_VARS, AppImageEnvSnapshot, DefaultEnvPolicy,
    EnvPolicy, build_environment, validate_env_key, validate_env_value,
};
pub use shell::{ResolvedShell, is_executable_file, resolve_shell, resolve_shell_with_env};

/// Hook de ciclo de vida invocado durante a preparação e partida de um PTY (Fase 05).
///
/// Permite que subsistemas externos (ex.: `mt-shell`) mutem [`PreparedSpawn`] antes da criação
/// do processo filho e registrem contexto de runtime após o PID nascer, porém **antes** da
/// liberação do launcher para o `execve` do shell e do primeiro `read()` do master PTY.
pub trait SpawnLifecycle: Send + Sync {
    /// Invocado antes de disparar o processo filho. Permite injetar flags e variáveis em `spawn`.
    ///
    /// # Errors
    /// Retornar `Err` aborta o spawn antes de criar o cgroup/processo e aciona [`Self::failed`].
    fn prepare(&self, spawn: &mut PreparedSpawn) -> PtyResult<()> {
        let _ = spawn;
        Ok(())
    }

    /// Invocado após o processo filho nascer (`pid` e `start_time` conhecidos), mas **antes**
    /// de liberar o helper para o `execve` do shell real.
    ///
    /// # Errors
    /// Retornar `Err` encerra imediatamente o processo filho recém-criado e aciona [`Self::failed`].
    fn started(&self, summary: &PtySummary) -> PtyResult<()> {
        let _ = summary;
        Ok(())
    }

    /// Invocado caso ocorra falha em qualquer etapa do spawn, permitindo limpeza de recursos.
    fn failed(&self, pty_id: PtyId) {
        let _ = pty_id;
    }
}

/// Estrutura validada e pronta para disparo do processo no PTY.
#[derive(Debug, Clone)]
pub struct PreparedSpawn {
    /// Identificador único atribuído ao PTY.
    pub pty_id: PtyId,
    /// Caminho resolvido do executável.
    pub executable: PathBuf,
    /// Argumentos efetivos do comando.
    pub args: Vec<String>,
    /// Diretório de trabalho inicial canonicalizado.
    pub cwd: PathBuf,
    /// Mapa completo de variáveis de ambiente validadas.
    pub env: BTreeMap<String, String>,
    /// Dimensões iniciais validadas.
    pub size: PtySize,
    /// Vínculo de aba/painel.
    pub owner: PtyOwner,
}

fn resolve_home_dir() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("HOME")
        && !home.is_empty()
    {
        return Some(PathBuf::from(home));
    }
    nix::unistd::User::from_uid(nix::unistd::getuid())
        .ok()
        .flatten()
        .map(|u| u.dir)
}

/// Resolve, expande `~`, canonicaliza e valida permissões de leitura/travessia do `cwd`.
pub fn resolve_cwd(requested: Option<&str>) -> PtyResult<PathBuf> {
    let raw_path = match requested {
        None => resolve_home_dir()
            .ok_or_else(|| PtyError::CwdInvalid("Diretório HOME não encontrado".to_string()))?,
        Some(s) => {
            if s.is_empty() || s.contains('\0') {
                return Err(PtyError::CwdInvalid(s.to_string()));
            }
            if s == "~" {
                resolve_home_dir().ok_or_else(|| {
                    PtyError::CwdInvalid("Diretório HOME não encontrado".to_string())
                })?
            } else if let Some(rest) = s.strip_prefix("~/") {
                let home = resolve_home_dir().ok_or_else(|| {
                    PtyError::CwdInvalid("Diretório HOME não encontrado".to_string())
                })?;
                home.join(rest)
            } else {
                PathBuf::from(s)
            }
        }
    };

    validate_canonical_cwd(&raw_path, requested.unwrap_or("~"))
}

fn validate_canonical_cwd(path: &Path, display_input: &str) -> PtyResult<PathBuf> {
    let canon =
        fs::canonicalize(path).map_err(|_| PtyError::CwdInvalid(display_input.to_string()))?;
    let meta = fs::metadata(&canon).map_err(|_| PtyError::CwdInvalid(display_input.to_string()))?;
    if !meta.is_dir() {
        return Err(PtyError::CwdInvalid(display_input.to_string()));
    }
    access(&canon, Access::READ_OK | Access::EXEC_OK)
        .map_err(|_| PtyError::CwdInvalid(display_input.to_string()))?;
    Ok(canon)
}

/// Executa todas as validações de `SpawnSpec` e constrói um `PreparedSpawn`.
pub fn prepare_spawn(
    pty_id: PtyId,
    spec: &SpawnSpec,
    policy: &dyn EnvPolicy,
) -> PtyResult<PreparedSpawn> {
    spec.size.validate()?;
    let cwd = resolve_cwd(spec.cwd.as_deref())?;
    let resolved_shell = resolve_shell(spec.shell.as_deref(), spec.login, &spec.args)?;
    let env = build_environment(pty_id, spec, policy)?;

    Ok(PreparedSpawn {
        pty_id,
        executable: resolved_shell.executable,
        args: resolved_shell.args,
        cwd,
        env,
        size: spec.size,
        owner: spec.owner.clone(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn resolves_valid_cwd_and_rejects_nonexistent_or_file() {
        let tmp = tempfile::tempdir().unwrap();
        let resolved = resolve_cwd(Some(tmp.path().to_str().unwrap())).unwrap();
        assert_eq!(resolved, fs::canonicalize(tmp.path()).unwrap());

        let err_missing = resolve_cwd(Some("/caminho/que/nao/existe/12345")).unwrap_err();
        assert_eq!(err_missing.code(), "pty.cwd_invalid");

        let file_path = tmp.path().join("arquivo.txt");
        fs::write(&file_path, b"oi").unwrap();
        let err_file = resolve_cwd(Some(file_path.to_str().unwrap())).unwrap_err();
        assert_eq!(err_file.code(), "pty.cwd_invalid");
    }

    #[test]
    fn expands_tilde_in_cwd() {
        if resolve_home_dir().is_some() {
            let home_resolved = resolve_cwd(Some("~")).unwrap();
            let default_resolved = resolve_cwd(None).unwrap();
            assert_eq!(home_resolved, default_resolved);
        }
    }
}
