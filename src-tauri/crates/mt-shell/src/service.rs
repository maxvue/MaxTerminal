//! Orquestrador central [`ShellService`] (Fase 05).
//!
//! Integra `mt-pty` (`SpawnLifecycle`, `OutputTap`, `PtyEventListener`) e `mt-db`
//! (`SessionsRepo`, `CommandBlocksRepo`), mantendo [`TabStateRegistry`] e [`RecentBlocksCache`]
//! sincronizados em tempo real e emitindo notificações via [`ShellEventSink`].

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mt_core::error::{MtError, codes};
use mt_core::ids::{CommandBlockId, PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    BlockStatus, CommandBlockDto, CommandBlockHit, CommandBlockListQuery, CommandBlockSearchQuery,
    ContinuousPolicy, ErrorContext, FocusSnapshot, IntegrationInfo, IntegrationMode, Page,
    RuntimeBinding, SeenRequest, ShellCwdChangedEvent, ShellIntegrationModeEvent,
    ShellIntegrationRequest, TabStateRemovedEvent, TabStateSnapshot, format_plan_mode_prompt,
};
use mt_db::Db;
use mt_db::repos::{FinishCommandBlock, NewCommandBlock, NewSession, SessionEnd};
use mt_db::sanitizer::{SanitizedText, prepare_external_payload};
use mt_pty::{
    ExitReason, OutputTap, PreparedSpawn, PtyError, PtyEvent, PtyEventListener, PtyManager,
    PtyResult, PtySummary, SpawnLifecycle, SpawnSpec, TapHandle,
};
use parking_lot::{Mutex, RwLock};

use crate::capture::ERROR_EXCERPT_BUDGET_BYTES;
use crate::fallback::{FallbackAction, FallbackTracker};
use crate::inject::{cleanup_injection_dir, inject_for_spawn};
use crate::osc::OscScanner;
use crate::persist::{NullShellEventSink, ShellEventSink, ShellPersister};
use crate::runtime::{PendingSpawnContext, PtyShellRuntime, RecentBlocksCache};
use crate::state::{TabStateRegistry, TransitionInput};
use crate::tracker::{CommandTracker, TrackerEvent, open_block_to_dto};

/// Orçamento máximo do campo `command` preparado para envio externo ao `MaxCode` (4 KiB).
pub const EXTERNAL_COMMAND_MAX_BYTES: usize = 4 * 1024;

/// Serviço central da Fase 05 responsável pela integração de shell, blocos de comando e estados de abas.
pub struct ShellService {
    pty_manager: Arc<PtyManager>,
    persister: ShellPersister,
    registry: Arc<TabStateRegistry>,
    sink: RwLock<Arc<dyn ShellEventSink>>,
    runtime_dir: PathBuf,
    pending_requests: Mutex<Vec<ShellIntegrationRequest>>,
    pending_spawns: Mutex<BTreeMap<PtyId, PendingSpawnContext>>,
    runtimes: Mutex<BTreeMap<PtyId, PtyShellRuntime>>,
    cache: RwLock<RecentBlocksCache>,
    tap_handle: Mutex<Option<TapHandle>>,
    shutting_down: AtomicBool,
}

impl std::fmt::Debug for ShellService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellService")
            .field("runtime_dir", &self.runtime_dir)
            .field("active_ptys", &self.runtimes.lock().len())
            .finish_non_exhaustive()
    }
}

impl ShellService {
    /// Inicializa o [`ShellService`], executa `close_dangling` no SQLite para recuperar blocos
    /// órfãos de inicializações anteriores, registra o [`OutputTap`] e o [`PtyEventListener`] no
    /// [`PtyManager`] e inicia o ticker periódico de supervisão (100 ms).
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn new(
        pty_manager: Arc<PtyManager>,
        db: Db,
        runtime_dir: PathBuf,
        sink: Option<Arc<dyn ShellEventSink>>,
    ) -> Arc<Self> {
        let persister = ShellPersister::new(db);
        let now = now_unix_ms();
        if let Err(err) = persister.blocks().close_dangling_blocking(now) {
            tracing::warn!(
                error = %err,
                "Falha ao executar close_dangling no startup do ShellService"
            );
        }

        let effective_sink: Arc<dyn ShellEventSink> =
            sink.unwrap_or_else(|| Arc::new(NullShellEventSink));

        let svc = Arc::new(Self {
            pty_manager: Arc::clone(&pty_manager),
            persister,
            registry: Arc::new(TabStateRegistry::new()),
            sink: RwLock::new(effective_sink),
            runtime_dir,
            pending_requests: Mutex::new(Vec::new()),
            pending_spawns: Mutex::new(BTreeMap::new()),
            runtimes: Mutex::new(BTreeMap::new()),
            cache: RwLock::new(RecentBlocksCache::new()),
            tap_handle: Mutex::new(None),
            shutting_down: AtomicBool::new(false),
        });

        let tap_arc: Arc<dyn OutputTap> = Arc::clone(&svc) as Arc<dyn OutputTap>;
        let handle = pty_manager.add_output_tap(tap_arc);
        *svc.tap_handle.lock() = Some(handle);

        let listener_arc: Arc<dyn PtyEventListener> = Arc::clone(&svc) as Arc<dyn PtyEventListener>;
        pty_manager.add_listener(listener_arc);

        Self::spawn_background_ticker(Arc::downgrade(&svc));
        svc
    }

