//! Crate `mt-os`: integração com o sistema operacional Linux/CachyOS (Fase 09).
//!
//! Fornece a fachada `OsIntegration` e os módulos puros e assíncronos de:
//! - Configurações (`settings`)
//! - Detecção de ambiente gráfico e D-Bus (`environment`)
//! - Contadores e fila de atenção (`status`, `attention`)
//! - Bandeja do sistema (`tray`)
//! - Ciclo de vida e desligamento coordenado (`lifecycle`)
//! - Modo Quake / Drop-down (`quake`)
//! - Atalho global (`shortcut`)
//! - Argumentos CLI e single-instance (`cli`)
//! - Roteador de ativação por deep link (`activation`)
//! - Resolução de links do terminal e abertura no editor (`links`, `editor`)
//! - Notificações nativas e alerta de comandos longos (`notify`, `long_command`)
//! - Inicialização automática XDG (`autostart`)

pub mod activation;
pub mod attention;
pub mod autostart;
pub mod cli;
pub mod editor;
pub mod environment;
pub mod error;
pub mod lifecycle;
pub mod links;
pub mod long_command;
pub mod notify;
pub mod quake;
pub mod settings;
pub mod shortcut;
pub mod status;
pub mod tray;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use mt_core::error::{MtError, MtResult};
use mt_core::ids::{PaneId, ProjectId, TabId};
use mt_db::repos::OsSettingsRepo;
use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub use activation::{
    ACTIVATION_DEDUPE_WINDOW, ACTIVATION_ENVELOPE_TTL, ActivationEnvelope, ActivationHandler,
    ActivationRouter, ActivationSource, MAX_ACTIVATION_QUEUE_LEN, MAX_ACTIVATION_URI_BYTES,
};
pub use attention::{AttentionReason, AttentionRegistry};
pub use autostart::{
    AUTOSTART_DESKTOP_FILENAME, AutostartStatus, AutostartSync, render_desktop_entry,
};
pub use cli::{CliAction, parse_cli_args};
pub use editor::{
    EditorAvailability, EditorInvocation, EditorKind, EditorLauncher, EditorOpener,
    build_editor_invocation, find_in_path,
};
pub use environment::{Compositor, DisplayServer, OsEnvironment, classify_display, probe};
pub use error::{codes, os_error};
pub use lifecycle::{
    CloseContext, CloseDecision, QuitOutcome, QuitSource, ShutdownCoordinator, ShutdownHooks,
    ShutdownPhase, ShutdownReport, decide_close,
};
pub use links::{LinkCandidate, LinkContext, LinkResolver, ResolvedLink, TabCwdProvider};
pub use long_command::{CommandFinishedInfo, format_long_command_notification, should_notify};
pub use notify::{
    DbusNotificationTransport, LimitDecision, NotificationKind, NotificationTransport, Notifier,
    NotifyOutcome, NotifyRequest, RateLimiter, SanitizedNotification,
    sanitize_and_truncate_notification,
};
pub use quake::{
    DropDownGeometry, QuakeController, ToggleDecision, ToggleDecisionDto, ToggleSource, WindowHost,
    WindowVisibilityPayload, decide_toggle, drop_down_geometry,
};
pub use settings::{
    CloseBehavior, DEFAULT_QUAKE_ACCELERATOR, EditorSettings, NotificationSettings,
    NotificationSettingsPatch, OS_SETTINGS_SCHEMA_VERSION, OsSettings, OsSettingsPatch, QuakeMode,
    QuakeSettings, QuakeSettingsPatch, SettingsDiff, validate_custom_argv,
};
pub use shortcut::{
    Accelerator, CLI_TOGGLE_COMMAND, CliShortcutBackend, DefaultShortcutBackendFactory,
    GlobalShortcutBackend, ShortcutBackendFactory, ShortcutBackendKind, ShortcutBackendPreference,
    ShortcutManager, ShortcutStatus, compositor_hint_for,
};
pub use status::{ActivitySnapshot, StatusCounters, TabStatusEntry, TabStatusProvider};
pub use tray::{TrayController, TrayHost, TrayMenuTexts, TrayVisualState};

