//! Implementações real (`ProcfsSource`) e em memória (`FakeProcSource`) de `ProcSource`.

use std::collections::{HashMap, HashSet};

use parking_lot::Mutex;

use crate::error::{PtyError, PtyResult};
use crate::proc::info::{read_process_identity, read_process_info};
use crate::proc::{ProcSource, current_boot_id, process_cgroup_inode};
use crate::types::{ProcessIdentity, ProcessInfo};

/// Fonte real de dados de processos apoiada no sistema de arquivos `/proc` do Linux.
#[derive(Debug, Clone)]
pub struct ProcfsSource {
    boot_id: String,
    page_size: u64,
}

impl Default for ProcfsSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcfsSource {
    /// Cria uma nova instância de `ProcfsSource` lendo o `boot_id` e `page_size` do kernel atual.
    #[must_use]
    pub fn new() -> Self {
        let page_size = u64::try_from(rustix::param::page_size()).unwrap_or(4096);
        Self {
            boot_id: current_boot_id().to_string(),
            page_size,
        }
    }

    /// Retorna o tamanho de página em bytes utilizado para converter `RSS` em bytes.
    #[must_use]
    pub const fn page_size(&self) -> u64 {
        self.page_size
    }
}

impl ProcSource for ProcfsSource {
    fn process(&self, pid: u32) -> PtyResult<Option<ProcessInfo>> {
        read_process_info(pid, self.page_size)
    }

    fn all_pids(&self) -> PtyResult<Vec<u32>> {
        let entries = std::fs::read_dir("/proc")
            .map_err(|e| PtyError::ProcUnavailable(format!("falha ao listar /proc: {e}")))?;

        let mut pids = Vec::new();
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str()
                && let Ok(pid) = name.parse::<u32>()
            {
                pids.push(pid);
            }
        }
        pids.sort_unstable();
        Ok(pids)
    }

    fn identity(&self, pid: u32) -> PtyResult<Option<ProcessIdentity>> {
        read_process_identity(pid, &self.boot_id, self.page_size)
    }

    fn boot_id(&self) -> &str {
        &self.boot_id
    }

    fn cgroup_id(&self, pid: u32) -> Option<u64> {
        process_cgroup_inode(pid)
    }
}

/// Fonte em memória de processos para testes unitários determinísticos.
#[derive(Debug)]
pub struct FakeProcSource {
    boot_id: String,
    processes: Mutex<HashMap<u32, ProcessInfo>>,
    cgroups: Mutex<HashMap<u32, u64>>,
    restricted_pids: Mutex<HashSet<u32>>,
    unavailable: Mutex<bool>,
}

impl Default for FakeProcSource {
    fn default() -> Self {
        Self::new("fake-boot-id")
    }
}

impl FakeProcSource {
    /// Cria um novo `FakeProcSource` com o `boot_id` informado.
    #[must_use]
    pub fn new(boot_id: impl Into<String>) -> Self {
        Self {
            boot_id: boot_id.into(),
            processes: Mutex::new(HashMap::new()),
            cgroups: Mutex::new(HashMap::new()),
            restricted_pids: Mutex::new(HashSet::new()),
            unavailable: Mutex::new(false),
        }
    }

    /// Insere ou atualiza um `ProcessInfo` na tabela em memória.
    pub fn insert(&self, info: ProcessInfo) {
        self.processes.lock().insert(info.pid, info);
    }

    /// Define o `cgroup_id` associado a um `pid`.
    pub fn set_cgroup(&self, pid: u32, cgroup_id: u64) {
        self.cgroups.lock().insert(pid, cgroup_id);
    }

    /// Remove um `pid` da tabela de processos e de cgroups.
    pub fn remove(&self, pid: u32) -> Option<ProcessInfo> {
        self.cgroups.lock().remove(&pid);
        self.restricted_pids.lock().remove(&pid);
        self.processes.lock().remove(&pid)
    }

    /// Marca ou desmarca um `pid` como inacessível por restrição de permissão (`EACCES`).
    pub fn set_restricted(&self, pid: u32, restricted: bool) {
        if restricted {
            self.restricted_pids.lock().insert(pid);
        } else {
            self.restricted_pids.lock().remove(&pid);
        }
    }

    /// Simula indisponibilidade global de `/proc`.
    pub fn set_unavailable(&self, unavailable: bool) {
        *self.unavailable.lock() = unavailable;
    }
}

impl ProcSource for FakeProcSource {
    fn process(&self, pid: u32) -> PtyResult<Option<ProcessInfo>> {
        if *self.unavailable.lock() {
            return Err(PtyError::ProcUnavailable(
                "/proc simulado indisponível".into(),
            ));
        }
        if self.restricted_pids.lock().contains(&pid) {
            return Err(PtyError::ProcUnavailable(format!(
                "permissão negada ao ler /proc/{pid}"
            )));
        }
        Ok(self.processes.lock().get(&pid).cloned())
    }

    fn all_pids(&self) -> PtyResult<Vec<u32>> {
        if *self.unavailable.lock() {
            return Err(PtyError::ProcUnavailable(
                "/proc simulado indisponível".into(),
            ));
        }
        let mut pids: Vec<u32> = self.processes.lock().keys().copied().collect();
        pids.sort_unstable();
        Ok(pids)
    }

