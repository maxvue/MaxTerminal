//! Adaptadores Tauri das portas de `mt_commands::groups` (Fase 15).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use mt_agents::detect::current_unix_ms;
use mt_agents::memory::CommandMemory;
use mt_ai::AiService;
use mt_ai::ids::AiRequestId;
use mt_ai::structured::StructuredRequest;
use mt_commands::groups::model::{CommandGroupInput, GroupErrorCode};
use mt_commands::groups::ports::{
    GroupAiPort, GroupPtyPort, GroupShellPort, GroupTabOpenSpec, GroupTabPort, HeadlessSpawnSpec,
    OpenedGroupTab, PaneExecutionLease, PaneShellState, ProjectShellTemplate, RawOutputTap,
    RecurringCommandEntry, RecurringCommandsSource, ShellLifecycleEvent, SuggestPromptContext,
    SupervisedExitInfo,
};
use mt_commands::groups::suggest::AI_SUGGEST_SCHEMA_ID;
use mt_core::error::{ErrorCode, MtError, MtResult};
use mt_core::ids::{PaneId, ProjectId, PtyId, SessionId, TabId};
use mt_core::shell::{
    BlockStatus, CommandFinished, CommandStarted, IntegrationMode, ShellCwdChangedEvent,
    ShellIntegrationModeEvent, TabStateRemovedEvent, TabStateSnapshot, TabStatus,
};
use mt_core::workspace::{
    CommandOverride, EnvValue, EnvironmentKind, InitialDispatchState, TabOpenOverrides,
    TabOpenRequest,
};
use mt_projects::ProjectService;
use mt_pty::{
    KillMode, OutputTap, PreparedSpawn, PtyEvent, PtyEventListener, PtyManager, PtyOwner,
    PtyResult, PtySize, SpawnLifecycle, SpawnSpec, TapHandle,
};
use mt_shell::{ShellEventSink, ShellService};
use parking_lot::{Mutex, RwLock};
use serde_json::json;
use tokio::sync::{broadcast, mpsc};

use crate::workspace::WorkspaceManager;

type TapEntry = (u64, Arc<dyn RawOutputTap>);

struct GroupPtyBridge {
    taps: RwLock<HashMap<PtyId, Vec<TapEntry>>>,
    next_tap_id: std::sync::atomic::AtomicU64,
    exits: Mutex<HashMap<PtyId, SupervisedExitInfo>>,
    exit_tx: broadcast::Sender<SupervisedExitInfo>,
}

impl OutputTap for GroupPtyBridge {
    fn on_output(&self, pty_id: PtyId, _offset_start: u64, chunk: &[u8]) {
        let targets: Vec<Arc<dyn RawOutputTap>> = self
            .taps
            .read()
            .get(&pty_id)
            .map(|list| list.iter().map(|(_, tap)| Arc::clone(tap)).collect())
            .unwrap_or_default();
        for tap in targets {
            tap.on_bytes(chunk);
        }
    }
}

impl PtyEventListener for GroupPtyBridge {
    fn on_event(&self, event: &PtyEvent) {
        if let PtyEvent::Exited { pty_id, exit, .. } = event {
            let drained = self.taps.write().remove(pty_id).unwrap_or_default();
            for (_, tap) in drained {
                tap.on_eof();
            }
            let info = SupervisedExitInfo {
                pty_id: *pty_id,
                exit_code: exit.code,
                signal: exit.signal,
                tree_cleaned: true,
            };
            self.exits.lock().insert(*pty_id, info.clone());
            let _ = self.exit_tx.send(info);
        }
    }
}

struct HeadlessSpawnHook {
    bridge: Arc<GroupPtyBridge>,
    tap: Arc<dyn RawOutputTap>,
}

impl SpawnLifecycle for HeadlessSpawnHook {
    fn prepare(&self, spawn: &mut PreparedSpawn) -> PtyResult<()> {
        let id = self
            .bridge
            .next_tap_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.bridge
            .taps
            .write()
            .entry(spawn.pty_id)
            .or_default()
            .push((id, Arc::clone(&self.tap)));
        Ok(())
    }
}

struct AttachedTapGuard {
    bridge: Arc<GroupPtyBridge>,
    pty_id: PtyId,
    tap_id: u64,
}