/// Envelope versionado emitido em todos os eventos `os://*`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OsEvent<T> {
    /// Revisão monotônica do evento na instância atual.
    pub revision: u32,
    /// Carga útil tipada do evento.
    pub payload: T,
}

/// Origem de uma solicitação de foco em aba (`os://focus-tab`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FocusTabSource {
    /// Clique em uma notificação nativa.
    Notification,
    /// Ação acionada a partir do menu da bandeja.
    Tray,
    /// Invocação via CLI ou deep link.
    Cli,
}

/// Payload do evento `os://focus-tab`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FocusTabPayload {
    /// Identificador da aba a ser focada.
    pub tab_id: TabId,
    /// Identificador do painel específico, quando aplicável.
    #[specta(optional)]
    pub pane_id: Option<PaneId>,
    /// Identificador do projeto ao qual pertence a aba.
    #[specta(optional)]
    pub project_id: Option<ProjectId>,
    /// Origem da solicitação de foco.
    pub source: FocusTabSource,
}

/// Payload do evento `os://open-preferences`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenPreferencesPayload {
    /// Seção inicial a ser exibida no diálogo de preferências (ex.: `"system"`).
    pub section: String,
}

/// Payload do evento `os://shutdown-started`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownStartedPayload {
    /// Origem do desligamento.
    pub source: QuitSource,
}

/// Payload do evento `os://shutdown-failed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ShutdownFailedPayload {
    /// Erro detalhando a falha de limpeza.
    pub error: MtError,
    /// Fase em que o coordenador permaneceu (`failed`).
    pub phase: ShutdownPhase,
}

/// Snapshot completo de runtime retornado por `os_runtime_snapshot` e `os_ui_ready`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OsRuntimeSnapshot {
    /// Revisão monotônica atual do subsistema `os`.
    pub revision: u32,
    /// Estado atual de visibilidade da janela principal.
    pub window: WindowVisibilityPayload,
    /// Ambiente gráfico e capacidades D-Bus detectadas.
    pub environment: OsEnvironment,
    /// Status atual do atalho global.
    pub shortcut_status: ShortcutStatus,
    /// Contadores globais agregados de abas.
    pub counters: StatusCounters,
    /// Fase atual do coordenador de desligamento.
    pub shutdown_phase: ShutdownPhase,
    /// Contextos ativos de resolução de links por painel.
    pub link_contexts: Vec<LinkContext>,
    /// Intenção de foco em aba pendente (recebida antes da hidratação da UI).
    pub pending_focus: Option<FocusTabPayload>,
}

/// Sink de eventos `os://*` implementado pelo crate binário Tauri.
pub trait OsEventSink: Send + Sync + 'static {
    /// Emite `os://focus-tab`.
    fn emit_focus_tab(&self, event: OsEvent<FocusTabPayload>);
    /// Emite `os://window-visibility`.
    fn emit_window_visibility(&self, event: OsEvent<WindowVisibilityPayload>);
    /// Emite `os://settings-changed`.
    fn emit_settings_changed(&self, event: OsEvent<OsSettings>);
    /// Emite `os://shortcut-status`.
    fn emit_shortcut_status(&self, event: OsEvent<ShortcutStatus>);
    /// Emite `os://counters`.
    fn emit_counters(&self, event: OsEvent<StatusCounters>);
    /// Emite `os://open-preferences`.
    fn emit_open_preferences(&self, event: OsEvent<OpenPreferencesPayload>);
    /// Emite `os://shutdown-started`.
    fn emit_shutdown_started(&self, event: OsEvent<ShutdownStartedPayload>);
    /// Emite `os://shutdown-failed`.
    fn emit_shutdown_failed(&self, event: OsEvent<ShutdownFailedPayload>);
    /// Emite `os://environment-changed`.
    fn emit_environment_changed(&self, event: OsEvent<OsEnvironment>);
    /// Emite `os://link-context-changed`.
    fn emit_link_context_changed(&self, event: OsEvent<LinkContext>);
}

/// Implementação no-op de `OsEventSink` para testes unitários.
#[derive(Debug, Default)]
pub struct NoopOsEventSink;

