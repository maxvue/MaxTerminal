//! Instância individual de um pseudo-terminal (`PtyInstance`) e coordenação de suas threads
//! trabalhadoras (`mt-pty-r-<id8>`, `mt-pty-w-<id8>`, `mt-pty-x-<id8>`).

use std::os::fd::{AsFd, OwnedFd};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use mt_core::ids::PtyId;
use parking_lot::{Condvar, Mutex, RwLock};
use portable_pty::{CommandBuilder, MasterPty, native_pty_system};
use rustix::io::{FdFlags, dup, fcntl_setfd};

use crate::config::PtyConfig;
use crate::containment::launcher::{
    DEFAULT_LAUNCHER_TIMEOUT, LauncherServer, resolve_helper_binary,
};
use crate::containment::{ContainmentHandle, WatchdogClient};
use crate::error::{PtyError, PtyResult};
use crate::events::{EventHub, PtyEvent};
use crate::guard::{DirectChildRegistry, RegistryEntry, RegistryFile};
use crate::io::{GlobalBufferPool, ReaderWorker, SharedOutputState, WriteQueue, WriterWorker};
use crate::kill::{KillControl, KillCoordinator, KillPlan};
use crate::proc::{
    ProcSource, Signaler, query_foreground_pgid, read_process_identity, resolve_foreground_process,
};
use crate::sink::OutputSink;
use crate::spawn::{PreparedSpawn, SpawnLifecycle};
use crate::supervisor::ForegroundTracker;
use crate::tap::TapRegistry;
use crate::types::{
    AttachInfo, AttachmentId, ExitInfo, ExitReason, KillMode, OutputCursor, ProcessIdentity,
    ProcessInfo, PtyInput, PtyOwner, PtyPids, PtySize, PtyStatus, PtySummary, U64Decimal,
};

/// Contexto de dependências compartilhadas injetadas durante o `spawn` de um [`PtyInstance`].
#[derive(Clone)]
pub struct SpawnContext {
    /// Handle do cgroup v2 dedicado a este PTY.
    pub containment: Arc<dyn ContainmentHandle>,
    /// Cliente opcional do processo watchdog.
    pub watchdog: Option<Arc<WatchdogClient>>,
    /// Arquivo de registro persistente da instância (`pty-registry.json`).
    pub registry_file: Option<Arc<RegistryFile>>,
    /// Registro de filhos diretos para proteção do reaper seletivo (Invariante C-09).
    pub direct_children: Arc<DirectChildRegistry>,
    /// Orçamento global de buffers entre todos os PTYs.
    pub global_pool: Arc<GlobalBufferPool>,
    /// Fonte de informações `/proc`.
    pub proc_source: Arc<dyn ProcSource>,
    /// Sinalizador validado por `pidfd`.
    pub signaler: Arc<dyn Signaler>,
    /// Hub de eventos de ciclo de vida.
    pub event_hub: Arc<EventHub>,
    /// Contador monotônico global de revisões de eventos.
    pub revision_counter: Arc<AtomicU64>,
    /// Registro de taps de saída bruta.
    pub taps: Arc<TapRegistry>,
}

impl std::fmt::Debug for SpawnContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpawnContext")
            .field("cgroup_path", &self.containment.cgroup_path())
            .finish_non_exhaustive()
    }
}

/// Representa uma instância ativa ou tombstone de um pseudo-terminal gerenciado.
pub struct PtyInstance {
    id: PtyId,
    owner: PtyOwner,
    shell: String,
    args: Vec<String>,
    initial_cwd: PathBuf,
    created_at_ms: u64,
    leader_identity: ProcessIdentity,
    pids: RwLock<PtyPids>,
    containment: Arc<dyn ContainmentHandle>,
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    tcgetpgrp_fd: Mutex<Option<OwnedFd>>,
    size: Mutex<PtySize>,
    resize_ioctl_count: AtomicU64,
    output_state: Arc<SharedOutputState>,
    write_queue: Arc<WriteQueue>,
    reader_stop: Arc<AtomicBool>,
    status: RwLock<PtyStatus>,
    revision: AtomicU64,
    revision_counter: Arc<AtomicU64>,
    exited_at_instant: Mutex<Option<Instant>>,
    exit_wait_mutex: Mutex<bool>,
    exit_wait_cv: Condvar,
    kill_control: Arc<KillControl>,
    kill_mutex: Mutex<()>,
    kill_coordinator: KillCoordinator,
    explicit_kill_reason: Mutex<Option<ExitReason>>,
    foreground_tracker: Mutex<ForegroundTracker>,
    config: PtyConfig,
    reader_join: Mutex<Option<JoinHandle<()>>>,
    writer_join: Mutex<Option<JoinHandle<()>>>,
    waiter_join: Mutex<Option<JoinHandle<()>>>,
}

