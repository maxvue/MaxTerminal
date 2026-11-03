//! Adaptadores de portas (`TabStatePort`, `NotifyPort`, `PtyPort`, `WorkspacePort`, `GitPort`),
//! sinks Tauri e hooks de saída/shell para integração de `mt-agents` no binário Tauri.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mt_agents::AgentRuntimeIndex;
use mt_agents::attention::detector::{AttentionDetector, AttentionSink};
use mt_agents::attention::{AttentionSignal, AttentionSnapshot};
use mt_agents::detect::current_unix_ms;
use mt_agents::isolation::ensure_agent_templates_in_db;
use mt_agents::memory::CommandMemory;
use mt_agents::ports::{
    AttentionKind, ForegroundProc, GitPort, NotifyPort, ProjectRecord, PtyPort, RunBinding,
    TabStatePort, WorkspacePort,
};
use mt_agents::registry::AgentRegistry;
use mt_agents::skills::SyncResult;
use mt_agents::skills::watch::{SkillsChangedPayload, SkillsEventSink};
use mt_agents::telemetry::AgentTelemetry;
use mt_agents::telemetry::aggregator::TelemetrySink;
use mt_core::error::{ErrorCode, MtError, MtResult};
use mt_core::ids::{ProjectId, PtyId, TabId};
use mt_core::shell::{
    CommandFinished, CommandStarted, ShellCwdChangedEvent, ShellIntegrationModeEvent,
    TabStateRemovedEvent, TabStateSnapshot,
};
use mt_db::Db;
use mt_git::{GitContext, GitService, WorktreeCreateRequest, WorktreeEntry, WorktreeManager};
use mt_os::{AttentionReason, NotificationKind, OsIntegration};
use mt_projects::ProjectService;
use mt_pty::{OutputTap, PtyManager};
use mt_shell::{ShellEventSink, ShellService};
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::process::Command;

use crate::events::agents::{
    AgentAttentionTauriEvent, AgentTelemetryTauriEvent, MemoryUpdatedPayload,
    MemoryUpdatedTauriEvent, SkillsChangedTauriEvent, SkillsSyncCompletedTauriEvent,
};
use crate::workspace::WorkspaceManager;

/// Sink de telemetria que emite `agent://telemetry` via `AppHandle`.
pub struct TauriTelemetrySink {
    app: AppHandle,
}

impl TauriTelemetrySink {
    /// Cria uma nova instância vinculada ao `AppHandle`.
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl TelemetrySink for TauriTelemetrySink {
    fn emit_telemetry(&self, telemetry: &AgentTelemetry) {
        let _ = AgentTelemetryTauriEvent(telemetry.clone()).emit(&self.app);
    }
}

/// Sink de atenção que emite `agent://attention` via `AppHandle`.
pub struct TauriAttentionSink {
    app: AppHandle,
}

impl TauriAttentionSink {
    /// Cria uma nova instância vinculada ao `AppHandle`.
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl AttentionSink for TauriAttentionSink {
    fn emit(&self, snapshot: AttentionSnapshot) {
        let _ = AgentAttentionTauriEvent(snapshot).emit(&self.app);
    }
}

/// Sink de eventos `MaxSkills` que emite `skills://changed` e `skills://sync-completed` via `AppHandle`.
pub struct TauriSkillsEventSink {
    app: AppHandle,
}

impl TauriSkillsEventSink {
    /// Cria um novo sink de eventos `MaxSkills`.
    #[must_use]
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self { app })
    }
}

impl SkillsEventSink for TauriSkillsEventSink {
    fn emit_changed(&self, payload: SkillsChangedPayload) {
        let _ = SkillsChangedTauriEvent(payload).emit(&self.app);
    }

    fn emit_sync_completed(&self, result: SyncResult) {
        let _ = SkillsSyncCompletedTauriEvent(result).emit(&self.app);
    }
}

/// Adaptador de `TabStatePort` delegando ao `ShellService`.
pub struct TauriTabStateAdapter {
    shell: Arc<ShellService>,
}

impl TauriTabStateAdapter {
    /// Cria um novo adaptador de `TabStatePort`.
    #[must_use]
    pub fn new(shell: Arc<ShellService>) -> Arc<Self> {
        Arc::new(Self { shell })
    }
}

impl TabStatePort for TauriTabStateAdapter {
    fn attention_required(&self, run: &RunBinding, episode: u64) -> bool {
        let snap = mt_core::shell::AttentionSnapshot {
            source: format!("{}:{}:{}", run.agent.as_ref_id(), run.run_id, episode),
            message: format!("Agente {} aguardando intervenção", run.agent.display_name()),
            raised_at: current_unix_ms(),
            seen: false,
        };
        self.shell
            .set_tab_attention(run.tab_id, Some(snap))
            .is_some()
    }

    fn attention_resolved(&self, run: &RunBinding, _episode: u64) {
        let _ = self.shell.set_tab_attention(run.tab_id, None);
    }
}

