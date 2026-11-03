//! Fachada principal `PtyManager` para orquestração de todas as sessões de PTY,
//! supervisão periódica, contenção cgroup v2, watchdog e desligamento coordenado.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::thread;
use std::time::Duration;

use crossbeam_channel::Receiver;
use mt_core::ids::{PaneId, PtyId, TabId};
use nix::sys::signal::Signal;
use parking_lot::{Mutex, RwLock};

use crate::config::PtyConfig;
use crate::containment::{
    ContainmentHandle, ContainmentProvider, SystemdCgroupProvider, WatchdogClient,
};
use crate::error::{PtyError, PtyResult};
use crate::events::{EventHub, PtyEvent, PtyEventListener};
use crate::guard::{
    DirectChildRegistry, REGISTRY_SCHEMA_VERSION, RegistryFile, RegistryHeader, enable_subreaper,
    install_panic_hook, install_signal_handler, notify_emergency_fd, reap_adopted_orphans,
    recover_stale_registries,
};
use crate::instance::{PtyInstance, SpawnContext, current_epoch_ms};
use crate::io::GlobalBufferPool;
use crate::kill::KillPlan;
use crate::proc::{
    PidfdSignaler, ProcSource, ProcfsSource, Signaler, build_process_report, read_process_identity,
};
use crate::sink::OutputSink;
use crate::spawn::{DefaultEnvPolicy, EnvPolicy, SpawnLifecycle, prepare_spawn};
use crate::supervisor::{SupervisorHandle, SupervisorHost};
use crate::tap::{OutputTap, TapHandle, TapRegistry};
use crate::types::{
    AttachInfo, AttachmentId, ExitInfo, ExitReason, KillMode, OutputCursor, ProcessIdentity,
    PtyInput, PtyProcessReport, PtySize, PtyStatus, PtySummary, ShutdownReport, SpawnSpec,
};

/// Handle RAII retornado por [`PtyManager::install_guards`].
#[derive(Debug)]
pub struct GuardHandle {
    _private: (),
}

/// Gerenciador central de pseudo-terminais do `MaxTerminal`.
pub struct PtyManager {
    config: PtyConfig,
    containment: Arc<dyn ContainmentProvider>,
    proc_source: Arc<dyn ProcSource>,
    signaler: Arc<dyn Signaler>,
    env_policy: Arc<dyn EnvPolicy>,
    instances: RwLock<HashMap<PtyId, Arc<PtyInstance>>>,
    spawn_gate: Mutex<()>,
    shutting_down: AtomicBool,
    direct_children: Arc<DirectChildRegistry>,
    global_pool: Arc<GlobalBufferPool>,
    event_hub: Arc<EventHub>,
    revision_counter: Arc<AtomicU64>,
    taps: Arc<TapRegistry>,
    watchdog: Option<Arc<WatchdogClient>>,
    registry_file: Option<Arc<RegistryFile>>,
    supervisor: Mutex<Option<SupervisorHandle>>,
}

