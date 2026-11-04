//! Helper e servidor local seguro para fornecimento de credencial ao OpenSSH via `SSH_ASKPASS`.
//!
//! O helper é despachado pelo OpenSSH e se comunica com o MaxTerminal por Unix Domain Socket
//! autenticado por token efêmero de uso único. Nenhuma senha trafega em argv, variáveis de ambiente ou arquivos.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mt_core::error::{MtError, MtResult, codes};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// Sessão efêmera do servidor askpass que atende exclusivamente a uma tentativa de autenticação SSH.
pub struct AskpassServer {
    socket_path: PathBuf,
    token: String,
    _password: Arc<Zeroizing<String>>,
    served: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    temp_dir: PathBuf,
}

impl AskpassServer {
    /// Inicia um novo servidor askpass em um socket Unix privado em diretório temporário protegido (`0700`).
    pub fn start(password: Zeroizing<String>) -> MtResult<Self> {
        let unique_id = Uuid::now_v7();
        let temp_dir = std::env::temp_dir().join(format!("mt-ssh-askpass-{}", unique_id));

        fs::create_dir_all(&temp_dir).map_err(|e| {
            MtError::new(
                codes::IO,
                format!("Falha ao criar diretório seguro para SSH askpass: {e}"),
            )
        })?;

        // Permissões estritas 0700 no diretório
        let _ = fs::set_permissions(&temp_dir, fs::Permissions::from_mode(0o700));

        let socket_path = temp_dir.join("askpass.sock");
        let listener = UnixListener::bind(&socket_path).map_err(|e| {
            MtError::new(
                codes::IO,
                format!("Falha ao vincular socket Unix do askpass: {e}"),
            )
        })?;

        let _ = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600));

        let token = Uuid::now_v7().to_string();
        let password_arc = Arc::new(password);
        let served = Arc::new(AtomicBool::new(false));

        let srv_password = Arc::clone(&password_arc);
        let srv_token = token.clone();
        let srv_served = Arc::clone(&served);
        let stopped = Arc::new(AtomicBool::new(false));
        let srv_stopped = Arc::clone(&stopped);

        std::thread::spawn(move || {
            if listener.set_nonblocking(true).is_err() {
                return;
            }
            // Encerra a espera ao cancelar o teste ou após 30 segundos sem autenticação.
            for _ in 0..300 {
                if srv_stopped.load(Ordering::Acquire) {
                    break;
                }
                let mut stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                    Err(_) => break,
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

                let mut buf = [0u8; 1024];
                if let Ok(n) = stream.read(&mut buf) {
                    let msg = String::from_utf8_lossy(&buf[..n]);
                    let mut lines = msg.lines();
                    let received_token = lines.next().unwrap_or("").trim();
                    let prompt = lines.next().unwrap_or("").to_lowercase();

                    // Valida o token e confirma que é um prompt de password
                    if received_token == srv_token
                        && (prompt.contains("password")
                            || prompt.contains("senha")
                            || prompt.contains("passphrase")
                            || prompt.is_empty())
                    {
                        let _ = stream.write_all(srv_password.as_bytes());
                        let _ = stream.write_all(b"\n");
                        let _ = stream.flush();
                        srv_served.store(true, Ordering::Release);
                    }
                }
                break;
            }
        });

        Ok(Self {
            socket_path,
            token,
            _password: password_arc,
            served,
            stopped,
            temp_dir,
        })
    }

    /// Retorna o caminho do socket Unix.
    #[must_use]
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Retorna o token de autorização.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Indica se a credencial já foi atendida pelo helper.
    #[must_use]
    pub fn was_served(&self) -> bool {
        self.served.load(Ordering::Acquire)
    }

    /// Desacopla a limpeza para execução em background após uma janela de espera.
    pub fn detach_with_timeout(self, timeout: Duration) {
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            while start.elapsed() < timeout {
                if self.served.load(Ordering::Acquire) {
                    std::thread::sleep(Duration::from_millis(500));
                    break;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            drop(self);
        });
    }
}

impl Drop for AskpassServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_dir_all(&self.temp_dir);
    }
}

/// Execução do subcomando `--ssh-askpass-helper`.
/// Despachado diretamente no entrypoint antes de carregar o app Tauri.
pub fn run_askpass_helper(args: &[String]) -> ! {
    // Esperado: [_, "--ssh-askpass-helper", socket_path, token, optional_prompt]
    if args.len() < 4 {
        std::process::exit(1);
    }

    let socket_path = &args[2];
    let token = &args[3];
    let prompt = args.get(4).map_or("", String::as_str);

    let mut stream = match UnixStream::connect(socket_path) {
        Ok(s) => s,
        Err(_) => std::process::exit(1),
    };

    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let request_payload = format!("{token}\n{prompt}\n");
    if stream.write_all(request_payload.as_bytes()).is_err() {
        std::process::exit(1);
    }
    let _ = stream.flush();

    let mut response = Vec::new();
    if stream.read_to_end(&mut response).is_err() || response.is_empty() {
        std::process::exit(1);
    }

    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    let _ = handle.write_all(&response);
    let _ = handle.flush();

    response.zeroize();
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dropping_askpass_releases_password_without_waiting_for_a_connection() {
        let server = AskpassServer::start(Zeroizing::new(String::from("senha-de-teste"))).unwrap();
        let password = Arc::downgrade(&server._password);
        let socket = server.socket_path().to_path_buf();
        drop(server);
        for _ in 0..20 {
            if password.upgrade().is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(password.upgrade().is_none());
        assert!(!socket.exists());
    }

    #[test]
    fn detached_askpass_releases_password_after_timeout() {
        let server = AskpassServer::start(Zeroizing::new(String::from("senha-de-teste"))).unwrap();
        let password = Arc::downgrade(&server._password);
        server.detach_with_timeout(Duration::from_millis(20));
        for _ in 0..40 {
            if password.upgrade().is_none() {
                break;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(password.upgrade().is_none());
    }
}
