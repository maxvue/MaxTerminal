//! Estado e integração Tauri do subsistema de Grupos de Comandos (`>>` — Fase 15).

pub mod adapters;

use std::sync::Arc;

use async_trait::async_trait;
use mt_agents::memory::CommandMemory;
use mt_ai::AiService;
use mt_commands::groups::events::GroupEventSink;
use mt_commands::groups::ports::{
    GroupAiPort, GroupPtyPort, GroupShellPort, GroupTabPort, RecurringCommandsSource,
};
use mt_commands::groups::repo::{CommandGroupRepo, SqliteCommandGroupRepo};
use mt_commands::groups::service::CommandGroupService;
use mt_core::error::MtResult;
use mt_core::ids::ProjectId;
use mt_db::Db;
use mt_projects::{ProjectRemovalHook, ProjectService};
use mt_pty::PtyManager;
use mt_shell::ShellService;
use tauri::AppHandle;

use crate::command_groups::adapters::{
    TauriGroupAiAdapter, TauriGroupPtyAdapter, TauriGroupShellAdapter, TauriGroupTabAdapter,
    TauriRecurringCommandsAdapter,
};
use crate::events::command_groups::TauriGroupEventSink;
use crate::events::shell::TauriShellEventSink;
use crate::workspace::WorkspaceManager;

/// Estado gerenciado pelo Tauri para Grupos de Comandos (`>>` — Fase 15).
#[derive(Clone)]
pub struct AppCommandGroupState {
    /// Fachada principal de grupos de comandos.
    pub service: Arc<CommandGroupService>,
    /// Banco de dados SQLite para sincronização incremental de busca.
    pub db: Db,
    /// Handle da aplicação Tauri.
    pub app: AppHandle,
}

struct GroupProjectRemovalHook {
    service: Arc<CommandGroupService>,
}

#[async_trait]
impl ProjectRemovalHook for GroupProjectRemovalHook {
    async fn before_remove(&self, project_id: ProjectId) -> MtResult<()> {
        let _ = self.service.on_project_deleting(&project_id).await;
        Ok(())
    }
}

impl AppCommandGroupState {
    /// Inicializa o [`AppCommandGroupState`], registra listeners no `PtyManager`, `TauriShellEventSink`
    /// e `ProjectService`, e executa `recover_interrupted()` para runs órfãos de boots anteriores.
    #[allow(clippy::too_many_arguments)]
    pub async fn init(
        app: AppHandle,
        db: Db,
        pty_manager: Arc<PtyManager>,
        shell_service: Arc<ShellService>,
        tauri_shell_sink: Arc<TauriShellEventSink>,
        project_service: Arc<ProjectService>,
        workspace_manager: WorkspaceManager,
        ai_service: Arc<AiService>,
        memory: Arc<CommandMemory>,
    ) -> MtResult<Self> {
        let repo: Arc<dyn CommandGroupRepo> = Arc::new(SqliteCommandGroupRepo::new(db.clone()));
        let pty_adapter = TauriGroupPtyAdapter::new(pty_manager);
        let shell_adapter = TauriGroupShellAdapter::new(shell_service, workspace_manager.clone());
        tauri_shell_sink
            .add_listener(Arc::clone(&shell_adapter) as Arc<dyn mt_shell::ShellEventSink>);

        let tab_adapter: Arc<dyn GroupTabPort> =
            TauriGroupTabAdapter::new(workspace_manager, Arc::clone(&project_service));
        let ai_adapter: Arc<dyn GroupAiPort> = TauriGroupAiAdapter::new(ai_service);
        let recurring_adapter: Arc<dyn RecurringCommandsSource> =
            TauriRecurringCommandsAdapter::new(memory);
        let events_sink: Arc<dyn GroupEventSink> = TauriGroupEventSink::new(app.clone());

        let service = CommandGroupService::new(
            repo,
            pty_adapter as Arc<dyn GroupPtyPort>,
            shell_adapter as Arc<dyn GroupShellPort>,
            tab_adapter,
            ai_adapter,
            Some(recurring_adapter),
            events_sink,
        );

        let _ = service.recover_interrupted().await?;

        project_service.register_removal_hook(Arc::new(GroupProjectRemovalHook {
            service: Arc::clone(&service),
        }));

        Ok(Self { service, db, app })
    }
}
