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
            let script_path = server
                .socket_path()
                .parent()
                .unwrap_or(server.socket_path())
                .join("askpass_helper.sh");

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

            env.insert(
                String::from("SSH_ASKPASS"),
                script_path.to_string_lossy().into_owned(),
            );
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

/// Protege o caminho para o shell remoto, expandindo apenas `~` no servidor.
pub fn remote_cwd_expression(cwd: Option<&str>) -> MtResult<String> {
    let cwd = cwd
        .map(str::trim)
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or("~");
    if cwd.chars().any(char::is_control) {
        return Err(MtError::new(
            codes::TEMPLATE_INVALID,
            "A pasta remota contém caracteres inválidos.",
        ));
    }
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    Ok(if cwd == "~" {
        String::from("\"$HOME\"")
    } else if let Some(relative) = cwd.strip_prefix("~/") {
        format!("\"$HOME\"/{}", quote(relative))
    } else {
        quote(cwd)
    })
}

/// Inicia o shell interativo na pasta do servidor, sem usar essa pasta no processo local.
pub fn configure_remote_shell(spec: &mut SpawnSpec, cwd: Option<&str>) -> MtResult<()> {
    let directory = remote_cwd_expression(cwd)?;
    spec.args.insert(0, String::from("-t"));
    spec.args.push(format!(
        "cd -- {directory} || exit 72; exec \"${{SHELL:-/bin/sh}}\" -l"
    ));
    Ok(())
}

