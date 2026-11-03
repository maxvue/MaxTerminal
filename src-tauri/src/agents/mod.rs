//! Subsistema de Agentes CLI (`AppAgentState`, `AgentSpawnHook`, `AgentWorkspaceObserver` — Fase 13).

pub mod adapters;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use mt_agents::attention::AttentionSignal;
use mt_agents::attention::detector::AttentionDetector;
use mt_agents::attention::patterns::PatternSet;
use mt_agents::detect::{Detector, current_unix_ms};
use mt_agents::error as agent_err;
use mt_agents::isolation::{
    AgentTabLink, AgentTabOpenRequest, IsolationService, get_template_options_in_db,
};
use mt_agents::launch::{LaunchPlanner, PendingLaunch, PendingRunBinding, ShellKind, command_line};
use mt_agents::memory::CommandMemory;
use mt_agents::ports::{PtyPort, RunBinding, WorkspacePort};
use mt_agents::registry::{AgentKind, AgentRegistry};
use mt_agents::relay::protocol::RelayMessage;
use mt_agents::relay::server::{AuthenticatedRelayEvent, RelayServer, RelayToken};
use mt_agents::settings::{AgentsSettings, SkillsSettings};
use mt_agents::skills::sync::SkillsService;
use mt_agents::skills::watch::{SkillsEventSink, SkillsWatcher};
use mt_agents::telemetry::JsonlTail;
use mt_agents::telemetry::aggregator::TelemetryAggregator;
use mt_agents::telemetry::claude_statusline::ClaudeStatusLineParser;
use mt_agents::telemetry::claude_transcript::ClaudeTranscriptSource;
use mt_agents::telemetry::codex_session::CodexSessionSource;
use mt_agents::{AgentRuntimeIndex, AgentService};
use mt_core::error::{MtError, MtResult};
use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId, TabSessionId};
use mt_core::paths::AppPaths;
use mt_core::workspace::{
    CommandOverride, EnvValue, EnvironmentKind, TabOpenOverrides, TabOpenRequest, TabView,
};
use mt_db::Db;
use mt_git::{GitService, WorktreeManager};
use mt_os::OsIntegration;
use mt_projects::ProjectService;
use mt_pty::{ExitInfo, OutputTap, PtyEvent, PtyEventListener, PtyManager};
use mt_shell::ShellService;
use parking_lot::{Mutex, RwLock};
use serde::Serialize;
use tauri::AppHandle;
use tauri_specta::Event;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::agents::adapters::{
    AgentOutputTap, MemoryShellEventListener, TauriAttentionSink, TauriGitAdapter,
    TauriNotifyAdapter, TauriPtyAdapter, TauriSkillsEventSink, TauriTabStateAdapter,
    TauriTelemetrySink, TauriWorkspaceAdapter,
};
use crate::events::agents::{AgentLinkChangedTauriEvent, AgentWorktreeClosePendingTauriEvent};
use crate::events::shell::TauriShellEventSink;
use crate::workspace::{
    SpawnReservationRequest, WorkspaceLifecycleObserver, WorkspaceManager, WorkspaceSpawnGuard,
    WorkspaceSpawnHook,
};

/// Retorno da abertura de uma aba de agente (`agent_tab_open`).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabOpened {
    /// Dados da aba aberta no `WorkspaceManager`.
    pub tab: TabView,
    /// Vínculo aba ↔ worktree registrado em `agent_tab_links`.
    pub link: Option<AgentTabLink>,
}

#[derive(Debug, Clone)]
struct StagedLaunch {
    agent: AgentKind,
    run_id: String,
    cwd: PathBuf,
    configured_model: Option<String>,
}

#[derive(Debug, Clone)]
struct PaneReservedAgent {
    pane_id: PaneId,
    project_id: ProjectId,
    run_id: String,
    agent: AgentKind,
    cwd: PathBuf,
    started_at_ms: i64,
    configured_model: Option<String>,
}

struct NoopAgentSpawnGuard;
impl WorkspaceSpawnGuard for NoopAgentSpawnGuard {
    fn release(self: Box<Self>) {}
}

