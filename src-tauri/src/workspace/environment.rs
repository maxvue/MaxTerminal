//! Resolvedor de ambiente (`EnvironmentResolver`) para abertura de painéis no workspace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mt_core::error::{MtError, MtResult, codes};
use mt_core::workspace::{
    EnvValue, EnvironmentKind, LaunchSnapshot, TemplateConnection, TemplateEnvironment,
};

use crate::workspace::errors::template_agent_unsupported;

/// Contexto resolvido para o spawn de um PTY de painel.
#[derive(Debug, Clone)]
pub struct ResolvedLaunchEnvironment {
    /// Diretório de trabalho absoluto validado.
    pub cwd: PathBuf,
    /// Mapa final de variáveis de ambiente a injetar no PTY.
    pub env: BTreeMap<String, String>,
    /// Ambiente efetivo resolvido.
    pub environment: TemplateEnvironment,
}

/// Serviço que resolve e valida o ambiente de execução de templates e abas.
#[derive(Debug, Default, Clone)]
pub struct EnvironmentResolver;

impl EnvironmentResolver {
    /// Cria uma nova instância do resolvedor de ambientes.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Valida se o tipo de ambiente solicitado está disponível na fase atual.
    pub fn validate_environment_available(env: &TemplateEnvironment) -> MtResult<()> {
        match env.kind {
            EnvironmentKind::Shell => {
                if env.agent_id.is_some() {
                    return Err(MtError::new(
                        codes::TEMPLATE_INVALID,
                        "O campo agentId deve ser nulo quando o ambiente é Shell.",
                    ));
                }
                Ok(())
            }
            EnvironmentKind::Agent => match env.agent_id.as_deref() {
                Some("claude-code" | "claude" | "codex" | "agy" | "opencode") => Ok(()),
                other => Err(template_agent_unsupported(other)),
            },
        }
    }

    /// Resolve o diretório de trabalho final a partir da raiz do projeto e do `custom_cwd` opcional.
    pub fn resolve_cwd(
        project_root: &Path,
        custom_cwd: Option<&str>,
        allow_fallback_to_project: bool,
    ) -> MtResult<PathBuf> {
        let candidate = match custom_cwd.map(str::trim).filter(|s| !s.is_empty()) {
            None => project_root.to_path_buf(),
            Some(raw) => {
                let p = if let Some(stripped) = raw.strip_prefix("~/") {
                    if let Ok(home) = std::env::var("HOME") {
                        PathBuf::from(home).join(stripped)
                    } else {
                        PathBuf::from(raw)
                    }
                } else {
                    PathBuf::from(raw)
                };
                if p.is_absolute() {
                    p
                } else {
                    project_root.join(p)
                }
            }
        };

        if candidate.is_dir() {
            return Ok(candidate);
        }

        if allow_fallback_to_project && project_root.is_dir() {
            return Ok(project_root.to_path_buf());
        }

        Err(MtError::new(
            codes::TEMPLATE_CWD_NOT_FOUND,
            format!(
                "O diretório de trabalho configurado não existe ou não é uma pasta válida: '{}'.",
                candidate.display()
            ),
        ))
    }

    /// Resolve o `LaunchSnapshot` completo para abertura ou restauração de um painel.
    pub fn resolve_snapshot(
        project_root: &Path,
        snapshot: &LaunchSnapshot,
        override_cwd: Option<&str>,
        allow_fallback_to_project: bool,
    ) -> MtResult<ResolvedLaunchEnvironment> {
        Self::validate_environment_available(&snapshot.environment)?;

        let effective_cwd_str = override_cwd
            .or(snapshot.configured_cwd.as_deref())
            .or(Some(snapshot.resolved_cwd.as_str()));
        let cwd = match snapshot.connection {
            TemplateConnection::Local => {
                Self::resolve_cwd(project_root, effective_cwd_str, allow_fallback_to_project)?
            }
            TemplateConnection::Ssh { .. } => {
                let remote_cwd = override_cwd
                    .or(snapshot.configured_cwd.as_deref())
                    .map(str::trim)
                    .filter(|cwd| !cwd.is_empty())
                    .unwrap_or("~");
                crate::remote::ssh::remote_cwd_expression(Some(remote_cwd))?;
                PathBuf::from(remote_cwd)
            }
        };

        let mut env = BTreeMap::new();
        for (key, val) in &snapshot.env_vars {
            match val {
                EnvValue::Literal { value } => {
                    env.insert(key.clone(), value.clone());
                }
                EnvValue::Inherited { source } => {
                    if let Ok(host_val) = std::env::var(source) {
                        env.insert(key.clone(), host_val);
                    }
                }
            }
        }

        if snapshot.environment.kind == EnvironmentKind::Agent
            && let Some(ref agent_id) = snapshot.environment.agent_id
        {
            env.entry("MAXTERMINAL_TEMPLATE_AGENT".to_string())
                .or_insert_with(|| agent_id.clone());
        }

        Ok(ResolvedLaunchEnvironment {
            cwd,
            env,
            environment: snapshot.environment.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mt_core::workspace::SshAuth;

    fn remote_snapshot(cwd: Option<&str>) -> LaunchSnapshot {
        LaunchSnapshot {
            template_id: None,
            title: String::from("Servidor"),
            icon: String::from("lucide:server"),
            resolved_cwd: String::from("/pasta/local/do/projeto"),
            configured_cwd: cwd.map(String::from),
            initial_command: None,
            continuous: false,
            environment: TemplateEnvironment::default(),
            env_vars: BTreeMap::new(),
            connection: TemplateConnection::Ssh {
                host: String::from("192.168.3.31"),
                user: String::from("engeapp"),
                auth: SshAuth::Key {
                    identity_file: None,
                },
            },
        }
    }

    #[test]
    fn remote_cwd_is_not_expanded_or_validated_locally() {
        let snapshot = remote_snapshot(Some("~/htdocs/engeapp.com.br/current"));
        let resolved = EnvironmentResolver::resolve_snapshot(
            Path::new("/projeto/inexistente"),
            &snapshot,
            None,
            false,
        )
        .unwrap();
        assert_eq!(
            resolved.cwd,
            PathBuf::from("~/htdocs/engeapp.com.br/current")
        );
    }

    #[test]
    fn remote_default_directory_is_server_home() {
        let snapshot = remote_snapshot(None);
        let resolved = EnvironmentResolver::resolve_snapshot(
            Path::new("/projeto/local"),
            &snapshot,
            None,
            true,
        )
        .unwrap();
        assert_eq!(resolved.cwd, PathBuf::from("~"));
    }

    #[test]
    fn remote_override_stays_on_server_even_with_fallback() {
        let snapshot = remote_snapshot(Some("~/htdocs"));
        let resolved = EnvironmentResolver::resolve_snapshot(
            Path::new("/tmp"),
            &snapshot,
            Some("/srv/app"),
            true,
        )
        .unwrap();
        assert_eq!(resolved.cwd, PathBuf::from("/srv/app"));
    }

    #[test]
    fn local_directory_still_requires_existing_folder() {
        let mut snapshot = remote_snapshot(Some("/pasta/inexistente/maxterminal-teste"));
        snapshot.connection = TemplateConnection::Local;
        let err = EnvironmentResolver::resolve_snapshot(Path::new("/tmp"), &snapshot, None, false)
            .unwrap_err();
        assert_eq!(err.code, codes::TEMPLATE_CWD_NOT_FOUND);
    }
}