impl std::fmt::Debug for PtyInstance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyInstance")
            .field("id", &self.id)
            .field("owner", &self.owner)
            .field("shell", &self.shell)
            .field("pids", &*self.pids.read())
            .field("status", &*self.status.read())
            .finish_non_exhaustive()
    }
}

impl PtyInstance {
    /// Cria e inicia um novo pseudo-terminal com inscrição pré-`exec` no cgroup v2 e 3 threads trabalhadoras.
    ///
    /// # Errors
    /// Retorna [`PtyError`] se a abertura do PTY, o handshake com o `mt-pty-helper --mode launcher`,
    /// a inscrição no cgroup ou o `execve` do shell falharem.
    #[allow(clippy::too_many_lines)]
    pub fn spawn(
        prepared: PreparedSpawn,
        config: &PtyConfig,
        ctx: SpawnContext,
        lifecycle: Option<&dyn SpawnLifecycle>,
    ) -> PtyResult<Arc<Self>> {
        let pty_id = prepared.pty_id;
        let pty_system = native_pty_system();
        let pty_size = portable_pty::PtySize {
            rows: prepared.size.rows,
            cols: prepared.size.cols,
            pixel_width: prepared.size.pixel_width,
            pixel_height: prepared.size.pixel_height,
        };

        let pair = pty_system
            .openpty(pty_size)
            .map_err(|e| PtyError::SpawnFailed(format!("Falha em openpty: {e}")))?;

        let helper_bin = resolve_helper_binary(config.helper_bin_override.as_deref())?;
        let launcher_server = LauncherServer::bind(&config.registry_dir, pty_id)?;

        let mut cmd = CommandBuilder::new(&helper_bin);
        cmd.arg("--mode");
        cmd.arg("launcher");
        cmd.arg("--socket");
        cmd.arg(launcher_server.socket_path());
        cmd.cwd(&prepared.cwd);

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| PtyError::SpawnFailed(format!("Falha ao iniciar helper launcher: {e}")))?;

        // Fecha imediatamente o descritor slave no processo pai para que o master receba EOF quando os filhos saírem.
        drop(pair.slave);

        let child_pid = child.process_id().ok_or_else(|| {
            let _ = child.kill();
            PtyError::SpawnFailed("portable-pty não informou o PID do processo filho".to_string())
        })?;

        // Registra como filho direto antes de qualquer possibilidade de órfãos (Invariante C-09).
        ctx.direct_children.register(child_pid);

