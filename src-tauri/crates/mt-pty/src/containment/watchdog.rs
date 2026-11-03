//! Cliente de comunicação e supervisão do processo auxiliar `mt-pty-helper --mode watchdog`.
//!
//! O watchdog roda fora dos cgroups das sessões de PTY e monitora o processo principal
//! por meio de dois canais Unix:
//! 1. Canal de controle (mensagens JSON enquadradas para registrar/remover cgroups).
//! 2. Canal de emergência (`emergency_fd`), que permite ao handler de sinal/panic ou ao
//!    fechamento abrupto do processo principal (`SIGKILL`) disparar `cgroup.kill` imediato
//!    em todas as árvores de PTY ativas.

use std::fs::{self, Permissions};
use std::io::Write;
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use mt_core::ids::PtyId;
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use parking_lot::Mutex;
use rustix::process::getuid;
use uuid::Uuid;

use crate::containment::launcher::{accept_with_timeout, resolve_helper_binary};
use crate::containment::{WatchdogCommand, WatchdogResponse, recv_framed_json, send_framed_json};
use crate::error::{PtyError, PtyResult};

/// Timeout padrão para comandos síncronos ao watchdog (5 segundos).
const WATCHDOG_IO_TIMEOUT: Duration = Duration::from_secs(5);

/// Cliente conectado ao processo `mt-pty-helper --mode watchdog`.
#[derive(Debug)]
pub struct WatchdogClient {
    child: Mutex<Child>,
    watchdog_pid: u32,
    control_stream: Mutex<UnixStream>,
    emergency_stream: UnixStream,
    socket_path: PathBuf,
}

impl WatchdogClient {
    /// Inicia o processo `mt-pty-helper --mode watchdog` e estabelece os canais de controle e emergência.
    ///
    /// # Errors
    /// Retorna [`PtyError::ContainmentFailed`] se o binário não puder ser iniciado ou se o handshake falhar.
    pub fn spawn(runtime_dir: &Path, helper_override: Option<&Path>) -> PtyResult<Self> {
        let helper_bin = resolve_helper_binary(helper_override)?;
        fs::create_dir_all(runtime_dir).map_err(PtyError::Io)?;
        fs::set_permissions(runtime_dir, Permissions::from_mode(0o700)).map_err(PtyError::Io)?;

        let nonce = &Uuid::new_v4().simple().to_string()[..8];
        let socket_path = runtime_dir.join(format!("wd-{nonce}.sock"));
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path).map_err(PtyError::Io)?;
        fs::set_permissions(&socket_path, Permissions::from_mode(0o600)).map_err(PtyError::Io)?;
        listener.set_nonblocking(true).map_err(PtyError::Io)?;

        let mut child = Command::new(&helper_bin)
            .arg("--mode")
            .arg("watchdog")
            .arg("--socket")
            .arg(&socket_path)
            .arg("--runtime-dir")
            .arg(runtime_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| {
                PtyError::ContainmentFailed(format!(
                    "Falha ao iniciar mt-pty-helper --mode watchdog ({}): {e}",
                    helper_bin.display()
                ))
            })?;

        let watchdog_pid = child.id();

        let accept_verified = |listener: &UnixListener| -> PtyResult<UnixStream> {
            let stream = accept_with_timeout(listener, WATCHDOG_IO_TIMEOUT)?;
            stream.set_nonblocking(false).map_err(PtyError::Io)?;
            stream
                .set_read_timeout(Some(WATCHDOG_IO_TIMEOUT))
                .map_err(PtyError::Io)?;
            stream
                .set_write_timeout(Some(WATCHDOG_IO_TIMEOUT))
                .map_err(PtyError::Io)?;

            let creds = getsockopt(&stream, PeerCredentials).map_err(|e| {
                PtyError::ContainmentFailed(format!("Falha ao ler SO_PEERCRED do watchdog: {e}"))
            })?;
            if creds.uid() != getuid().as_raw() {
                return Err(PtyError::ContainmentFailed(
                    "UID do processo watchdog inválido".to_string(),
                ));
            }
            let peer_pid = u32::try_from(creds.pid()).unwrap_or(0);
            if peer_pid != watchdog_pid {
                return Err(PtyError::ContainmentFailed(format!(
                    "PID conectado no socket do watchdog ({peer_pid}) difere do esperado ({watchdog_pid})"
                )));
            }
            Ok(stream)
        };

