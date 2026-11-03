//! Construção da árvore hierárquica de processos (`ProcessNode`) e relatório (`PtyProcessReport`).

use std::collections::{BTreeSet, HashMap, HashSet};

use mt_core::ids::PtyId;

use crate::error::PtyResult;
use crate::proc::ProcSource;
use crate::proc::foreground::resolve_foreground_process;
use crate::types::{
    ProcessIdentity, ProcessInfo, ProcessNode, PtyPids, PtyProcessReport, U64Decimal,
};

/// Constrói a árvore hierárquica `ppid -> filhos` enraizada no processo principal do shell.
///
/// Retorna `None` se o processo do shell não estiver presente em `candidates` com o mesmo `start_time`.
#[must_use]
pub fn build_process_tree(
    shell_identity: &ProcessIdentity,
    candidates: &[ProcessInfo],
) -> Option<ProcessNode> {
    let root_info = candidates
        .iter()
        .find(|p| p.pid == shell_identity.pid && p.start_time == shell_identity.start_time)?;

    let mut children_by_ppid: HashMap<u32, Vec<&ProcessInfo>> = HashMap::new();
    for proc_info in candidates {
        if proc_info.pid == shell_identity.pid && proc_info.start_time == shell_identity.start_time
        {
            continue;
        }
        if proc_info.pid == proc_info.ppid {
            continue;
        }
        children_by_ppid
            .entry(proc_info.ppid)
            .or_default()
            .push(proc_info);
    }

    for list in children_by_ppid.values_mut() {
        list.sort_by_key(|p| p.pid);
    }

    let mut visited = HashSet::new();
    visited.insert(root_info.pid);
    Some(build_node_recursive(
        root_info,
        &children_by_ppid,
        &mut visited,
    ))
}

fn build_node_recursive(
    info: &ProcessInfo,
    children_by_ppid: &HashMap<u32, Vec<&ProcessInfo>>,
    visited: &mut HashSet<u32>,
) -> ProcessNode {
    let mut children = Vec::new();
    if let Some(direct_children) = children_by_ppid.get(&info.pid) {
        for &child in direct_children {
            if visited.insert(child.pid) {
                children.push(build_node_recursive(child, children_by_ppid, visited));
            }
        }
    }
    ProcessNode {
        info: info.clone(),
        children,
    }
}

