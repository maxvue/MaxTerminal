//! Subsistema de supervisão de processos via `/proc` e sinalização validada via `pidfd`.

pub mod foreground;
pub mod info;
pub mod source;
pub mod tree;

use std::collections::HashMap;
use std::fmt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::{Arc, OnceLock};

use nix::sys::signal::Signal;
use parking_lot::Mutex;

use crate::error::{PtyError, PtyResult};
use crate::types::{ProcessIdentity, ProcessInfo};

pub use foreground::{query_foreground_pgid, resolve_foreground_process};
pub use info::{
    ParsedStat, map_proc_state, parse_proc_stat_line, parsed_stat_from_procfs,
    read_process_identity, read_process_info,
};
pub use source::{FakeProcSource, ProcfsSource};
pub use tree::{build_process_report, build_process_tree};

/// Retorna o `boot_id` atual do kernel Linux lido uma única vez de `/proc/sys/kernel/random/boot_id`.
#[must_use]
pub fn current_boot_id() -> &'static str {
    static BOOT_ID: OnceLock<String> = OnceLock::new();
    BOOT_ID.get_or_init(|| {
        std::fs::read_to_string("/proc/sys/kernel/random/boot_id")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "unknown-boot-id".to_string())
    })
}

/// Extrai o caminho relativo da hierarquia unificada cgroup v2 (`0::<caminho>`) do conteúdo de `/proc/<pid>/cgroup`.
#[must_use]
pub fn extract_cgroup_v2_rel_path(cgroup_content: &str) -> Option<&str> {
    for line in cgroup_content.lines() {
        if let Some(rel) = line.strip_prefix("0::") {
            return Some(rel.trim());
        }
    }
    None
}

/// Obtém o inode do diretório cgroup v2 atual do processo em `/sys/fs/cgroup`.
#[must_use]
pub fn process_cgroup_inode(pid: u32) -> Option<u64> {
    let content = std::fs::read_to_string(format!("/proc/{pid}/cgroup")).ok()?;
    let rel_path = extract_cgroup_v2_rel_path(&content)?;
    let trimmed = rel_path.trim_start_matches('/');
    let cgroup_dir = if trimmed.is_empty() {
        Path::new("/sys/fs/cgroup").to_path_buf()
    } else {
        Path::new("/sys/fs/cgroup").join(trimmed)
    };
    std::fs::metadata(cgroup_dir).ok().map(|m| m.ino())
}

/// Fonte de dados de processos do sistema operacional.
pub trait ProcSource: Send + Sync + 'static {
    /// Retorna informações detalhadas do processo ou `Ok(None)` se o PID não existir mais.
    fn process(&self, pid: u32) -> PtyResult<Option<ProcessInfo>>;

    /// Lista todos os PIDs visíveis em `/proc`.
    fn all_pids(&self) -> PtyResult<Vec<u32>>;

    /// Obtém a identidade imutável `(boot_id, pid, start_time)` de um PID vivo.
    fn identity(&self, pid: u32) -> PtyResult<Option<ProcessIdentity>>;

    /// Retorna o `boot_id` atual do kernel Linux associado a esta fonte.
    fn boot_id(&self) -> &str;

    /// Retorna o identificador (inode) do cgroup v2 ao qual o processo pertence.
    fn cgroup_id(&self, pid: u32) -> Option<u64> {
        process_cgroup_inode(pid)
    }
}

/// Abstração para envio de sinais validados por identidade de processo (`pidfd`).
pub trait Signaler: Send + Sync + 'static {
    /// Envia `sig` ao processo se e somente se sua identidade `(boot_id, pid, start_time)`
    /// continuar válida no momento da abertura do `pidfd`.
    fn signal(&self, process: &ProcessIdentity, sig: Signal) -> PtyResult<()>;

    /// Envia `sig` validando simultaneamente a identidade do processo e sua presença no cgroup indicado.
    fn signal_in_cgroup(
        &self,
        process: &ProcessIdentity,
        cgroup_id: u64,
        sig: Signal,
    ) -> PtyResult<()>;
}

