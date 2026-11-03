//! Parsing resiliente de `/proc/<pid>/stat` e leitura anti-TOCTOU de `ProcessInfo`.

use crate::error::{PtyError, PtyResult};
use crate::types::{ProcState, ProcessIdentity, ProcessInfo, U64Decimal};

/// Converte o caractere de estado de `/proc/<pid>/stat` para `ProcState`.
#[must_use]
pub const fn map_proc_state(state_char: char) -> ProcState {
    match state_char {
        'R' => ProcState::Running,
        'S' => ProcState::Sleeping,
        'D' => ProcState::DiskSleep,
        'T' => ProcState::Stopped,
        't' => ProcState::TracingStop,
        'Z' => ProcState::Zombie,
        'X' | 'x' => ProcState::Dead,
        'I' => ProcState::Idle,
        _ => ProcState::Unknown,
    }
}

/// Campos estruturados extraídos de `/proc/<pid>/stat`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedStat {
    /// PID do processo (campo 1).
    pub pid: u32,
    /// Nome curto do executável entre parênteses (campo 2).
    pub comm: String,
    /// Estado atual no escalonador do kernel (campo 3).
    pub state: ProcState,
    /// PID do processo pai (campo 4).
    pub ppid: u32,
    /// Process Group ID (campo 5).
    pub pgid: u32,
    /// Session ID (campo 6).
    pub sid: u32,
    /// Foreground PGID do terminal controlador (campo 8).
    pub tpgid: i32,
    /// Quantidade de threads do processo (campo 20).
    pub threads: u32,
    /// Ticks de clock desde o boot no início do processo (campo 22).
    pub start_time: u64,
    /// Memória residente (`RSS`) em bytes (`rss_pages * page_size`, campo 24).
    pub rss_bytes: u64,
}

/// Extrai os campos de uma linha bruta de `/proc/<pid>/stat`, lidando com segurança com espaços
/// e parênteses aninhados dentro de `comm`.
pub fn parse_proc_stat_line(line: &str, page_size: u64) -> PtyResult<ParsedStat> {
    let open_paren = line.find('(').ok_or_else(|| {
        PtyError::ProcUnavailable("formato inválido em /proc/<pid>/stat: '(' ausente".into())
    })?;
    let close_paren = line.rfind(')').ok_or_else(|| {
        PtyError::ProcUnavailable("formato inválido em /proc/<pid>/stat: ')' ausente".into())
    })?;

    if close_paren <= open_paren {
        return Err(PtyError::ProcUnavailable(
            "formato inválido em /proc/<pid>/stat: delimitadores de comm invertidos".into(),
        ));
    }

    let pid_str = line[..open_paren].trim();
    let pid = pid_str.parse::<u32>().map_err(|e| {
        PtyError::ProcUnavailable(format!(
            "PID inválido em /proc/<pid>/stat ('{pid_str}'): {e}"
        ))
    })?;

    let comm = line[open_paren + 1..close_paren].to_string();
    let rest: Vec<&str> = line[close_paren + 1..].split_whitespace().collect();

    // Após `(comm)`, o campo 3 está em rest[0] e o campo 24 (rss) está em rest[21].
    if rest.len() < 22 {
        return Err(PtyError::ProcUnavailable(format!(
            "campos insuficientes em /proc/{pid}/stat: esperados >= 22 após comm, obtidos {}",
            rest.len()
        )));
    }

    let state_char = rest[0].chars().next().unwrap_or('?');
    let state = map_proc_state(state_char);

    let parent_pid = parse_non_negative_u32(rest[1], "ppid")?;
    let group_id = parse_non_negative_u32(rest[2], "pgrp")?;
    let session_id = parse_non_negative_u32(rest[3], "session")?;
    let tpgid = rest[5].parse::<i32>().map_err(|e| {
        PtyError::ProcUnavailable(format!("tpgid inválido em /proc/{pid}/stat: {e}"))
    })?;
    let threads = parse_non_negative_u32(rest[17], "num_threads")?;
    let start_time = rest[19].parse::<u64>().map_err(|e| {
        PtyError::ProcUnavailable(format!("starttime inválido em /proc/{pid}/stat: {e}"))
    })?;
    let rss_pages = rest[21]
        .parse::<i64>()
        .map_err(|e| PtyError::ProcUnavailable(format!("rss inválido em /proc/{pid}/stat: {e}")))?;
    let rss_bytes = u64::try_from(rss_pages.max(0))
        .unwrap_or(0)
        .saturating_mul(page_size);

    Ok(ParsedStat {
        pid,
        comm,
        state,
        ppid: parent_pid,
        pgid: group_id,
        sid: session_id,
        tpgid,
        threads,
        start_time,
        rss_bytes,
    })
}