impl OsEventSink for NoopOsEventSink {
    fn emit_focus_tab(&self, _event: OsEvent<FocusTabPayload>) {}
    fn emit_window_visibility(&self, _event: OsEvent<WindowVisibilityPayload>) {}
    fn emit_settings_changed(&self, _event: OsEvent<OsSettings>) {}
    fn emit_shortcut_status(&self, _event: OsEvent<ShortcutStatus>) {}
    fn emit_counters(&self, _event: OsEvent<StatusCounters>) {}
    fn emit_open_preferences(&self, _event: OsEvent<OpenPreferencesPayload>) {}
    fn emit_shutdown_started(&self, _event: OsEvent<ShutdownStartedPayload>) {}
    fn emit_shutdown_failed(&self, _event: OsEvent<ShutdownFailedPayload>) {}
    fn emit_environment_changed(&self, _event: OsEvent<OsEnvironment>) {}
    fn emit_link_context_changed(&self, _event: OsEvent<LinkContext>) {}
}

/// Dependências injetadas na construção de `OsIntegration`.
pub struct OsDeps {
    /// Provedor de estados de abas e atividade.
    pub status: Arc<dyn TabStatusProvider>,
    /// Provedor de diretórios de trabalho por painel/PTY.
    pub cwd: Arc<dyn TabCwdProvider>,
    /// Hooks de encerramento do workspace, PTYs e SQLite.
    pub shutdown: Arc<dyn ShutdownHooks>,
    /// Repositório SQLite da linha única `os_settings`.
    pub settings_repo: Arc<OsSettingsRepo>,
    /// Adaptador da janela principal.
    pub window_host: Arc<dyn WindowHost>,
    /// Fábrica de backends de atalho global.
    pub shortcut_factory: Arc<dyn ShortcutBackendFactory>,
    /// Transporte de notificações nativas.
    pub notification_transport: Arc<dyn NotificationTransport>,
    /// Sincronizador de autostart XDG.
    pub autostart_sync: AutostartSync,
    /// Sink de eventos `os://*`.
    pub event_sink: Arc<dyn OsEventSink>,
    /// Diretório home do usuário para expansão de `~/`.
    pub home_dir: PathBuf,
    /// Ação CLI recebida no startup (`--show`, `--toggle`, `--hide`, `--start-hidden`, `None`).
    pub startup_cli_action: CliAction,
}

/// Fachada central da Fase 09 coordenando todos os subsistemas de integração com o SO.
pub struct OsIntegration {
    /// Configurações ativas em memória.
    pub settings: Arc<RwLock<OsSettings>>,
    raw_settings_obj: Mutex<Map<String, Value>>,
    settings_update_lock: tokio::sync::Mutex<()>,
    /// Ambiente gráfico e capacidades D-Bus detectadas.
    pub env: Arc<RwLock<OsEnvironment>>,
    /// Registro transversal de abas aguardando intervenção.
    pub attention: Arc<AttentionRegistry>,
    /// Controlador opcional da bandeja do sistema (`None` se indisponível).
    pub tray: RwLock<Option<Arc<TrayController>>>,
    /// Controlador de visibilidade e modo Quake / Drop-down.
    pub quake: Arc<QuakeController>,
    /// Gerenciador de atalho global.
    pub shortcuts: Arc<ShortcutManager>,
    shortcut_usable: Arc<AtomicBool>,
    /// Resolvedor de links clicáveis do terminal.
    pub links: Arc<LinkResolver>,
    /// Lançador de editores externos.
    pub editor: Arc<EditorLauncher>,
    /// Despachante de notificações nativas.
    pub notifier: Arc<Notifier>,
    /// Coordenador de encerramento seguro.
    pub shutdown: Arc<ShutdownCoordinator>,
    /// Roteador de deep links `maxterminal://`.
    pub activations: ActivationRouter,
    /// Sincronizador de inicialização automática (`autostart`).
    pub autostart_sync: AutostartSync,
    deps: OsDeps,
    revision: AtomicU32,
    ui_ready_completed: AtomicBool,
    link_contexts: RwLock<HashMap<PaneId, LinkContext>>,
    pending_focus: Mutex<Option<FocusTabPayload>>,
}

