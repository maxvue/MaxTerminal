//! Fonte de árvores de processos por painel/PTY e leitura segura de metadados `/proc/<pid>`.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use mt_core::ids::{PaneId, PtyId, TabId};
use mt_db::sanitizer::sanitize;

use crate::error::{PortsError, PortsResult};
use crate::fdmap::{MAX_FDS_PER_PID, collect_process_socket_inodes};
use crate::procnet::{RawTcpListenEntry, parse_proc_net_tcp};
use crate::types::IpFamily;

/// Limite máximo de caracteres para `process_name` (`/proc/<pid>/comm`).
pub const MAX_PROCESS_NAME_CHARS: usize = 64;

/// Limite máximo de caracteres para `cmdline_preview` (`/proc/<pid>/cmdline`).
pub const MAX_CMDLINE_PREVIEW_CHARS: usize = 160;

/// Limite máximo de PIDs inspecionados na árvore de um PTY em um ciclo.
pub const MAX_PIDS_PER_PTY: usize = 256;

/// Vínculo de um painel ativo com seu PTY e aba proprietária.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivePaneTarget {
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel.
    pub pane_id: PaneId,
    /// Identificador do PTY ativo no painel.
    pub pty_id: PtyId,
}

/// Processo rastreado pertencente à árvore de um PTY.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedPaneProcess {
    /// PID numérico no namespace atual.
    pub pid: u32,
    /// `starttime` em clock ticks desde o boot (`/proc/<pid>/stat` campo 22).
    pub start_time_ticks: u64,
    /// Nome curto do executável (até 64 chars).
    pub process_name: String,
    /// Prévia sanitizada da linha de comando (até 160 chars).
    pub cmdline_preview: Option<String>,
    /// Profundidade na árvore do PTY (`0` = líder da sessão).
    pub depth: usize,
}

/// Abstração da fonte de processos e tabelas `/proc/net/tcp{,6}` para permitir testes determinísticos.
pub trait ProcessTreeSource: Send + Sync + 'static {
    /// Lista os painéis atualmente ativos com seus respectivos PTYs.
    fn active_panes(&self) -> Vec<ActivePaneTarget>;

    /// Retorna os processos pertencentes ao PTY indicado (até `MAX_PIDS_PER_PTY`).
    fn processes_for_pty(&self, pty_id: PtyId) -> Vec<TrackedPaneProcess>;

    /// Retorna os inodes de sockets abertos por `pid`.
    fn socket_inodes_for_pid(&self, pid: u32) -> HashSet<u64>;

    /// Retorna se `pid` ainda existe com o mesmo `start_time_ticks`.
    fn verify_process_alive(&self, pid: u32, expected_start_ticks: u64) -> bool;

    /// Lê todas as entradas `TCP_LISTEN` de `/proc/net/tcp` e `/proc/net/tcp6` filtradas pelo UID atual.
    fn read_listen_entries(&self) -> PortsResult<Vec<RawTcpListenEntry>>;
}

/// Helper para ler metadados de `/proc/<pid>` em sistemas Linux reais ou diretórios sintéticos de teste.
#[derive(Debug, Clone)]
pub struct ProcfsTreeSource {
    proc_root: PathBuf,
    uid: u32,
}

impl Default for ProcfsTreeSource {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcfsTreeSource {
    /// Cria um leitor apontando para `/proc` real e o `uid` efetivo do processo atual.
    #[must_use]
    pub fn new() -> Self {
        let uid = nix::unistd::geteuid().as_raw();
        Self {
            proc_root: PathBuf::from("/proc"),
            uid,
        }
    }

    /// Cria um leitor sobre uma raiz `/proc` customizada e `uid` explícito (para testes).
    #[must_use]
    pub fn with_root(proc_root: impl Into<PathBuf>, uid: u32) -> Self {
        Self {
            proc_root: proc_root.into(),
            uid,
        }
    }

    /// Retorna a raiz `/proc` configurada.
    #[must_use]
    pub fn proc_root(&self) -> &Path {
        &self.proc_root
    }