fn parse_non_negative_u32(raw: &str, field_name: &str) -> PtyResult<u32> {
    let val = raw.parse::<i64>().map_err(|e| {
        PtyError::ProcUnavailable(format!("campo {field_name} inválido ('{raw}'): {e}"))
    })?;
    Ok(u32::try_from(val.max(0)).unwrap_or(0))
}

/// Converte uma estrutura `procfs::process::Stat` para `ParsedStat`.
#[must_use]
pub fn parsed_stat_from_procfs(stat: &procfs::process::Stat, page_size: u64) -> ParsedStat {
    let proc_pid = u32::try_from(stat.pid.max(0)).unwrap_or(0);
    let parent_pid = u32::try_from(stat.ppid.max(0)).unwrap_or(0);
    let group_id = u32::try_from(stat.pgrp.max(0)).unwrap_or(0);
    let session_id = u32::try_from(stat.session.max(0)).unwrap_or(0);
    let threads = u32::try_from(stat.num_threads.max(0)).unwrap_or(1);
    let rss_bytes = stat.rss.saturating_mul(page_size);

    ParsedStat {
        pid: proc_pid,
        comm: stat.comm.clone(),
        state: map_proc_state(stat.state),
        ppid: parent_pid,
        pgid: group_id,
        sid: session_id,
        tpgid: stat.tpgid,
        threads,
        start_time: stat.starttime,
        rss_bytes,
    }
}

fn is_proc_gone(err: &procfs::ProcError) -> bool {
    match err {
        procfs::ProcError::NotFound(_) => true,
        procfs::ProcError::Io(io_err, _) => {
            io_err.kind() == std::io::ErrorKind::NotFound
                || io_err.raw_os_error() == Some(rustix::io::Errno::SRCH.raw_os_error())
        }
        _ => false,
    }
}

/// Lê a identidade `(boot_id, pid, start_time)` de `/proc/<pid>/stat`.
///
/// Retorna `Ok(None)` caso o processo já tenha encerrado (`ENOENT`/`ESRCH`).
pub fn read_process_identity(
    pid: u32,
    boot_id: &str,
    page_size: u64,
) -> PtyResult<Option<ProcessIdentity>> {
    let Ok(raw_pid) = i32::try_from(pid) else {
        return Ok(None);
    };
    if raw_pid <= 0 {
        return Ok(None);
    }

    let proc = match procfs::process::Process::new(raw_pid) {
        Ok(p) => p,
        Err(err) if is_proc_gone(&err) => return Ok(None),
        Err(err) => return Err(PtyError::ProcUnavailable(err.to_string())),
    };

    let stat = match proc.stat() {
        Ok(s) => s,
        Err(err) if is_proc_gone(&err) => return Ok(None),
        Err(err) => return Err(PtyError::ProcUnavailable(err.to_string())),
    };

    let parsed = parsed_stat_from_procfs(&stat, page_size);
    Ok(Some(ProcessIdentity {
        boot_id: boot_id.to_string(),
        pid: parsed.pid,
        start_time: U64Decimal(parsed.start_time),
    }))
}

