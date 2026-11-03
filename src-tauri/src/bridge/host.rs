//! Implementação de `BridgeHost` e `BridgeSettingsStore` conectada ao `Db`, `ProjectService`,
//! `WorkspaceManager`, `ShellService` e `OsIntegration`.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;

use async_trait::async_trait;
use mt_bridge::{
    AgentKind, BridgeError, BridgeErrorCode, BridgeHost, BridgeProjectSummary, BridgeSettings,
    BridgeSettingsStore, InspectFallbackMode, InspectableBlockContext, OpenTerminalRequest,
    OpenedTerminalInfo,
};
use mt_core::ids::{CommandBlockId, ProjectId};
use mt_core::workspace::{CommandOverride, EnvironmentKind, TabOpenOverrides, TabOpenRequest};
use mt_db::{Db, SettingChangeOrigin};
use mt_os::OsIntegration;
use mt_projects::{NewProject, ProjectService};
use mt_shell::ShellService;
use serde_json::{Value, json};
use tokio::process::Command;
use uuid::Uuid;

use crate::workspace::WorkspaceManager;

/// Implementação persistente de `BridgeSettingsStore` sobre a tabela `settings` do `mt-db`.
pub struct DbBridgeSettingsStore {
    db: Arc<Db>,
}

impl DbBridgeSettingsStore {
    /// Cria uma nova instância de `DbBridgeSettingsStore`.
    #[must_use]
    pub fn new(db: Arc<Db>) -> Arc<Self> {
        Arc::new(Self { db })
    }
}

#[async_trait]
impl BridgeSettingsStore for DbBridgeSettingsStore {
    async fn load(&self) -> Result<BridgeSettings, BridgeError> {
        let Ok(repo) = self.db.settings() else {
            return Ok(BridgeSettings::default());
        };
        let enabled = repo
            .get_typed::<bool>("bridge.enabled")
            .ok()
            .flatten()
            .unwrap_or(true);
        let auto_sync_active_project = repo
            .get_typed::<bool>("bridge.auto_sync_active_project")
            .ok()
            .flatten()
            .unwrap_or(true);
        let auto_register_unknown_project = repo
            .get_typed::<bool>("bridge.auto_register_unknown_project")
            .ok()
            .flatten()
            .unwrap_or(true);
        let focus_window_on_incoming_command = repo
            .get_typed::<bool>("bridge.focus_window_on_incoming_command")
            .ok()
            .flatten()
            .unwrap_or(true);
        let inspect_fallback_mode = repo
            .get_typed::<InspectFallbackMode>("bridge.inspect_fallback_mode")
            .ok()
            .flatten()
            .unwrap_or_default();
        let preferred_port = repo
            .get_typed::<u16>("bridge.preferred_port")
            .ok()
            .flatten();

        Ok(BridgeSettings {
            enabled,
            auto_sync_active_project,
            auto_register_unknown_project,
            focus_window_on_incoming_command,
            inspect_fallback_mode,
            preferred_port,
        })
    }

    async fn save(&self, settings: &BridgeSettings) -> Result<(), BridgeError> {
        let repo = self.db.settings().map_err(|e| {
            BridgeError::new(
                BridgeErrorCode::TransportIo,
                format!("Falha ao acessar repositório de configurações: {e}"),
            )
        })?;

        let _ = repo
            .set(
                "bridge.enabled",
                json!(settings.enabled),
                None,
                SettingChangeOrigin::Ui,
            )
            .await;
        let _ = repo
            .set(
                "bridge.auto_sync_active_project",
                json!(settings.auto_sync_active_project),
                None,
                SettingChangeOrigin::Ui,
            )
            .await;
        let _ = repo
            .set(
                "bridge.auto_register_unknown_project",
                json!(settings.auto_register_unknown_project),
                None,
                SettingChangeOrigin::Ui,
            )
            .await;
        let _ = repo
            .set(
                "bridge.focus_window_on_incoming_command",
                json!(settings.focus_window_on_incoming_command),
                None,
                SettingChangeOrigin::Ui,
            )
            .await;
        let mode_val =
            serde_json::to_value(settings.inspect_fallback_mode).unwrap_or(json!("uri_scheme"));
        let _ = repo
            .set(
                "bridge.inspect_fallback_mode",
                mode_val,
                None,
                SettingChangeOrigin::Ui,
            )
            .await;
        let port_val = settings
            .preferred_port
            .map_or(Value::Null, |p| json!(u64::from(p)));
        let _ = repo
            .set(
                "bridge.preferred_port",
                port_val,
                None,
                SettingChangeOrigin::Ui,
            )
            .await;

        Ok(())
    }
}