    /// Substitui o [`ShellEventSink`] ativo (útil quando o `AppHandle` do Tauri fica pronto no `setup`).
    pub fn set_event_sink(&self, sink: Arc<dyn ShellEventSink>) {
        *self.sink.write() = sink;
    }

    /// Retorna referência ao [`TabStateRegistry`].
    #[must_use]
    pub fn registry(&self) -> &Arc<TabStateRegistry> {
        &self.registry
    }

    /// Dispara um novo PTY aplicando a injeção de scripts OSC 133 e preparando a sessão no SQLite
    /// antes do início das threads de leitura (`PtyManager::spawn_with_lifecycle`).
    pub fn spawn_pty(
        self: &Arc<Self>,
        spec: SpawnSpec,
        integration: Option<ShellIntegrationRequest>,
    ) -> PtyResult<PtySummary> {
        let req = integration.unwrap_or_default();
        let lifecycle = SpawnLifecycleHook {
            service: Arc::clone(self),
            request: req,
            prepared_session_id: None,
        };
        self.pty_manager
            .spawn_with_lifecycle(spec, Some(&lifecycle))
    }

    /// Dispara um novo PTY reutilizando um [`SessionId`] já preparado em transação do workspace (Fase 07).
    pub fn spawn_pty_with_session(
        self: &Arc<Self>,
        spec: SpawnSpec,
        session_id: SessionId,
        request: ShellIntegrationRequest,
    ) -> PtyResult<PtySummary> {
        let lifecycle = SpawnLifecycleHook {
            service: Arc::clone(self),
            request,
            prepared_session_id: Some(session_id),
        };
        self.pty_manager
            .spawn_with_lifecycle(spec, Some(&lifecycle))
    }

    /// Retorna o último `cwd` confirmado pelo rastreador do PTY, se disponível.
    #[must_use]
    pub fn current_cwd(&self, pty_id: PtyId) -> Option<String> {
        let guard = self.runtimes.lock();
        guard
            .get(&pty_id)
            .and_then(|rt| rt.tracker.current_cwd())
            .map(|s| s.as_str().to_string())
    }

    /// Retorna o `SessionId` associado a um `pty_id` rastreado pelo `ShellService`, se registrado.
    #[must_use]
    pub fn session_id_for_pty(&self, pty_id: PtyId) -> Option<SessionId> {
        self.runtimes
            .lock()
            .get(&pty_id)
            .map(|rt| rt.tracker.binding().session_id)
    }

    /// Indica se o shell do PTY já emitiu marcador de prompt confiável (`OSC 133 ; A` ou `B`).
    #[must_use]
    pub fn is_prompt_ready(&self, pty_id: PtyId) -> bool {
        let guard = self.runtimes.lock();
        guard.get(&pty_id).is_some_and(|rt| {
            rt.tracker.osc_events_received() > 0
                && matches!(
                    rt.tracker.phase(),
                    crate::tracker::PromptPhase::InPrompt
                        | crate::tracker::PromptPhase::AwaitingInput
                )
        })
    }

    /// Finaliza sincronamente o rastreamento e a sessão SQLite de um PTY (usado antes de arquivar `tab_sessions`).
    pub fn finalize_pty_session(
        &self,
        pty_id: PtyId,
        exit_code: Option<i32>,
        signal: Option<String>,
        now_ms: i64,
    ) -> Option<(SessionId, SessionEnd)> {
        let removed = self.runtimes.lock().remove(&pty_id)?;
        let mut rt = removed;
        let binding = rt.tracker.binding().clone();
        let shell_kind = rt.tracker.shell_kind().clone();
        let events = rt.tracker.on_pty_exited(exit_code, signal.clone(), now_ms);
        if events.is_empty() {
            let is_focused = self.registry.is_pty_effectively_focused(pty_id);
            if let Some(snap) = self.registry.apply_pty_transition(
                pty_id,
                TransitionInput::PtyExited {
                    exit_code,
                    signal: signal.clone(),
                    is_effectively_focused: is_focused,
                    at_ms: now_ms,
                },
                now_ms,
            ) {
                self.sink.read().on_tab_state_changed(&snap);
            }
        } else {
            self.dispatch_tracker_events(pty_id, &binding, &shell_kind, events, now_ms);
        }

        let end = SessionEnd {
            ended_at: now_ms,
            exit_code,
            exit_signal: signal,
            last_cwd: rt.tracker.current_cwd().cloned(),
            integration_mode: rt.tracker.mode(),
            command_count: rt.tracker.total_commands(),
            error_count: rt.tracker.total_errors(),
        };
        self.persister
            .close_session_blocking(binding.session_id, end.clone());
        cleanup_injection_dir(&self.runtime_dir, pty_id);
        Some((binding.session_id, end))
    }

    /// Remove um painel individual do [`TabStateRegistry`] e emite `tab://state-changed`.
    pub fn remove_pane(&self, tab_id: TabId, pane_id: PaneId) -> Option<TabStateSnapshot> {
        let now = now_unix_ms();
        let snap = self.registry.remove_pane(tab_id, pane_id, now)?;
        self.sink.read().on_tab_state_changed(&snap);
        Some(snap)
    }

