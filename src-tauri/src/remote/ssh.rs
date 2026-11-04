//! Construção de `SpawnSpec` para sessões remotas via cliente OpenSSH nativo.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

use mt_core::error::{MtError, MtResult, codes};
use mt_core::ids::{PaneId, TabId};
use mt_core::workspace::{SshAuth, TemplateConnection};
use mt_pty::types::{PtyOwner, PtySize, SpawnSpec};
use zeroize::Zeroizing;

use crate::remote::askpass::AskpassServer;

/// Sessão ativa de askpass que deve permanecer viva durante a tentativa de spawn do PTY.
pub struct SshSpawnSession {
    /// Especificação do PTY pronta para execução do processo SSH.
    pub spec: SpawnSpec,
    /// Servidor askpass ativo se a conexão usar autenticação por senha.
    pub askpass_server: Option<AskpassServer>,
    /// Script helper temporário invocado pelo OpenSSH.
    helper_script: Option<PathBuf>,
}

impl SshSpawnSession {
    /// Desacopla a limpeza do servidor askpass e do helper script para background.
    pub fn keep_alive_in_background(mut self, timeout: std::time::Duration) {
        let script_opt = self.helper_script.take();
        let server_opt = self.askpass_server.take();

        std::thread::spawn(move || {
            if let Some(server) = server_opt {
                server.detach_with_timeout(timeout);
            }
            std::thread::sleep(timeout);
            if let Some(ref script) = script_opt {
                let _ = fs::remove_file(script);
            }
        });
    }
}

impl Drop for SshSpawnSession {
    fn drop(&mut self) {
        if let Some(ref script) = self.helper_script {
            let _ = fs::remove_file(script);
        }
    }
}

/// Constrói um [`SpawnSpec`] estruturado para iniciar o cliente OpenSSH nativo em um PTY.
pub fn build_ssh_spawn(
    connection: &TemplateConnection,
    password_opt: Option<Zeroizing<String>>,
    tab_id: TabId,
    pane_id: PaneId,
    pty_size: PtySize,
) -> MtResult<SshSpawnSession> {
    let TemplateConnection::Ssh { host, user, auth } = connection else {
        return Err(MtError::invalid_argument(
            "connection",
            "A conexão deve ser do tipo SSH para construir spawn remoto.",
        ));
    };

    let mut args = vec![
        String::from("-p"),
        String::from("22"),
        String::from("-o"),
        String::from("BatchMode=no"),
        String::from("-o"),
        String::from("NumberOfPasswordPrompts=1"),
        String::from("-o"),
        String::from("StrictHostKeyChecking=ask"),
    ];

    let mut env = BTreeMap::new();
    let mut askpass_server = None;
    let mut helper_script = None;

    match auth {
        SshAuth::Key { identity_file } => {
            if let Some(path_str) = identity_file {
                let trimmed = path_str.trim();
                if !trimmed.is_empty() {
                    let expanded = if trimmed == "~" {
                        std::env::var("HOME").unwrap_or_default()
                    } else if let Some(rest) = trimmed.strip_prefix("~/") {
                        let home = std::env::var("HOME").unwrap_or_default();
                        format!("{home}/{rest}")
                    } else {
                        trimmed.to_string()
                    };
                    args.push(String::from("-i"));
                    args.push(expanded);
                }
            }
        }
        SshAuth::Password { .. } => {
            let password = password_opt.ok_or_else(|| {
                MtError::new(
                    codes::CREDENTIAL_LOCKED,
                    "Senha SSH não disponível (o cofre de credenciais precisa ser desbloqueado).",
                )
            })?;

            let server = AskpassServer::start(password)?;
            let current_exe = std::env::current_exe().map_err(|e| {
                MtError::new(
                    codes::INTERNAL,
                    format!("Falha ao resolver caminho do executável atual: {e}"),
                )
            })?;

            let socket_str = server.socket_path().to_string_lossy();
            let token = server.token();
            let script_path = server.socket_path().parent().unwrap_or(server.socket_path()).join("askpass_helper.sh");

            let script_content = format!(
                "#!/bin/sh\nexec \"{}\" --ssh-askpass-helper \"{}\" \"{}\" \"$@\"\n",
                current_exe.to_string_lossy(),
                socket_str,
                token
            );

            fs::write(&script_path, script_content).map_err(|e| {
                MtError::new(
                    codes::IO,
                    format!("Falha ao criar script helper do askpass: {e}"),
                )
            })?;

            let _ = fs::set_permissions(&script_path, fs::Permissions::from_mode(0o700));

            env.insert(String::from("SSH_ASKPASS"), script_path.to_string_lossy().into_owned());
            env.insert(String::from("SSH_ASKPASS_REQUIRE"), String::from("force"));
            // DISPLAY dummy para compatibilidade com versões do OpenSSH que exigem DISPLAY para askpass
            if std::env::var_os("DISPLAY").is_none() {
                env.insert(String::from("DISPLAY"), String::from(":0"));
            }

            helper_script = Some(script_path);
            askpass_server = Some(server);
        }
    }

    args.push(format!("{user}@{host}"));

    let spec = SpawnSpec {
        shell: Some(String::from("ssh")),
        args,
        login: false,
        cwd: None,
        env,
        env_remove: vec![
            String::from("MT_SHELL_INJECTION"),
            String::from("MT_AGENT_ID"),
        ],
        size: pty_size,
        owner: PtyOwner {
            tab_id: Some(tab_id),
            pane_id: Some(pane_id),
        },
    };

    Ok(SshSpawnSession {
        spec,
        askpass_server,
        helper_script,
    })
}