/// Implementação de `BridgeHost` que orquestra os subsistemas do `MaxTerminal`.
pub struct TauriBridgeHost {
    db: Arc<Db>,
    projects: Arc<ProjectService>,
    workspace: WorkspaceManager,
    shell: Arc<ShellService>,
    os: Arc<OsIntegration>,
}

impl TauriBridgeHost {
    /// Cria uma nova instância compartilhada de `TauriBridgeHost`.
    #[must_use]
    pub fn new(
        db: Arc<Db>,
        projects: Arc<ProjectService>,
        workspace: WorkspaceManager,
        shell: Arc<ShellService>,
        os: Arc<OsIntegration>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            projects,
            workspace,
            shell,
            os,
        })
    }
}

#[async_trait]
impl BridgeHost for TauriBridgeHost {
    async fn active_project_path(&self) -> Option<String> {
        let selected_id_str = self
            .db
            .settings()
            .ok()?
            .get_typed::<String>("projects.selected_id")
            .ok()??;
        let list = self.projects.list().await.ok()?;
        list.into_iter()
            .find(|p| p.id.to_string() == selected_id_str)
            .map(|p| p.path)
    }

    async fn find_project_by_path(
        &self,
        canonical_path: &Path,
    ) -> Result<Option<BridgeProjectSummary>, BridgeError> {
        let target = canonical_path.to_string_lossy();
        let list = self
            .projects
            .list()
            .await
            .map_err(|e| BridgeError::new(BridgeErrorCode::TransportIo, e.message))?;
        for proj in list {
            let proj_canon = std::fs::canonicalize(&proj.path)
                .map_or_else(|_| proj.path.clone(), |p| p.to_string_lossy().into_owned());
            if proj_canon == target {
                return Ok(Some(BridgeProjectSummary {
                    id: proj.id.to_string(),
                    title: proj.title,
                    path: proj_canon,
                }));
            }
        }
        Ok(None)
    }

    async fn create_project_for_path(
        &self,
        canonical_path: &Path,
        preferred_name: Option<&str>,
    ) -> Result<BridgeProjectSummary, BridgeError> {
        let path_str = canonical_path.to_string_lossy().into_owned();
        let created = self
            .projects
            .add(NewProject {
                path: path_str,
                title: preferred_name.map(str::to_string),
                icon: None,
                color: None,
            })
            .await
            .map_err(|e| BridgeError::new(BridgeErrorCode::TransportIo, e.message))?;

        Ok(BridgeProjectSummary {
            id: created.id.to_string(),
            title: created.title,
            path: created.path,
        })
    }

    async fn focus_project(&self, project_id: &str) -> Result<(), BridgeError> {
        if let Ok(repo) = self.db.settings() {
            let _ = repo
                .set(
                    "projects.selected_id",
                    json!(project_id),
                    None,
                    SettingChangeOrigin::Sync,
                )
                .await;
        }
        Ok(())
    }