    /// Retorna o UID filtrado.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// Lê `/proc/net/tcp` e `/proc/net/tcp6` a partir de `proc_root`.
    pub fn read_proc_net_listen(&self) -> PortsResult<Vec<RawTcpListenEntry>> {
        let tcp4_path = self.proc_root.join("net").join("tcp");
        let tcp6_path = self.proc_root.join("net").join("tcp6");

        let tcp4_content =
            fs::read_to_string(&tcp4_path).map_err(|err| PortsError::ProcfsUnavailable {
                reason: format!("Falha ao ler {}: {err}", tcp4_path.display()),
            })?;

        let mut entries = parse_proc_net_tcp(&tcp4_content, IpFamily::Ipv4, Some(self.uid));

        if let Ok(tcp6_content) = fs::read_to_string(&tcp6_path) {
            let mut v6 = parse_proc_net_tcp(&tcp6_content, IpFamily::Ipv6, Some(self.uid));
            entries.append(&mut v6);
        }

        Ok(entries)
    }

    /// Lê os inodes de socket de `pid` em `<proc_root>/<pid>/fd`.
    #[must_use]
    pub fn socket_inodes_for_pid(&self, pid: u32) -> HashSet<u64> {
        collect_process_socket_inodes(&self.proc_root, pid, MAX_FDS_PER_PID)
    }

    /// Lê `starttime` (campo 22 de `/proc/<pid>/stat`), `comm` e `cmdline` sanitizado.
    #[must_use]
    pub fn read_process_metadata(&self, pid: u32, depth: usize) -> Option<TrackedPaneProcess> {
        let pid_dir = self.proc_root.join(pid.to_string());
        let stat_raw = fs::read_to_string(pid_dir.join("stat")).ok()?;
        let (comm_from_stat, start_time_ticks) = parse_proc_stat_comm_and_starttime(&stat_raw)?;

        let comm = fs::read_to_string(pid_dir.join("comm"))
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or(comm_from_stat);

        let process_name: String = comm.chars().take(MAX_PROCESS_NAME_CHARS).collect();

        let cmdline_preview = fs::read(pid_dir.join("cmdline"))
            .ok()
            .and_then(|bytes| format_and_sanitize_cmdline(&bytes));

        Some(TrackedPaneProcess {
            pid,
            start_time_ticks,
            process_name,
            cmdline_preview,
            depth,
        })
    }

    /// Verifica se o processo `pid` ainda existe em `proc_root` com o mesmo `start_time_ticks`.
    #[must_use]
    pub fn verify_alive(&self, pid: u32, expected_start_ticks: u64) -> bool {
        let stat_path = self.proc_root.join(pid.to_string()).join("stat");
        let Ok(stat_raw) = fs::read_to_string(stat_path) else {
            return false;
        };
        let Some((_, current_ticks)) = parse_proc_stat_comm_and_starttime(&stat_raw) else {
            return false;
        };
        current_ticks == expected_start_ticks
    }
}

/// Extrai `(comm, starttime_ticks)` de uma linha de `/proc/<pid>/stat`.
#[must_use]
pub fn parse_proc_stat_comm_and_starttime(stat_line: &str) -> Option<(String, u64)> {
    let open_paren = stat_line.find('(')?;
    let close_paren = stat_line.rfind(')')?;
    if close_paren <= open_paren {
        return None;
    }
    let comm = stat_line[open_paren + 1..close_paren].to_string();
    let after_comm = stat_line.get(close_paren + 2..)?;
    // Após `(comm) `, o primeiro campo é o campo 3 (`state`).
    // O campo 22 (`starttime`) está no índice `22 - 3 = 19` (0-based).
    let starttime_str = after_comm.split_whitespace().nth(19)?;
    let start_time_ticks = starttime_str.parse::<u64>().ok()?;
    Some((comm, start_time_ticks))
}

/// Converte bytes NUL-separated de `/proc/<pid>/cmdline`, sanitiza segredos e trunca em 160 chars.
#[must_use]
pub fn format_and_sanitize_cmdline(raw: &[u8]) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    let parts: Vec<String> = raw
        .split(|&b| b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let joined = parts.join(" ");
    let sanitized = sanitize(&joined);
    let trimmed = sanitized.trim();
    if trimmed.is_empty() {
        return None;
    }
    let preview: String = trimmed.chars().take(MAX_CMDLINE_PREVIEW_CHARS).collect();
    Some(preview)
}