/// Implementação real de sinalização via `pidfd_open` + revalidação de `/proc/<pid>/stat` e `/proc/<pid>/cgroup`.
pub struct PidfdSignaler {
    proc_source: Arc<dyn ProcSource>,
}

impl fmt::Debug for PidfdSignaler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PidfdSignaler")
            .field("boot_id", &self.proc_source.boot_id())
            .finish_non_exhaustive()
    }
}

impl PidfdSignaler {
    /// Cria um novo sinalizador baseado em `pidfd` utilizando a fonte `/proc` informada.
    #[must_use]
    pub fn new(proc_source: Arc<dyn ProcSource>) -> Self {
        Self { proc_source }
    }

    fn validate_and_signal(
        &self,
        process: &ProcessIdentity,
        expected_cgroup_id: Option<u64>,
        sig: Signal,
    ) -> PtyResult<()> {
        if process.boot_id != self.proc_source.boot_id() {
            return Err(PtyError::PidNotOwned(process.pid));
        }

        let raw_pid = i32::try_from(process.pid)
            .ok()
            .filter(|&p| p > 0)
            .ok_or(PtyError::PidNotOwned(process.pid))?;
        let rpid =
            rustix::process::Pid::from_raw(raw_pid).ok_or(PtyError::PidNotOwned(process.pid))?;

        let pidfd = match rustix::process::pidfd_open(rpid, rustix::process::PidfdFlags::empty()) {
            Ok(fd) => fd,
            Err(rustix::io::Errno::SRCH) => {
                // Caso o processo não exista no kernel (já encerrou ou é um PID simulado em FakeProcSource),
                // ainda consultamos `proc_source` para detectar reciclagem de PID ou violação de cgroup.
                return match self.proc_source.identity(process.pid)? {
                    Some(current) => {
                        if current.start_time != process.start_time
                            || current.boot_id != process.boot_id
                        {
                            return Err(PtyError::PidNotOwned(process.pid));
                        }
                        if let Some(cid) = expected_cgroup_id
                            && self.proc_source.cgroup_id(process.pid) != Some(cid)
                        {
                            return Err(PtyError::PidNotOwned(process.pid));
                        }
                        Ok(())
                    }
                    None => {
                        if expected_cgroup_id.is_some() {
                            Err(PtyError::PidNotOwned(process.pid))
                        } else {
                            Ok(())
                        }
                    }
                };
            }
            Err(err) => {
                return Err(PtyError::KillFailed(format!(
                    "falha em pidfd_open para PID {}: {err}",
                    process.pid
                )));
            }
        };

        // Com o `pidfd` aberto (referência congelada à task_struct no kernel),
        // revalidamos a identidade `(boot_id, pid, start_time)` contra TOCTOU.
        let current = self
            .proc_source
            .identity(process.pid)?
            .ok_or(PtyError::PidNotOwned(process.pid))?;

        if current.start_time != process.start_time || current.boot_id != process.boot_id {
            return Err(PtyError::PidNotOwned(process.pid));
        }

        if let Some(cid) = expected_cgroup_id
            && self.proc_source.cgroup_id(process.pid) != Some(cid)
        {
            return Err(PtyError::PidNotOwned(process.pid));
        }

        let rsig = rustix::process::Signal::from_raw(sig as i32).ok_or_else(|| {
            PtyError::KillFailed(format!("sinal POSIX inválido para pidfd: {sig}"))
        })?;

        match rustix::process::pidfd_send_signal(&pidfd, rsig) {
            Ok(()) | Err(rustix::io::Errno::SRCH) => Ok(()),
            Err(err) => Err(PtyError::KillFailed(format!(
                "falha em pidfd_send_signal({sig}) para PID {}: {err}",
                process.pid
            ))),
        }
    }
}