/// Estado gerenciado pelo Tauri para o subsistema de Agentes CLI, Telemetria, Skills e Memória (Fase 13).
#[derive(Clone)]
pub struct AppAgentState {
    /// Fachada central do crate `mt-agents`.
    pub service: Arc<AgentService>,
    /// Gerenciador de workspace da Fase 07.
    pub workspace: WorkspaceManager,
    /// Banco de dados SQLite.
    pub db: Db,
    /// Handle da aplicação Tauri.
    pub app: AppHandle,
    /// Diretórios XDG resolvidos.
    pub paths: AppPaths,
    /// Configurações de `MaxSkills` compartilhadas.
    pub skills_settings: Arc<RwLock<SkillsSettings>>,
    /// Watcher da fonte de skills, se iniciado.
    pub watcher: Option<Arc<SkillsWatcher>>,
    staged_launches: Arc<Mutex<HashMap<String, StagedLaunch>>>,
    pane_reservations: Arc<Mutex<HashMap<PaneId, PaneReservedAgent>>>,
}

struct AgentSpawnHook {
    state: AppAgentState,
}

impl WorkspaceSpawnHook for AgentSpawnHook {
    fn reserve(
        &self,
        req: &SpawnReservationRequest,
        env: &mut BTreeMap<String, String>,
    ) -> Result<Box<dyn WorkspaceSpawnGuard>, MtError> {
        let staged_opt = if let Some(launch_id) = env.remove("MAXTERMINAL_AGENT_LAUNCH") {
            self.state.staged_launches.lock().remove(&launch_id)
        } else {
            None
        }
        .or_else(|| {
            let agent_slug = env.remove("MAXTERMINAL_TEMPLATE_AGENT")?;
            let agent = AgentKind::from_ref_str(&agent_slug).ok()?;
            Some(StagedLaunch {
                agent,
                run_id: Uuid::now_v7().to_string(),
                cwd: req.cwd.clone(),
                configured_model: None,
            })
        });

        if let Some(staged) = staged_opt
            && let (Some(tab_id), Some(pane_id)) = (req.owner.tab_id, req.owner.pane_id)
        {
            let now_ms = current_unix_ms();
            let pending_binding = PendingRunBinding {
                tab_id,
                pane_id,
                session_id: SessionId::new(),
                generation: 1,
                project_id: req.project_id,
                run_id: staged.run_id.clone(),
                agent: staged.agent,
                cwd: staged.cwd.clone(),
                started_at_ms: now_ms,
            };

            let token = if let Some(ref relay) = self.state.service.relay {
                relay.issue(pending_binding)
            } else {
                RelayToken::generate().0
            };

            let relay_sock = self.state.service.relay.as_ref().map_or_else(
                || self.state.paths.cache_dir.join("agent-relay.sock"),
                |r| r.socket_path().to_path_buf(),
            );
            let relay_bin = resolve_relay_bin(&self.state.paths);

            env.insert(
                "MAXTERMINAL_AGENT".to_string(),
                staged.agent.as_ref_id().to_string(),
            );
            env.insert(
                "MAXTERMINAL_AGENT_TOKEN".to_string(),
                token.as_str().to_string(),
            );
            env.insert(
                "MAXTERMINAL_RELAY_SOCK".to_string(),
                relay_sock.to_string_lossy().into_owned(),
            );
            env.insert(
                "MAXTERMINAL_RELAY_BIN".to_string(),
                relay_bin.to_string_lossy().into_owned(),
            );

            self.state.pane_reservations.lock().insert(
                pane_id,
                PaneReservedAgent {
                    pane_id,
                    project_id: req.project_id,
                    run_id: staged.run_id,
                    agent: staged.agent,
                    cwd: staged.cwd,
                    started_at_ms: now_ms,
                    configured_model: staged.configured_model,
                },
            );
        }
        Ok(Box::new(NoopAgentSpawnGuard))
    }
}

struct AgentWorkspaceObserver {
    state: AppAgentState,
    pty_port: Arc<dyn PtyPort>,
}