    /// Limpa os painéis antigos de uma aba antes de registrar os novos painéis de uma nova sessão.
    pub fn clear_tab_panes(&self, tab_id: TabId) {
        let now = now_unix_ms();
        self.registry.clear_tab_panes(tab_id, now);
    }

    /// Empilha uma [`ShellIntegrationRequest`] para a próxima chamada direta a `prepare` caso
    /// `ShellService` seja passado diretamente como `&dyn SpawnLifecycle`.
    pub fn push_pending_request(&self, request: ShellIntegrationRequest) {
        self.pending_requests.lock().push(request);
    }

    /// Consulta diagnósticos da integração de shell de um PTY (`shell_integration_info`).
    pub fn integration_info(&self, pty_id: PtyId) -> Result<IntegrationInfo, MtError> {
        let guard = self.runtimes.lock();
        let rt = guard.get(&pty_id).ok_or_else(|| {
            MtError::new(
                codes::SHELL_NOT_REGISTERED,
                format!("Sessão de shell para o PTY {pty_id} não encontrada."),
            )
        })?;
        Ok(rt.integration_info())
    }

    /// Lista todos os snapshots de abas ativas (`tab_state_list`).
    #[must_use]
    pub fn tab_state_list(&self) -> Vec<TabStateSnapshot> {
        self.registry.list_tabs()
    }

    /// Obtém o snapshot de estado de uma aba específica (`tab_state_get`).
    pub fn tab_state_get(&self, tab_id: TabId) -> Result<TabStateSnapshot, MtError> {
        self.registry.get_tab(tab_id).ok_or_else(|| {
            MtError::new(
                codes::SHELL_TAB_NOT_FOUND,
                format!("Aba {tab_id} não encontrada no registro de estados."),
            )
        })
    }

    /// Marca conclusões não vistas e alertas de uma aba/painel como visualizados (`tab_mark_seen`).
    pub fn tab_mark_seen(&self, request: &SeenRequest) -> Result<TabStateSnapshot, MtError> {
        let now = now_unix_ms();
        let snapshot = self.registry.mark_seen(request, now).ok_or_else(|| {
            MtError::new(
                codes::SHELL_TAB_NOT_FOUND,
                format!(
                    "Aba {} não encontrada no registro de estados.",
                    request.tab_id
                ),
            )
        })?;
        self.sink.read().on_tab_state_changed(&snapshot);
        Ok(snapshot)
    }

    /// Atualiza o foco global da janela e da aba/painel ativo (`tab_set_focus`).
    pub fn tab_set_focus(&self, focus: FocusSnapshot) -> Vec<TabStateSnapshot> {
        let now = now_unix_ms();
        let changed = self.registry.set_focus(focus, now);
        let sink = self.sink.read().clone();
        for snap in &changed {
            sink.on_tab_state_changed(snap);
        }
        changed
    }

    /// Atualiza a política de execução contínua (`ContinuousPolicy`) de uma aba/painel (`tab_set_continuous`).
    pub fn tab_set_continuous(
        &self,
        tab_id: TabId,
        pane_id: Option<PaneId>,
        policy: ContinuousPolicy,
    ) -> Result<TabStateSnapshot, MtError> {
        let now = now_unix_ms();
        let snapshot = self
            .registry
            .set_continuous_policy(tab_id, pane_id, policy, now)
            .ok_or_else(|| {
                MtError::new(
                    codes::SHELL_TAB_NOT_FOUND,
                    format!("Aba {tab_id} não encontrada no registro de estados."),
                )
            })?;

        {
            let mut runtimes = self.runtimes.lock();
            for rt in runtimes.values_mut() {
                if rt.tracker.binding().tab_id == Some(tab_id)
                    && (pane_id.is_none() || rt.tracker.binding().pane_id == pane_id)
                {
                    rt.tracker.set_continuous_policy(policy);
                }
            }
        }

        self.sink.read().on_tab_state_changed(&snapshot);
        Ok(snapshot)
    }

    /// Remove uma aba do registro e emite `tab://state-removed`.
    pub fn remove_tab(&self, tab_id: TabId) -> Option<TabStateRemovedEvent> {
        let ev = self.registry.remove_tab(tab_id)?;
        self.sink.read().on_tab_state_removed(&ev);
        Some(ev)
    }

    /// Define ou limpa a sobreposição de atenção de Agente CLI de uma aba e emite `tab://state-changed`.
    pub fn set_tab_attention(
        &self,
        tab_id: TabId,
        attention: Option<mt_core::shell::AttentionSnapshot>,
    ) -> Option<TabStateSnapshot> {
        let now = now_unix_ms();
        let snap = self.registry.set_attention(tab_id, attention, now)?;
        self.sink.read().on_tab_state_changed(&snap);
        Some(snap)
    }

    /// Busca um bloco de comando pelo ID (`command_block_get`), consultando primeiro o SQLite
    /// e usando o cache em memória para blocos ativos ou cuja persistência falhou.
    pub async fn command_block_get(
        &self,
        block_id: CommandBlockId,
    ) -> Result<CommandBlockDto, MtError> {
        if let Ok(Some(dto)) = self.persister.blocks().get(block_id).await {
            return Ok(dto);
        }

        if let Some(cached) = self.cache.read().get(block_id) {
            return Ok(cached);
        }

        {
            let runtimes = self.runtimes.lock();
            for rt in runtimes.values() {
                if let Some(open) = rt.tracker.open_block()
                    && open.id == block_id
                {
                    return Ok(open_block_to_dto(rt.tracker.binding(), open));
                }
            }
        }

        Err(MtError::new(
            codes::SHELL_BLOCK_NOT_FOUND,
            format!("Bloco de comando {block_id} não encontrado."),
        ))
    }