        let control_stream = match accept_verified(&listener) {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&socket_path);
                return Err(e);
            }
        };

        let emergency_stream = match accept_verified(&listener) {
            Ok(s) => s,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = fs::remove_file(&socket_path);
                return Err(e);
            }
        };

        let _ = fs::remove_file(&socket_path);

        let client = Self {
            child: Mutex::new(child),
            watchdog_pid,
            control_stream: Mutex::new(control_stream),
            emergency_stream,
            socket_path,
        };

        client.ping()?;
        Ok(client)
    }

    /// Retorna o PID do processo watchdog em execução.
    #[must_use]
    pub const fn watchdog_pid(&self) -> u32 {
        self.watchdog_pid
    }

    /// Retorna o descritor de arquivo de emergência (sem lock) para uso em handlers de panic/sinal.
    #[must_use]
    pub fn emergency_fd(&self) -> BorrowedFd<'_> {
        self.emergency_stream.as_fd()
    }

    /// Aciona imediatamente a varredura de emergência escrevendo 1 byte no canal de emergência.
    ///
    /// # Errors
    /// Retorna erro caso a escrita no socket falhe.
    pub fn trigger_emergency(&self) -> PtyResult<()> {
        let mut stream = &self.emergency_stream;
        stream.write_all(b"!").map_err(PtyError::Io)?;
        stream.flush().map_err(PtyError::Io)?;
        Ok(())
    }

    /// Registra um novo cgroup de PTY junto ao watchdog.
    ///
    /// # Errors
    /// Retorna erro se a comunicação com o watchdog falhar.
    pub fn register_cgroup(
        &self,
        pty_id: PtyId,
        cgroup_path: &Path,
        cgroup_id: u64,
    ) -> PtyResult<()> {
        let cmd = WatchdogCommand::RegisterCgroup {
            pty_id,
            cgroup_path: cgroup_path.to_path_buf(),
            cgroup_id,
        };
        match self.send_command(&cmd)? {
            WatchdogResponse::Ack => Ok(()),
            WatchdogResponse::Pong => Err(PtyError::ContainmentFailed(
                "Resposta inesperada (Pong) ao registrar cgroup no watchdog".to_string(),
            )),
            WatchdogResponse::Error { message } => Err(PtyError::ContainmentFailed(message)),
        }
    }

    /// Remove o registro de um cgroup de PTY após seu encerramento limpo.
    ///
    /// # Errors
    /// Retorna erro se a comunicação com o watchdog falhar.
    pub fn unregister_cgroup(&self, pty_id: PtyId) -> PtyResult<()> {
        let cmd = WatchdogCommand::UnregisterCgroup { pty_id };
        match self.send_command(&cmd)? {
            WatchdogResponse::Ack => Ok(()),
            WatchdogResponse::Pong => Err(PtyError::ContainmentFailed(
                "Resposta inesperada (Pong) ao desregistrar cgroup no watchdog".to_string(),
            )),
            WatchdogResponse::Error { message } => Err(PtyError::ContainmentFailed(message)),
        }
    }

    /// Verifica a vivacidade do processo watchdog.
    ///
    /// # Errors
    /// Retorna erro se o watchdog não responder `Pong`.
    pub fn ping(&self) -> PtyResult<()> {
        match self.send_command(&WatchdogCommand::Ping)? {
            WatchdogResponse::Pong => Ok(()),
            WatchdogResponse::Ack => Err(PtyError::ContainmentFailed(
                "Resposta inesperada (Ack) ao pingar watchdog".to_string(),
            )),
            WatchdogResponse::Error { message } => Err(PtyError::ContainmentFailed(message)),
        }
    }

    /// Encerra graciosamente o processo watchdog.
    ///
    /// # Errors
    /// Retorna erro se o comando de desligamento falhar.
    pub fn shutdown(&self) -> PtyResult<()> {
        let _ = self.send_command(&WatchdogCommand::Shutdown);
        let mut child = self.child.lock();
        let _ = child.wait();
        Ok(())
    }

    fn send_command(&self, cmd: &WatchdogCommand) -> PtyResult<WatchdogResponse> {
        let mut stream = self.control_stream.lock();
        send_framed_json(&mut *stream, cmd)?;
        recv_framed_json::<_, WatchdogResponse>(&mut *stream)?.ok_or_else(|| {
            PtyError::ContainmentFailed(
                "Canal de controle do watchdog fechou prematuramente".to_string(),
            )
        })
    }
}

impl Drop for WatchdogClient {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}
