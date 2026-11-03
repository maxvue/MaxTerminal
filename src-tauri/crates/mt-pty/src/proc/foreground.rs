//! Resolução do grupo de processos em foreground (`tcgetpgrp`) e identificação do líder ativo.

use std::os::fd::BorrowedFd;

use crate::proc::ProcSource;
use crate::types::ProcessInfo;

/// Obtém o `PGID` em foreground do terminal associado ao descritor via `tcgetpgrp`.
#[must_use]
pub fn query_foreground_pgid(pty_fd: BorrowedFd<'_>) -> Option<u32> {
    let pid = rustix::termios::tcgetpgrp(pty_fd).ok()?;
    u32::try_from(pid.as_raw_nonzero().get()).ok()
}

/// Determina o processo representativo do grupo em foreground:
/// 1. O processo vivo cujo `pid == fg_pgid` (líder do grupo de processos).
/// 2. Se o líder já tiver encerrado, o membro vivo com `pgid == fg_pgid` que possua o maior `start_time`.
#[must_use]
pub fn resolve_foreground_process(
    fg_pgid: Option<u32>,
    owned_processes: &[ProcessInfo],
    proc_source: &dyn ProcSource,
) -> Option<ProcessInfo> {
    let pgid = fg_pgid.filter(|&g| g > 0)?;

    if let Some(leader) = owned_processes.iter().find(|p| p.pid == pgid) {
        return Some(leader.clone());
    }

    if let Some(newest_member) = owned_processes
        .iter()
        .filter(|p| p.pgid == pgid)
        .max_by_key(|p| (p.start_time.0, p.pid))
    {
        return Some(newest_member.clone());
    }

    proc_source
        .process(pgid)
        .ok()
        .flatten()
        .filter(|p| p.pgid == pgid)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::proc::FakeProcSource;
    use crate::types::{ProcState, U64Decimal};

    fn make_proc(
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
            exe: None,
            cwd: Some("/tmp".to_string()),
            state: ProcState::Sleeping,
            start_time: U64Decimal(start_time),
            rss_bytes: Some(U64Decimal(4096)),
            threads: 1,
        }
    }

    #[test]
    fn prefers_group_leader_when_alive() {
        let fake = FakeProcSource::default();
        let leader = make_proc(200, 100, 200, 1000, "bash");
        let child = make_proc(201, 200, 200, 1500, "sleep");
        let owned = vec![leader.clone(), child];

        let resolved = resolve_foreground_process(Some(200), &owned, &fake).unwrap();
        assert_eq!(resolved.pid, 200);
        assert_eq!(resolved.comm, "bash");
    }

    #[test]
    fn falls_back_to_member_with_highest_start_time_when_leader_exited() {
        let fake = FakeProcSource::default();
        let member_old = make_proc(301, 100, 300, 2000, "sh");
        let member_new = make_proc(302, 301, 300, 2500, "sleep");
        let owned = vec![member_old, member_new];

        let resolved = resolve_foreground_process(Some(300), &owned, &fake).unwrap();
        assert_eq!(resolved.pid, 302);
        assert_eq!(resolved.comm, "sleep");
    }
}