        let pending_launcher = match launcher_server.accept_and_enroll(
            child_pid,
            &*ctx.containment,
            DEFAULT_LAUNCHER_TIMEOUT,
        ) {
            Ok(p) => p,
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                ctx.direct_children.unregister(child_pid);
                let _ = ctx.containment.kill_all();
                let _ = ctx.containment.release();
                return Err(e);
            }
        };

        let leader_identity = resolve_leader_identity(&*ctx.proc_source, child_pid);

        if let Some(wd) = &ctx.watchdog
            && let Err(e) = wd.register_cgroup(
                pty_id,
                ctx.containment.cgroup_path(),
                ctx.containment.cgroup_id(),
            )
        {
            let _ = child.kill();
            let _ = child.wait();
            ctx.direct_children.unregister(child_pid);
            let _ = ctx.containment.kill_all();
            let _ = ctx.containment.release();
            return Err(e);
        }

        let created_at_ms = current_epoch_ms();
        let pids = PtyPids {
            shell_pid: child_pid,
            pgid: child_pid,
            sid: child_pid,
            foreground_pgid: Some(child_pid),
        };

        if let Some(reg) = &ctx.registry_file {
            let entry = RegistryEntry {
                pty_id,
                cgroup_path: ctx.containment.cgroup_path().to_path_buf(),
                cgroup_id: ctx.containment.cgroup_id(),
                pgid: i32::try_from(child_pid).unwrap_or(0),
                sid: i32::try_from(child_pid).unwrap_or(0),
                shell_pid: child_pid,
                shell_starttime_ticks: leader_identity.start_time.0,
                created_at_epoch_ms: created_at_ms,
            };
            if let Err(e) = reg.upsert(entry) {
                if let Some(wd) = &ctx.watchdog {
                    let _ = wd.unregister_cgroup(pty_id);
                }
                let _ = child.kill();
                let _ = child.wait();
                ctx.direct_children.unregister(child_pid);
                let _ = ctx.containment.kill_all();
                let _ = ctx.containment.release();
                return Err(e);
            }
        }

        let raw_master_fd = pair.master.as_raw_fd().ok_or_else(|| {
            PtyError::SpawnFailed("MasterPty não expôs descritor de arquivo bruto".to_string())
        })?;

        let (reader_fd, writer_fd, tcgetpgrp_fd) = match duplicate_master_fds(raw_master_fd) {
            Ok(fds) => fds,
            Err(e) => {
                if let Some(reg) = &ctx.registry_file {
                    let _ = reg.remove(pty_id);
                }
                if let Some(wd) = &ctx.watchdog {
                    let _ = wd.unregister_cgroup(pty_id);
                }
                let _ = child.kill();
                let _ = child.wait();
                ctx.direct_children.unregister(child_pid);
                let _ = ctx.containment.kill_all();
                let _ = ctx.containment.release();
                return Err(e);
            }
        };

        let output_state = Arc::new(SharedOutputState::from_config(config, ctx.taps.clone())?);
        let write_queue = Arc::new(WriteQueue::from_config(config, ctx.global_pool.clone())?);
        let reader_stop = Arc::new(AtomicBool::new(false));
        let kill_coordinator = KillCoordinator::new(ctx.proc_source.clone(), ctx.signaler.clone());
        let initial_rev = ctx.revision_counter.fetch_add(1, Ordering::SeqCst) + 1;

        let short_id = &pty_id.as_uuid().simple().to_string()[..8];

        // 1. Thread Reader (`mt-pty-r-<id8>`) iniciada antes do `release` para drenar imediatamente o master PTY.
        let reader_worker = ReaderWorker::from_config(
            pty_id,
            reader_fd,
            output_state.clone(),
            config,
            reader_stop.clone(),
        );
        let reader_handle = thread::Builder::new()
            .name(format!("mt-pty-r-{short_id}"))
            .stack_size(config.thread_stack_size)
            .spawn(move || {
                let _ = reader_worker.run();
            })
            .map_err(PtyError::Io)?;

        // 2. Thread Writer (`mt-pty-w-<id8>`)
        let writer_worker =
            WriterWorker::new(writer_fd, write_queue.clone(), config.write_chunk_bytes);
        let writer_handle = thread::Builder::new()
            .name(format!("mt-pty-w-{short_id}"))
            .stack_size(config.thread_stack_size)
            .spawn(move || {
                let _ = writer_worker.run();
            })
            .map_err(PtyError::Io)?;

        let initial_summary = PtySummary {
            id: pty_id,
            owner: prepared.owner.clone(),
            shell: prepared.executable.to_string_lossy().into_owned(),
            args: prepared.args.clone(),
            cwd_initial: prepared.cwd.to_string_lossy().into_owned(),
            size: prepared.size,
            status: PtyStatus::Running,
            pids,
            attached: false,
            created_at_ms: U64Decimal::from(created_at_ms),
            bytes_out: U64Decimal::from(0),
            seq: U64Decimal::from(0),
            revision: U64Decimal::from(initial_rev),
        };

        if let Some(hook) = lifecycle
            && let Err(e) = hook.started(&initial_summary)
        {
            reader_stop.store(true, Ordering::Release);
            output_state.wake().wake();
            write_queue.close();
            drop(pair.master);
            drop(tcgetpgrp_fd);
            if let Some(reg) = &ctx.registry_file {
                let _ = reg.remove(pty_id);
            }
            if let Some(wd) = &ctx.watchdog {
                let _ = wd.unregister_cgroup(pty_id);
            }
            let _ = child.kill();
            let _ = child.wait();
            ctx.direct_children.unregister(child_pid);
            let _ = ctx.containment.kill_all();
            let _ = ctx.containment.release();
            let _ = reader_handle.join();
            let _ = writer_handle.join();
            return Err(e);
        }

        // Autoriza o helper já contido no cgroup a realizar `execve` do shell real.
        if let Err(e) = pending_launcher.release(&prepared) {
            reader_stop.store(true, Ordering::Release);
            output_state.wake().wake();
            write_queue.close();
            drop(pair.master);
            drop(tcgetpgrp_fd);
            if let Some(reg) = &ctx.registry_file {
                let _ = reg.remove(pty_id);
            }
            if let Some(wd) = &ctx.watchdog {
                let _ = wd.unregister_cgroup(pty_id);
            }
            let _ = child.kill();
            let _ = child.wait();
            ctx.direct_children.unregister(child_pid);
            let _ = ctx.containment.kill_all();
            let _ = ctx.containment.release();
            let _ = reader_handle.join();
            let _ = writer_handle.join();
            return Err(e);
        }

        let instance = Arc::new(Self {
            id: pty_id,
            owner: prepared.owner,
            shell: prepared.executable.to_string_lossy().into_owned(),
            args: prepared.args,
            initial_cwd: prepared.cwd,
            created_at_ms,
            leader_identity,
            pids: RwLock::new(pids),
            containment: ctx.containment.clone(),
            master: Mutex::new(Some(pair.master)),
            tcgetpgrp_fd: Mutex::new(Some(tcgetpgrp_fd)),
            size: Mutex::new(prepared.size),
            resize_ioctl_count: AtomicU64::new(0),
            output_state,
            write_queue,
            reader_stop,
            status: RwLock::new(PtyStatus::Running),
            revision: AtomicU64::new(initial_rev),
            revision_counter: ctx.revision_counter.clone(),
            exited_at_instant: Mutex::new(None),
            exit_wait_mutex: Mutex::new(false),
            exit_wait_cv: Condvar::new(),
            kill_control: Arc::new(KillControl::new()),
            kill_mutex: Mutex::new(()),
            kill_coordinator,
            explicit_kill_reason: Mutex::new(None),
            foreground_tracker: Mutex::new(ForegroundTracker::new()),
            config: config.clone(),
            reader_join: Mutex::new(Some(reader_handle)),
            writer_join: Mutex::new(Some(writer_handle)),
            waiter_join: Mutex::new(None),
        });

        // 3. Thread Waiter (`mt-pty-x-<id8>`)
        let inst_for_waiter = Arc::clone(&instance);
        let exit_monitor_handle = thread::Builder::new()
            .name(format!("mt-pty-x-{short_id}"))
            .stack_size(config.thread_stack_size)
            .spawn(move || {
                inst_for_waiter.run_waiter_loop(child, &ctx);
            })
            .map_err(PtyError::Io)?;
        *instance.waiter_join.lock() = Some(exit_monitor_handle);

        Ok(instance)
    }

    /// Retorna o identificador único do PTY.
    #[must_use]
    pub const fn id(&self) -> PtyId {
        self.id
    }

    /// Retorna o vínculo de aba/painel do PTY.
    #[must_use]
    pub fn owner(&self) -> PtyOwner {
        self.owner.clone()
    }

    /// Retorna a identidade imutável `(boot_id, pid, start_time)` do processo líder do shell.
    #[must_use]
    pub const fn leader_identity(&self) -> &ProcessIdentity {
        &self.leader_identity
    }

    /// Retorna o status atual do PTY (`Running`, `Exiting`, `Exited`, `CleanupFailed`).
    #[must_use]
    pub fn status(&self) -> PtyStatus {
        self.status.read().clone()
    }

    /// Retorna os identificadores POSIX (`shell_pid`, `pgid`, `sid`, `foreground_pgid`) do PTY.
    #[must_use]
    pub fn pids(&self) -> PtyPids {
        *self.pids.read()
    }

    /// Retorna o handle de contenção cgroup v2 do PTY.
    #[must_use]
    pub const fn containment(&self) -> &Arc<dyn ContainmentHandle> {
        &self.containment
    }

    /// Retorna o estado compartilhado de saída e replay do PTY.
    #[must_use]
    pub const fn output_state(&self) -> &Arc<SharedOutputState> {
        &self.output_state
    }

    /// Retorna a revisão monotônica atual do PTY.
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    fn bump_revision(&self) -> u64 {
        let next = self.revision_counter.fetch_add(1, Ordering::SeqCst) + 1;
        self.revision.store(next, Ordering::Release);
        next
    }

    /// Constrói o resumo atual ([`PtySummary`]) do PTY.
    #[must_use]
    pub fn summary(&self) -> PtySummary {
        let mut pids = *self.pids.read();
        if matches!(self.status(), PtyStatus::Running)
            && let Some(fg) = self.foreground_pgid()
        {
            pids.foreground_pgid = Some(fg);
        }
        PtySummary {
            id: self.id,
            owner: self.owner.clone(),
            shell: self.shell.clone(),
            args: self.args.clone(),
            cwd_initial: self.initial_cwd.to_string_lossy().into_owned(),
            size: *self.size.lock(),
            status: self.status(),
            pids,
            attached: self.output_state.is_attached(),
            created_at_ms: U64Decimal(self.created_at_ms),
            bytes_out: U64Decimal(self.output_state.bytes_out()),
            seq: U64Decimal(self.output_state.last_seq()),
            revision: U64Decimal(self.revision()),
        }
    }

    /// Enfileira bytes para escrita no descritor mestre do PTY.
    ///
    /// # Errors
    /// Retorna [`PtyError::AlreadyExited`], [`PtyError::InputInvalid`] ou [`PtyError::WriteQueueFull`].
    pub fn write(&self, data: &[u8]) -> PtyResult<usize> {
        if !matches!(self.status(), PtyStatus::Running) {
            return Err(PtyError::AlreadyExited);
        }
        let len = data.len();
        self.write_queue.enqueue(Bytes::copy_from_slice(data))?;
        Ok(len)
    }

    /// Enfileira um [`PtyInput`] (texto UTF-8 ou bytes brutos) para escrita no PTY.
    ///
    /// # Errors
    /// Retorna erro caso o PTY já tenha encerrado ou a fila esteja cheia.
    pub fn write_input(&self, input: PtyInput) -> PtyResult<usize> {
        match input {
            PtyInput::Text { text } => self.write(text.as_bytes()),
            PtyInput::Bytes { bytes } => self.write(&bytes),
        }
    }

    /// Redimensiona a janela do PTY validando limites `1..=1000` e deduplicando chamadas idênticas (CA-07).
    ///
    /// # Errors
    /// Retorna [`PtyError::ResizeInvalid`] se fora dos limites ou [`PtyError::AlreadyExited`] se encerrado.
    pub fn resize(&self, size: PtySize) -> PtyResult<()> {
        size.validate()?;
        if !matches!(self.status(), PtyStatus::Running) {
            return Err(PtyError::AlreadyExited);
        }

        let mut size_guard = self.size.lock();
        if *size_guard == size {
            return Ok(());
        }

        let master_guard = self.master.lock();
        let Some(master) = master_guard.as_ref() else {
            return Err(PtyError::AlreadyExited);
        };

        master
            .resize(portable_pty::PtySize {
                rows: size.rows,
                cols: size.cols,
                pixel_width: size.pixel_width,
                pixel_height: size.pixel_height,
            })
            .map_err(|e| {
                PtyError::Io(std::io::Error::other(format!("Falha em resize ioctl: {e}")))
            })?;

        *size_guard = size;
        self.resize_ioctl_count.fetch_add(1, Ordering::Relaxed);
        let _ = self.bump_revision();
        Ok(())
    }

    /// Retorna a quantidade de chamadas reais `ioctl(TIOCSWINSZ)` efetuadas neste PTY (CA-07).
    #[must_use]
    pub fn resize_ioctl_count(&self) -> u64 {
        self.resize_ioctl_count.load(Ordering::Relaxed)
    }

    /// Anexa um novo [`OutputSink`] a partir do cursor opcional.
    ///
    /// # Errors
    /// Retorna [`PtyError::CursorInvalid`] se o cursor informado estiver no futuro ou for inconsistente.
    pub fn attach(
        &self,
        sink: Arc<dyn OutputSink>,
        from: Option<OutputCursor>,
    ) -> PtyResult<AttachInfo> {
        let info = self.output_state.attach(sink, from)?;
        let _ = self.bump_revision();
        Ok(info)
    }

    /// Desanexa uma geração de conexão específica (no-op idempotente se já substituída).
    ///
    /// # Errors
    /// Retorna erro caso o estado de fluxo falhe.
    pub fn detach(&self, attachment_id: AttachmentId) -> PtyResult<()> {
        self.output_state.detach(attachment_id)?;
        let _ = self.bump_revision();
        Ok(())
    }

    /// Registra confirmação cumulativa de bytes consumidos (`consumed_bytes`) para `attachment_id`.
    ///
    /// # Errors
    /// Retorna [`PtyError::AttachmentStale`] ou [`PtyError::AckInvalid`].
    pub fn ack(&self, attachment_id: AttachmentId, consumed_bytes: u64) -> PtyResult<()> {
        self.output_state.ack(attachment_id, consumed_bytes)
    }

    /// Solicita o encerramento do PTY com o modo indicado (`Graceful` ou `Force`).
    ///
    /// Se já encerrado, retorna o [`ExitInfo`] de forma idempotente.
    /// Se `Graceful` estiver em andamento e `Force` for solicitado, promove imediatamente para `Force`.
    ///
    /// # Errors
    /// Retorna [`PtyError::CleanupFailed`] se restarem processos sobreviventes no cgroup após `Force`.
    pub fn kill(&self, mode: KillMode) -> PtyResult<ExitInfo> {
        let plan = KillPlan::from_config(&self.config, mode);
        self.kill_with_plan(plan, Some(ExitReason::KilledByUser))
    }

    /// Executa um [`KillPlan`] customizado (ex.: durante `shutdown`) e aguarda a conclusão do `waiter`.
    ///
    /// # Errors
    /// Retorna [`PtyError::CleanupFailed`] se processos sobreviverem após `cgroup.kill`.
    pub fn kill_with_plan(
        &self,
        plan: KillPlan,
        reason_override: Option<ExitReason>,
    ) -> PtyResult<ExitInfo> {
        if let PtyStatus::Exited { exit } = self.status() {
            return Ok(exit);
        }

        self.write_queue.close();

        {
            let mut reason_guard = self.explicit_kill_reason.lock();
            if reason_guard.is_none() {
                *reason_guard = Some(reason_override.unwrap_or(ExitReason::KilledByUser));
            }
        }

        if plan.mode == KillMode::Force {
            self.kill_control.request_force();
        }

        {
            let mut status_guard = self.status.write();
            if matches!(*status_guard, PtyStatus::Running) {
                *status_guard = PtyStatus::Exiting {
                    since_ms: U64Decimal(current_epoch_ms()),
                };
                let _ = self.bump_revision();
            }
        }

        let outcome = {
            let _kill_lock = self.kill_mutex.lock();
            self.kill_coordinator.execute(
                &self.leader_identity,
                &*self.containment,
                &plan,
                &self.kill_control,
            )?
        };

        let exit_info = self.wait_for_exit_completion(Duration::from_secs(5));

        if !outcome.cgroup_empty {
            return Err(PtyError::CleanupFailed(format!(
                "Processos sobreviventes no cgroup do PTY {}: {:?}",
                self.id, outcome.survivors
            )));
        }

        Ok(exit_info)
    }

    /// Consulta o `PGID` atualmente em foreground via `tcgetpgrp` se o PTY estiver em execução.
    #[must_use]
    pub fn foreground_pgid(&self) -> Option<u32> {
        if !matches!(self.status(), PtyStatus::Running) {
            return None;
        }
        let guard = self.tcgetpgrp_fd.lock();
        let fd = guard.as_ref()?;
        query_foreground_pgid(fd.as_fd())
    }

    /// Avalia se o grupo de processos em foreground mudou e emite `PtyEvent::ForegroundChanged` deduplicado.
    pub fn poll_foreground(&self, proc_source: &dyn ProcSource, event_hub: &EventHub) {
        if !matches!(self.status(), PtyStatus::Running) {
            return;
        }
        let fg_pgid = self.foreground_pgid();
        {
            let mut pids_guard = self.pids.write();
            pids_guard.foreground_pgid = fg_pgid;
        }

        let mut owned_procs: Vec<ProcessInfo> = Vec::new();
        if let Ok(members) = self.containment.members() {
            for pid in members {
                if let Ok(Some(info)) = proc_source.process(pid) {
                    owned_procs.push(info);
                }
            }
        }

        let fg_proc = resolve_foreground_process(fg_pgid, &owned_procs, proc_source);

        let changed = self
            .foreground_tracker
            .lock()
            .observe(fg_pgid, fg_proc.as_ref());

        if changed {
            let rev = self.bump_revision();
            event_hub.publish(&PtyEvent::ForegroundChanged {
                pty_id: self.id,
                pgid: fg_pgid,
                process: fg_proc,
                revision: U64Decimal(rev),
            });
        }
    }

    /// Indica se esta instância encerrada já cumpriu o `tombstone_ttl` e pode ser removida.
    ///
    /// Instâncias em `CleanupFailed` nunca expiram automaticamente.
    #[must_use]
    pub fn is_tombstone_expired(&self, ttl: Duration) -> bool {
        match self.status() {
            PtyStatus::Exited { .. } => self
                .exited_at_instant
                .lock()
                .is_some_and(|at| at.elapsed() >= ttl),
            PtyStatus::Running | PtyStatus::Exiting { .. } | PtyStatus::CleanupFailed { .. } => {
                false
            }
        }
    }

    /// Prepara a instância para remoção definitiva da tabela de tombstones.
    pub fn finalize_tombstone(&self) {
        self.output_state.force_detach();
        self.join_all_workers();
    }

    fn wait_for_exit_completion(&self, timeout: Duration) -> ExitInfo {
        let mut guard = self.exit_wait_mutex.lock();
        if !*guard {
            let _ = self.exit_wait_cv.wait_for(&mut guard, timeout);
        }
        drop(guard);

        match self.status() {
            PtyStatus::Exited { exit } => exit,
            _ => ExitInfo {
                code: None,
                signal: Some(9),
                signal_name: Some("SIGKILL".to_string()),
                reason: self
                    .explicit_kill_reason
                    .lock()
                    .unwrap_or(ExitReason::KilledByUser),
                at_ms: U64Decimal(current_epoch_ms()),
            },
        }
    }

    fn run_waiter_loop(
        &self,
        mut child: Box<dyn portable_pty::Child + Send + Sync>,
        ctx: &SpawnContext,
    ) {
        let wait_res = child.wait();
        ctx.direct_children.unregister(self.leader_identity.pid);

        // Fecha a fila de escrita e acorda imediatamente a thread `mt-pty-w-<id8>`.
        self.write_queue.close();

        // Garante que qualquer processo em background remanescente na sessão/cgroup seja encerrado
        // para que nenhum descritor slave permaneça aberto bloqueando o EOF do ReaderWorker.
        let cleanup_outcome = {
            let _kill_lock = self.kill_mutex.lock();
            self.kill_coordinator.cleanup_after_leader_exit(
                &self.leader_identity,
                &*self.containment,
                &self.config,
            )
        };

        // Libera o MasterPty e o descritor de tcgetpgrp no pai antes de aguardar o ReaderWorker.
        let _ = self.master.lock().take();
        let _ = self.tcgetpgrp_fd.lock().take();

        // Aguarda o ReaderWorker drenar o buffer do kernel até EOF/EIO.
        if let Some(reader_handle) = self.reader_join.lock().take() {
            let _ = reader_handle.join();
        }
        if let Some(writer_handle) = self.writer_join.lock().take() {
            let _ = writer_handle.join();
        }

        let explicit_reason = *self.explicit_kill_reason.lock();
        let exit_info = build_exit_info(wait_res.ok().as_ref(), explicit_reason);

        let mut survivors_identities = Vec::new();
        let cgroup_empty = match &cleanup_outcome {
            Ok(outcome) => {
                if !outcome.cgroup_empty {
                    for &pid in &outcome.survivors {
                        if let Ok(Some(id)) = ctx.proc_source.identity(pid) {
                            survivors_identities.push(id);
                        } else {
                            survivors_identities.push(ProcessIdentity {
                                boot_id: ctx.proc_source.boot_id().to_string(),
                                pid,
                                start_time: U64Decimal(0),
                            });
                        }
                    }
                }
                outcome.cgroup_empty
            }
            Err(_) => false,
        };

        let rev = self.bump_revision();
        {
            let mut status_guard = self.status.write();
            if cgroup_empty || survivors_identities.is_empty() {
                *status_guard = PtyStatus::Exited {
                    exit: exit_info.clone(),
                };
            } else {
                *status_guard = PtyStatus::CleanupFailed {
                    error: format!("Processos sobreviventes no cgroup: {survivors_identities:?}"),
                };
            }
        }
        *self.exited_at_instant.lock() = Some(Instant::now());

        // Emite frame End na stream binária e notifica OutputTap antes de emitir PtyEvent::Exited.
        self.output_state.record_end(exit_info.clone());
        ctx.taps.dispatch_exit(self.id, &exit_info);

        if let Some(reg) = &ctx.registry_file {
            let _ = reg.remove(self.id);
        }
        if let Some(wd) = &ctx.watchdog
            && cgroup_empty
        {
            let _ = wd.unregister_cgroup(self.id);
        }

        let final_seq = self.output_state.last_seq();
        ctx.event_hub.publish(&PtyEvent::Exited {
            pty_id: self.id,
            owner: self.owner.clone(),
            exit: exit_info,
            final_seq: U64Decimal(final_seq),
            revision: U64Decimal(rev),
        });

        if !survivors_identities.is_empty() {
            let fail_rev = self.bump_revision();
            ctx.event_hub.publish(&PtyEvent::CleanupFailed {
                pty_id: self.id,
                survivors: survivors_identities,
                revision: U64Decimal(fail_rev),
            });
        }

        {
            let mut done = self.exit_wait_mutex.lock();
            *done = true;
            self.exit_wait_cv.notify_all();
        }
    }

    fn join_all_workers(&self) {
        self.reader_stop.store(true, Ordering::Release);
        self.output_state.wake().wake();
        self.write_queue.close();

        let current_thread_id = thread::current().id();
        if let Some(h) = self.reader_join.lock().take()
            && h.thread().id() != current_thread_id
        {
            let _ = h.join();
        }
        if let Some(h) = self.writer_join.lock().take()
            && h.thread().id() != current_thread_id
        {
            let _ = h.join();
        }
        if let Some(h) = self.waiter_join.lock().take()
            && h.thread().id() != current_thread_id
        {
            let _ = h.join();
        }
    }
}