impl WorkspaceLifecycleObserver for AgentWorkspaceObserver {
    fn on_pane_spawned(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        pty_id: PtyId,
        session_id: SessionId,
    ) {
        let Some(reserved) = self.state.pane_reservations.lock().remove(&pane_id) else {
            return;
        };

        let binding = RunBinding {
            tab_id,
            pane_id: reserved.pane_id,
            pty_id,
            session_id,
            generation: 1,
            project_id: reserved.project_id,
            run_id: reserved.run_id,
            agent: reserved.agent,
            cwd: reserved.cwd,
            started_at_ms: reserved.started_at_ms,
        };

        self.state.service.runtime.insert(binding.clone());
        if let Some(ref relay) = self.state.service.relay {
            let _ = relay.activate(binding.clone());
        }

        let now_ms = current_unix_ms();
        let initial_tel = self.state.service.telemetry.register_run(
            binding.clone(),
            reserved.configured_model,
            now_ms,
        );
        let _ = crate::events::agents::AgentTelemetryTauriEvent(initial_tel).emit(&self.state.app);
        self.state.service.attention.register_run(binding, now_ms);
    }

    fn on_pane_exited(&self, tab_id: TabId, pane_id: PaneId, _pty_id: PtyId, _exit: &ExitInfo) {
        if let Some(binding) = self.state.service.runtime.remove_by_pane(pane_id) {
            if let Some(ref relay) = self.state.service.relay {
                let _ = relay.revoke_run(&binding.run_id);
            }
            self.state
                .service
                .telemetry
                .mark_inactive(tab_id, &binding.run_id, current_unix_ms());
            self.state.service.attention.unregister_tab(tab_id);
        }
    }

    fn on_session_archived(&self, tab_id: TabId, _tab_session_id: TabSessionId) {
        if let Some(binding) = self.state.service.runtime.remove_by_tab(tab_id)
            && let Some(ref relay) = self.state.service.relay
        {
            let _ = relay.revoke_run(&binding.run_id);
        }
        self.state.service.attention.unregister_tab(tab_id);
    }

    fn on_tab_closed(&self, _project_id: ProjectId, tab_id: TabId) {
        self.state.service.runtime.remove_by_tab(tab_id);
        if let Some(ref relay) = self.state.service.relay {
            let _ = relay.revoke_tab(tab_id);
        }
        self.state.service.attention.unregister_tab(tab_id);
        self.state.service.telemetry.remove_tab(tab_id);

        let svc = Arc::clone(&self.state.service);
        let app = self.state.app.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(Some(inspection)) = svc
                .isolation
                .mark_tab_closed(tab_id, current_unix_ms())
                .await
            {
                let _ = AgentLinkChangedTauriEvent(inspection.link.clone()).emit(&app);
                if inspection.link.worktree_path.is_some() && inspection.link.resolution.is_none() {
                    let _ = AgentWorktreeClosePendingTauriEvent(inspection).emit(&app);
                }
            }
        });
    }
}

impl PtyEventListener for AgentWorkspaceObserver {
    fn on_event(&self, event: &PtyEvent) {
        if let PtyEvent::ForegroundChanged { pty_id, .. } = event
            && let Some(binding) = self.state.service.runtime.by_pty(*pty_id)
        {
            let desc = self.state.service.registry.get(binding.agent);
            let is_agent = self.pty_port.foreground(*pty_id).is_some_and(|fg| {
                desc.process_names
                    .iter()
                    .any(|p| fg.comm.eq_ignore_ascii_case(p))
                    || fg
                        .cmdline
                        .first()
                        .is_some_and(|arg0| arg0.ends_with(&desc.binary))
            });
            let now_ms = current_unix_ms();
            self.state
                .service
                .telemetry
                .set_active(binding.tab_id, is_agent, now_ms);
            let attention = Arc::clone(&self.state.service.attention);
            let tab_id = binding.tab_id;
            tauri::async_runtime::spawn(async move {
                attention
                    .on_signal(tab_id, AttentionSignal::Foreground { is_agent }, now_ms)
                    .await;
            });
        }
    }
}

