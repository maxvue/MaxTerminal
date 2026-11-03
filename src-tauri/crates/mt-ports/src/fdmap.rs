//! Mapeamento de descritores `/proc/<pid>/fd/*` para inodes de sockets (`socket:[<inode>]`).

use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::path::Path;

/// Limite máximo de descritores inspecionados por processo em um ciclo de varredura.
pub const MAX_FDS_PER_PID: usize = 1024;

/// Extrai o inode numérico de um alvo de `readlink` em `/proc/<pid>/fd/<n>`
/// nos formatos `socket:[<inode>]` ou `[0000]:<inode>`.
#[must_use]
pub fn parse_socket_inode_link(target: &OsStr) -> Option<u64> {
    let s = target.to_str()?;
    if let Some(rest) = s.strip_prefix("socket:[") {
        let digits = rest.strip_suffix(']')?;
        return digits.parse::<u64>().ok().filter(|&v| v > 0);
    }
    if let Some(rest) = s.strip_prefix("[0000]:") {
        return rest.parse::<u64>().ok().filter(|&v| v > 0);
    }
    None
}

/// Varre `/proc/<pid>/fd` (ou um diretório equivalente em testes) e retorna o conjunto de inodes
/// de sockets abertos pelo processo, limitado a `max_fds` entradas.
#[must_use]
pub fn collect_process_socket_inodes(proc_root: &Path, pid: u32, max_fds: usize) -> HashSet<u64> {
    let fd_dir = proc_root.join(pid.to_string()).join("fd");
    let Ok(read_dir) = fs::read_dir(fd_dir) else {
        return HashSet::new();
    };

    let mut inodes = HashSet::new();
    for entry in read_dir.take(max_fds).flatten() {
        if let Ok(link_target) = fs::read_link(entry.path())
            && let Some(inode) = parse_socket_inode_link(link_target.as_os_str())
        {
            inodes.insert(inode);
        }
    }
    inodes
}
