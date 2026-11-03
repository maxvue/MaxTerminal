//! Coordenador de varredura periódica (`/proc/net/tcp{,6}`), histerese anti-flap e eventos (`PortsService`).

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use mt_core::ids::{PaneId, PtyId, TabId};
use parking_lot::{Mutex, RwLock};
use tokio::sync::Notify;

use crate::error::{PortsError, PortsResult};
use crate::killer::{ExactProcessSignaler, kill_port_process};
use crate::opener::{PortUrlOpener, build_safe_loopback_url};
use crate::tree::{ProcessTreeSource, TrackedPaneProcess};
use crate::types::{
    ListeningPort, PollMode, PortAddress, PortClosed, PortId, PortKillResult, PortOpened,
    PortProtocol, PortSnapshot, PortUpdated,
};

/// Intervalo de varredura quando `PollMode::Active` (`1500 ms`).
pub const POLL_INTERVAL_ACTIVE: Duration = Duration::from_millis(1500);

/// Intervalo de varredura quando `PollMode::Background` (`5000 ms`).
pub const POLL_INTERVAL_BACKGROUND: Duration = Duration::from_millis(5000);

/// Quantidade de ciclos consecutivos necessários para confirmar a abertura de uma porta (`2`).
pub const OPEN_HYSTERESIS_CYCLES: u8 = 2;

/// Quantidade de ciclos consecutivos de ausência do socket para fechar porta cujo PID segue vivo (`2`).
pub const CLOSE_HYSTERESIS_SOCKET_CYCLES: u8 = 2;

/// Sink de eventos do detector de portas para desacoplar o `mt-ports` do runtime Tauri.
pub trait PortsEventSink: Send + Sync + 'static {
    /// Emitido quando uma porta passa pela histerese de abertura.
    fn emit_port_opened(&self, payload: PortOpened);
    /// Emitido quando uma porta confirmada muda de PID ou endereços de bind.
    fn emit_port_updated(&self, payload: PortUpdated);
    /// Emitido quando uma porta é encerrada.
    fn emit_port_closed(&self, payload: PortClosed);
}

/// Sink nulo para testes ou execução sem UI.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopPortsEventSink;

impl PortsEventSink for NoopPortsEventSink {
    fn emit_port_opened(&self, _payload: PortOpened) {}
    fn emit_port_updated(&self, _payload: PortUpdated) {}
    fn emit_port_closed(&self, _payload: PortClosed) {}
}

#[derive(Debug, Clone)]
struct CandidateObservation {
    tab_id: TabId,
    pane_id: PaneId,
    pty_id: PtyId,
    port: u16,
    addresses: Vec<PortAddress>,
    owner: TrackedPaneProcess,
}

#[derive(Debug, Clone)]
struct TrackedPortState {
    consecutive_seen: u8,
    consecutive_missing: u8,
    confirmed: bool,
    current: ListeningPort,
}

/// Serviço central de detecção, listagem, abertura no navegador e encerramento de portas ativas.
#[derive(Clone)]
pub struct PortsService {
    inner: Arc<PortsServiceInner>,
}

struct PortsServiceInner {
    tree_source: Arc<dyn ProcessTreeSource>,
    signaler: Arc<dyn ExactProcessSignaler>,
    opener: Arc<dyn PortUrlOpener>,
    event_sink: Arc<dyn PortsEventSink>,
    revision: AtomicU64,
    poll_mode: RwLock<PollMode>,
    mode_notify: Notify,
    shutting_down: AtomicBool,
    tracked: Mutex<BTreeMap<(PaneId, u16), TrackedPortState>>,
}

impl std::fmt::Debug for PortsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PortsService")
            .field("revision", &self.inner.revision.load(Ordering::Relaxed))
            .field("poll_mode", &*self.inner.poll_mode.read())
            .finish_non_exhaustive()
    }
}

fn now_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