    /// Lista blocos de comando paginados (`command_block_list`), mesclando dados do SQLite com
    /// o cache em memória caso existam blocos ainda não persistidos.
    pub async fn command_block_list(
        &self,
        query: CommandBlockListQuery,
    ) -> Result<Page<CommandBlockDto>, MtError> {
        match self.persister.blocks().list(query.clone()).await {
            Ok(mut page) => {
                if query.cursor.is_none()
                    && let Some(session_id) = query.session_id
                {
                    let cached = self.cache.read().list_for_session(session_id);
                    for item in cached {
                        if !page.items.iter().any(|existing| existing.id == item.id) {
                            page.items.push(item);
                        }
                    }
                    page.items.sort_by(|a, b| {
                        b.started_at
                            .cmp(&a.started_at)
                            .then_with(|| b.command_seq.cmp(&a.command_seq))
                    });
                }
                Ok(page)
            }
            Err(err) => {
                if let Some(session_id) = query.session_id {
                    let items = self.cache.read().list_for_session(session_id);
                    if !items.is_empty() {
                        return Ok(Page {
                            items,
                            next_cursor: None,
                        });
                    }
                }
                Err(err.into())
            }
        }
    }

    /// Executa busca textual FTS5 sobre `command_blocks_fts` (`command_block_search`).
    pub async fn command_block_search(
        &self,
        query: CommandBlockSearchQuery,
    ) -> Result<Page<CommandBlockHit>, MtError> {
        if query.query.len() > 4096 {
            return Err(MtError::new(
                codes::SHELL_QUERY_INVALID,
                "A consulta de busca excede o limite máximo de 4096 bytes.",
            ));
        }
        self.persister
            .blocks()
            .search(query)
            .await
            .map_err(Into::into)
    }

    /// Constrói o [`ErrorContext`] sanitizado e truncado via `prepare_external_payload` para
    /// abertura em Plan Mode no `MaxCode` (`command_block_error_context`).
    pub async fn command_block_error_context(
        &self,
        block_id: CommandBlockId,
    ) -> Result<ErrorContext, MtError> {
        let block = self.command_block_get(block_id).await?;

        if block.status == BlockStatus::Running {
            return Err(MtError::new(
                codes::SHELL_BLOCK_NOT_ERROR,
                "O comando ainda está em execução; aguarde o término para inspecionar.",
            ));
        }

        if !block.inspectable
            && !matches!(
                block.status,
                BlockStatus::Failed | BlockStatus::TerminatedBySignal | BlockStatus::PtyExited
            )
        {
            return Err(MtError::new(
                codes::SHELL_BLOCK_NOT_ERROR,
                "Este bloco de comando não terminou com erro inspecionável.",
            ));
        }

        let raw_cmd = block
            .command_text
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("<comando indisponível>");
        let raw_err = block
            .error_excerpt
            .as_deref()
            .or(block.output_text.as_deref())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("<sem saída de erro capturada>");

        let cmd_payload = prepare_external_payload(raw_cmd, EXTERNAL_COMMAND_MAX_BYTES);
        let err_payload = prepare_external_payload(raw_err, ERROR_EXCERPT_BUDGET_BYTES);

        let command_clean = cmd_payload.text().as_str().to_string();
        let error_clean = err_payload.text().as_str().to_string();
        let prompt = format_plan_mode_prompt(&command_clean, &error_clean);

        Ok(ErrorContext {
            block_id: block.id,
            session_id: block.session_id,
            tab_id: block.tab_id,
            pane_id: block.pane_id,
            command: command_clean,
            command_text_quality: block.command_text_quality,
            error: error_clean,
            exit_code: block.exit_code,
            signal: block.signal,
            cwd: block.cwd,
            duration_ms: block.duration_ms.unwrap_or(0),
            started_at: block.started_at,
            ended_at: block.ended_at.unwrap_or(block.started_at),
            truncated: cmd_payload.truncated()
                || err_payload.truncated()
                || block.output_kind == mt_core::shell::OutputKind::HeadTailTruncated,
            plan_mode_prompt: prompt,
        })
    }