fn resolve_relay_bin(paths: &AppPaths) -> PathBuf {
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let sidecar = dir.join("mt-agent-relay");
        if sidecar.exists() {
            return sidecar;
        }
        if let Some(grandparent) = dir.parent() {
            let uncle = grandparent.join("mt-agent-relay");
            if uncle.exists() {
                return uncle;
            }
            let lib_relay = grandparent.join("lib/maxterminal/mt-agent-relay");
            if lib_relay.exists() {
                return lib_relay;
            }
        }
    }

    for candidate in [
        Path::new("/usr/lib/maxterminal/mt-agent-relay"),
        Path::new("/usr/bin/mt-agent-relay"),
    ] {
        if candidate.exists() {
            return candidate.to_path_buf();
        }
    }

    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        for rel in [
            ".local/lib/maxterminal/mt-agent-relay",
            ".local/bin/mt-agent-relay",
        ] {
            let candidate = home_path.join(rel);
            if candidate.exists() {
                return candidate;
            }
        }
    }

    paths.cache_dir.join("bin").join("mt-agent-relay")
}

fn detect_user_shell() -> ShellKind {
    match std::env::var("SHELL").ok().as_deref() {
        Some(s) if s.ends_with("zsh") => ShellKind::Zsh,
        Some(s) if s.ends_with("fish") => ShellKind::Fish,
        Some(s) if s.ends_with("bash") => ShellKind::Bash,
        _ => ShellKind::Posix,
    }
}