impl PortsService {
    /// Cria uma nova instância do [`PortsService`].
    #[must_use]
    pub fn new(
        tree_source: Arc<dyn ProcessTreeSource>,
        signaler: Arc<dyn ExactProcessSignaler>,
        opener: Arc<dyn PortUrlOpener>,
        event_sink: Arc<dyn PortsEventSink>,
    ) -> Self {
        Self {
            inner: Arc::new(PortsServiceInner {
                tree_source,
                signaler,
                opener,
                event_sink,
                revision: AtomicU64::new(0),
                poll_mode: RwLock::new(PollMode::Active),
                mode_notify: Notify::new(),
                shutting_down: AtomicBool::new(false),
                tracked: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Inicia a task assíncrona de polling adaptativo no runtime Tokio atual.
    pub fn start_background_loop(&self) {
        let svc = self.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                svc.run_loop().await;
            });
        }
    }

    async fn run_loop(&self) {
        while !self.inner.shutting_down.load(Ordering::Acquire) {
            let mode = *self.inner.poll_mode.read();
            let active_panes = self.inner.tree_source.active_panes();

            if mode == PollMode::Paused || active_panes.is_empty() {
                tokio::select! {
                    () = self.inner.mode_notify.notified() => {},
                    () = tokio::time::sleep(POLL_INTERVAL_BACKGROUND) => {},
                }
                continue;
            }

            let _ = self.tick_once();

            let sleep_dur = match mode {
                PollMode::Active => POLL_INTERVAL_ACTIVE,
                PollMode::Background | PollMode::Paused => POLL_INTERVAL_BACKGROUND,
            };

            tokio::select! {
                () = self.inner.mode_notify.notified() => {},
                () = tokio::time::sleep(sleep_dur) => {},
            }
        }
    }

    /// Altera o modo de polling (`Active`, `Background`, `Paused`) e acorda o loop imediatamente.
    pub fn set_poll_mode(&self, mode: PollMode) {
        *self.inner.poll_mode.write() = mode;
        self.inner.mode_notify.notify_waiters();
    }

    /// Retorna o modo de polling atual.
    #[must_use]
    pub fn poll_mode(&self) -> PollMode {
        *self.inner.poll_mode.read()
    }

    /// Acorda o loop de polling (ex.: quando um novo painel é aberto).
    pub fn notify_activity(&self) {
        self.inner.mode_notify.notify_waiters();
    }

    /// Executa um ciclo completo de varredura e aplica as regras de histerese de abertura/fechamento.
    pub fn tick_once(&self) -> PortsResult<()> {
        let active_panes = self.inner.tree_source.active_panes();
        if active_panes.is_empty() {
            self.purge_all_confirmations();
            return Ok(());
        }

        let listen_entries = self.inner.tree_source.read_listen_entries()?;
        let observed = self.correlate_entries(&active_panes, &listen_entries);
        self.reconcile_observations(observed);
        Ok(())
    }

    /// Executa uma varredura imediata sem esperar 2 ciclos quando solicitado manualmente (`ports_list_for_tab`),
    /// retornando o [`PortSnapshot`] da aba solicitada.
    #[must_use]
    pub fn list_for_tab(&self, tab_id: TabId) -> PortSnapshot {
        let tracked = self.inner.tracked.lock();
        let mut ports: Vec<ListeningPort> = tracked
            .values()
            .filter(|st| st.confirmed && st.current.tab_id == tab_id)
            .map(|st| st.current.clone())
            .collect();
        ports.sort_by(|a, b| {
            a.port
                .cmp(&b.port)
                .then_with(|| a.pane_id.to_string().cmp(&b.pane_id.to_string()))
        });
        PortSnapshot {
            tab_id,
            ports,
            revision: self.inner.revision.load(Ordering::Acquire),
        }
    }

    /// Remove imediatamente todas as portas de um painel encerrado (`on_pane_exited`)
    /// e emite `ports://closed` para cada porta confirmada.
    pub fn remove_pane(&self, pane_id: PaneId) {
        let mut closed_events = Vec::new();
        {
            let mut tracked = self.inner.tracked.lock();
            let keys: Vec<(PaneId, u16)> = tracked
                .keys()
                .filter(|(pid, _)| *pid == pane_id)
                .copied()
                .collect();
            for key in keys {
                if let Some(removed) = tracked.remove(&key)
                    && removed.confirmed
                {
                    let rev = self.next_revision();
                    closed_events.push(PortClosed {
                        tab_id: removed.current.tab_id,
                        pane_id: removed.current.pane_id,
                        id: removed.current.id,
                        port: removed.current.port,
                        revision: rev,
                    });
                }
            }
        }
        for ev in closed_events {
            self.inner.event_sink.emit_port_closed(ev);
        }
    }

    /// Remove todas as portas associadas a uma aba fechada (`on_tab_closed`).
    pub fn remove_tab(&self, tab_id: TabId) {
        let mut closed_events = Vec::new();
        {
            let mut tracked = self.inner.tracked.lock();
            let keys: Vec<(PaneId, u16)> = tracked
                .iter()
                .filter_map(|(k, st)| (st.current.tab_id == tab_id).then_some(*k))
                .collect();
            for key in keys {
                if let Some(removed) = tracked.remove(&key)
                    && removed.confirmed
                {
                    let rev = self.next_revision();
                    closed_events.push(PortClosed {
                        tab_id: removed.current.tab_id,
                        pane_id: removed.current.pane_id,
                        id: removed.current.id,
                        port: removed.current.port,
                        revision: rev,
                    });
                }
            }
        }
        for ev in closed_events {
            self.inner.event_sink.emit_port_closed(ev);
        }
    }

    /// Abre `http://127.0.0.1:<port>/` no navegador padrão validando que a porta pertence a `pane_id`.
    pub async fn open_browser(&self, pane_id: PaneId, port: u16) -> PortsResult<String> {
        let exists = {
            let tracked = self.inner.tracked.lock();
            tracked.get(&(pane_id, port)).is_some_and(|st| st.confirmed)
        };
        if !exists {
            return Err(PortsError::NotFound { pane_id, port });
        }
        let url = build_safe_loopback_url(port)?;
        self.inner.opener.open_url(port, &url).await?;
        Ok(url)
    }

    /// Encerra cirurgicamente o processo proprietário da porta `(pane_id, port)` e emite `ports://closed`.
    pub async fn kill_port(
        &self,
        pane_id: PaneId,
        port: u16,
        expected_pid: Option<u32>,
    ) -> PortsResult<PortKillResult> {
        let target_port = {
            let tracked = self.inner.tracked.lock();
            tracked
                .get(&(pane_id, port))
                .filter(|st| st.confirmed)
                .map(|st| st.current.clone())
                .ok_or(PortsError::NotFound { pane_id, port })?
        };

        if let Some(exp_pid) = expected_pid
            && exp_pid != target_port.pid
        {
            return Err(PortsError::ProcessMismatch {
                port,
                expected_pid: exp_pid,
            });
        }

        let result = kill_port_process(
            &self.inner.signaler,
            pane_id,
            target_port.pty_id,
            port,
            target_port.pid,
            target_port.start_time_ticks,
        )
        .await?;

        // Remove imediatamente do mapa e emite ports://closed
        let closed_opt = {
            let mut tracked = self.inner.tracked.lock();
            tracked.remove(&(pane_id, port)).and_then(|removed| {
                removed.confirmed.then(|| {
                    let rev = self.next_revision();
                    PortClosed {
                        tab_id: removed.current.tab_id,
                        pane_id: removed.current.pane_id,
                        id: removed.current.id,
                        port: removed.current.port,
                        revision: rev,
                    }
                })
            })
        };

        if let Some(ev) = closed_opt {
            self.inner.event_sink.emit_port_closed(ev);
        }

        Ok(result)
    }

    /// Encerra o loop de background do detector de portas.
    pub fn shutdown(&self) {
        self.inner.shutting_down.store(true, Ordering::SeqCst);
        self.inner.mode_notify.notify_waiters();
    }

    fn next_revision(&self) -> u64 {
        self.inner.revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    fn purge_all_confirmations(&self) {
        let mut closed = Vec::new();
        {
            let mut tracked = self.inner.tracked.lock();
            let all_keys: Vec<(PaneId, u16)> = tracked.keys().copied().collect();
            for k in all_keys {
                if let Some(removed) = tracked.remove(&k)
                    && removed.confirmed
                {
                    let rev = self.next_revision();
                    closed.push(PortClosed {
                        tab_id: removed.current.tab_id,
                        pane_id: removed.current.pane_id,
                        id: removed.current.id,
                        port: removed.current.port,
                        revision: rev,
                    });
                }
            }
        }
        for ev in closed {
            self.inner.event_sink.emit_port_closed(ev);
        }
    }

    fn correlate_entries(
        &self,
        active_panes: &[crate::tree::ActivePaneTarget],
        listen_entries: &[crate::procnet::RawTcpListenEntry],
    ) -> BTreeMap<(PaneId, u16), CandidateObservation> {
        if listen_entries.is_empty() {
            return BTreeMap::new();
        }

        let listen_inodes: HashSet<u64> = listen_entries.iter().map(|e| e.inode).collect();

        // Mapeia inode -> (ActivePaneTarget, TrackedPaneProcess)
        // Se múltiplos processos na mesma árvore (worker fork) compartilham o mesmo socket inode,
        // escolhemos o processo mais específico (maior depth, ou menor PID em empate).
        let mut inode_owners: HashMap<u64, (crate::tree::ActivePaneTarget, TrackedPaneProcess)> =
            HashMap::new();

        for pane in active_panes {
            let procs = self.inner.tree_source.processes_for_pty(pane.pty_id);
            for proc_info in procs {
                let inodes = self.inner.tree_source.socket_inodes_for_pid(proc_info.pid);
                for inode in inodes {
                    if !listen_inodes.contains(&inode) {
                        continue;
                    }
                    inode_owners
                        .entry(inode)
                        .and_modify(|(_, existing)| {
                            if (proc_info.depth, std::cmp::Reverse(proc_info.pid))
                                > (existing.depth, std::cmp::Reverse(existing.pid))
                            {
                                *existing = proc_info.clone();
                            }
                        })
                        .or_insert_with(|| (*pane, proc_info.clone()));
                }
            }
        }

        let mut grouped: BTreeMap<(PaneId, u16), CandidateObservation> = BTreeMap::new();

        for raw in listen_entries {
            let Some((pane_target, owner_proc)) = inode_owners.get(&raw.inode) else {
                continue;
            };
            let addr = raw.to_port_address();
            let key = (pane_target.pane_id, raw.local_port);

            grouped
                .entry(key)
                .and_modify(|obs| {
                    if !obs.addresses.contains(&addr) {
                        obs.addresses.push(addr.clone());
                        obs.addresses.sort();
                    }
                    if (owner_proc.depth, std::cmp::Reverse(owner_proc.pid))
                        > (obs.owner.depth, std::cmp::Reverse(obs.owner.pid))
                    {
                        obs.owner = owner_proc.clone();
                    }
                })
                .or_insert_with(|| CandidateObservation {
                    tab_id: pane_target.tab_id,
                    pane_id: pane_target.pane_id,
                    pty_id: pane_target.pty_id,
                    port: raw.local_port,
                    addresses: vec![addr],
                    owner: owner_proc.clone(),
                });
        }

        grouped
    }

    #[allow(clippy::too_many_lines)]
    fn reconcile_observations(&self, mut observed: BTreeMap<(PaneId, u16), CandidateObservation>) {
        let mut opened_events = Vec::new();
        let mut updated_events = Vec::new();
        let mut closed_events = Vec::new();
        let now_ms = now_epoch_ms();

        {
            let mut tracked = self.inner.tracked.lock();
            let existing_keys: Vec<(PaneId, u16)> = tracked.keys().copied().collect();

            for key in existing_keys {
                if let Some(obs) = observed.remove(&key) {
                    if let Some(state) = tracked.get_mut(&key) {
                        state.consecutive_missing = 0;
                        state.consecutive_seen = state.consecutive_seen.saturating_add(1);

                        let pid_changed = state.current.pid != obs.owner.pid
                            || state.current.start_time_ticks != obs.owner.start_time_ticks;
                        let addr_changed = state.current.addresses != obs.addresses;

                        state.current.tab_id = obs.tab_id;
                        state.current.pty_id = obs.pty_id;
                        state.current.addresses = obs.addresses;
                        state.current.pid = obs.owner.pid;
                        state.current.start_time_ticks = obs.owner.start_time_ticks;
                        state.current.process_name = obs.owner.process_name;
                        state.current.cmdline_preview = obs.owner.cmdline_preview;

                        if !state.confirmed && state.consecutive_seen >= OPEN_HYSTERESIS_CYCLES {
                            state.confirmed = true;
                            state.current.detected_at_ms = now_ms;
                            let rev = self.next_revision();
                            opened_events.push(PortOpened {
                                tab_id: state.current.tab_id,
                                pane_id: state.current.pane_id,
                                port: state.current.clone(),
                                revision: rev,
                            });
                        } else if state.confirmed && (pid_changed || addr_changed) {
                            let rev = self.next_revision();
                            updated_events.push(PortUpdated {
                                tab_id: state.current.tab_id,
                                pane_id: state.current.pane_id,
                                port: state.current.clone(),
                                revision: rev,
                            });
                        }
                    }
                } else {
                    // Porta não foi vista neste ciclo
                    let should_remove = if let Some(state) = tracked.get_mut(&key) {
                        if state.confirmed {
                            state.consecutive_missing = state.consecutive_missing.saturating_add(1);
                            let pid_alive = self.inner.tree_source.verify_process_alive(
                                state.current.pid,
                                state.current.start_time_ticks,
                            );
                            // Histerese de fechamento: 1 ciclo se PID morreu, 2 ciclos se PID segue vivo mas socket sumiu
                            !pid_alive
                                || state.consecutive_missing >= CLOSE_HYSTERESIS_SOCKET_CYCLES
                        } else {
                            true
                        }
                    } else {
                        false
                    };

                    if should_remove
                        && let Some(removed) = tracked.remove(&key)
                        && removed.confirmed
                    {
                        let rev = self.next_revision();
                        closed_events.push(PortClosed {
                            tab_id: removed.current.tab_id,
                            pane_id: removed.current.pane_id,
                            id: removed.current.id,
                            port: removed.current.port,
                            revision: rev,
                        });
                    }
                }
            }

            // Novas portas vistas pela 1ª vez neste ciclo
            for (key, obs) in observed {
                let port_dto = ListeningPort {
                    id: PortId::for_pane_tcp(obs.pane_id, obs.port),
                    tab_id: obs.tab_id,
                    pane_id: obs.pane_id,
                    pty_id: obs.pty_id,
                    port: obs.port,
                    protocol: PortProtocol::Tcp,
                    addresses: obs.addresses,
                    browser_url: format!("http://127.0.0.1:{}/", obs.port),
                    pid: obs.owner.pid,
                    start_time_ticks: obs.owner.start_time_ticks,
                    process_name: obs.owner.process_name,
                    cmdline_preview: obs.owner.cmdline_preview,
                    detected_at_ms: now_ms,
                };
                tracked.insert(
                    key,
                    TrackedPortState {
                        consecutive_seen: 1,
                        consecutive_missing: 0,
                        confirmed: false,
                        current: port_dto,
                    },
                );
            }
        }

        for ev in opened_events {
            self.inner.event_sink.emit_port_opened(ev);
        }
        for ev in updated_events {
            self.inner.event_sink.emit_port_updated(ev);
        }
        for ev in closed_events {
            self.inner.event_sink.emit_port_closed(ev);
        }
    }
}
