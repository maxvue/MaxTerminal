//! Subsistema de contenção de processos Linux (cgroup v2, launcher e watchdog).

pub mod cgroup;
pub mod launcher;
pub mod watchdog;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mt_core::ids::PtyId;
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::error::{PtyError, PtyResult};

pub use cgroup::{
    FakeContainmentHandle, FakeContainmentProvider, SystemdCgroupHandle, SystemdCgroupProvider,
};
pub use launcher::{LauncherServer, PendingLauncher, resolve_helper_binary};
pub use watchdog::WatchdogClient;

/// Tamanho máximo permitido para mensagens enquadradas nos canais de controle locais (4 MiB).
pub const MAX_CONTROL_FRAME_LEN: usize = 4 * 1024 * 1024;

/// Mensagem enviada do processo pai para o `mt-pty-helper --mode launcher` autorizando o `execve`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherReleaseMsg {
    /// Caminho do executável do shell.
    pub executable: String,
    /// Argumentos passados ao executável (`argv[1..]`).
    pub argv: Vec<String>,
    /// Diretório de trabalho inicial canonicalizado.
    pub cwd: String,
    /// Pares `(chave, valor)` do ambiente completo validado.
    pub env: Vec<(String, String)>,
}

/// Resposta enviada pelo `mt-pty-helper --mode launcher` caso o `execve` falhe.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherResponseMsg {
    /// Indica se a preparação antes do `execve` teve sucesso.
    pub ok: bool,
    /// Mensagem de erro se o `chdir` ou `execve` falhou.
    pub error: Option<String>,
}

/// Comandos enviados do processo principal para o processo `mt-pty-helper --mode watchdog`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatchdogCommand {
    /// Registra um novo cgroup de PTY sob supervisão do watchdog antes de liberar o `execve`.
    RegisterCgroup {
        /// Identificador do PTY.
        pty_id: PtyId,
        /// Caminho absoluto do diretório cgroup v2.
        cgroup_path: PathBuf,
        /// Inode esperado do diretório cgroup v2.
        cgroup_id: u64,
    },
    /// Remove um cgroup da lista ativa após confirmação de `populated == 0`.
    UnregisterCgroup {
        /// Identificador do PTY encerrado.
        pty_id: PtyId,
    },
    /// Verifica se o watchdog está saudável.
    Ping,
    /// Solicita encerramento gracioso do watchdog quando todos os PTYs já saíram.
    Shutdown,
}

/// Respostas enviadas pelo `mt-pty-helper --mode watchdog` ao processo principal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WatchdogResponse {
    /// Confirmação de registro, remoção ou desligamento.
    Ack,
    /// Resposta ao comando `Ping`.
    Pong,
    /// Erro operacional reportado pelo watchdog.
    Error {
        /// Descrição do erro.
        message: String,
    },
}

/// Envia uma estrutura serializada em JSON precedida por seu tamanho em `u32` Little-Endian.
pub fn send_framed_json<W: Write, T: Serialize>(writer: &mut W, msg: &T) -> PtyResult<()> {
    let payload = serde_json::to_vec(msg).map_err(|e| {
        PtyError::ContainmentFailed(format!("Falha ao serializar mensagem de controle: {e}"))
    })?;
    if payload.len() > MAX_CONTROL_FRAME_LEN {
        return Err(PtyError::ContainmentFailed(
            "Mensagem de controle excede o limite de 4 MiB".to_string(),
        ));
    }
    let len = u32::try_from(payload.len())
        .map_err(|_| PtyError::ContainmentFailed("Tamanho de payload inválido".to_string()))?;
    writer.write_all(&len.to_le_bytes()).map_err(PtyError::Io)?;
    writer.write_all(&payload).map_err(PtyError::Io)?;
    writer.flush().map_err(PtyError::Io)?;
    Ok(())
}

/// Recebe uma estrutura JSON enquadrada por `u32` Little-Endian.
/// Retorna `Ok(None)` se o stream encontrar EOF imediato no primeiro byte do cabeçalho.
pub fn recv_framed_json<R: Read, T: DeserializeOwned>(reader: &mut R) -> PtyResult<Option<T>> {
    let mut len_buf = [0u8; 4];
    let mut read_header = 0usize;
    while read_header < 4 {
        match reader.read(&mut len_buf[read_header..]) {
            Ok(0) => {
                if read_header == 0 {
                    return Ok(None);
                }
                return Err(PtyError::ContainmentFailed(
                    "EOF inesperado no cabeçalho de controle".to_string(),
                ));
            }
            Ok(n) => read_header += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(PtyError::Io(e)),
        }
    }

    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_CONTROL_FRAME_LEN {
        return Err(PtyError::ContainmentFailed(format!(
            "Comprimento de mensagem de controle inválido: {len}"
        )));
    }

    let mut payload = vec![0u8; len];
    reader.read_exact(&mut payload).map_err(PtyError::Io)?;

    let parsed = serde_json::from_slice(&payload).map_err(|e| {
        PtyError::ContainmentFailed(format!("Falha ao desserializar mensagem de controle: {e}"))
    })?;
    Ok(Some(parsed))
}

/// Handle para um cgroup v2 dedicado a um único PTY.
pub trait ContainmentHandle: Send + Sync + 'static {
    /// Caminho do diretório do cgroup no filesystem `/sys/fs/cgroup`.
    fn cgroup_path(&self) -> &Path;

    /// Identificador único (inode) do diretório do cgroup.
    fn cgroup_id(&self) -> u64;

    /// Inscreve um PID (como o helper launcher bloqueado) em `cgroup.procs`.
    fn enroll_pid(&self, pid: u32) -> PtyResult<()>;

    /// Lista todos os PIDs atualmente presentes neste cgroup e em eventuais subcgroups.
    fn members(&self) -> PtyResult<Vec<u32>>;

    /// Aciona `cgroup.kill` escrevendo `"1"`, enviando `SIGKILL` atômico a todos os processos do grupo.
    fn kill_all(&self) -> PtyResult<()>;

    /// Verifica se o cgroup está vazio (`populated 0` em `cgroup.events` e sem PIDs em `cgroup.procs`).
    fn is_empty(&self) -> PtyResult<bool>;

    /// Aguarda até que o cgroup fique completamente vazio dentro de `timeout`.
    fn wait_empty(&self, timeout: Duration) -> PtyResult<bool>;

    /// Remove o diretório do cgroup após confirmação de esvaziamento.
    fn release(&self) -> PtyResult<()>;
}

/// Provedor de contenção responsável por criar cgroups isolados por PTY.
pub trait ContainmentProvider: Send + Sync + 'static {
    /// Cria um novo cgroup exclusivo para `pty_id`.
    fn create(&self, pty_id: PtyId) -> PtyResult<Arc<dyn ContainmentHandle>>;

    /// Indica se a contenção real está disponível e operacional.
    fn is_available(&self) -> bool;

    /// Retorna o diretório raiz delegado onde os cgroups de PTYs são criados.
    fn subtree_root(&self) -> Option<&Path>;
}