    async fn open_terminal_for_issue(
        &self,
        request: OpenTerminalRequest,
    ) -> Result<OpenedTerminalInfo, BridgeError> {
        let proj_uuid = Uuid::parse_str(&request.project_id).map_err(|_| {
            BridgeError::new(
                BridgeErrorCode::InvalidEnvelope,
                "ID de projeto inválido ao abrir aba para issue.",
            )
        })?;
        let project_id = ProjectId::try_from_uuid(proj_uuid)
            .map_err(|e| BridgeError::new(BridgeErrorCode::InvalidEnvelope, e.message))?;

        let _ = self.focus_project(&request.project_id).await;

        let agent_slug = request.agent.map(|a| match a {
            AgentKind::Claude => "claude-code",
            AgentKind::Codex => "codex",
            AgentKind::Agy => "agy",
            AgentKind::OpenCode => "opencode",
        });

        let template_id = if let Some(slug) = agent_slug
            && let Ok(templates) = self.workspace.template_list(project_id).await
        {
            templates
                .into_iter()
                .find(|t| {
                    t.config.environment.kind == EnvironmentKind::Agent
                        && (t.config.environment.agent_id.as_deref() == Some(slug)
                            || (slug == "claude-code"
                                && t.config.environment.agent_id.as_deref() == Some("claude")))
                })
                .map(|t| t.id)
        } else {
            None
        };

        let title = if let Some(ref issue_title) = request.issue_title
            && !issue_title.trim().is_empty()
        {
            let trimmed = issue_title.trim();
            let short: String = trimmed.chars().take(36).collect();
            format!("#{} {short}", request.issue_number)
        } else {
            format!("Issue #{}", request.issue_number)
        };

        let initial_command = request
            .initial_command
            .map_or(CommandOverride::Inherit, |cmd| CommandOverride::Replace {
                command: cmd,
            });

        let tab_view = self
            .workspace
            .tab_open(TabOpenRequest {
                request_id: Uuid::now_v7().to_string(),
                project_id,
                template_id,
                overrides: Some(TabOpenOverrides {
                    title: Some(title),
                    cwd: Some(request.cwd),
                    initial_command,
                    extra_env: BTreeMap::new(),
                    continuous: None,
                    cwd_fallback_to_project: true,
                }),
                focus: true,
            })
            .await
            .map_err(|e| BridgeError::new(BridgeErrorCode::TransportIo, e.message))?;

        Ok(OpenedTerminalInfo {
            project_id: request.project_id,
            tab_id: tab_view.id.to_string(),
        })
    }

    async fn load_inspectable_block(
        &self,
        block_id: &str,
    ) -> Result<InspectableBlockContext, BridgeError> {
        let uuid = Uuid::parse_str(block_id).map_err(|_| {
            BridgeError::new(
                BridgeErrorCode::BlockNotFound,
                "ID de bloco de comando inválido.",
            )
        })?;
        let cmd_block_id = CommandBlockId::try_from_uuid(uuid)
            .map_err(|e| BridgeError::new(BridgeErrorCode::BlockNotFound, e.message))?;
        let ctx = self
            .shell
            .command_block_error_context(cmd_block_id)
            .await
            .map_err(|e| BridgeError::new(BridgeErrorCode::BlockNotInspectable, e.message))?;

        let cwd = ctx.cwd.clone().unwrap_or_else(|| "/".to_string());
        let mut project_path = cwd.clone();
        let tab_id_str = ctx.tab_id.map_or_else(String::new, |tid| tid.to_string());

        if let Some(tid) = ctx.tab_id
            && let Some((proj_id, _)) = self.workspace.tab_project_and_cwd(tid)
            && let Ok(projects) = self.projects.list().await
            && let Some(proj) = projects.into_iter().find(|p| p.id == proj_id)
        {
            project_path = proj.path;
        }

        let duration_ms = u64::try_from(ctx.duration_ms.max(0)).ok();

        Ok(InspectableBlockContext {
            block_id: ctx.block_id.to_string(),
            session_id: ctx.session_id.to_string(),
            tab_id: tab_id_str,
            project_path,
            cwd,
            command: ctx.command,
            error_output: ctx.error,
            exit_code: ctx.exit_code,
            signal: ctx.signal,
            duration_ms,
        })
    }

    async fn bring_window_to_front(&self) -> Result<(), BridgeError> {
        self.os
            .quake
            .show_and_focus()
            .map_err(|e| BridgeError::new(BridgeErrorCode::TransportIo, e.message))
    }

    async fn launch_external_uri(&self, uri: &str) -> Result<(), BridgeError> {
        if !uri.starts_with("vscode://") {
            return Err(BridgeError::new(
                BridgeErrorCode::InvalidUri,
                "Apenas URIs vscode:// podem ser disparadas externamente pela ponte.",
            ));
        }

        let status = Command::new("xdg-open")
            .arg(uri)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        if status.is_ok_and(|s| s.success()) {
            return Ok(());
        }

        let fallback = Command::new("code")
            .arg("--open-url")
            .arg(uri)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|e| {
                BridgeError::new(
                    BridgeErrorCode::UriLaunchFailed,
                    format!("Não foi possível invocar xdg-open nem code --open-url: {e}"),
                )
            })?;

        if fallback.success() {
            Ok(())
        } else {
            Err(BridgeError::new(
                BridgeErrorCode::UriLaunchFailed,
                "O lançador de URI do sistema operacional retornou erro ao abrir vscode://.",
            ))
        }
    }
}