/// Constrói o relatório completo de supervisão de processos de um PTY (`PtyProcessReport`).
///
/// Processos que encerram entre a listagem de `cgroup_pids` e a leitura de `/proc/<pid>`
/// são omitidos silenciosamente sem gerar erro (CA-26).
pub fn build_process_report(
    pty_id: PtyId,
    shell_identity: &ProcessIdentity,
    pids: PtyPids,
    cgroup_pids: &[u32],
    proc_source: &dyn ProcSource,
    sampled_at_ms: u64,
) -> PtyResult<PtyProcessReport> {
    let mut unique_pids = BTreeSet::new();
    unique_pids.insert(shell_identity.pid);
    for &pid in cgroup_pids {
        unique_pids.insert(pid);
    }

    let mut owned_processes = Vec::with_capacity(unique_pids.len());
    let mut partial = false;

    for pid in unique_pids {
        match proc_source.process(pid) {
            Ok(Some(info)) => {
                if pid == shell_identity.pid && info.start_time != shell_identity.start_time {
                    // O PID original do shell terminou e foi reutilizado por outro processo.
                    continue;
                }
                owned_processes.push(info);
            }
            Ok(None) => {
                // O processo encerrou entre a listagem do cgroup e a leitura de /proc/<pid> (CA-26).
            }
            Err(err) => {
                // Se nem mesmo `/proc` estiver disponível globalmente, propaga ProcUnavailable.
                if pid == shell_identity.pid && proc_source.all_pids().is_err() {
                    return Err(err);
                }
                partial = true;
            }
        }
    }

    let shell = owned_processes
        .iter()
        .find(|p| p.pid == shell_identity.pid && p.start_time == shell_identity.start_time)
        .cloned();

    let tree = build_process_tree(shell_identity, &owned_processes);

    let extra_count = if shell.is_some() {
        owned_processes.len().saturating_sub(1)
    } else {
        owned_processes.len()
    };
    let descendant_count = u32::try_from(extra_count).unwrap_or(u32::MAX);

    let foreground =
        resolve_foreground_process(pids.foreground_pgid, &owned_processes, proc_source);

    Ok(PtyProcessReport {
        pty_id,
        pids,
        shell,
        foreground,
        tree,
        owned_processes,
        partial,
        descendant_count,
        sampled_at_ms: U64Decimal(sampled_at_ms),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::proc::FakeProcSource;
    use crate::types::ProcState;

    fn make_proc(
        proc_pid: u32,
        parent_pid: u32,
        group_id: u32,
        start_time: u64,
        comm: &str,
        cmdline: &[&str],
    ) -> ProcessInfo {
        ProcessInfo {
            pid: proc_pid,
            ppid: parent_pid,
            pgid: group_id,
            sid: 100,
            tpgid: i32::try_from(group_id).unwrap(),
            comm: comm.to_string(),
            cmdline: cmdline.iter().map(|s| (*s).to_string()).collect(),
            exe: Some(format!("/usr/bin/{comm}")),
            cwd: Some("/tmp".to_string()),
            state: ProcState::Sleeping,
            start_time: U64Decimal(start_time),
            rss_bytes: Some(U64Decimal(4096)),
            threads: 1,
        }
    }

    #[test]
    fn builds_three_level_process_tree_and_omits_exited_processes_ca25_ca26() {
        let fake = FakeProcSource::new("boot-xyz");

        // 3 níveis: sh (100) -> sh (200) -> sleep (300), mais um daemon reparentado (400 -> ppid 1)
        let sh_root = make_proc(100, 1, 100, 1000, "sh", &["sh"]);
        let sh_sub = make_proc(200, 100, 200, 1100, "sh", &["sh", "-c", "sleep 30"]);
        let sleep_leaf = make_proc(300, 200, 200, 1200, "sleep", &["sleep", "30"]);
        let daemon = make_proc(400, 1, 400, 1300, "daemon", &["daemon"]);

        fake.insert(sh_root);
        fake.insert(sh_sub);
        fake.insert(sleep_leaf);
        fake.insert(daemon);

        let shell_identity = ProcessIdentity {
            boot_id: "boot-xyz".to_string(),
            pid: 100,
            start_time: U64Decimal(1000),
        };
        let pids = PtyPids {
            shell_pid: 100,
            pgid: 100,
            sid: 100,
            foreground_pgid: Some(200),
        };

        // O PID 999 está na listagem do cgroup mas já saiu antes da leitura de /proc/999 (CA-26)
        let report = build_process_report(
            PtyId::new(),
            &shell_identity,
            pids,
            &[100, 200, 300, 400, 999],
            &fake,
            1_700_000_000_000,
        )
        .unwrap();

        assert!(!report.partial);
        assert_eq!(report.owned_processes.len(), 4);
        assert_eq!(report.descendant_count, 3);
        assert_eq!(report.shell.as_ref().unwrap().pid, 100);

        // Foreground deve ser o líder do PGID 200 ou membro vivo
        let fg = report.foreground.as_ref().unwrap();
        assert_eq!(fg.pid, 200);

        // Verifica os 3 níveis na árvore: 100 -> 200 -> 300
        let tree = report.tree.as_ref().unwrap();
        assert_eq!(tree.info.pid, 100);
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].info.pid, 200);
        assert_eq!(tree.children[0].children.len(), 1);
        assert_eq!(tree.children[0].children[0].info.pid, 300);
        assert_eq!(
            tree.children[0].children[0].info.cmdline,
            vec!["sleep", "30"]
        );
    }
}
