//! Servidor e protocolo de sincronização para o helper `mt-pty-helper --mode launcher`.
//!
//! Garante que nenhum processo de usuário execute (`execve`) antes de estar inscrito no
//! cgroup v2 dedicado do PTY e registrado no watchdog (Invariante C-04).

use std::fs::{self, Permissions};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use mt_core::ids::PtyId;
use nix::sys::socket::{getsockopt, sockopt::PeerCredentials};
use rustix::event::{PollFd, PollFlags, poll};
use rustix::process::getuid;
use uuid::Uuid;

use crate::containment::{
    ContainmentHandle, LauncherReleaseMsg, LauncherResponseMsg, recv_framed_json, send_framed_json,
};
use crate::error::{PtyError, PtyResult};
use crate::spawn::PreparedSpawn;

/// Timeout padrão para handshake e inscrição do processo launcher (5 segundos).
pub const DEFAULT_LAUNCHER_TIMEOUT: Duration = Duration::from_secs(5);

/// Localiza o binário auxiliar `mt-pty-helper`.
///
/// Ordem de busca:
/// 1. Caminho explícito configurado em `override_path`.
/// 2. Variável de ambiente `CARGO_BIN_EXE_mt-pty-helper` (definida pelo Cargo em testes).
/// 3. Mesmo diretório do executável atual (`current_exe().parent()`).
/// 4. Diretório pai do diretório do executável atual (`target/debug/` quando rodando de `target/debug/deps/`).
///
/// # Errors
/// Retorna [`PtyError::ContainmentFailed`] se o binário não for encontrado.
pub fn resolve_helper_binary(override_path: Option<&Path>) -> PtyResult<PathBuf> {
    if let Some(path) = override_path {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(PtyError::ContainmentFailed(format!(
            "Binário mt-pty-helper configurado não existe: {}",
            path.display()
        )));
    }

    if let Some(env_bin) = std::env::var_os("CARGO_BIN_EXE_mt-pty-helper") {
        let path = PathBuf::from(env_bin);
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Ok(current_exe) = std::env::current_exe() {
        if let Some(parent) = current_exe.parent() {
            let sibling = parent.join("mt-pty-helper");
            if sibling.is_file() {
                return Ok(sibling);
            }
            if let Some(grandparent) = parent.parent() {
                let uncle = grandparent.join("mt-pty-helper");
                if uncle.is_file() {
                    return Ok(uncle);
                }
                let lib_helper = grandparent.join("lib/maxterminal/mt-pty-helper");
                if lib_helper.is_file() {
                    return Ok(lib_helper);
                }
            }
        }
        if current_exe.file_name().and_then(|s| s.to_str()) == Some("mt-pty-helper")
            && current_exe.is_file()
        {
            return Ok(current_exe);
        }
    }

    for candidate in [
        Path::new("/usr/lib/maxterminal/mt-pty-helper"),
        Path::new("/usr/bin/mt-pty-helper"),
    ] {
        if candidate.is_file() {
            return Ok(candidate.to_path_buf());
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        for rel in [
            ".local/lib/maxterminal/mt-pty-helper",
            ".local/bin/mt-pty-helper",
        ] {
            let candidate = home_path.join(rel);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    Err(PtyError::ContainmentFailed(
        "Não foi possível localizar o executável auxiliar mt-pty-helper".to_string(),
    ))
}

/// Servidor de socket Unix temporário que aguarda a conexão de um único processo `mt-pty-helper --mode launcher`.
#[derive(Debug)]
pub struct LauncherServer {
    listener: UnixListener,
    socket_path: PathBuf,
}

impl LauncherServer {
    /// Cria e vincula um socket Unix `0600` exclusivo para o spawn de `pty_id`.
    ///
    /// # Errors
    /// Retorna erro se o diretório ou socket não puderem ser criados com permissões restritas.
    pub fn bind(runtime_dir: &Path, pty_id: PtyId) -> PtyResult<Self> {
        let launchers_dir = runtime_dir.join("launchers");
        fs::create_dir_all(&launchers_dir).map_err(PtyError::Io)?;
        fs::set_permissions(&launchers_dir, Permissions::from_mode(0o700)).map_err(PtyError::Io)?;

        // Mantém o caminho curto para respeitar o limite sun_path (108 bytes).
        let short_id = &pty_id.as_uuid().simple().to_string()[..8];
        let nonce = &Uuid::new_v4().simple().to_string()[..8];
        let socket_path = launchers_dir.join(format!("l-{short_id}-{nonce}.sock"));

        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path).map_err(PtyError::Io)?;
        fs::set_permissions(&socket_path, Permissions::from_mode(0o600)).map_err(PtyError::Io)?;
        listener.set_nonblocking(true).map_err(PtyError::Io)?;

        Ok(Self {
            listener,
            socket_path,
        })
    }

    /// Retorna o caminho do socket Unix onde o `mt-pty-helper --mode launcher` deve conectar.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Aguarda a conexão do helper recém-criado, valida suas credenciais (`SO_PEERCRED`)
    /// e inscreve seu PID no `ContainmentHandle` antes de permitir o `execve`.
    ///
    /// # Errors
    /// Retorna erro se o timeout expirar, se o UID/PID do peer divergir ou se a inscrição no cgroup falhar.
    pub fn accept_and_enroll(
        self,
        expected_pid: u32,
        containment: &dyn ContainmentHandle,
        timeout: Duration,
    ) -> PtyResult<PendingLauncher> {
        let stream = accept_with_timeout(&self.listener, timeout)?;
        stream.set_nonblocking(false).map_err(PtyError::Io)?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(PtyError::Io)?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(PtyError::Io)?;

        let creds = getsockopt(&stream, PeerCredentials).map_err(|e| {
            PtyError::ContainmentFailed(format!("Falha ao obter SO_PEERCRED do launcher: {e}"))
        })?;

        let my_uid = getuid().as_raw();
        if creds.uid() != my_uid {
            return Err(PtyError::ContainmentFailed(format!(
                "UID do processo launcher ({}) difere do UID esperado ({my_uid})",
                creds.uid()
            )));
        }

        let peer_pid = u32::try_from(creds.pid()).map_err(|_| {
            PtyError::ContainmentFailed("SO_PEERCRED não informou um PID válido".to_string())
        })?;

        if peer_pid != expected_pid {
            return Err(PtyError::ContainmentFailed(format!(
                "PID conectado no launcher ({peer_pid}) difere do PID esperado ({expected_pid})"
            )));
        }

        // Inscreve o processo bloqueado no cgroup v2 dedicado antes de enviar a liberação.
        containment.enroll_pid(expected_pid)?;

        Ok(PendingLauncher {
            stream,
            socket_path: self.socket_path.clone(),
        })
    }
}

impl Drop for LauncherServer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}

/// Conexão estabelecida com o `mt-pty-helper --mode launcher` já inscrito no cgroup,
/// aguardando autorização final para executar `execve`.
#[derive(Debug)]
pub struct PendingLauncher {
    stream: UnixStream,
    socket_path: PathBuf,
}

impl PendingLauncher {
    /// Envia a especificação validada para o launcher executar `execve` e aguarda confirmação.
    ///
    /// Como o socket do launcher possui `SOCK_CLOEXEC`, o sucesso do `execve` fecha
    /// automaticamente o descritor no filho, produzindo EOF imediato (`Ok(None)`).
    /// Se o `chdir` ou `execve` falhar no filho, ele envia um [`LauncherResponseMsg`] com o erro.
    ///
    /// # Errors
    /// Retorna [`PtyError::SpawnFailed`] se o launcher reportar erro no `execve`, ou erro de I/O.
    pub fn release(mut self, prepared: &PreparedSpawn) -> PtyResult<()> {
        let env_pairs: Vec<(String, String)> = prepared
            .env
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        let msg = LauncherReleaseMsg {
            executable: prepared.executable.to_string_lossy().into_owned(),
            argv: prepared.args.clone(),
            cwd: prepared.cwd.to_string_lossy().into_owned(),
            env: env_pairs,
        };

        send_framed_json(&mut self.stream, &msg)?;

        match recv_framed_json::<_, LauncherResponseMsg>(&mut self.stream)? {
            None => Ok(()),
            Some(resp) if resp.ok => Ok(()),
            Some(resp) => Err(PtyError::SpawnFailed(resp.error.unwrap_or_else(|| {
                "Falha desconhecida ao executar execve no launcher".to_string()
            }))),
        }
    }
}

impl Drop for PendingLauncher {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.socket_path);
    }
}

/// Aceita uma conexão de um `UnixListener` não-bloqueante respeitando `timeout`.
pub(crate) fn accept_with_timeout(
    listener: &UnixListener,
    timeout: Duration,
) -> PtyResult<UnixStream> {
    let deadline = Instant::now() + timeout;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                let now = Instant::now();
                if now >= deadline {
                    return Err(PtyError::ContainmentFailed(
                        "Timeout aguardando conexão do processo auxiliar mt-pty-helper".to_string(),
                    ));
                }
                let remaining = deadline.saturating_duration_since(now);
                let poll_ms = i32::try_from(remaining.as_millis().min(250)).unwrap_or(250);
                let mut fds = [PollFd::new(listener, PollFlags::IN)];
                let _ = poll(&mut fds, poll_ms);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(PtyError::Io(e)),
        }
    }
}