/// Adaptador de `NotifyPort` delegando a `OsIntegration` e `ShellService`.
pub struct TauriNotifyAdapter {
    os: Arc<OsIntegration>,
    shell: Arc<ShellService>,
}

impl TauriNotifyAdapter {
    /// Cria um novo adaptador de `NotifyPort`.
    #[must_use]
    pub fn new(os: Arc<OsIntegration>, shell: Arc<ShellService>) -> Arc<Self> {
        Arc::new(Self { os, shell })
    }
}

#[async_trait]
impl NotifyPort for TauriNotifyAdapter {
    fn set_attention(&self, tab: TabId, kind: AttentionKind) {
        let reason = match kind {
            AttentionKind::AwaitingApproval => AttentionReason::AwaitingApproval,
            AttentionKind::AwaitingInput => AttentionReason::AwaitingInput,
        };
        let _ = self.os.set_attention(tab, Some(reason));
    }

    fn clear_attention(&self, tab: TabId) {
        let _ = self.os.set_attention(tab, None);
    }

    fn is_tab_focused_and_active(&self, tab: TabId) -> bool {
        self.shell
            .tab_state_get(tab)
            .ok()
            .is_some_and(|s| s.focused)
    }

    async fn notify(&self, tab: TabId, title: &str, body: &str) {
        let _ = self
            .os
            .notifier
            .notify_user(NotificationKind::AgentAwaitingInput, title, body, Some(tab))
            .await;
    }
}

/// Adaptador de `PtyPort` delegando a `PtyManager` e `WorkspaceManager`.
pub struct TauriPtyAdapter {
    pty: Arc<PtyManager>,
    workspace: WorkspaceManager,
}

impl TauriPtyAdapter {
    /// Cria um novo adaptador de `PtyPort`.
    #[must_use]
    pub fn new(pty: Arc<PtyManager>, workspace: WorkspaceManager) -> Arc<Self> {
        Arc::new(Self { pty, workspace })
    }
}

impl PtyPort for TauriPtyAdapter {
    fn foreground(&self, pty: PtyId) -> Option<ForegroundProc> {
        let report = self.pty.process_report(pty).ok()?;
        let fg = report.foreground?;
        Some(ForegroundProc {
            pid: fg.pid,
            comm: fg.comm,
            exe: fg.exe.map(std::path::PathBuf::from),
            cmdline: fg.cmdline,
        })
    }

    fn has_active_processes(&self, tab: TabId) -> bool {
        let targets = self.workspace.active_pane_targets();
        targets.iter().any(|(tid, _, _, _)| *tid == tab)
    }
}

/// Adaptador de `WorkspacePort` delegando a `ProjectService`, `WorkspaceManager`, `AgentRegistry` e `Db`.
pub struct TauriWorkspaceAdapter {
    projects: Arc<ProjectService>,
    workspace: WorkspaceManager,
    registry: Arc<AgentRegistry>,
    db: Db,
}

impl TauriWorkspaceAdapter {
    /// Cria um novo adaptador de `WorkspacePort`.
    #[must_use]
    pub fn new(
        projects: Arc<ProjectService>,
        workspace: WorkspaceManager,
        registry: Arc<AgentRegistry>,
        db: Db,
    ) -> Arc<Self> {
        Arc::new(Self {
            projects,
            workspace,
            registry,
            db,
        })
    }
}

#[async_trait]
impl WorkspacePort for TauriWorkspaceAdapter {
    async fn project(&self, id: ProjectId) -> MtResult<ProjectRecord> {
        let list = self.projects.list().await?;
        let found = list.into_iter().find(|p| p.id == id).ok_or_else(|| {
            MtError::new(
                ErrorCode("project.not_found"),
                format!("Projeto '{id}' não encontrado."),
            )
        })?;
        let stack_str = found.stack.as_db_str().to_string();
        Ok(ProjectRecord {
            id: found.id,
            title: found.title,
            path: std::path::PathBuf::from(found.path),
            stack: stack_str,
        })
    }

    fn project_of_tab(&self, tab: TabId) -> Option<ProjectId> {
        self.workspace.tab_project_and_cwd(tab).map(|(pid, _)| pid)
    }

    fn tabs_using_cwd(&self, dir: &Path) -> Vec<TabId> {
        let mut out = Vec::new();
        for (tab_id, _, _, cwd_str) in self.workspace.active_pane_targets() {
            let p = Path::new(&cwd_str);
            if p.starts_with(dir) && !out.contains(&tab_id) {
                out.push(tab_id);
            }
        }
        out
    }

    async fn ensure_agent_templates(&self, project: ProjectId) -> MtResult<u32> {
        let _ = self.project(project).await?;
        ensure_agent_templates_in_db(&self.db, &self.registry, project, current_unix_ms()).await
    }
}

/// Adaptador de `GitPort` delegando a `WorktreeManager` e `GitService`.
pub struct TauriGitAdapter {
    git_service: GitService,
    worktrees: Arc<dyn WorktreeManager>,
}