    fn identity(&self, pid: u32) -> PtyResult<Option<ProcessIdentity>> {
        if *self.unavailable.lock() {
            return Err(PtyError::ProcUnavailable(
                "/proc simulado indisponível".into(),
            ));
        }
        if self.restricted_pids.lock().contains(&pid) {
            return Err(PtyError::ProcUnavailable(format!(
                "permissão negada ao ler identidade de /proc/{pid}"
            )));
        }
        Ok(self.processes.lock().get(&pid).map(|info| ProcessIdentity {
            boot_id: self.boot_id.clone(),
            pid: info.pid,
            start_time: info.start_time,
        }))
    }

    fn boot_id(&self) -> &str {
        &self.boot_id
    }

    fn cgroup_id(&self, pid: u32) -> Option<u64> {
        self.cgroups.lock().get(&pid).copied()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::Arc;

    use nix::sys::signal::Signal;

    use super::*;
    use crate::proc::{PidfdSignaler, RecordingSignaler, Signaler};
    use crate::types::{ProcState, U64Decimal};

    fn sample_process(
        proc_pid: u32,
        parent_pid: u32,
        group_id: u32,
        start_time: u64,
        comm: &str,
    ) -> ProcessInfo {
        ProcessInfo {
            pid: proc_pid,
            ppid: parent_pid,
            pgid: group_id,
            sid: 100,
            tpgid: i32::try_from(group_id).unwrap(),
            comm: comm.to_string(),
            cmdline: vec![comm.to_string()],
            exe: Some(format!("/usr/bin/{comm}")),
            cwd: Some("/tmp".to_string()),
            state: ProcState::Running,
            start_time: U64Decimal(start_time),
            rss_bytes: Some(U64Decimal(4096)),
            threads: 1,
        }
    }

    #[test]
    fn pidfd_signaler_rejects_recycled_pid_and_foreign_cgroup_ca27() {
        let self_pid = std::process::id();
        let fake = Arc::new(FakeProcSource::new("boot-1"));
        fake.insert(sample_process(self_pid, 1, self_pid, 5000, "bash"));
        fake.set_cgroup(self_pid, 777);

        let signaler = PidfdSignaler::new(fake.clone());

        // 1. start_time divergente (PID reciclado) -> pty.pid_not_owned (CA-27)
        let stale_identity = ProcessIdentity {
            boot_id: "boot-1".to_string(),
            pid: self_pid,
            start_time: U64Decimal(1234),
        };
        let err = signaler
            .signal(&stale_identity, Signal::SIGTERM)
            .unwrap_err();
        assert_eq!(err.code(), "pty.pid_not_owned");

        // 2. boot_id divergente -> pty.pid_not_owned
        let wrong_boot = ProcessIdentity {
            boot_id: "boot-old".to_string(),
            pid: self_pid,
            start_time: U64Decimal(5000),
        };
        let err_boot = signaler.signal(&wrong_boot, Signal::SIGTERM).unwrap_err();
        assert_eq!(err_boot.code(), "pty.pid_not_owned");

        // 3. Processo fora do cgroup do PTY -> pty.pid_not_owned (CA-27)
        let valid_identity = ProcessIdentity {
            boot_id: "boot-1".to_string(),
            pid: self_pid,
            start_time: U64Decimal(5000),
        };
        let err_cgroup = signaler
            .signal_in_cgroup(&valid_identity, 999, Signal::SIGTERM)
            .unwrap_err();
        assert_eq!(err_cgroup.code(), "pty.pid_not_owned");

        // 4. Processo que não pertence ao PTY (ausente) em signal_in_cgroup -> pty.pid_not_owned
        let unknown_identity = ProcessIdentity {
            boot_id: "boot-1".to_string(),
            pid: 999_998,
            start_time: U64Decimal(100),
        };
        let err_unknown = signaler
            .signal_in_cgroup(&unknown_identity, 777, Signal::SIGTERM)
            .unwrap_err();
        assert_eq!(err_unknown.code(), "pty.pid_not_owned");
    }

    #[test]
    fn recording_signaler_tracks_signals_and_enforces_cgroup_ownership() {
        let fake = Arc::new(FakeProcSource::new("boot-test"));
        fake.insert(sample_process(200, 100, 200, 900, "sleep"));
        fake.set_cgroup(200, 42);

        let rec = RecordingSignaler::with_proc_source(fake);
        let id = ProcessIdentity {
            boot_id: "boot-test".to_string(),
            pid: 200,
            start_time: U64Decimal(900),
        };

        assert!(rec.signal_in_cgroup(&id, 42, Signal::SIGINT).is_ok());
        assert_eq!(rec.sent_signals().len(), 1);
        assert_eq!(rec.sent_signals()[0], (id.clone(), Signal::SIGINT));

        let err = rec.signal_in_cgroup(&id, 99, Signal::SIGTERM).unwrap_err();
        assert_eq!(err.code(), "pty.pid_not_owned");
        assert_eq!(rec.sent_signals().len(), 1);
    }
}