impl AppAgentState {
    /// Inicializa o subsistema completo `mt-agents` e conecta todos os hooks e tarefas em background.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub async fn init(
        app: AppHandle,
        paths: AppPaths,
        db: Db,
        pty_manager: Arc<PtyManager>,
        shell_service: Arc<ShellService>,
        shell_sink: Arc<TauriShellEventSink>,
        project_service: Arc<ProjectService>,
        workspace_manager: WorkspaceManager,
        git_service: GitService,
        worktree_manager: Arc<dyn WorktreeManager>,
        os_integration: Arc<OsIntegration>,
    ) -> MtResult<Self> {
        let (registry_val, _) = AgentRegistry::load(&paths);
        let registry = Arc::new(registry_val);

        let agents_settings = Arc::new(RwLock::new(
            AgentsSettings::load_from_db(&db).unwrap_or_default(),
        ));
        let skills_settings = Arc::new(RwLock::new(
            SkillsSettings::load_from_db(&db).unwrap_or_default(),
        ));
        let attention_settings = Arc::new(RwLock::new(agents_settings.read().attention.clone()));

        let detector = Arc::new(Detector::new());
        let relay_bin = resolve_relay_bin(&paths);
        let planner = Arc::new(LaunchPlanner::new(paths.clone(), relay_bin));

        let tab_state_port = TauriTabStateAdapter::new(Arc::clone(&shell_service));
        let notify_port =
            TauriNotifyAdapter::new(Arc::clone(&os_integration), Arc::clone(&shell_service));
        let pty_port = TauriPtyAdapter::new(Arc::clone(&pty_manager), workspace_manager.clone());
        let workspace_port = TauriWorkspaceAdapter::new(
            Arc::clone(&project_service),
            workspace_manager.clone(),
            Arc::clone(&registry),
            db.clone(),
        );
        let git_port = TauriGitAdapter::new(git_service, worktree_manager);

        let isolation = Arc::new(IsolationService::new(
            git_port,
            Arc::clone(&pty_port) as Arc<dyn PtyPort>,
            Arc::clone(&workspace_port) as Arc<dyn WorkspacePort>,
            db.clone(),
            Arc::clone(&agents_settings),
        ));

        let (relay_tx, mut relay_rx) =
            mpsc::channel::<AuthenticatedRelayEvent>(RelayServer::CHANNEL_CAPACITY);
        let relay = RelayServer::bind(&paths, relay_tx).await.ok();

        let telemetry_sink = TauriTelemetrySink::new(app.clone());
        let telemetry = TelemetryAggregator::new(
            Arc::clone(&agents_settings),
            telemetry_sink,
            Some(db.clone()),
        );
        let _ = telemetry.hydrate_from_db().await;

        let patterns = Arc::new(PatternSet::load(&paths));
        let attention_sink = TauriAttentionSink::new(app.clone());
        let attention = Arc::new(AttentionDetector::new(
            patterns,
            Arc::clone(&registry),
            tab_state_port,
            notify_port,
            Arc::clone(&pty_port) as Arc<dyn PtyPort>,
            attention_sink,
            attention_settings,
        ));

        let home_dir =
            std::env::var_os("HOME").map_or_else(|| paths.config_dir.clone(), PathBuf::from);
        let skills = SkillsService::new(
            db.clone(),
            Arc::clone(&workspace_port) as Arc<dyn WorkspacePort>,
            paths.clone(),
            home_dir,
            Arc::clone(&skills_settings),
        );

        let memory = Arc::new(CommandMemory::new(
            db.clone(),
            Arc::clone(&workspace_port) as Arc<dyn WorkspacePort>,
        ));

        let runtime = Arc::new(AgentRuntimeIndex::new());

        let service = Arc::new(AgentService {
            registry,
            detector,
            planner,
            isolation,
            relay,
            telemetry: Arc::clone(&telemetry),
            attention: Arc::clone(&attention),
            skills: Arc::clone(&skills),
            memory: Arc::clone(&memory),
            settings: agents_settings,
            runtime: Arc::clone(&runtime),
        });

        let skills_sink = TauriSkillsEventSink::new(app.clone());
        let watcher =
            SkillsWatcher::start(Arc::clone(&skills), skills_sink as Arc<dyn SkillsEventSink>)
                .map(Arc::new);

        let state = Self {
            service: Arc::clone(&service),
            workspace: workspace_manager.clone(),
            db,
            app: app.clone(),
            paths,
            skills_settings,
            watcher,
            staged_launches: Arc::new(Mutex::new(HashMap::new())),
            pane_reservations: Arc::new(Mutex::new(HashMap::new())),
        };

        workspace_manager
            .spawn_hooks()
            .register(Arc::new(AgentSpawnHook {
                state: state.clone(),
            }));

        let observer = Arc::new(AgentWorkspaceObserver {
            state: state.clone(),
            pty_port: pty_port as Arc<dyn PtyPort>,
        });
        workspace_manager.register_observer(observer.clone());
        pty_manager.add_listener(observer);

        let tap = AgentOutputTap::new(runtime, Arc::clone(&attention));
        let _ = pty_manager.add_output_tap(tap as Arc<dyn OutputTap>);

        let mem_listener = MemoryShellEventListener::new(app.clone(), workspace_manager, memory);
        shell_sink.add_listener(mem_listener);

        // Processador de eventos do RelayServer
        let tel_for_relay = Arc::clone(&telemetry);
        let att_for_relay = Arc::clone(&attention);
        let settings_for_relay = Arc::clone(&service.settings);
        tauri::async_runtime::spawn(async move {
            while let Some(ev) = relay_rx.recv().await {
                match ev.message {
                    RelayMessage::Statusline { payload } => {
                        let (upd, hint) =
                            ClaudeStatusLineParser::parse(&ev.binding, &payload, ev.received_at_ms);
                        let _ = tel_for_relay.apply(upd, ev.received_at_ms);
                        if let Some(h) = hint
                            && let Some(path) = h.transcript_path
                            && settings_for_relay.read().telemetry.read_session_files
                            && let Ok(mut tail) = JsonlTail::open_from_end(path).await
                            && let Ok(batch) = tail.read_new().await
                            && let Some(tr_upd) = ClaudeTranscriptSource::parse_records(
                                &ev.binding,
                                &batch.records,
                                batch.reset_detected,
                                ev.received_at_ms,
                            )
                        {
                            let _ = tel_for_relay.apply(tr_upd, ev.received_at_ms);
                        }
                    }
                    RelayMessage::Hook { event, payload } => {
                        let sig = match event.as_str() {
                            "Notification" => {
                                let msg = payload
                                    .get("message")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string);
                                let ntype = payload
                                    .get("notification_type")
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string);
                                let sub = payload
                                    .get("subagent_id")
                                    .or_else(|| payload.get("agent_id"))
                                    .and_then(serde_json::Value::as_str)
                                    .map(str::to_string);
                                Some(AttentionSignal::HookNotification {
                                    message: msg,
                                    notification_type: ntype,
                                    subagent_id: sub,
                                })
                            }
                            "Stop" => {
                                let is_sub = payload
                                    .get("subagent_id")
                                    .or_else(|| payload.get("agent_id"))
                                    .and_then(serde_json::Value::as_str)
                                    .is_some();
                                Some(AttentionSignal::HookStop {
                                    is_subagent: is_sub,
                                })
                            }
                            "UserPromptSubmit" => Some(AttentionSignal::HookUserPromptSubmit),
                            "PreToolUse" => Some(AttentionSignal::HookPreToolUse),
                            _ => None,
                        };
                        if let Some(s) = sig {
                            att_for_relay
                                .on_signal(ev.binding.tab_id, s, ev.received_at_ms)
                                .await;
                        }
                    }
                    RelayMessage::CodexNotify { payload } => {
                        let thread_id = payload
                            .get("thread-id")
                            .or_else(|| payload.get("thread_id"))
                            .and_then(serde_json::Value::as_str);
                        if let Some(codex_upd) = CodexSessionSource::parse_records(
                            &ev.binding,
                            thread_id,
                            std::slice::from_ref(&payload),
                            false,
                            ev.received_at_ms,
                        ) {
                            let _ = tel_for_relay.apply(codex_upd, ev.received_at_ms);
                        }
                        let last_msg = payload
                            .get("last-assistant-message")
                            .or_else(|| payload.get("last_assistant_message"))
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_string);
                        att_for_relay
                            .on_signal(
                                ev.binding.tab_id,
                                AttentionSignal::CodexTurnComplete {
                                    last_message: last_msg,
                                },
                                ev.received_at_ms,
                            )
                            .await;
                    }
                }
            }
        });

        // Ticker de silêncio do AttentionDetector (200 ms)
        let att_for_tick = Arc::clone(&attention);
        tauri::async_runtime::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(200));
            interval.tick().await;
            loop {
                interval.tick().await;
                att_for_tick.on_silence_tick(current_unix_ms());
            }
        });

        Ok(state)
    }

    /// Abre uma nova aba de Agente CLI com suporte a isolamento opcional em Git Worktree (`agent_tab_open`).
    #[allow(clippy::too_many_lines)]
    pub async fn open_agent_tab(&self, req: AgentTabOpenRequest) -> MtResult<AgentTabOpened> {
        let now_ms = current_unix_ms();
        let settings = self.service.settings.read().clone();
        if !settings.enabled.get(&req.agent).copied().unwrap_or(true) {
            return Err(agent_err::settings_invalid(
                format!(
                    "O agente {} está desabilitado nas preferências.",
                    req.agent.display_name()
                ),
                None,
                None,
            ));
        }

        let infos = self
            .service
            .detector
            .list(&self.service.registry, &settings)
            .await;
        let info = infos
            .into_iter()
            .find(|i| i.descriptor.kind == req.agent)
            .ok_or_else(|| agent_err::not_installed(req.agent.display_name()))?;

        if !info.installation.installed {
            return Err(agent_err::not_installed(&info.descriptor.display_name));
        }

        let _ = self
            .service
            .isolation
            .ensure_templates(req.project_id)
            .await?;

        let template = match req.template_id {
            Some(tid) => {
                let tpl = self.workspace.template_get(tid).await?;
                if tpl.project_id != req.project_id {
                    return Err(agent_err::tab_not_agent(tid.to_string()));
                }
                if tpl.config.environment.kind != EnvironmentKind::Agent
                    || tpl.config.environment.agent_id.as_deref() != Some(req.agent.as_ref_id())
                {
                    return Err(agent_err::tab_not_agent(tid.to_string()));
                }
                tpl
            }
            None => {
                let list = self.workspace.template_list(req.project_id).await?;
                list.into_iter()
                    .find(|t| {
                        t.config.environment.kind == EnvironmentKind::Agent
                            && t.config.environment.agent_id.as_deref()
                                == Some(req.agent.as_ref_id())
                    })
                    .ok_or_else(|| agent_err::tab_not_agent(req.agent.as_ref_id()))?
            }
        };

        let opts = get_template_options_in_db(&self.db, template.id).await?;

        let effective_model = req.model.clone().or_else(|| opts.model.clone());
        let _ = LaunchPlanner::resolve_cli_model(&info.descriptor, effective_model.as_deref())?;

        let project_rec = self
            .service
            .isolation
            .project_record(req.project_id)
            .await?;
        let prepared = self
            .service
            .isolation
            .prepare(&project_rec, req.agent, &req.isolation)
            .await?;

        let target_cwd = prepared
            .as_ref()
            .map_or_else(|| project_rec.path.clone(), |p| p.worktree_path.clone());

        let launch_uuid = Uuid::now_v7();
        let launch_id_str = prepared
            .as_ref()
            .map_or_else(|| launch_uuid.to_string(), |p| p.launch_id.clone());
        let run_id = Uuid::now_v7().to_string();

        let dummy_pending_binding = PendingRunBinding {
            tab_id: TabId::new(),
            pane_id: PaneId::new(),
            session_id: SessionId::new(),
            generation: 1,
            project_id: req.project_id,
            run_id: run_id.clone(),
            agent: req.agent,
            cwd: target_cwd.clone(),
            started_at_ms: now_ms,
        };
        let pending_launch = PendingLaunch {
            launch_id: launch_uuid,
            agent: req.agent,
            model: effective_model,
            extra_args: opts.extra_args.clone(),
            link: prepared.as_ref().map(|p| p.to_link_draft()),
        };
        let relay_sock = self.service.relay.as_ref().map_or_else(
            || self.paths.cache_dir.join("agent-relay.sock"),
            |r| r.socket_path().to_path_buf(),
        );
        let dummy_token = RelayToken::generate().0;

        let plan = self.service.planner.build_plan(
            &info.descriptor,
            &info.installation,
            &opts,
            Some(&pending_launch),
            dummy_pending_binding,
            dummy_token,
            &relay_sock,
            &settings,
        )?;

        let shell_kind = detect_user_shell();
        let full_cmd = command_line(
            &plan,
            template.config.initial_command.as_deref(),
            shell_kind,
        )?;

        self.staged_launches.lock().insert(
            launch_id_str.clone(),
            StagedLaunch {
                agent: req.agent,
                run_id,
                cwd: target_cwd.clone(),
                configured_model: plan.configured_model.clone(),
            },
        );

        let mut extra_env = BTreeMap::new();
        extra_env.insert(
            "MAXTERMINAL_AGENT_LAUNCH".to_string(),
            EnvValue::Literal {
                value: launch_id_str.clone(),
            },
        );

        let open_res = self
            .workspace
            .tab_open(TabOpenRequest {
                request_id: launch_id_str.clone(),
                project_id: req.project_id,
                template_id: Some(template.id),
                overrides: Some(TabOpenOverrides {
                    title: req.title.clone(),
                    cwd: Some(target_cwd.to_string_lossy().into_owned()),
                    initial_command: CommandOverride::Replace { command: full_cmd },
                    extra_env,
                    continuous: None,
                    cwd_fallback_to_project: false,
                }),
                focus: req.focus,
            })
            .await;

        match open_res {
            Ok(tab_view) => {
                let draft = prepared.as_ref().map(|p| p.to_link_draft());
                let link = self
                    .service
                    .isolation
                    .record_tab_link(
                        tab_view.id,
                        req.project_id,
                        req.agent,
                        draft.as_ref(),
                        prepared.as_ref().map(|p| p.launch_id.as_str()),
                        now_ms,
                    )
                    .await?;
                let _ = AgentLinkChangedTauriEvent(link.clone()).emit(&self.app);
                Ok(AgentTabOpened {
                    tab: tab_view,
                    link: Some(link),
                })
            }
            Err(err) => {
                self.staged_launches.lock().remove(&launch_id_str);
                if let Some(ref prep) = prepared {
                    let _ = self.service.isolation.rollback(prep).await;
                }
                Err(err)
            }
        }
    }
}