    /// Executa um tick de supervisão (avalia timers de 80 ms / 15 s e fallback `/proc`).
    pub fn tick(&self) {
        if self.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        let now = now_unix_ms();

        // 1. Avalia timers de histerese (80 ms) e promoção contínua (15 s) no registro
        let timer_snapshots = self.registry.evaluate_timers(now);
        if !timer_snapshots.is_empty() {
            let sink = self.sink.read().clone();
            let mut runtimes = self.runtimes.lock();
            for snap in &timer_snapshots {
                for pane in &snap.panes {
                    if pane.status == mt_core::shell::TabStatus::Continuous
                        && let Some(active) = &pane.active_execution
                        && let Some(rt) = runtimes.get_mut(&pane.pty_id)
                    {
                        rt.tracker.mark_active_block_continuous(active.block_id);
                    }
                }
                sink.on_tab_state_changed(snap);
            }
        }

        // 2. Avalia janelas de ativação e debounce de fallback `/proc`
        let proc_source = self.pty_manager.proc_source();
        let mut pending_dispatch: Vec<(
            PtyId,
            RuntimeBinding,
            mt_core::shell::ShellKind,
            Vec<TrackerEvent>,
        )> = Vec::new();

        {
            let mut runtimes = self.runtimes.lock();
            for (&pty_id, rt) in runtimes.iter_mut() {
                if rt.tracker.mode() == IntegrationMode::Osc133
                    || rt.tracker.mode() == IntegrationMode::Disabled
                {
                    continue;
                }
                let current_fg = rt.last_fg_pgid;
                let Some(fb) = &mut rt.fallback else {
                    continue;
                };
                let actions = fb.poll_tick(current_fg, proc_source, now);
                if actions.is_empty() {
                    continue;
                }

                let mut events = Vec::new();
                for action in actions {
                    match action {
                        FallbackAction::ActivationWindowExpired => {
                            if let Some(ev) = rt.tracker.on_activation_window_expired() {
                                events.push(ev);
                            }
                        }
                        FallbackAction::StartCommand { cmdline, cwd, .. } => {
                            events.extend(rt.tracker.start_proc_fallback_block(cmdline, cwd, now));
                        }
                        FallbackAction::FinishCommand { shell_cwd } => {
                            events.extend(
                                rt.tracker.finish_proc_fallback_block(None, shell_cwd, now),
                            );
                        }
                    }
                }

                if !events.is_empty() {
                    pending_dispatch.push((
                        pty_id,
                        rt.tracker.binding().clone(),
                        rt.tracker.shell_kind().clone(),
                        events,
                    ));
                }
            }
        }

        for (pty_id, binding, shell_kind, events) in pending_dispatch {
            self.dispatch_tracker_events(pty_id, &binding, &shell_kind, events, now);
        }
    }