impl TauriGitAdapter {
    /// Cria um novo adaptador de `GitPort`.
    #[must_use]
    pub fn new(git_service: GitService, worktrees: Arc<dyn WorktreeManager>) -> Arc<Self> {
        Arc::new(Self {
            git_service,
            worktrees,
        })
    }
}

#[async_trait]
impl GitPort for TauriGitAdapter {
    async fn create_worktree(&self, req: WorktreeCreateRequest) -> MtResult<WorktreeEntry> {
        self.worktrees.create(req).await.map_err(MtError::from)
    }

    async fn remove_worktree(
        &self,
        _repo_root: &Path,
        worktree_path: &Path,
        force: bool,
    ) -> MtResult<()> {
        self.worktrees
            .remove(worktree_path, force)
            .await
            .map_err(MtError::from)
    }

    async fn status(&self, cwd: &Path) -> MtResult<Option<GitContext>> {
        let ctx = self
            .git_service
            .status_for_path(cwd)
            .await
            .map_err(MtError::from)?;
        Ok(Some(ctx))
    }

    async fn run(&self, cwd: &Path, args: &[&str]) -> MtResult<String> {
        let mut cmd = Command::new("git");
        cmd.current_dir(cwd)
            .env("LC_ALL", "C")
            .env("GIT_TERMINAL_PROMPT", "0")
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = tokio::time::timeout(Duration::from_secs(10), cmd.output())
            .await
            .map_err(|_| {
                MtError::new(
                    ErrorCode("git.timeout"),
                    "Tempo limite excedido ao executar comando git.",
                )
            })?
            .map_err(|e| {
                MtError::new(
                    ErrorCode("git.command_failed"),
                    format!("Falha ao executar git: {e}"),
                )
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(MtError::new(
                ErrorCode("git.command_failed"),
                if stderr.is_empty() {
                    format!("Comando git falhou com status {}", output.status)
                } else {
                    stderr
                },
            ));
        }

        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// `OutputTap` que alimenta o `AttentionDetector` com bytes de PTYs pertencentes a execuções de agente ativas.
pub struct AgentOutputTap {
    runtime: Arc<AgentRuntimeIndex>,
    attention: Arc<AttentionDetector>,
}

impl AgentOutputTap {
    /// Cria um novo tap de output para o detector de atenção.
    #[must_use]
    pub fn new(runtime: Arc<AgentRuntimeIndex>, attention: Arc<AttentionDetector>) -> Arc<Self> {
        Arc::new(Self { runtime, attention })
    }
}

impl OutputTap for AgentOutputTap {
    fn on_output(&self, pty_id: PtyId, _offset_start: u64, chunk: &[u8]) {
        if let Some(binding) = self.runtime.by_pty(pty_id) {
            let attention = Arc::clone(&self.attention);
            let bytes = chunk.to_vec();
            let tab_id = binding.tab_id;
            let now_ms = current_unix_ms();
            tauri::async_runtime::spawn(async move {
                attention
                    .on_signal(tab_id, AttentionSignal::OutputBytes { bytes }, now_ms)
                    .await;
            });
        }
    }
}

/// Listener de eventos de shell que alimenta incrementalmente a `CommandMemory` quando um bloco termina.
pub struct MemoryShellEventListener {
    app: AppHandle,
    workspace: WorkspaceManager,
    memory: Arc<CommandMemory>,
}

impl MemoryShellEventListener {
    /// Cria um novo listener de memória contextual.
    #[must_use]
    pub fn new(
        app: AppHandle,
        workspace: WorkspaceManager,
        memory: Arc<CommandMemory>,
    ) -> Arc<Self> {
        Arc::new(Self {
            app,
            workspace,
            memory,
        })
    }
}

impl ShellEventSink for MemoryShellEventListener {
    fn on_tab_state_changed(&self, _snapshot: &TabStateSnapshot) {}
    fn on_tab_state_removed(&self, _event: &TabStateRemovedEvent) {}
    fn on_command_started(&self, _event: &CommandStarted) {}

    fn on_command_finished(&self, event: &CommandFinished) {
        let block_id = event.block_id;
        let tab_id = event.binding.tab_id;
        let workspace = self.workspace.clone();
        let memory = Arc::clone(&self.memory);
        let app = self.app.clone();

        tauri::async_runtime::spawn(async move {
            let Ok(updated) = memory
                .on_command_finished(block_id, current_unix_ms())
                .await
            else {
                return;
            };
            if updated
                && let Some(tid) = tab_id
                && let Some((project_id, _)) = workspace.tab_project_and_cwd(tid)
            {
                let _ = MemoryUpdatedTauriEvent(MemoryUpdatedPayload { project_id }).emit(&app);
            }
        });
    }

    fn on_cwd_changed(&self, _event: &ShellCwdChangedEvent) {}
    fn on_integration_mode_changed(&self, _event: &ShellIntegrationModeEvent) {}
}