impl Signaler for PidfdSignaler {
    fn signal(&self, process: &ProcessIdentity, sig: Signal) -> PtyResult<()> {
        self.validate_and_signal(process, None, sig)
    }

    fn signal_in_cgroup(
        &self,
        process: &ProcessIdentity,
        cgroup_id: u64,
        sig: Signal,
    ) -> PtyResult<()> {
        self.validate_and_signal(process, Some(cgroup_id), sig)
    }
}

/// Implementação para testes unitários que grava a sequência exata de sinais enviados.
#[derive(Default)]
pub struct RecordingSignaler {
    /// Lista de sinais efetivamente enviados após validação.
    pub sent: Mutex<Vec<(ProcessIdentity, Signal)>>,
    /// Mapa opcional de `pid -> cgroup_id` registrado diretamente no sinalizador.
    pub membership: Mutex<HashMap<u32, u64>>,
    proc_source: Option<Arc<dyn ProcSource>>,
}

impl fmt::Debug for RecordingSignaler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RecordingSignaler")
            .field("sent", &*self.sent.lock())
            .field("membership", &*self.membership.lock())
            .field("has_proc_source", &self.proc_source.is_some())
            .finish()
    }
}

impl RecordingSignaler {
    /// Cria um novo `RecordingSignaler` vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Cria um `RecordingSignaler` acoplado a um `ProcSource` para validar `boot_id`, `start_time` e cgroup.
    #[must_use]
    pub fn with_proc_source(proc_source: Arc<dyn ProcSource>) -> Self {
        Self {
            sent: Mutex::new(Vec::new()),
            membership: Mutex::new(HashMap::new()),
            proc_source: Some(proc_source),
        }
    }

    /// Associa um `pid` a um `cgroup_id` na tabela interna de membros.
    pub fn set_cgroup(&self, pid: u32, cgroup_id: u64) {
        self.membership.lock().insert(pid, cgroup_id);
    }

    /// Remove um `pid` da tabela interna de membros.
    pub fn remove_cgroup(&self, pid: u32) {
        self.membership.lock().remove(&pid);
    }

    /// Retorna uma cópia de todos os sinais registrados até o momento.
    #[must_use]
    pub fn sent_signals(&self) -> Vec<(ProcessIdentity, Signal)> {
        self.sent.lock().clone()
    }
}

impl Signaler for RecordingSignaler {
    fn signal(&self, process: &ProcessIdentity, sig: Signal) -> PtyResult<()> {
        if let Some(source) = &self.proc_source {
            if process.boot_id != source.boot_id() {
                return Err(PtyError::PidNotOwned(process.pid));
            }
            match source.identity(process.pid)? {
                Some(current) => {
                    if current.start_time != process.start_time
                        || current.boot_id != process.boot_id
                    {
                        return Err(PtyError::PidNotOwned(process.pid));
                    }
                }
                None => return Ok(()),
            }
        }
        self.sent.lock().push((process.clone(), sig));
        Ok(())
    }

    fn signal_in_cgroup(
        &self,
        process: &ProcessIdentity,
        cgroup_id: u64,
        sig: Signal,
    ) -> PtyResult<()> {
        if let Some(source) = &self.proc_source {
            if process.boot_id != source.boot_id() {
                return Err(PtyError::PidNotOwned(process.pid));
            }
            let current = source
                .identity(process.pid)?
                .ok_or(PtyError::PidNotOwned(process.pid))?;
            if current.start_time != process.start_time || current.boot_id != process.boot_id {
                return Err(PtyError::PidNotOwned(process.pid));
            }
        }

        let actual_cgroup = self
            .membership
            .lock()
            .get(&process.pid)
            .copied()
            .or_else(|| {
                self.proc_source
                    .as_ref()
                    .and_then(|s| s.cgroup_id(process.pid))
            });

        if actual_cgroup != Some(cgroup_id) {
            return Err(PtyError::PidNotOwned(process.pid));
        }

        self.sent.lock().push((process.clone(), sig));
        Ok(())
    }
}