    /// Encerra graciosamente todas as sessões e blocos abertos antes do fechamento do banco.
    pub fn shutdown(&self) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            return;
        }
        let now = now_unix_ms();

        let drained: Vec<(PtyId, PtyShellRuntime)> = {
            let mut guard = self.runtimes.lock();
            std::mem::take(&mut *guard).into_iter().collect()
        };

        for (pty_id, mut rt) in drained {
            let binding = rt.tracker.binding().clone();
            let shell_kind = rt.tracker.shell_kind().clone();
            let events = rt.tracker.on_app_shutdown(now);
            self.dispatch_tracker_events(pty_id, &binding, &shell_kind, events, now);

            let end = SessionEnd {
                ended_at: now,
                exit_code: None,
                exit_signal: None,
                last_cwd: rt.tracker.current_cwd().cloned(),
                integration_mode: rt.tracker.mode(),
                command_count: rt.tracker.total_commands(),
                error_count: rt.tracker.total_errors(),
            };
            self.persister
                .close_session_blocking(binding.session_id, end);
            cleanup_injection_dir(&self.runtime_dir, pty_id);
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    fn prepare_with_request(
        &self,
        spawn: &mut PreparedSpawn,
        request: &ShellIntegrationRequest,
        prepared_session_id: Option<SessionId>,
    ) -> PtyResult<()> {
        let pty_id = spawn.pty_id;
        let session_id = prepared_session_id.unwrap_or_default();
        let tab_id = spawn.owner.tab_id;
        let pane_id = spawn.owner.pane_id;

        let outcome = inject_for_spawn(spawn, &self.runtime_dir, request.enabled);
        let initial_mode = if !request.enabled {
            IntegrationMode::Disabled
        } else if outcome.injected {
            IntegrationMode::Pending
        } else {
            IntegrationMode::FallbackProc
        };

        let initial_cwd = SanitizedText::new(&spawn.cwd.to_string_lossy());
        let shell_exec = SanitizedText::new(&spawn.executable.to_string_lossy());
        let now = now_unix_ms();

        let new_session = NewSession {
            id: session_id,
            tab_id,
            pane_id,
            project_id: request.project_id,
            tab_session_id: request.tab_session_id,
            pty_id,
            shell_kind: outcome.shell_kind.clone(),
            shell_executable: shell_exec,
            initial_cwd: initial_cwd.clone(),
            integration_mode: initial_mode,
            started_at: now,
        };

        if let Err(err) = self.persister.sessions().prepare_blocking(new_session) {
            tracing::warn!(
                pty_id = %pty_id,
                session_id = %session_id,
                error = %err,
                "Falha ao gravar sessão inicial no SQLite; prosseguindo em modo degradado em memória"
            );
        }

        let binding = RuntimeBinding {
            pty_id,
            session_id,
            tab_id,
            pane_id,
        };

        self.pending_spawns.lock().insert(
            pty_id,
            PendingSpawnContext {
                binding,
                shell_kind: outcome.shell_kind,
                nonce: outcome.nonce,
                initial_mode,
                continuous_policy: request.continuous_policy,
                initial_cwd,
                scripts_version: outcome.scripts_version,
                immediate_fallback: !outcome.injected && request.enabled,
            },
        );

        Ok(())
    }

    #[allow(clippy::unnecessary_wraps)]
    fn started_internal(&self, summary: &PtySummary) -> PtyResult<()> {
        let pty_id = summary.id;
        let Some(ctx) = self.pending_spawns.lock().remove(&pty_id) else {
            return Ok(());
        };

        let now = now_unix_ms();
        let tracker = CommandTracker::new(
            ctx.binding.clone(),
            ctx.shell_kind,
            ctx.nonce,
            ctx.initial_mode,
            ctx.continuous_policy,
            Some(ctx.initial_cwd.clone()),
        );

        let fallback = if ctx.initial_mode == IntegrationMode::Disabled {
            None
        } else {
            Some(FallbackTracker::new(
                summary.pids.shell_pid,
                summary.pids.pgid,
                now,
                ctx.immediate_fallback,
            ))
        };

        self.runtimes.lock().insert(
            pty_id,
            PtyShellRuntime {
                scanner: OscScanner::new(),
                tracker,
                fallback,
                scripts_version: ctx.scripts_version,
                last_fg_pgid: summary.pids.foreground_pgid,
            },
        );

        if let Some(tab_id) = ctx.binding.tab_id {
            let snap = self.registry.register_pane(
                tab_id,
                ctx.binding.pane_id,
                pty_id,
                ctx.binding.session_id,
                ctx.initial_mode,
                ctx.continuous_policy,
                Some(ctx.initial_cwd.into_inner()),
                now,
            );
            self.sink.read().on_tab_state_changed(&snap);
        }

        Ok(())
    }

    fn failed_internal(&self, pty_id: PtyId) {
        if let Some(ctx) = self.pending_spawns.lock().remove(&pty_id) {
            let _ = self
                .persister
                .sessions()
                .abort_blocking(ctx.binding.session_id);
        }
        self.runtimes.lock().remove(&pty_id);
        cleanup_injection_dir(&self.runtime_dir, pty_id);
    }

    #[allow(clippy::too_many_lines)]
    fn dispatch_tracker_events(
        &self,
        pty_id: PtyId,
        binding: &RuntimeBinding,
        shell_kind: &mt_core::shell::ShellKind,
        events: Vec<TrackerEvent>,
        now_ms: i64,
    ) {
        let sink = self.sink.read().clone();

        for event in events {
            match event {
                TrackerEvent::IntegrationModeChanged { mode, reason } => {
                    if let Some(snap) = self.registry.apply_pty_transition(
                        pty_id,
                        TransitionInput::IntegrationModeChanged {
                            mode,
                            at_ms: now_ms,
                        },
                        now_ms,
                    ) {
                        sink.on_tab_state_changed(&snap);
                    }

                    self.persister
                        .update_session_runtime_blocking(binding.session_id, None, mode);

                    sink.on_integration_mode_changed(&ShellIntegrationModeEvent {
                        binding: binding.clone(),
                        mode,
                        shell_kind: shell_kind.clone(),
                        reason: Some(reason),
                    });
                }

                TrackerEvent::CwdChanged { cwd, at_ms } => {
                    let cwd_str = cwd.as_str().to_string();
                    if let Some(snap) = self.registry.apply_pty_transition(
                        pty_id,
                        TransitionInput::CwdChanged {
                            cwd: cwd_str.clone(),
                            at_ms,
                        },
                        at_ms,
                    ) {
                        sink.on_tab_state_changed(&snap);
                    }

                    let current_mode = self
                        .runtimes
                        .lock()
                        .get(&pty_id)
                        .map_or(IntegrationMode::Osc133, |rt| rt.tracker.mode());
                    self.persister.update_session_runtime_blocking(
                        binding.session_id,
                        Some(cwd),
                        current_mode,
                    );

                    sink.on_cwd_changed(&ShellCwdChangedEvent {
                        binding: binding.clone(),
                        cwd: cwd_str,
                        at: at_ms,
                    });
                }

                TrackerEvent::BlockStarted {
                    mut started,
                    sanitized_command,
                    sanitized_cwd,
                    is_continuous,
                } => {
                    if let Some(snap) = self.registry.apply_pty_transition(
                        pty_id,
                        TransitionInput::CommandStarted {
                            execution: started.execution.clone(),
                            command_text: started.command_text.clone(),
                            cwd: started.cwd.clone(),
                            apply_hysteresis: false,
                            at_ms: started.execution.started_at,
                        },
                        started.execution.started_at,
                    ) {
                        sink.on_tab_state_changed(&snap);
                    }

                    let new_block = NewCommandBlock {
                        id: started.execution.block_id,
                        session_id: binding.session_id,
                        tab_id: binding.tab_id,
                        pane_id: binding.pane_id,
                        pty_id,
                        command_seq: started.execution.command_seq,
                        command_text: sanitized_command,
                        command_text_quality: started.command_text_quality,
                        cwd: sanitized_cwd,
                        started_at: started.execution.started_at,
                        is_continuous,
                        source: started.execution.source,
                        confidence: started.confidence,
                        start_pty_seq: started.start_pos.pty_seq,
                        start_byte_offset: started.start_pos.byte_offset,
                        output_start_pty_seq: started.output_start_pos.map(|p| p.pty_seq),
                        output_start_byte_offset: started.output_start_pos.map(|p| p.byte_offset),
                    };

                    let p_state = self.persister.persist_block_start_blocking(new_block);
                    started.persistence = p_state;
                    sink.on_command_started(&started);
                }

                TrackerEvent::BlockFinished {
                    finished,
                    output,
                    sanitized_command,
                    sanitized_cwd,
                    is_continuous,
                } => {
                    let is_focused = self.registry.is_pty_effectively_focused(pty_id);
                    if let Some(snap) = self.registry.apply_pty_transition(
                        pty_id,
                        TransitionInput::CommandFinished {
                            block_id: finished.block_id,
                            block_status: finished.status,
                            end_reason: finished.end_reason,
                            exit_code: finished.exit_code,
                            signal: finished.signal.clone(),
                            duration_ms: finished.duration_ms,
                            is_effectively_focused: is_focused,
                            at_ms: finished.ended_at,
                        },
                        finished.ended_at,
                    ) {
                        sink.on_tab_state_changed(&snap);
                    }

                    let finish_db = FinishCommandBlock {
                        id: finished.block_id,
                        session_id: binding.session_id,
                        tab_id: binding.tab_id,
                        pane_id: binding.pane_id,
                        pty_id,
                        command_seq: finished.command_seq,
                        command_text: sanitized_command,
                        command_text_quality: finished.command_text_quality,
                        cwd: sanitized_cwd,
                        started_at: finished.started_at,
                        ended_at: finished.ended_at,
                        duration_ms: finished.duration_ms,
                        exit_code: finished.exit_code,
                        signal: finished.signal.clone(),
                        status: finished.status,
                        end_reason: finished.end_reason,
                        is_continuous,
                        source: finished.source,
                        confidence: finished.confidence,
                        output_text: output.output_text.clone(),
                        output_kind: output.output_kind,
                        output_bytes: output.output_bytes,
                        output_lines: output.output_lines,
                        error_excerpt: output.error_excerpt.clone(),
                        inspectable: finished.inspectable,
                        start_pty_seq: finished.start_pos.pty_seq,
                        start_byte_offset: finished.start_pos.byte_offset,
                        output_start_pty_seq: finished.output_start_pos.map(|p| p.pty_seq),
                        output_start_byte_offset: finished.output_start_pos.map(|p| p.byte_offset),
                        end_pty_seq: finished.end_pos.pty_seq,
                        end_byte_offset: finished.end_pos.byte_offset,
                    };

                    let persistence = self.persister.persist_block_finish_blocking(
                        finish_db,
                        finished.clone(),
                        &sink,
                    );

                    let dto = CommandBlockDto {
                        id: finished.block_id,
                        session_id: binding.session_id,
                        tab_id: binding.tab_id,
                        pane_id: binding.pane_id,
                        pty_id,
                        command_seq: finished.command_seq,
                        command_text: finished.command_text,
                        command_text_quality: finished.command_text_quality,
                        cwd: finished.cwd,
                        started_at: finished.started_at,
                        ended_at: Some(finished.ended_at),
                        duration_ms: Some(finished.duration_ms),
                        exit_code: finished.exit_code,
                        signal: finished.signal,
                        status: finished.status,
                        end_reason: Some(finished.end_reason),
                        is_continuous,
                        source: finished.source,
                        confidence: finished.confidence,
                        output_text: output.output_text.map(SanitizedText::into_inner),
                        output_kind: output.output_kind,
                        output_bytes: output.output_bytes,
                        output_lines: output.output_lines,
                        error_excerpt: output.error_excerpt.map(SanitizedText::into_inner),
                        inspectable: finished.inspectable,
                        start_pty_seq: finished.start_pos.pty_seq,
                        start_byte_offset: finished.start_pos.byte_offset,
                        output_start_pty_seq: finished.output_start_pos.map(|p| p.pty_seq),
                        output_start_byte_offset: finished.output_start_pos.map(|p| p.byte_offset),
                        end_pty_seq: Some(finished.end_pos.pty_seq),
                        end_byte_offset: Some(finished.end_pos.byte_offset),
                        persistence,
                    };
                    self.cache.write().upsert(dto);
                }
            }
        }
    }

    fn spawn_background_ticker(weak_svc: std::sync::Weak<Self>) {
        let _ = std::thread::Builder::new()
            .name("mt-shell-ticker".to_string())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(100));
                    let Some(svc) = weak_svc.upgrade() else {
                        break;
                    };
                    if svc.shutting_down.load(Ordering::Relaxed) {
                        break;
                    }
                    svc.tick();
                }
            });
    }
}