impl std::fmt::Debug for PtyManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyManager")
            .field("active_count", &self.instances.read().len())
            .field("shutting_down", &self.shutting_down.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

struct ManagerSupervisorHost {
    manager: Weak<PtyManager>,
}

impl SupervisorHost for ManagerSupervisorHost {
    fn poll_foreground_changes(&self) {
        let Some(mgr) = self.manager.upgrade() else {
            return;
        };
        let list: Vec<Arc<PtyInstance>> = mgr.instances.read().values().cloned().collect();
        for inst in list {
            inst.poll_foreground(&*mgr.proc_source, &mgr.event_hub);
        }
    }

    fn reap_adopted_orphans(&self) {
        let Some(mgr) = self.manager.upgrade() else {
            return;
        };
        let cgroups: Vec<Arc<dyn ContainmentHandle>> = mgr
            .instances
            .read()
            .values()
            .map(|inst| inst.containment().clone())
            .collect();
        let _ = reap_adopted_orphans(&mgr.direct_children, &cgroups);
    }

    fn expire_tombstones(&self) {
        let Some(mgr) = self.manager.upgrade() else {
            return;
        };
        let ttl = mgr.config.tombstone_ttl;
        let mut expired = Vec::new();
        {
            let guard = mgr.instances.read();
            for (id, inst) in guard.iter() {
                if inst.is_tombstone_expired(ttl) {
                    expired.push(*id);
                }
            }
        }
        if !expired.is_empty() {
            let mut removed = Vec::with_capacity(expired.len());
            {
                let mut guard = mgr.instances.write();
                for id in expired {
                    if guard.get(&id).is_some_and(|i| i.is_tombstone_expired(ttl))
                        && let Some(inst) = guard.remove(&id)
                    {
                        removed.push(inst);
                    }
                }
            }
            for inst in removed {
                inst.finalize_tombstone();
            }
        }
    }

    fn check_leases_and_watchdog(&self) {
        let Some(mgr) = self.manager.upgrade() else {
            return;
        };
        let list: Vec<Arc<PtyInstance>> = mgr.instances.read().values().cloned().collect();
        for inst in list {
            let _ = inst.output_state().check_ack_timeout();
        }
        if let Some(wd) = &mgr.watchdog {
            let _ = wd.ping();
        }
    }
}

impl PtyManager {
    /// Cria e inicializa um [`PtyManager`] com provedores reais de Linux (`SystemdCgroupProvider`,
    /// `ProcfsSource`, `PidfdSignaler` e `DefaultEnvPolicy`).
    ///
    /// # Errors
    /// Retorna [`PtyError::ContainmentUnavailable`] se cgroup v2 não estiver delegado/acessível.
    pub fn new(config: PtyConfig) -> PtyResult<Arc<Self>> {
        let instance_id = config
            .registry_dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("default");
        let containment = Arc::new(SystemdCgroupProvider::discover(
            config.cgroup_root_override.as_deref(),
            instance_id,
        ));
        let proc_source: Arc<dyn ProcSource> = Arc::new(ProcfsSource::new());
        let signaler: Arc<dyn Signaler> = Arc::new(PidfdSignaler::new(proc_source.clone()));
        let env_policy: Arc<dyn EnvPolicy> = Arc::new(DefaultEnvPolicy);

        Self::with_providers(config, containment, proc_source, signaler, env_policy)
    }

    /// Cria um [`PtyManager`] com provedores explícitos (permitindo injeção de mocks em testes).
    ///
    /// # Errors
    /// Retorna [`PtyError`] se a contenção estiver indisponível ou se o diretório de runtime falhar.
    #[allow(clippy::needless_pass_by_value)]
    pub fn with_providers(
        config: PtyConfig,
        containment: Arc<dyn ContainmentProvider>,
        proc_source: Arc<dyn ProcSource>,
        signaler: Arc<dyn Signaler>,
        env_policy: Arc<dyn EnvPolicy>,
    ) -> PtyResult<Arc<Self>> {
        if !containment.is_available() {
            return Err(PtyError::ContainmentUnavailable(
                "Provedor de contenção reportou indisponibilidade".to_string(),
            ));
        }

        if config.orphan_sweep_on_startup {
            if let Some(parent) = config.registry_dir.parent() {
                let _ = recover_stale_registries(parent);
            }
            let _ = recover_stale_registries(&config.registry_dir);
        }

        let _ = enable_subreaper();

        let direct_children = Arc::new(DirectChildRegistry::new());
        let watchdog = match WatchdogClient::spawn(
            &config.registry_dir,
            config.helper_bin_override.as_deref(),
        ) {
            Ok(wd) => {
                direct_children.register(wd.watchdog_pid());
                Some(Arc::new(wd))
            }
            Err(_) => None,
        };

        let app_pid = std::process::id();
        let app_start = read_process_identity(app_pid, proc_source.boot_id(), 4096)
            .ok()
            .flatten()
            .map_or(0, |id| id.start_time.0);

        let wd_pid = watchdog.as_ref().map(|w| w.watchdog_pid());
        let wd_start = wd_pid.and_then(|pid| {
            read_process_identity(pid, proc_source.boot_id(), 4096)
                .ok()
                .flatten()
                .map(|id| id.start_time.0)
        });

        let header = RegistryHeader {
            version: REGISTRY_SCHEMA_VERSION,
            app_pid,
            app_starttime_ticks: app_start,
            watchdog_pid: wd_pid,
            watchdog_starttime_ticks: wd_start,
            cgroup_subtree_root: containment
                .subtree_root()
                .map_or_else(PathBuf::new, PathBuf::from),
        };

        let registry_file = RegistryFile::open(&config.registry_dir, header)
            .ok()
            .map(Arc::new);

        let global_pool = Arc::new(GlobalBufferPool::new(config.global_buffer_budget));
        let event_hub = Arc::new(EventHub::new());
        let revision_counter = Arc::new(AtomicU64::new(0));
        let taps = Arc::new(TapRegistry::new());

        let manager = Arc::new(Self {
            config: config.clone(),
            containment,
            proc_source,
            signaler,
            env_policy,
            instances: RwLock::new(HashMap::new()),
            spawn_gate: Mutex::new(()),
            shutting_down: AtomicBool::new(false),
            direct_children,
            global_pool,
            event_hub,
            revision_counter,
            taps,
            watchdog,
            registry_file,
            supervisor: Mutex::new(None),
        });

        let host = Arc::new(ManagerSupervisorHost {
            manager: Arc::downgrade(&manager),
        });
        let supervisor_handle =
            SupervisorHandle::spawn(config.supervisor_interval, config.thread_stack_size, host)
                .map_err(PtyError::Io)?;
        *manager.supervisor.lock() = Some(supervisor_handle);

        Ok(manager)
    }

    /// Cria e inicia um novo PTY a partir da especificação informada.
    ///
    /// # Errors
    /// Retorna [`PtyError::ShuttingDown`], [`PtyError::LimitReached`] ou erros de validação/spawn.
    #[allow(clippy::needless_pass_by_value)]
    pub fn spawn(&self, spec: SpawnSpec) -> PtyResult<PtySummary> {
        self.spawn_with_lifecycle(spec, None)
    }

    /// Cria e inicia um novo PTY permitindo injeção determinística de [`SpawnLifecycle`] (Fase 05).
    ///
    /// # Errors
    /// Retorna [`PtyError::ShuttingDown`], [`PtyError::LimitReached`] ou erros de validação/spawn/lifecycle.
    #[allow(clippy::needless_pass_by_value)]
    pub fn spawn_with_lifecycle(
        &self,
        spec: SpawnSpec,
        hook: Option<&dyn SpawnLifecycle>,
    ) -> PtyResult<PtySummary> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(PtyError::ShuttingDown);
        }

        let _gate = self.spawn_gate.lock();
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(PtyError::ShuttingDown);
        }

        let active_count = self
            .instances
            .read()
            .values()
            .filter(|inst| {
                matches!(
                    inst.status(),
                    PtyStatus::Running | PtyStatus::Exiting { .. }
                )
            })
            .count();

        if active_count >= self.config.max_ptys {
            return Err(PtyError::LimitReached(self.config.max_ptys));
        }

        let pty_id = PtyId::new();
        let mut prepared = prepare_spawn(pty_id, &spec, &*self.env_policy)?;
        if let Some(lifecycle) = hook
            && let Err(err) = lifecycle.prepare(&mut prepared)
        {
            lifecycle.failed(pty_id);
            return Err(err);
        }

        let cg_handle = match self.containment.create(pty_id) {
            Ok(cg) => cg,
            Err(err) => {
                if let Some(lifecycle) = hook {
                    lifecycle.failed(pty_id);
                }
                return Err(err);
            }
        };

        let ctx = SpawnContext {
            containment: cg_handle,
            watchdog: self.watchdog.clone(),
            registry_file: self.registry_file.clone(),
            direct_children: self.direct_children.clone(),
            global_pool: self.global_pool.clone(),
            proc_source: self.proc_source.clone(),
            signaler: self.signaler.clone(),
            event_hub: self.event_hub.clone(),
            revision_counter: self.revision_counter.clone(),
            taps: self.taps.clone(),
        };

        let instance = match PtyInstance::spawn(prepared, &self.config, ctx, hook) {
            Ok(inst) => inst,
            Err(err) => {
                if let Some(lifecycle) = hook {
                    lifecycle.failed(pty_id);
                }
                return Err(err);
            }
        };
        let summary = instance.summary();

        self.instances.write().insert(pty_id, instance);
        self.event_hub.publish(&PtyEvent::Spawned {
            summary: summary.clone(),
            revision: summary.revision,
        });

        Ok(summary)
    }

    /// Retorna referência ao provedor `ProcSource` configurado neste gerenciador.
    #[must_use]
    pub const fn proc_source(&self) -> &Arc<dyn ProcSource> {
        &self.proc_source
    }

    /// Anexa um [`OutputSink`] a um PTY existente (ativo ou tombstone dentro do TTL).
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`] ou [`PtyError::CursorInvalid`].
    pub fn attach(
        &self,
        id: PtyId,
        sink: Arc<dyn OutputSink>,
        from: Option<OutputCursor>,
    ) -> PtyResult<AttachInfo> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        inst.attach(sink, from)
    }

    /// Desanexa uma geração específica de [`OutputSink`].
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`] se o PTY não existir.
    pub fn detach(&self, id: PtyId, attachment_id: AttachmentId) -> PtyResult<()> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        inst.detach(attachment_id)
    }

    /// Registra confirmação cumulativa de bytes consumidos (`consumed_bytes`) na conexão `attachment_id`.
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`], [`PtyError::AttachmentStale`] ou [`PtyError::AckInvalid`].
    pub fn ack(
        &self,
        id: PtyId,
        attachment_id: AttachmentId,
        consumed_bytes: u64,
    ) -> PtyResult<()> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        inst.ack(attachment_id, consumed_bytes)
    }

    /// Escreve um slice de bytes na entrada do PTY.
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`], [`PtyError::AlreadyExited`], [`PtyError::InputInvalid`] ou [`PtyError::WriteQueueFull`].
    pub fn write(&self, id: PtyId, data: &[u8]) -> PtyResult<usize> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        inst.write(data)
    }

    /// Escreve um [`PtyInput`] tipado na entrada do PTY.
    ///
    /// # Errors
    /// Retorna erro caso o PTY não exista, já tenha saído ou a fila esteja cheia.
    pub fn write_input(&self, id: PtyId, input: PtyInput) -> PtyResult<usize> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        inst.write_input(input)
    }

    /// Redimensiona a janela do PTY indicado.
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`], [`PtyError::ResizeInvalid`] ou [`PtyError::AlreadyExited`].
    pub fn resize(&self, id: PtyId, size: PtySize) -> PtyResult<()> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        inst.resize(size)
    }

    /// Retorna a contagem de `ioctl(TIOCSWINSZ)` reais executados no PTY (CA-07).
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`] se o PTY não existir.
    pub fn resize_ioctl_count(&self, id: PtyId) -> PtyResult<u64> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        Ok(inst.resize_ioctl_count())
    }

    /// Encerra um PTY específico usando o modo solicitado (`Graceful` ou `Force`).
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`] ou [`PtyError::CleanupFailed`].
    pub fn kill(&self, id: PtyId, mode: KillMode) -> PtyResult<ExitInfo> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        inst.kill(mode)
    }

    /// Encerra todos os PTYs pertencentes a `tab_id` (e opcionalmente `pane_id`).
    ///
    /// # Errors
    /// Retorna erro se algum dos encerramentos falhar na limpeza do cgroup.
    pub fn kill_by_owner(
        &self,
        tab_id: TabId,
        pane_id: Option<PaneId>,
        mode: KillMode,
    ) -> PtyResult<Vec<(PtyId, ExitInfo)>> {
        let targets: Vec<Arc<PtyInstance>> = self
            .instances
            .read()
            .values()
            .filter(|inst| {
                let owner = inst.owner();
                owner.tab_id == Some(tab_id) && (pane_id.is_none() || owner.pane_id == pane_id)
            })
            .cloned()
            .collect();

        let mut results = Vec::with_capacity(targets.len());
        for inst in targets {
            let info = inst.kill(mode)?;
            results.push((inst.id(), info));
        }
        Ok(results)
    }

    /// Executa o desligamento global coordenado de todos os PTYs em paralelo dentro de `deadline` (CA-20).
    #[must_use]
    pub fn shutdown(&self, deadline: Duration) -> ShutdownReport {
        self.shutting_down.store(true, Ordering::SeqCst);

        if let Some(mut sup) = self.supervisor.lock().take() {
            sup.shutdown();
        }

        let active_instances: Vec<Arc<PtyInstance>> = self
            .instances
            .read()
            .values()
            .filter(|inst| {
                matches!(
                    inst.status(),
                    PtyStatus::Running | PtyStatus::Exiting { .. }
                )
            })
            .cloned()
            .collect();

        let mut base_plan = KillPlan::for_shutdown(&self.config);
        if base_plan.hup_grace + base_plan.term_grace > deadline {
            let third = deadline / 3;
            base_plan.hup_grace = third.min(Duration::from_millis(300));
            base_plan.term_grace = third.min(Duration::from_millis(500));
            base_plan.force_wait = third.max(Duration::from_millis(200));
        }

        let outcomes: Vec<(PtyId, PtyResult<ExitInfo>)> = thread::scope(|s| {
            let mut handles = Vec::with_capacity(active_instances.len());
            for inst in active_instances {
                let plan = base_plan;
                handles.push(s.spawn(move || {
                    let res = inst.kill_with_plan(plan, Some(ExitReason::Shutdown));
                    (inst.id(), res)
                }));
            }
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });

        let mut completed = Vec::new();
        let mut pending = Vec::new();

        for (pty_id, res) in outcomes {
            if res.is_ok() {
                completed.push(pty_id);
            } else {
                pending.push(pty_id);
            }
        }

        let all_instances: Vec<Arc<PtyInstance>> =
            self.instances.read().values().cloned().collect();
        for inst in all_instances {
            inst.finalize_tombstone();
        }

        let _ = reap_adopted_orphans(&self.direct_children, &[]);

        if pending.is_empty() {
            if let Some(wd) = &self.watchdog {
                self.direct_children.unregister(wd.watchdog_pid());
                let _ = wd.shutdown();
            }
            if let Some(reg) = &self.registry_file {
                reg.cleanup_files();
            }
            if let Some(root) = self.containment.subtree_root() {
                let _ = std::fs::remove_dir(root);
            }
        }

        ShutdownReport { completed, pending }
    }

    /// Retorna o resumo ([`PtySummary`]) de um PTY pelo seu identificador.
    #[must_use]
    pub fn get(&self, id: PtyId) -> Option<PtySummary> {
        self.get_instance(id).map(|inst| inst.summary())
    }

    /// Retorna a referência `Arc<PtyInstance>` de um PTY pelo seu identificador.
    #[must_use]
    pub fn get_instance(&self, id: PtyId) -> Option<Arc<PtyInstance>> {
        self.instances.read().get(&id).cloned()
    }

    /// Lista os resumos de todos os PTYs ativos e tombstones retidos, ordenados por criação.
    #[must_use]
    pub fn list(&self) -> Vec<PtySummary> {
        let mut summaries: Vec<PtySummary> = self
            .instances
            .read()
            .values()
            .map(|inst| inst.summary())
            .collect();
        summaries.sort_by(|a, b| {
            a.created_at_ms
                .0
                .cmp(&b.created_at_ms.0)
                .then_with(|| a.id.as_uuid().cmp(b.id.as_uuid()))
        });
        summaries
    }

    /// Retorna as identidades de todos os processos atualmente pertencentes ao cgroup do PTY.
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`] se o PTY não existir.
    pub fn tracked_processes(&self, id: PtyId) -> PtyResult<Vec<ProcessIdentity>> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        let pids = inst.containment().members()?;
        let mut identities = Vec::with_capacity(pids.len());
        for pid in pids {
            if let Ok(Some(identity)) = self.proc_source.identity(pid) {
                identities.push(identity);
            }
        }
        Ok(identities)
    }

    /// Constrói o relatório completo de árvore de processos e foreground do PTY (`pty_process_info`).
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`] se o PTY não existir ou [`PtyError::AlreadyExited`] se já encerrou.
    pub fn process_report(&self, id: PtyId) -> PtyResult<PtyProcessReport> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        if matches!(
            inst.status(),
            PtyStatus::Exited { .. } | PtyStatus::CleanupFailed { .. }
        ) {
            return Err(PtyError::AlreadyExited);
        }

        let mut pids = inst.pids();
        pids.foreground_pgid = inst.foreground_pgid();
        let members = inst.containment().members()?;

        build_process_report(
            id,
            inst.leader_identity(),
            pids,
            &members,
            &*self.proc_source,
            current_epoch_ms(),
        )
    }

    /// Envia um sinal POSIX validado por `pidfd` e cgroup a um processo pertencente ao PTY `id`.
    ///
    /// # Errors
    /// Retorna [`PtyError::NotFound`] ou [`PtyError::PidNotOwned`] se o processo não pertencer ao PTY.
    pub fn signal_process(
        &self,
        id: PtyId,
        process: &ProcessIdentity,
        sig: Signal,
    ) -> PtyResult<()> {
        let inst = self.get_instance(id).ok_or(PtyError::NotFound(id))?;
        let members = inst.containment().members()?;
        if !members.contains(&process.pid) && process.pid != inst.leader_identity().pid {
            return Err(PtyError::PidNotOwned(process.pid));
        }
        self.signaler
            .signal_in_cgroup(process, inst.containment().cgroup_id(), sig)
    }

    /// Descobre a qual PTY ativo pertence a identidade de processo informada.
    #[must_use]
    pub fn owner_of_process(&self, process: &ProcessIdentity) -> Option<PtyId> {
        let instances: Vec<Arc<PtyInstance>> = self.instances.read().values().cloned().collect();
        for inst in instances {
            if let Ok(members) = inst.containment().members()
                && members.contains(&process.pid)
                && let Ok(Some(current)) = self.proc_source.identity(process.pid)
                && &current == process
            {
                return Some(inst.id());
            }
        }
        None
    }

    /// Registra um listener de eventos de ciclo de vida ([`PtyEventListener`]).
    pub fn add_listener(&self, listener: Arc<dyn PtyEventListener>) {
        self.event_hub.add_listener(listener);
    }

    /// Registra um tap de saída bruta ([`OutputTap`]) e retorna seu [`TapHandle`] RAII.
    #[must_use]
    pub fn add_output_tap(&self, tap: Arc<dyn OutputTap>) -> TapHandle {
        self.taps.register(tap)
    }

    /// Inscreve um canal receptor para todos os [`PtyEvent`] emitidos pelo gerenciador.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<PtyEvent> {
        self.event_hub.subscribe()
    }

    /// Instala os hooks globais de `panic!` e sinais POSIX conectados ao canal de emergência do watchdog.
    ///
    /// # Errors
    /// Retorna erro caso a duplicação do descritor de emergência ou o registro de sinais falhe.
    pub fn install_guards(self: &Arc<Self>) -> PtyResult<GuardHandle> {
        if let Some(wd) = &self.watchdog {
            install_panic_hook(wd.emergency_fd())?;
        }
        let weak_mgr = Arc::downgrade(self);
        install_signal_handler(move |_sig| {
            if let Some(mgr) = weak_mgr.upgrade() {
                mgr.emergency_kill_all();
            }
        })?;
        Ok(GuardHandle { _private: () })
    }

    /// Dispara o desligamento de emergência imediato (`cgroup.kill`) em todos os PTYs e notifica o watchdog.
    pub fn emergency_kill_all(&self) {
        notify_emergency_fd();
        if let Some(wd) = &self.watchdog {
            let _ = wd.trigger_emergency();
        }
        let list: Vec<Arc<PtyInstance>> = self.instances.read().values().cloned().collect();
        for inst in list {
            let _ = inst.containment().kill_all();
        }
    }
}

impl Drop for PtyManager {
    fn drop(&mut self) {
        if let Some(mut sup) = self.supervisor.lock().take() {
            sup.shutdown();
        }
    }
}
