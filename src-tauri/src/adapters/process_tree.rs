//! Adaptador `WorkspaceProcessTreeSource` que une `WorkspaceManager`, `PtyManager` e `ProcfsTreeSource`
//! para alimentar o `PortsService` e o `ActiveWorktreeGuard`.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::Arc;

use mt_core::ids::PtyId;
use mt_git::{ActiveWorktreeGuard, GitService};
use mt_ports::{
    ActivePaneTarget, MAX_PIDS_PER_PTY, PortsResult, ProcessTreeSource, ProcfsTreeSource,
    RawTcpListenEntry, TrackedPaneProcess,
};
use mt_pty::PtyManager;

use crate::workspace::WorkspaceManager;

/// Implementação de [`ProcessTreeSource`] integrada ao [`WorkspaceManager`] e [`PtyManager`].
pub struct WorkspaceProcessTreeSource {
    workspace: WorkspaceManager,
    pty_manager: Arc<PtyManager>,
    procfs: ProcfsTreeSource,
}

impl std::fmt::Debug for WorkspaceProcessTreeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceProcessTreeSource")
            .finish_non_exhaustive()
    }
}

impl WorkspaceProcessTreeSource {
    /// Cria uma nova instância conectada ao workspace e ao gerenciador de PTYs.
    #[must_use]
    pub fn new(workspace: WorkspaceManager, pty_manager: Arc<PtyManager>) -> Arc<Self> {
        Arc::new(Self {
            workspace,
            pty_manager,
            procfs: ProcfsTreeSource::new(),
        })
    }
}

impl ProcessTreeSource for WorkspaceProcessTreeSource {
    fn active_panes(&self) -> Vec<ActivePaneTarget> {
        self.workspace
            .active_pane_targets()
            .into_iter()
            .map(|(tab_id, pane_id, pty_id, _cwd)| ActivePaneTarget {
                tab_id,
                pane_id,
                pty_id,
            })
            .collect()
    }

    fn processes_for_pty(&self, pty_id: PtyId) -> Vec<TrackedPaneProcess> {
        let Ok(identities) = self.pty_manager.tracked_processes(pty_id) else {
            return Vec::new();
        };
        let mut list = Vec::with_capacity(identities.len().min(MAX_PIDS_PER_PTY));
        for (idx, id) in identities.into_iter().take(MAX_PIDS_PER_PTY).enumerate() {
            if let Some(mut meta) = self.procfs.read_process_metadata(id.pid, idx) {
                meta.start_time_ticks = id.start_time.0;
                list.push(meta);
            }
        }
        list
    }

    fn socket_inodes_for_pid(&self, pid: u32) -> HashSet<u64> {
        self.procfs.socket_inodes_for_pid(pid)
    }

    fn verify_process_alive(&self, pid: u32, expected_start_ticks: u64) -> bool {
        self.procfs.verify_alive(pid, expected_start_ticks)
    }

    fn read_listen_entries(&self) -> PortsResult<Vec<RawTcpListenEntry>> {
        self.procfs.read_proc_net_listen()
    }
}

/// Guarda que verifica se uma worktree está em uso por algum painel do [`WorkspaceManager`] ou [`GitService`].
pub struct WorkspaceActiveWorktreeGuard {
    workspace: WorkspaceManager,
    git_service: GitService,
}

impl std::fmt::Debug for WorkspaceActiveWorktreeGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkspaceActiveWorktreeGuard")
            .finish_non_exhaustive()
    }
}

impl WorkspaceActiveWorktreeGuard {
    /// Cria um novo guarda de worktrees ativas.
    #[must_use]
    pub fn new(workspace: WorkspaceManager, git_service: GitService) -> Arc<Self> {
        Arc::new(Self {
            workspace,
            git_service,
        })
    }
}

impl ActiveWorktreeGuard for WorkspaceActiveWorktreeGuard {
    fn is_worktree_in_use(&self, worktree_path: &Path) -> bool {
        if self.git_service.is_worktree_in_use(worktree_path) {
            return true;
        }
        let canonical =
            fs::canonicalize(worktree_path).unwrap_or_else(|_| worktree_path.to_path_buf());
        self.workspace
            .active_pane_targets()
            .into_iter()
            .any(|(_tab_id, _pane_id, _pty_id, cwd)| {
                let pane_path =
                    fs::canonicalize(&cwd).unwrap_or_else(|_| std::path::PathBuf::from(&cwd));
                pane_path.starts_with(&canonical)
            })
    }
}