/// Testa autenticação e acesso à pasta remota sem criar sessão nem executar o comando inicial.
pub async fn test_connection(
    connection: &TemplateConnection,
    password: Option<Zeroizing<String>>,
    cwd: Option<&str>,
) -> MtResult<String> {
    use std::process::Stdio;
    use std::time::Duration;

    let directory = remote_cwd_expression(cwd)?;
    let session = build_ssh_spawn(
        connection,
        password,
        TabId::new(),
        PaneId::new(),
        PtySize::default(),
    )?;
    let mut args = session.spec.args.clone();
    for arg in &mut args {
        if arg == "StrictHostKeyChecking=ask" {
            *arg = String::from("StrictHostKeyChecking=yes");
        }
        if arg == "BatchMode=no"
            && matches!(
                connection,
                TemplateConnection::Ssh {
                    auth: SshAuth::Key { .. },
                    ..
                }
            )
        {
            *arg = String::from("BatchMode=yes");
        }
    }
    args.splice(
        0..0,
        [
            "-T",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ConnectionAttempts=1",
            "-o",
            "ControlMaster=no",
            "-o",
            "ControlPath=none",
        ]
        .map(String::from),
    );
    args.push(format!("cd -- {directory} || exit 72; pwd -P"));
    let mut command = tokio::process::Command::new("ssh");
    command
        .args(args)
        .env_remove("SSH_ASKPASS")
        .envs(&session.spec.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in &session.spec.env_remove {
        command.env_remove(name);
    }
    run_connection_test(command, Duration::from_secs(15)).await
}

async fn run_connection_test(
    mut command: tokio::process::Command,
    timeout: std::time::Duration,
) -> MtResult<String> {
    let child = command.kill_on_drop(true).spawn().map_err(|_| {
        MtError::new(
            codes::SSH_CONNECTION_FAILED,
            "Não foi possível iniciar o cliente OpenSSH.",
        )
    })?;
    let output = tokio::time::timeout(timeout, child.wait_with_output())
        .await
        .map_err(|_| {
            MtError::new(
                codes::SSH_CONNECTION_FAILED,
                "O teste de conexão SSH excedeu o tempo limite de 15 segundos.",
            )
        })?
        .map_err(|_| {
            MtError::new(
                codes::SSH_CONNECTION_FAILED,
                "Falha ao aguardar o teste de conexão SSH.",
            )
        })?;
    if output.status.code() == Some(72) {
        return Err(MtError::new(
            codes::TEMPLATE_CWD_NOT_FOUND,
            "Conexão SSH estabelecida, mas a pasta remota não existe, não é um diretório ou não permite acesso.",
        ));
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if stderr.contains("Host key verification failed")
            || stderr.contains("REMOTE HOST IDENTIFICATION HAS CHANGED")
        {
            "Não foi possível verificar a identidade do servidor. Conecte-se pelo terminal SSH para verificar a chave do host antes de testar novamente."
        } else if stderr.contains("Permission denied") {
            "Autenticação SSH recusada. Verifique o usuário, a senha ou a chave SSH."
        } else {
            "Não foi possível conectar ao servidor SSH. Verifique o endereço, a rede e a disponibilidade do servidor."
        };
        return Err(MtError::new(codes::SSH_CONNECTION_FAILED, message));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    #[tokio::test]
    async fn connection_test_reports_success_missing_directory_and_authentication_failure() {
        for (script, expected) in [
            (
                "printf '/home/engeapp/htdocs/app\\n'",
                Ok("/home/engeapp/htdocs/app"),
            ),
            ("exit 72", Err(codes::TEMPLATE_CWD_NOT_FOUND)),
            (
                "printf 'Permission denied' >&2; exit 255",
                Err(codes::SSH_CONNECTION_FAILED),
            ),
            (
                "printf 'Host key verification failed' >&2; exit 255",
                Err(codes::SSH_CONNECTION_FAILED),
            ),
        ] {
            let mut command = tokio::process::Command::new("/bin/sh");
            command
                .args(["-c", script])
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());
            let result = run_connection_test(command, std::time::Duration::from_secs(1)).await;
            match expected {
                Ok(cwd) => assert_eq!(result.unwrap(), cwd),
                Err(code) => assert_eq!(result.unwrap_err().code, code),
            }
        }
    }

    #[tokio::test]
    async fn connection_test_has_bounded_wait() {
        let mut command = tokio::process::Command::new("/bin/sleep");
        command.arg("2");
        let error = run_connection_test(command, std::time::Duration::from_millis(20))
            .await
            .unwrap_err();
        assert_eq!(error.code, codes::SSH_CONNECTION_FAILED);
        assert!(error.message.contains("tempo limite"));
    }

    #[test]
    fn remote_tilde_expands_with_server_home_and_preserves_literal_characters() {
        let home = tempfile::tempdir().unwrap();
        let relative = "htdocs/engeapp's $(printf injetado); pasta/current";
        let folder = home.path().join(relative);
        std::fs::create_dir_all(&folder).unwrap();
        let expr = remote_cwd_expression(Some(&format!("~/{relative}"))).unwrap();
        let output = Command::new("/bin/sh")
            .args(["-c", &format!("cd -- {expr} && pwd -P")])
            .env("HOME", home.path())
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            folder.to_str().unwrap()
        );
    }

    #[test]
    fn remote_paths_support_home_absolute_relative_and_empty_values() {
        for (path, expected) in [
            (None, "\"$HOME\""),
            (Some("~"), "\"$HOME\""),
            (Some(" "), "\"$HOME\""),
            (Some("/srv/app"), "'/srv/app'"),
            (Some("htdocs/app"), "'htdocs/app'"),
        ] {
            assert_eq!(remote_cwd_expression(path).unwrap(), expected);
        }
        assert!(remote_cwd_expression(Some("pasta\ncomando")).is_err());
        assert!(remote_cwd_expression(Some("pasta\0comando")).is_err());
    }

    #[test]
    fn remote_shell_uses_server_cwd_and_keeps_local_spawn_directory_unset() {
        let connection = TemplateConnection::Ssh {
            host: String::from("192.168.3.31"),
            user: String::from("engeapp"),
            auth: SshAuth::Key {
                identity_file: None,
            },
        };
        let mut session = build_ssh_spawn(
            &connection,
            None,
            TabId::new(),
            PaneId::new(),
            PtySize::default(),
        )
        .unwrap();
        configure_remote_shell(&mut session.spec, Some("~/htdocs/app")).unwrap();
        assert_eq!(session.spec.cwd, None);
        assert_eq!(session.spec.args[0], "-t");
        assert_eq!(
            session.spec.args.last().unwrap(),
            "cd -- \"$HOME\"/'htdocs/app' || exit 72; exec \"${SHELL:-/bin/sh}\" -l"
        );
    }
}