/// Lê os metadados completos de `/proc/<pid>` com dupla verificação de `start_time`
/// antes e depois de `cmdline`, `exe` e `cwd` para impedir mistura de dados em reciclagem de PID.
pub fn read_process_info(pid: u32, page_size: u64) -> PtyResult<Option<ProcessInfo>> {
    let Ok(raw_pid) = i32::try_from(pid) else {
        return Ok(None);
    };
    if raw_pid <= 0 {
        return Ok(None);
    }

    let proc = match procfs::process::Process::new(raw_pid) {
        Ok(p) => p,
        Err(err) if is_proc_gone(&err) => return Ok(None),
        Err(err) => return Err(PtyError::ProcUnavailable(err.to_string())),
    };

    let stat_before = match proc.stat() {
        Ok(s) => s,
        Err(err) if is_proc_gone(&err) => return Ok(None),
        Err(err) => return Err(PtyError::ProcUnavailable(err.to_string())),
    };

    // Erros EACCES/EPERM ou desaparecimento de symlinks opcionais resultam em vazio/None
    // sem abortar a leitura do processo.
    let cmdline = proc.cmdline().ok().unwrap_or_default();
    let exe = proc.exe().ok().map(|p| p.to_string_lossy().into_owned());
    let cwd = proc.cwd().ok().map(|p| p.to_string_lossy().into_owned());

    let stat_after = match proc.stat() {
        Ok(s) => s,
        Err(err) if is_proc_gone(&err) => return Ok(None),
        Err(err) => return Err(PtyError::ProcUnavailable(err.to_string())),
    };

    if stat_after.starttime != stat_before.starttime {
        return Ok(None);
    }

    let parsed = parsed_stat_from_procfs(&stat_after, page_size);
    Ok(Some(ProcessInfo {
        pid: parsed.pid,
        ppid: parsed.ppid,
        pgid: parsed.pgid,
        sid: parsed.sid,
        tpgid: parsed.tpgid,
        comm: parsed.comm,
        cmdline,
        exe,
        cwd,
        state: parsed.state,
        start_time: U64Decimal(parsed.start_time),
        rss_bytes: Some(U64Decimal(parsed.rss_bytes)),
        threads: parsed.threads,
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn maps_all_proc_state_characters() {
        assert_eq!(map_proc_state('R'), ProcState::Running);
        assert_eq!(map_proc_state('S'), ProcState::Sleeping);
        assert_eq!(map_proc_state('D'), ProcState::DiskSleep);
        assert_eq!(map_proc_state('T'), ProcState::Stopped);
        assert_eq!(map_proc_state('t'), ProcState::TracingStop);
        assert_eq!(map_proc_state('Z'), ProcState::Zombie);
        assert_eq!(map_proc_state('X'), ProcState::Dead);
        assert_eq!(map_proc_state('x'), ProcState::Dead);
        assert_eq!(map_proc_state('I'), ProcState::Idle);
        assert_eq!(map_proc_state('?'), ProcState::Unknown);
    }

    #[test]
    fn parses_stat_line_with_spaces_and_nested_parentheses_in_comm() {
        let line = "4242 (meu (proc) de teste (v2)) S 1000 4240 1000 34816 4240 4194304 120 0 0 0 15 5 0 0 20 0 4 0 987654321 12345678 350 18446744073709551615 0 0 0 0 0 0 0 0 0 0 0 0 17 2 0 0 0 0 0";
        let parsed = parse_proc_stat_line(line, 4096).unwrap();

        assert_eq!(parsed.pid, 4242);
        assert_eq!(parsed.comm, "meu (proc) de teste (v2)");
        assert_eq!(parsed.state, ProcState::Sleeping);
        assert_eq!(parsed.ppid, 1000);
        assert_eq!(parsed.pgid, 4240);
        assert_eq!(parsed.sid, 1000);
        assert_eq!(parsed.tpgid, 4240);
        assert_eq!(parsed.threads, 4);
        assert_eq!(parsed.start_time, 987_654_321);
        assert_eq!(parsed.rss_bytes, 350 * 4096);
    }

    #[test]
    fn reads_current_process_info_and_handles_non_existent_pid() {
        let self_pid = std::process::id();
        let info = read_process_info(self_pid, 4096).unwrap().unwrap();
        assert_eq!(info.pid, self_pid);
        assert!(info.start_time.0 > 0);
        assert!(info.threads >= 1);
        assert!(info.cwd.is_some());

        let missing = read_process_info(u32::MAX - 1, 4096).unwrap();
        assert!(missing.is_none());
    }
}