impl OsIntegration {
    /// Inicializa o subsistema `OsIntegration` carregando `os_settings`, sondando o ambiente e aplicando efeitos.
    pub async fn init(deps: OsDeps, initial_env: OsEnvironment) -> MtResult<Arc<Self>> {
        let persisted = deps.settings_repo.get().await.map_err(|e| {
            os_error(
                codes::SETTINGS_INVALID,
                format!("Falha ao ler os_settings: {e}"),
            )
        })?;

        let (mut loaded_settings, mut raw_obj) =
            OsSettings::from_persisted_row(persisted.schema_version, &persisted.value_json)
                .unwrap_or_else(|err| {
                    tracing::warn!(
                        target: "mt_os",
                        error = %err,
                        "Usando defaults de OsSettings devido a erro ao carregar registro inicial"
                    );
                    (OsSettings::default(), Map::new())
                });

        // Reconcilia autostart com o estado real do arquivo .desktop (fonte da verdade = sistema)
        if let Ok(actual_auto) = deps.autostart_sync.status()
            && loaded_settings.autostart != actual_auto.enabled
        {
            loaded_settings.autostart = actual_auto.enabled;
            if let Ok(merged) = loaded_settings.to_merged_json(&raw_obj) {
                let _ = deps
                    .settings_repo
                    .save(loaded_settings.schema_version, merged)
                    .await;
                if let Ok(Value::Object(m)) = serde_json::to_value(&loaded_settings) {
                    raw_obj = m;
                }
            }
        }

        let settings_arc = Arc::new(RwLock::new(loaded_settings.clone()));
        let env_arc = Arc::new(RwLock::new(initial_env.clone()));
        let attention = Arc::new(AttentionRegistry::new());
        let shortcut_usable = Arc::new(AtomicBool::new(false));

        let quake = Arc::new(QuakeController::new(
            Arc::clone(&deps.window_host),
            Arc::clone(&settings_arc),
            Arc::clone(&env_arc),
            Arc::clone(&shortcut_usable),
        ));

        let shortcuts = Arc::new(ShortcutManager::new(Arc::clone(&deps.shortcut_factory)));
        let links = Arc::new(LinkResolver::new(
            Arc::clone(&deps.cwd),
            deps.home_dir.clone(),
        ));
        let editor = Arc::new(EditorLauncher::new(Arc::clone(&settings_arc)));
        let notifier = Arc::new(Notifier::new(
            Arc::clone(&deps.notification_transport),
            Arc::clone(&settings_arc),
            Arc::clone(&env_arc),
        ));
        let shutdown = Arc::new(ShutdownCoordinator::new(Arc::clone(&deps.shutdown)));
        let autostart_sync = deps.autostart_sync.clone();

        let instance = Arc::new(Self {
            settings: settings_arc,
            raw_settings_obj: Mutex::new(raw_obj),
            settings_update_lock: tokio::sync::Mutex::new(()),
            env: env_arc,
            attention,
            tray: RwLock::new(None),
            quake: Arc::clone(&quake),
            shortcuts: Arc::clone(&shortcuts),
            shortcut_usable: Arc::clone(&shortcut_usable),
            links,
            editor,
            notifier,
            shutdown,
            activations: ActivationRouter::new(),
            autostart_sync,
            deps,
            revision: AtomicU32::new(1),
            ui_ready_completed: AtomicBool::new(false),
            link_contexts: RwLock::new(HashMap::new()),
            pending_focus: Mutex::new(None),
        });

        let quake_for_trigger = Arc::clone(&quake);
        let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
            let _ = quake_for_trigger.toggle(ToggleSource::Shortcut);
        });

        let sc_status = shortcuts
            .apply(&loaded_settings.quake, &initial_env, trigger)
            .await
            .unwrap_or_default();
        shortcut_usable.store(sc_status.registered, Ordering::Release);

        Ok(instance)
    }

    /// Incrementa e retorna a próxima revisão monotônica para eventos `os://*`.
    pub fn next_revision(&self) -> u32 {
        self.revision
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1)
    }

    /// Retorna a revisão atual do runtime.
    #[must_use]
    pub fn current_revision(&self) -> u32 {
        self.revision.load(Ordering::Acquire)
    }

    /// Registra o `TrayController` ativo após criação bem-sucedida da bandeja no binário.
    pub fn attach_tray(&self, tray: Arc<TrayController>) {
        self.env.write().tray_available = true;
        *self.tray.write() = Some(tray);
    }

    /// Calcula os `StatusCounters` atuais a partir do `TabStatusProvider` e `AttentionRegistry`.
    #[must_use]
    pub fn current_counters(&self) -> StatusCounters {
        let entries = self.deps.status.snapshot();
        StatusCounters::aggregate(&entries, &self.attention)
    }

    /// Retorna o `OsRuntimeSnapshot` completo atual.
    #[must_use]
    pub fn runtime_snapshot(&self) -> OsRuntimeSnapshot {
        let link_contexts = self.link_contexts.read().values().cloned().collect();
        let pending_focus = self.pending_focus.lock().clone();
        OsRuntimeSnapshot {
            revision: self.current_revision(),
            window: self.quake.visibility(),
            environment: self.env.read().clone(),
            shortcut_status: self.shortcuts.status(),
            counters: self.current_counters(),
            shutdown_phase: self.shutdown.phase(),
            link_contexts,
            pending_focus,
        }
    }

    /// Handshake idempotente invocado pela UI após montar (`os_ui_ready`), aplicando a intenção inicial de visibilidade.
    pub fn ui_ready(&self) -> MtResult<OsRuntimeSnapshot> {
        if !self.ui_ready_completed.swap(true, Ordering::AcqRel) {
            let start_hidden_requested = self.deps.startup_cli_action == CliAction::StartHidden;
            let hide_requested = self.deps.startup_cli_action == CliAction::Hide;
            let tray_ok = self.env.read().tray_available;

            if (start_hidden_requested || hide_requested) && tray_ok {
                let _ = self.quake.hide();
            } else {
                let _ = self.quake.show_and_focus();
            }
        }
        let snap = self.runtime_snapshot();
        // Limpa pending_focus após entregá-lo no handshake
        *self.pending_focus.lock() = None;
        Ok(snap)
    }

    /// Aplica uma ação CLI vinda de segunda instância (`handle_second_instance`).
    pub fn handle_cli_action(&self, action: CliAction) -> MtResult<()> {
        match action {
            CliAction::Toggle => {
                let _ = self.quake.toggle(ToggleSource::Cli)?;
            }
            CliAction::Show | CliAction::None => {
                self.quake.show_and_focus()?;
            }
            CliAction::Hide => {
                self.quake.hide()?;
            }
            CliAction::StartHidden => {
                // Segunda instância com --start-hidden não esconde instância já aberta
            }
        }
        Ok(())
    }

    /// Solicita foco em uma aba (mostrando a janela e emitindo `os://focus-tab`).
    pub fn request_focus_tab(
        &self,
        tab_id: TabId,
        pane_id: Option<PaneId>,
        source: FocusTabSource,
    ) {
        let project_id = self.deps.status.project_of(&tab_id);
        let payload = FocusTabPayload {
            tab_id,
            pane_id,
            project_id,
            source,
        };
        let _ = self.quake.show_and_focus();
        if !self.ui_ready_completed.load(Ordering::Acquire) {
            *self.pending_focus.lock() = Some(payload.clone());
        }
        let rev = self.next_revision();
        self.deps.event_sink.emit_focus_tab(OsEvent {
            revision: rev,
            payload,
        });
    }

    /// Solicita a abertura do diálogo de Preferências na seção indicada (`os://open-preferences`).
    pub fn request_open_preferences(&self, section: &str) {
        let _ = self.quake.show_and_focus();
        let rev = self.next_revision();
        self.deps.event_sink.emit_open_preferences(OsEvent {
            revision: rev,
            payload: OpenPreferencesPayload {
                section: section.to_owned(),
            },
        });
    }

    /// Atualiza ou registra o `LinkContext` de um painel e emite `os://link-context-changed`.
    pub fn update_link_context(&self, context: LinkContext) {
        self.link_contexts
            .write()
            .insert(context.pane_id, context.clone());
        let rev = self.next_revision();
        self.deps.event_sink.emit_link_context_changed(OsEvent {
            revision: rev,
            payload: context,
        });
    }

    /// Remove o `LinkContext` quando um painel é fechado.
    pub fn remove_pane_context(&self, pane_id: &PaneId) {
        self.link_contexts.write().remove(pane_id);
    }

    /// Remove atenção pendente e contextos quando uma aba é fechada.
    pub fn on_tab_closed(&self, tab_id: &TabId) {
        let changed = self.attention.clear(tab_id);
        self.link_contexts
            .write()
            .retain(|_, ctx| &ctx.tab_id != tab_id);
        if changed {
            self.on_tab_state_changed();
        }
    }

    /// Define ou limpa o estado de atenção de uma aba (`os_attention_set`).
    pub fn set_attention(
        &self,
        tab_id: TabId,
        reason: Option<AttentionReason>,
    ) -> MtResult<StatusCounters> {
        let exists = self.deps.status.project_of(&tab_id).is_some()
            || self
                .deps
                .status
                .snapshot()
                .iter()
                .any(|e| e.tab_id == tab_id);
        if !exists {
            return Err(os_error(
                codes::TAB_NOT_FOUND,
                format!("Aba '{tab_id}' não encontrada."),
            ));
        }

        let changed = match reason {
            Some(r) => self.attention.set(tab_id, r),
            None => self.attention.clear(&tab_id),
        };

        let counters = self.current_counters();
        if changed {
            if let Some(tray) = self.tray.read().clone() {
                tray.request_refresh();
            }
            let rev = self.next_revision();
            self.deps.event_sink.emit_counters(OsEvent {
                revision: rev,
                payload: counters,
            });
        }
        Ok(counters)
    }

    /// Reage à mudança de estado de qualquer aba (Fase 05 `tab://state-changed`).
    pub fn on_tab_state_changed(&self) {
        if let Some(tray) = self.tray.read().clone() {
            tray.request_refresh();
        }
        let counters = self.current_counters();
        let rev = self.next_revision();
        self.deps.event_sink.emit_counters(OsEvent {
            revision: rev,
            payload: counters,
        });
    }

    /// Reage ao término de um comando do shell (Fase 05) avaliando `long_command::should_notify`.
    pub async fn on_command_finished(&self, info: CommandFinishedInfo) {
        let notif_settings = self.settings.read().notifications.clone();
        let vis = self.quake.visibility();
        if let Some(kind) = should_notify(&info, &notif_settings, vis.focused, vis.visible) {
            let (title, body) = format_long_command_notification(&info);
            let _ = self
                .notifier
                .notify_user(kind, &title, &body, Some(info.tab_id))
                .await;
        }
    }

    /// Reage à mudança de foco da janela principal.
    pub fn on_window_focus(&self, focused: bool) {
        if !focused {
            self.quake.on_blur();
        }
        let mut snap = self.quake.visibility();
        snap.focused = focused;
        let rev = self.next_revision();
        self.deps.event_sink.emit_window_visibility(OsEvent {
            revision: rev,
            payload: snap,
        });
    }

    /// Avalia a decisão pura ao receber `WindowEvent::CloseRequested`.
    #[must_use]
    pub fn close_requested(&self, force_quit: bool) -> CloseDecision {
        let behavior = self.settings.read().close_behavior;
        let activity = self.deps.status.activity();
        let has_active = activity.has_active_work() || self.attention.count() > 0;
        let tray_available = self.env.read().tray_available;
        decide_close(CloseContext {
            behavior,
            has_active,
            tray_available,
            force_quit,
        })
    }

    /// Atualiza as configurações aplicando efeitos com compensação em caso de falha e persistindo no SQLite.
    #[allow(clippy::too_many_lines)]
    pub async fn update_settings(&self, patch: OsSettingsPatch) -> MtResult<OsSettings> {
        let _guard = self.settings_update_lock.lock().await;

        let prev_settings = self.settings.read().clone();
        let prev_raw_obj = self.raw_settings_obj.lock().clone();
        let row = self.deps.settings_repo.get().await.map_err(|e| {
            os_error(
                codes::SETTINGS_INVALID,
                format!("Falha ao consultar os_settings: {e}"),
            )
        })?;
        if row.schema_version > OS_SETTINGS_SCHEMA_VERSION {
            return Err(os_error(
                codes::SETTINGS_INVALID,
                format!(
                    "Não é permitido sobrescrever preferências de uma versão futura do schema (v{}).",
                    row.schema_version
                ),
            ));
        }

        let (next_settings, diff) = prev_settings.apply_patch(patch)?;
        let env_snap = self.env.read().clone();

        // 1. Efeito: atalho global
        if diff.shortcut {
            let quake_for_trigger = Arc::clone(&self.quake);
            let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                let _ = quake_for_trigger.toggle(ToggleSource::Shortcut);
            });
            match self
                .shortcuts
                .apply(&next_settings.quake, &env_snap, trigger)
                .await
            {
                Ok(st) => {
                    self.shortcut_usable.store(st.registered, Ordering::Release);
                    let rev = self.next_revision();
                    self.deps.event_sink.emit_shortcut_status(OsEvent {
                        revision: rev,
                        payload: st,
                    });
                }
                Err(err) => {
                    let rev = self.next_revision();
                    self.deps.event_sink.emit_shortcut_status(OsEvent {
                        revision: rev,
                        payload: self.shortcuts.status(),
                    });
                    return Err(err);
                }
            }
        }

        // 2. Efeito: autostart
        if diff.autostart
            && let Err(err) = self
                .autostart_sync
                .apply(next_settings.autostart, next_settings.start_minimized)
        {
            if diff.shortcut {
                let quake_for_trigger = Arc::clone(&self.quake);
                let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let _ = quake_for_trigger.toggle(ToggleSource::Shortcut);
                });
                let _ = self
                    .shortcuts
                    .apply(&prev_settings.quake, &env_snap, trigger)
                    .await;
            }
            return Err(err);
        }

        // 3. Persistência SQLite
        let merged_json = next_settings.to_merged_json(&prev_raw_obj)?;
        if let Err(db_err) = self
            .deps
            .settings_repo
            .save(next_settings.schema_version, merged_json.clone())
            .await
        {
            // Compensação de efeitos externos aplicados
            if diff.autostart {
                let _ = self
                    .autostart_sync
                    .apply(prev_settings.autostart, prev_settings.start_minimized);
            }
            if diff.shortcut {
                let quake_for_trigger = Arc::clone(&self.quake);
                let trigger: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
                    let _ = quake_for_trigger.toggle(ToggleSource::Shortcut);
                });
                let _ = self
                    .shortcuts
                    .apply(&prev_settings.quake, &env_snap, trigger)
                    .await;
            }
            return Err(os_error(
                codes::SETTINGS_INVALID,
                format!("Falha ao persistir preferências do sistema: {db_err}"),
            ));
        }

        if let Ok(Value::Object(updated_map)) = serde_json::from_str::<Value>(&merged_json) {
            *self.raw_settings_obj.lock() = updated_map;
        }
        *self.settings.write() = next_settings.clone();

        if diff.quake_geometry {
            let _ = self.quake.reapply_geometry_if_needed();
        }

        let rev = self.next_revision();
        self.deps.event_sink.emit_settings_changed(OsEvent {
            revision: rev,
            payload: next_settings.clone(),
        });

        Ok(next_settings)
    }

    /// Alterna o autostart usando o mesmo fluxo transacional e compensado de `update_settings` (`os_set_autostart`).
    pub async fn set_autostart(&self, enabled: bool) -> MtResult<AutostartStatus> {
        let _ = self
            .update_settings(OsSettingsPatch {
                autostart: Some(enabled),
                ..Default::default()
            })
            .await?;
        self.autostart_sync.status()
    }
}