impl Drop for AttachedTapGuard {
    fn drop(&mut self) {
        let mut guard = self.bridge.taps.write();
        if let Some(list) = guard.get_mut(&self.pty_id) {
            list.retain(|(id, _)| *id != self.tap_id);
            if list.is_empty() {
                guard.remove(&self.pty_id);
            }
        }
    }
}

/// Adaptador de [`GroupPtyPort`] apoiado no [`PtyManager`] nativo.
pub struct TauriGroupPtyAdapter {
    pty_manager: Arc<PtyManager>,
    bridge: Arc<GroupPtyBridge>,
    _tap_handle: Mutex<Option<TapHandle>>,
}

impl TauriGroupPtyAdapter {
    /// Cria e registra o adaptador de PTY no [`PtyManager`].
    #[must_use]
    pub fn new(pty_manager: Arc<PtyManager>) -> Arc<Self> {
        let (exit_tx, _) = broadcast::channel(256);
        let bridge = Arc::new(GroupPtyBridge {
            taps: RwLock::new(HashMap::new()),
            next_tap_id: std::sync::atomic::AtomicU64::new(1),
            exits: Mutex::new(HashMap::new()),
            exit_tx,
        });
        let tap_handle = pty_manager.add_output_tap(Arc::clone(&bridge) as Arc<dyn OutputTap>);
        pty_manager.add_listener(Arc::clone(&bridge) as Arc<dyn PtyEventListener>);

        Arc::new(Self {
            pty_manager,
            bridge,
            _tap_handle: Mutex::new(Some(tap_handle)),
        })
    }
}

