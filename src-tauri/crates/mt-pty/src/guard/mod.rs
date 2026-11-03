//! Guards de ciclo de vida do processo (`PR_SET_CHILD_SUBREAPER`, colheita seletiva de órfãos,
//! `pty-registry.json`, panic hook e captura de sinais).

pub mod panic;
pub mod registry_file;
pub mod signals;

use std::collections::HashSet;
use std::fs;
use std::sync::Arc;

use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::Pid;
use parking_lot::RwLock;
use rustix::process::set_child_subreaper;

use crate::containment::ContainmentHandle;
use crate::error::{PtyError, PtyResult};

pub use panic::{install_panic_hook, notify_emergency_fd};
pub use registry_file::{
    REGISTRY_SCHEMA_VERSION, RegistryEntry, RegistryFile, RegistryHeader, RegistrySnapshot,
    recover_stale_registries,
};
pub use signals::install_signal_handler;

/// Habilita `PR_SET_CHILD_SUBREAPER` no processo atual para que processos órfãos
/// das sessões de PTY sejam reparentados ao `MaxTerminal` em vez do PID 1.
///
/// # Errors
/// Retorna [`PtyError::ContainmentFailed`] caso a syscall `prctl` falhe.
pub fn enable_subreaper() -> PtyResult<()> {
    set_child_subreaper(Some(rustix::process::getpid())).map_err(|e| {
        PtyError::ContainmentFailed(format!("Falha ao configurar PR_SET_CHILD_SUBREAPER: {e}"))
    })
}

/// Registro thread-safe dos PIDs que são filhos diretos gerenciados (ex.: líderes de PTY e watchdog).
///
/// Garante o cumprimento da Invariante C-09: o colhedor de órfãos adotados nunca chama
/// `waitpid` nos filhos diretos cujos exit codes pertencem a `Child::try_wait()` ou `Command::wait()`.
#[derive(Debug, Default, Clone)]
pub struct DirectChildRegistry {
    pids: Arc<RwLock<HashSet<u32>>>,
}

impl DirectChildRegistry {
    /// Cria um novo registro vazio de filhos diretos.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra um PID como filho direto gerenciado.
    pub fn register(&self, pid: u32) {
        self.pids.write().insert(pid);
    }

    /// Remove um PID do registro de filhos diretos.
    pub fn unregister(&self, pid: u32) {
        self.pids.write().remove(&pid);
    }

    /// Verifica se `pid` é um filho direto gerenciado.
    #[must_use]
    pub fn contains(&self, pid: u32) -> bool {
        self.pids.read().contains(&pid)
    }
}

/// Colhe processos zumbis órfãos adotados via `PR_SET_CHILD_SUBREAPER` usando exclusivamente
/// `waitpid(Pid::from_raw(pid), WNOHANG)` por PID específico (Invariante C-09: nunca `waitpid(-1)`).
///
/// Retorna a quantidade de processos órfãos colhidos nesta passagem.
pub fn reap_adopted_orphans(
    direct_children: &DirectChildRegistry,
    cgroups: &[Arc<dyn ContainmentHandle>],
) -> usize {
    let my_pid = std::process::id();
    let mut candidate_pids = HashSet::new();

    for cg in cgroups {
        if let Ok(members) = cg.members() {
            for pid in members {
                if !direct_children.contains(pid) {
                    candidate_pids.insert(pid);
                }
            }
        }
    }

    // Inspeciona /proc para localizar zumbis cujo PPid é o processo atual, mas que não são filhos diretos.
    if let Ok(entries) = fs::read_dir("/proc") {
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let Some(name_str) = file_name.to_str() else {
                continue;
            };
            let Ok(pid) = name_str.parse::<u32>() else {
                continue;
            };
            if pid <= 1 || direct_children.contains(pid) {
                continue;
            }
            if is_adopted_zombie_of(pid, my_pid) {
                candidate_pids.insert(pid);
            }
        }
    }

    let mut reaped = 0usize;
    for pid in candidate_pids {
        if direct_children.contains(pid) {
            continue;
        }
        let Ok(raw_pid) = i32::try_from(pid) else {
            continue;
        };
        if raw_pid <= 1 {
            continue;
        }
        if let Ok(WaitStatus::Exited(_, _) | WaitStatus::Signaled(_, _, _)) =
            waitpid(Pid::from_raw(raw_pid), Some(WaitPidFlag::WNOHANG))
        {
            reaped += 1;
        }
    }

    reaped
}

fn is_adopted_zombie_of(pid: u32, expected_ppid: u32) -> bool {
    let stat_path = format!("/proc/{pid}/stat");
    let Ok(stat) = fs::read_to_string(stat_path) else {
        return false;
    };
    let Some(close_paren) = stat.rfind(')') else {
        return false;
    };
    let rest = stat.get(close_paren + 2..).unwrap_or("");
    let mut parts = rest.split_whitespace();
    let Some(_state) = parts.next() else {
        return false;
    };
    let Some(ppid_str) = parts.next() else {
        return false;
    };
    let Ok(parent_id) = ppid_str.parse::<u32>() else {
        return false;
    };
    parent_id == expected_ppid
}