struct SpawnLifecycleHook {
    service: Arc<ShellService>,
    request: ShellIntegrationRequest,
    prepared_session_id: Option<SessionId>,
}

impl SpawnLifecycle for SpawnLifecycleHook {
    fn prepare(&self, spawn: &mut PreparedSpawn) -> PtyResult<()> {
        self.service
            .prepare_with_request(spawn, &self.request, self.prepared_session_id)
    }

    fn started(&self, summary: &PtySummary) -> PtyResult<()> {
        self.service.started_internal(summary)
    }

    fn failed(&self, pty_id: PtyId) {
        self.service.failed_internal(pty_id);
    }
}

impl SpawnLifecycle for ShellService {
    fn prepare(&self, spawn: &mut PreparedSpawn) -> PtyResult<()> {
        let req = self.pending_requests.lock().pop().unwrap_or_default();
        self.prepare_with_request(spawn, &req, None)
    }

    fn started(&self, summary: &PtySummary) -> PtyResult<()> {
        self.started_internal(summary)
    }

    fn failed(&self, pty_id: PtyId) {
        self.failed_internal(pty_id);
    }
}

impl OutputTap for ShellService {
    fn on_output(&self, pty_id: PtyId, seq: u64, bytes: &[u8]) {
        if self.shutting_down.load(Ordering::Relaxed) || bytes.is_empty() {
            return;
        }

        let now = now_unix_ms();
        let maybe_dispatch = {
            let mut guard = self.runtimes.lock();
            let Some(rt) = guard.get_mut(&pty_id) else {
                return;
            };
            let scan = rt.scanner.scan_chunk(seq, bytes);
            let events = rt.tracker.process_scan_output(&scan, now);
            if rt.tracker.mode() == IntegrationMode::Osc133
                && let Some(fb) = &mut rt.fallback
            {
                fb.notify_osc_received();
            }
            if events.is_empty() {
                None
            } else {
                Some((
                    rt.tracker.binding().clone(),
                    rt.tracker.shell_kind().clone(),
                    events,
                ))
            }
        };

        if let Some((binding, shell_kind, events)) = maybe_dispatch {
            self.dispatch_tracker_events(pty_id, &binding, &shell_kind, events, now);
        }
    }
}

