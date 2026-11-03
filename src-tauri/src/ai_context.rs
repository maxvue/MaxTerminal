//! Adaptador de coleta de contexto de terminal (`TauriAiContextSource`) e observador de ciclo de vida
//! de painéis/abas para cancelamento automático de requisições em andamento (`mt-ai` — Fase 12).

use std::sync::Arc;

use async_trait::async_trait;
use mt_ai::AiService;
use mt_ai::config::ContextSettings;
use mt_ai::context::{
    AiContext, AiContextSource, BlockFailure, PaneBindingSnapshot, RecentCommand,
    detect_linux_os_info,
};
use mt_core::error::{MtError, MtResult, codes};
use mt_core::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_core::shell::{CommandBlockListQuery, ShellKind};
use mt_projects::ProjectService;
use mt_pty::ExitInfo;
use mt_shell::ShellService;
use parking_lot::RwLock;

use crate::workspace::manager::WorkspaceManager;
use crate::workspace::observer::WorkspaceLifecycleObserver;

/// Adaptador que coleta dados reais de `WorkspaceManager`, `ProjectService`
/// e `ShellService` para o `AiService` do `mt-ai`, além de cancelar requisições em voo
/// quando painéis, sessões ou abas são encerrados (`CA-15`, `CA-33`).
pub struct TauriAiContextSource {
    workspace: WorkspaceManager,
    projects: Arc<ProjectService>,
    shell: Arc<ShellService>,
    ai_service: RwLock<Option<Arc<AiService>>>,
}

impl TauriAiContextSource {
    /// Cria uma nova fonte de contexto integrada aos serviços do host Tauri.
    #[must_use]
    pub fn new(
        workspace: WorkspaceManager,
        projects: Arc<ProjectService>,
        shell: Arc<ShellService>,
    ) -> Self {
        Self {
            workspace,
            projects,
            shell,
            ai_service: RwLock::new(None),
        }
    }

    /// Vincula a instância do `AiService` após a inicialização para cancelamento automático
    /// nos callbacks de `WorkspaceLifecycleObserver`.
    pub fn bind_service(&self, service: Arc<AiService>) {
        *self.ai_service.write() = Some(service);
    }

    fn cancel_pane(&self, pane_id: PaneId) {
        if let Some(svc) = self.ai_service.read().as_ref() {
            let _ = svc.requests.cancel_pane(pane_id);
        }
    }

    async fn resolve_project_info(
        &self,
        project_id: ProjectId,
    ) -> (Option<String>, Option<String>) {
        let Ok(list) = self.projects.list().await else {
            return (None, None);
        };
        let Some(proj) = list.into_iter().find(|p| p.id == project_id) else {
            return (None, None);
        };
        let stack_str = serde_json::to_value(proj.stack)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string));
        (Some(proj.title), stack_str)
    }

    async fn build_recent_commands(
        &self,
        session_id: SessionId,
        pty_id: Option<PtyId>,
        limit: u8,
    ) -> (Vec<RecentCommand>, Option<String>) {
        if limit == 0 {
            return (Vec::new(), None);
        }
        let Ok(page) = self
            .shell
            .command_block_list(CommandBlockListQuery {
                session_id: Some(session_id),
                tab_id: None,
                pty_id,
                statuses: None,
                limit: Some(u32::from(limit)),
                cursor: None,
            })
            .await
        else {
            return (Vec::new(), None);
        };

        let last_tail = page
            .items
            .first()
            .and_then(|b| b.output_text.clone().or_else(|| b.error_excerpt.clone()));

        let mut recent: Vec<RecentCommand> = page
            .items
            .into_iter()
            .filter_map(|b| {
                let cmd = b.command_text?;
                if cmd.trim().is_empty() {
                    return None;
                }
                Some(RecentCommand {
                    command: cmd,
                    exit_code: b.exit_code,
                    cwd: b.cwd,
                })
            })
            .collect();
        recent.reverse();
        (recent, last_tail)
    }
}