#[async_trait]
impl GroupPtyPort for TauriGroupPtyAdapter {
    async fn spawn_headless(
        &self,
        spec: HeadlessSpawnSpec,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<PtyId> {
        let pty_spec = SpawnSpec {
            shell: Some(spec.program.to_string_lossy().into_owned()),
            args: spec.args,
            login: spec.login,
            cwd: Some(spec.cwd.to_string_lossy().into_owned()),
            env: spec.env,
            env_remove: Vec::new(),
            size: PtySize::default(),
            owner: PtyOwner {
                tab_id: None,
                pane_id: None,
            },
        };
        let hook = HeadlessSpawnHook {
            bridge: Arc::clone(&self.bridge),
            tap: output_tap,
        };
        let summary = self
            .pty_manager
            .spawn_with_lifecycle(pty_spec, Some(&hook))
            .map_err(|e| {
                GroupErrorCode::SpawnFailed.into_error(format!("Falha ao criar PTY headless: {e}"))
            })?;
        Ok(summary.id)
    }

    async fn wait_exit(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo> {
        let mut rx = self.bridge.exit_tx.subscribe();
        if let Some(existing) = self.bridge.exits.lock().get(pty_id).cloned() {
            return Ok(existing);
        }
        loop {
            match rx.recv().await {
                Ok(info) if info.pty_id == *pty_id => return Ok(info),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    if let Some(existing) = self.bridge.exits.lock().get(pty_id).cloned() {
                        return Ok(existing);
                    }
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Ok(SupervisedExitInfo {
                        pty_id: *pty_id,
                        exit_code: None,
                        signal: Some(9),
                        tree_cleaned: true,
                    });
                }
            }
        }
    }

    async fn write_to_pty(&self, pty_id: &PtyId, data: &[u8]) -> MtResult<()> {
        self.pty_manager
            .write(*pty_id, data)
            .map(|_| ())
            .map_err(|e| MtError::new(ErrorCode("pty.write_failed"), e.to_string()))
    }

    async fn kill_graceful(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo> {
        match self.pty_manager.kill(*pty_id, KillMode::Graceful) {
            Ok(outcome) => Ok(SupervisedExitInfo {
                pty_id: *pty_id,
                exit_code: outcome.code,
                signal: outcome.signal.or(Some(15)),
                tree_cleaned: true,
            }),
            Err(_) => {
                if let Some(existing) = self.bridge.exits.lock().get(pty_id).cloned() {
                    Ok(existing)
                } else {
                    Ok(SupervisedExitInfo {
                        pty_id: *pty_id,
                        exit_code: None,
                        signal: Some(15),
                        tree_cleaned: true,
                    })
                }
            }
        }
    }

    async fn attach_item_tap(
        &self,
        pty_id: &PtyId,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<Box<dyn Send + Sync>> {
        let tap_id = self
            .bridge
            .next_tap_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.bridge
            .taps
            .write()
            .entry(*pty_id)
            .or_default()
            .push((tap_id, output_tap));
        Ok(Box::new(AttachedTapGuard {
            bridge: Arc::clone(&self.bridge),
            pty_id: *pty_id,
            tap_id,
        }))
    }
}

struct TauriPaneLease {
    state: PaneShellState,
    rx: broadcast::Receiver<ShellLifecycleEvent>,
    leases: Arc<Mutex<HashSet<PaneId>>>,
    released: bool,
}

impl Drop for TauriPaneLease {
    fn drop(&mut self) {
        if !self.released {
            self.leases.lock().remove(&self.state.pane_id);
        }
    }
}

#[async_trait]
impl PaneExecutionLease for TauriPaneLease {
    fn state(&self) -> &PaneShellState {
        &self.state
    }

    async fn next_event(&mut self) -> Option<ShellLifecycleEvent> {
        loop {
            match self.rx.recv().await {
                Ok(ev) => {
                    let matches = match &ev {
                        ShellLifecycleEvent::BlockStarted { pty_id, .. }
                        | ShellLifecycleEvent::BlockFinished { pty_id, .. }
                        | ShellLifecycleEvent::PromptReady { pty_id, .. } => {
                            *pty_id == self.state.pty_id
                        }
                        ShellLifecycleEvent::GenerationInvalidated { pane_id, .. } => {
                            *pane_id == self.state.pane_id
                        }
                    };
                    if matches {
                        return Some(ev);
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    }

    async fn release(mut self: Box<Self>) {
        self.leases.lock().remove(&self.state.pane_id);
        self.released = true;
    }
}

/// Adaptador de [`GroupShellPort`] apoiado no [`ShellService`], [`WorkspaceManager`] e [`ShellEventSink`].
pub struct TauriGroupShellAdapter {
    shell: Arc<ShellService>,
    workspace: WorkspaceManager,
    leases: Arc<Mutex<HashSet<PaneId>>>,
    tx: broadcast::Sender<ShellLifecycleEvent>,
}

impl TauriGroupShellAdapter {
    /// Cria um novo adaptador de shell para grupos de comandos.
    #[must_use]
    pub fn new(shell: Arc<ShellService>, workspace: WorkspaceManager) -> Arc<Self> {
        let (tx, _) = broadcast::channel(256);
        Arc::new(Self {
            shell,
            workspace,
            leases: Arc::new(Mutex::new(HashSet::new())),
            tx,
        })
    }

    fn generation_for_pane(&self, pane_id: Option<PaneId>) -> u64 {
        pane_id
            .and_then(|pid| self.workspace.pane_runtime_meta(pid))
            .map_or(1, |(_, _, _, g, _)| g)
    }
}

impl ShellEventSink for TauriGroupShellAdapter {
    fn on_tab_state_changed(&self, _snapshot: &TabStateSnapshot) {}
    fn on_tab_state_removed(&self, _event: &TabStateRemovedEvent) {}
    fn on_cwd_changed(&self, _event: &ShellCwdChangedEvent) {}
    fn on_integration_mode_changed(&self, _event: &ShellIntegrationModeEvent) {}

    fn on_command_started(&self, event: &CommandStarted) {
        let generation = self.generation_for_pane(event.binding.pane_id);
        let _ = self.tx.send(ShellLifecycleEvent::BlockStarted {
            pty_id: event.binding.pty_id,
            session_id: event.binding.session_id,
            block_id: event.execution.block_id,
            generation,
        });
    }

    fn on_command_finished(&self, event: &CommandFinished) {
        let generation = self.generation_for_pane(event.binding.pane_id);
        let exit_code = event
            .exit_code
            .unwrap_or(if event.status == BlockStatus::Succeeded {
                0
            } else {
                1
            });
        let _ = self.tx.send(ShellLifecycleEvent::BlockFinished {
            pty_id: event.binding.pty_id,
            session_id: event.binding.session_id,
            block_id: event.block_id,
            generation,
            exit_code,
            signal: None,
        });
        let _ = self.tx.send(ShellLifecycleEvent::PromptReady {
            pty_id: event.binding.pty_id,
            session_id: event.binding.session_id,
            generation,
        });
    }
}

static ENV_CHDIR_SUPPORTED: OnceLock<bool> = OnceLock::new();

#[async_trait]
impl GroupShellPort for TauriGroupShellAdapter {
    async fn inspect_pane(&self, pane_id: &PaneId) -> MtResult<PaneShellState> {
        let (tab_id, pty_id, session_id, generation, _disp) =
            self.workspace.pane_runtime_meta(*pane_id).ok_or_else(|| {
                GroupErrorCode::NotFound.into_error(format!(
                    "Painel '{pane_id}' não encontrado ou sem PTY ativo."
                ))
            })?;

        let info = self.shell.integration_info(pty_id).ok();
        let tracking_available = info
            .as_ref()
            .is_some_and(|i| i.mode == IntegrationMode::Osc133 && i.osc_events_received > 0);
        let prompt_ready = self.shell.is_prompt_ready(pty_id);
        let foreground_busy = self
            .shell
            .tab_state_get(tab_id)
            .ok()
            .and_then(|snap| snap.panes.into_iter().find(|p| p.pane_id == Some(*pane_id)))
            .is_some_and(|p| matches!(p.status, TabStatus::Running | TabStatus::Continuous));

        let shell_path = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from("/bin/bash"));

        Ok(PaneShellState {
            tab_id,
            pane_id: *pane_id,
            pty_id,
            session_id,
            shell_path,
            generation,
            prompt_ready,
            in_alt_screen: false,
            foreground_busy,
            tracking_available,
        })
    }

    async fn acquire_pane_lease(&self, pane_id: &PaneId) -> MtResult<Box<dyn PaneExecutionLease>> {
        let state = self.inspect_pane(pane_id).await?;
        if !state.tracking_available {
            return Err(GroupErrorCode::TrackingUnavailable.into_error(
                "Shell Integration (OSC 133) indisponível neste painel para execução sequencial.",
            ));
        }
        if !state.prompt_ready || state.in_alt_screen || state.foreground_busy {
            return Err(GroupErrorCode::PaneBusy
                .into_error("O painel selecionado está ocupado com outro processo em execução."));
        }

        {
            let mut leases = self.leases.lock();
            if !leases.insert(*pane_id) {
                return Err(GroupErrorCode::PaneBusy
                    .into_error("Já existe uma fila sequencial ativa neste painel."));
            }
        }

        Ok(Box::new(TauriPaneLease {
            state,
            rx: self.tx.subscribe(),
            leases: Arc::clone(&self.leases),
            released: false,
        }))
    }

    async fn subscribe_session_events(
        &self,
        session_id: &SessionId,
    ) -> MtResult<mpsc::UnboundedReceiver<ShellLifecycleEvent>> {
        let (sender, receiver) = mpsc::unbounded_channel();
        let mut rx = self.tx.subscribe();
        let target_sid = *session_id;
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let matches = match &ev {
                            ShellLifecycleEvent::BlockStarted {
                                session_id: sid, ..
                            }
                            | ShellLifecycleEvent::BlockFinished {
                                session_id: sid, ..
                            }
                            | ShellLifecycleEvent::PromptReady {
                                session_id: sid, ..
                            } => *sid == target_sid,
                            ShellLifecycleEvent::GenerationInvalidated { .. } => false,
                        };
                        if matches && sender.send(ev).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        Ok(receiver)
    }

    fn supports_env_chdir(&self) -> bool {
        *ENV_CHDIR_SUPPORTED.get_or_init(|| {
            std::process::Command::new("env")
                .args(["-C", "/", "true"])
                .status()
                .is_ok_and(|s| s.success())
        })
    }
}

/// Adaptador de [`GroupTabPort`] apoiado no [`WorkspaceManager`] e [`ProjectService`].
pub struct TauriGroupTabAdapter {
    workspace: WorkspaceManager,
    projects: Arc<ProjectService>,
}

impl TauriGroupTabAdapter {
    /// Cria um novo adaptador de abas/painéis para grupos de comandos.
    #[must_use]
    pub fn new(workspace: WorkspaceManager, projects: Arc<ProjectService>) -> Arc<Self> {
        Arc::new(Self {
            workspace,
            projects,
        })
    }
}

#[async_trait]
impl GroupTabPort for TauriGroupTabAdapter {
    async fn resolve_project_root(&self, project_id: &ProjectId) -> MtResult<PathBuf> {
        let list = self.projects.list().await?;
        let found = list
            .into_iter()
            .find(|p| p.id == *project_id && !p.missing)
            .ok_or_else(|| {
                GroupErrorCode::NotFound.into_error(format!(
                    "Projeto '{project_id}' não encontrado ou ausente no disco."
                ))
            })?;
        Ok(PathBuf::from(found.path))
    }

    async fn list_project_templates(
        &self,
        project_id: &ProjectId,
    ) -> MtResult<Vec<ProjectShellTemplate>> {
        let templates = self.workspace.template_list(*project_id).await?;
        let shell_path = std::env::var_os("SHELL")
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .unwrap_or_else(|| PathBuf::from("/bin/bash"));

        let mut out = Vec::with_capacity(templates.len());
        for tpl in templates {
            let mut base_env = BTreeMap::new();
            for (k, val) in tpl.config.env_vars {
                match val {
                    EnvValue::Literal { value } => {
                        base_env.insert(k, value);
                    }
                    EnvValue::Inherited { source } => {
                        if let Ok(v) = std::env::var(&source) {
                            base_env.insert(k, v);
                        }
                    }
                }
            }
            out.push(ProjectShellTemplate {
                id: tpl.id,
                project_id: tpl.project_id,
                name: tpl.config.name,
                is_default: tpl.is_default,
                is_shell_environment: tpl.config.environment.kind == EnvironmentKind::Shell,
                continuous: tpl.config.continuous,
                shell_path: shell_path.clone(),
                base_env,
            });
        }
        Ok(out)
    }

    async fn open_tab(&self, spec: GroupTabOpenSpec) -> MtResult<OpenedGroupTab> {
        let extra_env = spec
            .extra_env
            .into_iter()
            .map(|(k, v)| (k, EnvValue::Literal { value: v }))
            .collect();
        let req_id = uuid::Uuid::now_v7().to_string();
        let req = TabOpenRequest {
            request_id: req_id.clone(),
            project_id: spec.project_id,
            template_id: Some(spec.template_id),
            overrides: Some(TabOpenOverrides {
                title: Some(spec.title),
                cwd: Some(spec.cwd.to_string_lossy().into_owned()),
                initial_command: CommandOverride::Replace {
                    command: spec.initial_command,
                },
                extra_env,
                continuous: None,
                cwd_fallback_to_project: spec.cwd_fallback_to_project,
            }),
            focus: spec.focus,
        };

        let view = self.workspace.tab_open(req).await.map_err(|e| {
            GroupErrorCode::TabOpenFailed.into_error(format!(
                "Falha ao abrir aba no WorkspaceManager: {}",
                e.message
            ))
        })?;
        let primary_pane_id = view.primary_pane_id;
        let (_tab_id, pty_id, session_id, generation, _disp) = self
            .workspace
            .pane_runtime_meta(primary_pane_id)
            .ok_or_else(|| {
                GroupErrorCode::TabOpenFailed
                    .into_error("Painel primário da aba recém-aberta não possui PTY ativo.")
            })?;

        let _ = self
            .workspace
            .workspace_initial_command_run(
                view.id,
                primary_pane_id,
                i64::try_from(generation).unwrap_or(1),
                format!("{req_id}:init"),
            )
            .await;

        Ok(OpenedGroupTab {
            tab_id: view.id,
            pane_id: primary_pane_id,
            pty_id,
            session_id,
            initial_block_id: None,
        })
    }

    async fn close_tab_if_running_initial(
        &self,
        tab_id: &TabId,
        expected_session_id: &SessionId,
        expected_pty_id: &PtyId,
    ) -> MtResult<bool> {
        let pane_ids = self.workspace.tab_pane_ids(*tab_id);
        let Some(&first_pane) = pane_ids.first() else {
            return Ok(false);
        };
        let Some((_, pty_id, session_id, _, disp)) = self.workspace.pane_runtime_meta(first_pane)
        else {
            return Ok(false);
        };
        if session_id != *expected_session_id || pty_id != *expected_pty_id {
            return Ok(false);
        }
        let _ = disp == InitialDispatchState::Sent;
        self.workspace
            .tab_close(*tab_id, uuid::Uuid::now_v7().to_string())
            .await?;
        Ok(true)
    }
}

/// Adaptador de [`GroupAiPort`] apoiado no [`AiService`] (`ai_generate_structured`).
pub struct TauriGroupAiAdapter {
    ai: Arc<AiService>,
}

impl TauriGroupAiAdapter {
    /// Cria um novo adaptador de IA para grupos de comandos.
    #[must_use]
    pub fn new(ai: Arc<AiService>) -> Arc<Self> {
        Arc::new(Self { ai })
    }
}

fn suggestion_json_schema() -> serde_json::Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": ["scope", "name", "description", "labels", "mode", "separator", "env", "items"],
        "properties": {
            "scope": { "type": "string", "enum": ["local", "global"] },
            "projectId": { "type": ["string", "null"] },
            "name": { "type": "string", "minLength": 1, "maxLength": 80 },
            "description": { "type": "string", "maxLength": 2048 },
            "labels": {
                "type": "array",
                "maxItems": 20,
                "items": { "type": "string", "minLength": 1, "maxLength": 32 }
            },
            "mode": { "type": "string", "enum": ["sequential", "parallel", "multi_tabs"] },
            "separator": { "type": "string", "enum": ["and", "semicolon"] },
            "cwd": { "type": ["string", "null"] },
            "env": {
                "type": "object",
                "additionalProperties": { "type": "string" }
            },
            "items": {
                "type": "array",
                "minItems": 1,
                "maxItems": 50,
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["title", "command", "env", "continuous"],
                    "properties": {
                        "id": { "type": ["string", "null"] },
                        "title": { "type": "string", "minLength": 1, "maxLength": 80 },
                        "command": { "type": "string", "minLength": 1, "maxLength": 8192 },
                        "cwd": { "type": ["string", "null"] },
                        "env": {
                            "type": "object",
                            "additionalProperties": { "type": "string" }
                        },
                        "continuous": { "type": "boolean" }
                    }
                }
            }
        }
    })
}

#[async_trait]
impl GroupAiPort for TauriGroupAiAdapter {
    async fn generate_group_suggestion(
        &self,
        ai_request_id: &str,
        context: SuggestPromptContext,
    ) -> MtResult<CommandGroupInput> {
        let request_id = AiRequestId::parse(ai_request_id).unwrap_or_default();
        let prompt =
            serde_json::to_string_pretty(&context).unwrap_or_else(|_| context.goal.clone());
        let system = String::from(
            "Você é um assistente especialista em automação de terminal no MaxTerminal. \
             Gere um grupo de comandos estruturado em português do Brasil (pt-BR) para títulos e descrição, \
             usando apenas caminhos relativos em cwd e sem incluir segredos literais.",
        );

        let req = StructuredRequest {
            request_id,
            schema_id: String::from(AI_SUGGEST_SCHEMA_ID),
            schema: suggestion_json_schema(),
            prompt,
            system: Some(system),
            selection: None,
            pane_id: context.pane_id,
            project_id: context.project_id,
        };

        let res = self.ai.generate_structured(req).await?;
        serde_json::from_value::<CommandGroupInput>(res.value).map_err(|e| {
            GroupErrorCode::Validation.into_error(format!(
                "A resposta estruturada da IA não corresponde a CommandGroupInput: {e}"
            ))
        })
    }
}

/// Adaptador de [`RecurringCommandsSource`] apoiado na [`CommandMemory`] da Fase 13.
pub struct TauriRecurringCommandsAdapter {
    memory: Arc<CommandMemory>,
}

impl TauriRecurringCommandsAdapter {
    /// Cria um novo adaptador de comandos recorrentes.
    #[must_use]
    pub fn new(memory: Arc<CommandMemory>) -> Arc<Self> {
        Arc::new(Self { memory })
    }
}

#[async_trait]
impl RecurringCommandsSource for TauriRecurringCommandsAdapter {
    fn is_available(&self) -> bool {
        true
    }

    async fn list_recurring(
        &self,
        project_id: &ProjectId,
        limit: u32,
    ) -> MtResult<Vec<RecurringCommandEntry>> {
        let max_items = u16::try_from(limit).unwrap_or(15);
        let entries = self
            .memory
            .list(*project_id, false, max_items, current_unix_ms())
            .await?;
        Ok(entries
            .into_iter()
            .map(|e| RecurringCommandEntry {
                command: e.command,
                cwd: e.last_cwd,
                frequency: e.use_count,
                last_used_at: e.last_used_at_ms,
            })
            .collect())
    }
}