fn duplicate_master_fds(raw_fd: i32) -> PtyResult<(OwnedFd, OwnedFd, OwnedFd)> {
    use rustix::event::{EventfdFlags, eventfd};
    use std::os::fd::AsRawFd;

    let owned1 = eventfd(0, EventfdFlags::CLOEXEC)
        .map_err(|e| PtyError::SpawnFailed(format!("Falha ao alocar FD base: {e}")))?;
    nix::unistd::dup2(raw_fd, owned1.as_raw_fd())
        .map_err(|e| PtyError::SpawnFailed(format!("Falha ao duplicar master FD (reader): {e}")))?;
    let _ = fcntl_setfd(&owned1, FdFlags::CLOEXEC);

    let owned2 = dup(&owned1)
        .map_err(|e| PtyError::SpawnFailed(format!("Falha ao duplicar master FD (writer): {e}")))?;
    let _ = fcntl_setfd(&owned2, FdFlags::CLOEXEC);

    let owned3 = dup(&owned1).map_err(|e| {
        PtyError::SpawnFailed(format!("Falha ao duplicar master FD (tcgetpgrp): {e}"))
    })?;
    let _ = fcntl_setfd(&owned3, FdFlags::CLOEXEC);

    Ok((owned1, owned2, owned3))
}

fn resolve_leader_identity(proc_source: &dyn ProcSource, pid: u32) -> ProcessIdentity {
    if let Ok(Some(id)) = proc_source.identity(pid) {
        return id;
    }
    if let Ok(Some(id)) = read_process_identity(pid, proc_source.boot_id(), 4096) {
        return id;
    }
    ProcessIdentity {
        boot_id: proc_source.boot_id().to_string(),
        pid,
        start_time: U64Decimal(0),
    }
}