#[async_trait]
impl AiContextSource for TauriAiContextSource {
    async fn for_pane(&self, pane: PaneId, opts: &ContextSettings) -> MtResult<AiContext> {
        let (project_id, _tab_id, _tab_session_id, session_id, pty_id, cwd) =
            self.workspace.pane_full_info(pane).ok_or_else(|| {
                MtError::new(
                    codes::PANE_NOT_FOUND,
                    format!("Painel '{pane}' não encontrado no workspace ativo."),
                )
            })?;

        let shell_name = pty_id
            .and_then(|pid| self.shell.integration_info(pid).ok())
            .map_or_else(
                || "zsh".to_string(),
                |info| info.shell_kind.as_db_str().to_string(),
            );

        let (project_name, project_stack) = self.resolve_project_info(project_id).await;
        let (recent_commands, last_tail) = self
            .build_recent_commands(session_id, pty_id, opts.recent_commands)
            .await;

        let output_tail = if opts.include_output_tail {
            last_tail
        } else {
            None
        };

        let ctx = AiContext {
            shell: shell_name,
            os: detect_linux_os_info(),
            cwd,
            project_stack,
            project_name,
            recent_commands,
            output_tail,
        };
        Ok(ctx.sanitize_and_bound(opts.recent_commands, opts.output_tail_bytes))
    }

    async fn for_block(
        &self,
        block_id: CommandBlockId,
        tail_bytes: u32,
    ) -> MtResult<(AiContext, BlockFailure)> {
        let block = self.shell.command_block_get(block_id).await?;
        let error_ctx = self.shell.command_block_error_context(block_id).await.ok();

        let pane_id = block.pane_id.ok_or_else(|| {
            MtError::new(
                codes::PANE_NOT_FOUND,
                format!("Bloco '{block_id}' não possui painel associado."),
            )
        })?;

        let project_id = block
            .tab_id
            .and_then(|tid| self.workspace.tab_meta(tid).map(|(pid, _, _)| pid))
            .or_else(|| {
                self.workspace
                    .pane_full_info(pane_id)
                    .map(|(pid, _, _, _, _, _)| pid)
            });

        let (project_name, project_stack) = match project_id {
            Some(pid) => self.resolve_project_info(pid).await,
            None => (None, None),
        };

        let info = self.shell.integration_info(block.pty_id).ok();
        let shell_kind = info
            .as_ref()
            .map_or(ShellKind::Zsh, |i| i.shell_kind.clone());
        let is_interactive = matches!(shell_kind, ShellKind::Custom(_));

        let output_tail = error_ctx
            .as_ref()
            .map(|c| c.error.clone())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| block.error_excerpt.clone())
            .or_else(|| block.output_text.clone())
            .unwrap_or_default();

        let command = block
            .command_text
            .clone()
            .or_else(|| error_ctx.map(|c| c.command))
            .unwrap_or_default();

        let cwd = block.cwd.clone().unwrap_or_default();

        let ctx = AiContext {
            shell: shell_kind.as_db_str().to_string(),
            os: detect_linux_os_info(),
            cwd,
            project_stack,
            project_name,
            recent_commands: Vec::new(),
            output_tail: Some(output_tail.clone()),
        }
        .sanitize_and_bound(0, tail_bytes);

        let failure = BlockFailure {
            pane_id,
            session_id: block.session_id,
            pty_id: block.pty_id,
            project_id,
            command,
            exit_code: block.exit_code,
            output_tail: ctx.output_tail.clone().unwrap_or(output_tail),
            is_interactive,
        };

        Ok((ctx, failure))
    }

    fn pane_snapshot(&self, pane: PaneId) -> Option<PaneBindingSnapshot> {
        let (project_id, tab_id, _tab_session_id, session_id, pty_id, _cwd) =
            self.workspace.pane_full_info(pane)?;
        Some(PaneBindingSnapshot {
            project_id: Some(project_id),
            tab_id,
            pane_id: pane,
            session_id,
            pty_id: pty_id?,
        })
    }
}

impl WorkspaceLifecycleObserver for TauriAiContextSource {
    fn on_pane_exited(&self, _tab_id: TabId, pane_id: PaneId, _pty_id: PtyId, _exit: &ExitInfo) {
        self.cancel_pane(pane_id);
    }

    fn on_session_archived(&self, tab_id: TabId, _tab_session_id: TabSessionId) {
        for pane_id in self.workspace.tab_pane_ids(tab_id) {
            self.cancel_pane(pane_id);
        }
    }

    fn on_tab_closed(&self, _project_id: ProjectId, tab_id: TabId) {
        for pane_id in self.workspace.tab_pane_ids(tab_id) {
            self.cancel_pane(pane_id);
        }
    }
}