impl PtyEventListener for ShellService {
    #[allow(clippy::too_many_lines)]
    fn on_event(&self, event: &PtyEvent) {
        if self.shutting_down.load(Ordering::Relaxed) {
            return;
        }
        let now = now_unix_ms();

        match event {
            PtyEvent::ForegroundChanged { pty_id, pgid, .. } => {
                let pty_id = *pty_id;
                let proc_source = self.pty_manager.proc_source();
                let maybe_dispatch = {
                    let mut guard = self.runtimes.lock();
                    let Some(rt) = guard.get_mut(&pty_id) else {
                        return;
                    };
                    rt.last_fg_pgid = *pgid;
                    let Some(fb) = &mut rt.fallback else {
                        return;
                    };
                    let actions = fb.on_foreground_changed(*pgid, proc_source, now);
                    if actions.is_empty() {
                        return;
                    }
                    let mut events = Vec::new();
                    for action in actions {
                        match action {
                            FallbackAction::ActivationWindowExpired => {
                                if let Some(ev) = rt.tracker.on_activation_window_expired() {
                                    events.push(ev);
                                }
                            }
                            FallbackAction::StartCommand { cmdline, cwd, .. } => {
                                events.extend(
                                    rt.tracker.start_proc_fallback_block(cmdline, cwd, now),
                                );
                            }
                            FallbackAction::FinishCommand { shell_cwd } => {
                                events.extend(
                                    rt.tracker.finish_proc_fallback_block(None, shell_cwd, now),
                                );
                            }
                        }
                    }
                    if events.is_empty() {
                        None
                    } else {
                        Some((
                            rt.tracker.binding().clone(),
                            rt.tracker.shell_kind().clone(),
                            events,
                        ))
                    }
                };

                if let Some((binding, shell_kind, events)) = maybe_dispatch {
                    self.dispatch_tracker_events(pty_id, &binding, &shell_kind, events, now);
                }
            }

            PtyEvent::Exited { pty_id, exit, .. } => {
                let pty_id = *pty_id;
                let removed = self.runtimes.lock().remove(&pty_id);
                if let Some(mut rt) = removed {
                    let binding = rt.tracker.binding().clone();
                    let shell_kind = rt.tracker.shell_kind().clone();
                    let exit_code = exit.code;
                    let signal = exit.signal_name.clone().or_else(|| {
                        if matches!(exit.reason, ExitReason::Signaled | ExitReason::KilledByUser) {
                            Some("SIGKILL".to_string())
                        } else {
                            None
                        }
                    });

                    let events = rt.tracker.on_pty_exited(exit_code, signal.clone(), now);
                    if events.is_empty() {
                        let is_focused = self.registry.is_pty_effectively_focused(pty_id);
                        if let Some(snap) = self.registry.apply_pty_transition(
                            pty_id,
                            TransitionInput::PtyExited {
                                exit_code,
                                signal: signal.clone(),
                                is_effectively_focused: is_focused,
                                at_ms: now,
                            },
                            now,
                        ) {
                            self.sink.read().on_tab_state_changed(&snap);
                        }
                    } else {
                        self.dispatch_tracker_events(pty_id, &binding, &shell_kind, events, now);
                    }

                    let end = SessionEnd {
                        ended_at: i64::try_from(exit.at_ms.0).unwrap_or(now).max(now),
                        exit_code: exit.code,
                        exit_signal: signal,
                        last_cwd: rt.tracker.current_cwd().cloned(),
                        integration_mode: rt.tracker.mode(),
                        command_count: rt.tracker.total_commands(),
                        error_count: rt.tracker.total_errors(),
                    };
                    self.persister
                        .close_session_blocking(binding.session_id, end);
                }
                cleanup_injection_dir(&self.runtime_dir, pty_id);
            }

            PtyEvent::Spawned { .. } | PtyEvent::CleanupFailed { .. } => {}
        }
    }
}

/// Retorna o timestamp UNIX atual em milissegundos.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

/// Converte um [`PtyError`] em [`MtError`].
#[must_use]
pub fn pty_error_to_mt(err: PtyError) -> MtError {
    err.into()
}