fn build_exit_info(
    status: Option<&portable_pty::ExitStatus>,
    explicit_reason: Option<ExitReason>,
) -> ExitInfo {
    let at_ms = U64Decimal(current_epoch_ms());
    let Some(st) = status else {
        return ExitInfo {
            code: None,
            signal: Some(9),
            signal_name: Some("SIGKILL".to_string()),
            reason: explicit_reason.unwrap_or(ExitReason::KilledByUser),
            at_ms,
        };
    };

    if let Some(sig_name) = st.signal() {
        let (sig_num, canonical_name) = map_signal_name_to_number_and_name(sig_name);
        let reason = explicit_reason.unwrap_or(ExitReason::Signaled);
        ExitInfo {
            code: None,
            signal: Some(sig_num),
            signal_name: Some(canonical_name.to_string()),
            reason,
            at_ms,
        }
    } else {
        let code = i32::try_from(st.exit_code()).ok();
        let reason = explicit_reason.unwrap_or(ExitReason::Exited);
        ExitInfo {
            code,
            signal: None,
            signal_name: None,
            reason,
            at_ms,
        }
    }
}

fn map_signal_name_to_number_and_name(sig_name: &str) -> (i32, &'static str) {
    let lower = sig_name.to_ascii_lowercase();
    if lower.contains("hangup") || lower.contains("hup") {
        (1, "SIGHUP")
    } else if lower.contains("interrupt") || lower.contains("int") {
        (2, "SIGINT")
    } else if lower.contains("quit") {
        (3, "SIGQUIT")
    } else if lower.contains("illegal") || lower.contains("ill") {
        (4, "SIGILL")
    } else if lower.contains("trap") {
        (5, "SIGTRAP")
    } else if lower.contains("abort") || lower.contains("abrt") {
        (6, "SIGABRT")
    } else if lower.contains("bus") {
        (7, "SIGBUS")
    } else if lower.contains("floating") || lower.contains("fpe") {
        (8, "SIGFPE")
    } else if lower.contains("killed") || lower.contains("kill") {
        (9, "SIGKILL")
    } else if lower.contains("user defined signal 1") || lower.contains("usr1") {
        (10, "SIGUSR1")
    } else if lower.contains("segmentation") || lower.contains("segv") {
        (11, "SIGSEGV")
    } else if lower.contains("user defined signal 2") || lower.contains("usr2") {
        (12, "SIGUSR2")
    } else if lower.contains("pipe") {
        (13, "SIGPIPE")
    } else if lower.contains("alarm") || lower.contains("alrm") {
        (14, "SIGALRM")
    } else if lower.contains("terminated") || lower.contains("term") {
        (15, "SIGTERM")
    } else {
        (9, "SIGKILL")
    }
}

pub(crate) fn current_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
