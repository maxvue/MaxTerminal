//! Supervisor periódico de mudanças de foreground, tombstones, leases de ACK e órfãos adotados.

use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{RecvTimeoutError, Sender};

use crate::types::ProcessInfo;

/// Operações periódicas invocadas pela thread `mt-pty-supervisor`.
pub trait SupervisorHost: Send + Sync + 'static {
    /// Inspeciona o foreground dos PTYs em execução (`Running`) e emite `PtyEvent::ForegroundChanged`
    /// quando o par `(pgid, start_time)` muda.
    fn poll_foreground_changes(&self);

    /// Executa `waitpid(pid, WNOHANG)` nos processos órfãos adotados pelo subreaper que não são
    /// filhos diretos monitorados por um waiter de PTY.
    fn reap_adopted_orphans(&self);

    /// Expira tombstones cujo `Exited.at_ms` ultrapassou `tombstone_ttl` (nunca expira `CleanupFailed`).
    fn expire_tombstones(&self);

    /// Verifica timeouts de `ack_timeout` nas conexões pausadas/ativas e a saúde do watchdog.
    fn check_leases_and_watchdog(&self);
}

/// Rastreador de deduplicação de `PtyEvent::ForegroundChanged` por `(pgid, start_time)` (CA-28).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ForegroundTracker {
    last_seen: Option<(u32, u64)>,
}

impl ForegroundTracker {
    /// Cria um novo rastreador sem estado inicial observado.
    #[must_use]
    pub const fn new() -> Self {
        Self { last_seen: None }
    }

    /// Retorna o último par `(pgid, start_time)` observado.
    #[must_use]
    pub const fn last_seen(&self) -> Option<(u32, u64)> {
        self.last_seen
    }

    /// Registra a observação atual de foreground e retorna `true` se e somente se o par
    /// `(pgid, start_time)` mudou em relação à observação anterior (CA-28).
    pub fn observe(&mut self, pgid: Option<u32>, process: Option<&ProcessInfo>) -> bool {
        let current = pgid.map(|g| {
            let start_time = process.map_or(0, |p| p.start_time.0);
            (g, start_time)
        });

        if current == self.last_seen {
            false
        } else {
            self.last_seen = current;
            true
        }
    }
}

/// Handle de controle da thread periódica `mt-pty-supervisor`.
#[derive(Debug)]
pub struct SupervisorHandle {
    stop_tx: Sender<()>,
    join_handle: Option<JoinHandle<()>>,
}

impl SupervisorHandle {
    /// Inicia a thread `mt-pty-supervisor` com o intervalo e tamanho de pilha especificados.
    pub fn spawn(
        interval: Duration,
        stack_size: usize,
        host: Arc<dyn SupervisorHost>,
    ) -> std::io::Result<Self> {
        let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);

        let join_handle = thread::Builder::new()
            .name("mt-pty-supervisor".to_string())
            .stack_size(stack_size)
            .spawn(move || {
                loop {
                    match stop_rx.recv_timeout(interval) {
                        Ok(()) | Err(RecvTimeoutError::Disconnected) => break,
                        Err(RecvTimeoutError::Timeout) => {
                            host.poll_foreground_changes();
                            host.reap_adopted_orphans();
                            host.expire_tombstones();
                            host.check_leases_and_watchdog();
                        }
                    }
                }
            })?;

        Ok(Self {
            stop_tx,
            join_handle: Some(join_handle),
        })
    }

    /// Sinaliza parada imediata e aguarda o encerramento da thread `mt-pty-supervisor`.
    pub fn shutdown(&mut self) {
        let _ = self.stop_tx.try_send(());
        if let Some(handle) = self.join_handle.take()
            && handle.thread().id() != thread::current().id()
        {
            let _ = handle.join();
        }
    }
}

impl Drop for SupervisorHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::types::{ProcState, U64Decimal};

    fn sample_fg_process(proc_pid: u32, group_id: u32, start_time: u64) -> ProcessInfo {
        ProcessInfo {
            pid: proc_pid,
            ppid: 1,
            pgid: group_id,
            sid: 1,
            tpgid: i32::try_from(group_id).unwrap(),
            comm: "sleep".to_string(),
            cmdline: vec!["sleep".to_string(), "30".to_string()],
            exe: None,
            cwd: None,
            state: ProcState::Sleeping,
            start_time: U64Decimal(start_time),
            rss_bytes: None,
            threads: 1,
        }
    }

    #[test]
    fn foreground_tracker_deduplicates_identical_pgid_and_start_time_ca28() {
        let mut tracker = ForegroundTracker::new();
        let proc1 = sample_fg_process(200, 200, 1000);
        let proc1_same = sample_fg_process(200, 200, 1000);
        let proc2_recycled = sample_fg_process(200, 200, 2000);

        // Primeira mudança -> true
        assert!(tracker.observe(Some(200), Some(&proc1)));
        // Mesma observação -> false (deduplicado)
        assert!(!tracker.observe(Some(200), Some(&proc1_same)));
        // Mesmo PGID mas start_time mudou (PID reciclado) -> true
        assert!(tracker.observe(Some(200), Some(&proc2_recycled)));
        assert!(!tracker.observe(Some(200), Some(&proc2_recycled)));
        // Voltou para None -> true
        assert!(tracker.observe(None, None));
        assert!(!tracker.observe(None, None));
    }

    #[derive(Default)]
    struct CountingHost {
        fg_polls: AtomicUsize,
        orphan_reaps: AtomicUsize,
        tombstone_expires: AtomicUsize,
        lease_checks: AtomicUsize,
    }

    impl SupervisorHost for CountingHost {
        fn poll_foreground_changes(&self) {
            self.fg_polls.fetch_add(1, Ordering::SeqCst);
        }

        fn reap_adopted_orphans(&self) {
            self.orphan_reaps.fetch_add(1, Ordering::SeqCst);
        }

        fn expire_tombstones(&self) {
            self.tombstone_expires.fetch_add(1, Ordering::SeqCst);
        }

        fn check_leases_and_watchdog(&self) {
            self.lease_checks.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn supervisor_worker_invokes_all_host_callbacks_and_shuts_down_cleanly() {
        let host = Arc::new(CountingHost::default());
        let mut handle =
            SupervisorHandle::spawn(Duration::from_millis(15), 256 * 1024, host.clone()).unwrap();

        thread::sleep(Duration::from_millis(60));
        handle.shutdown();

        assert!(host.fg_polls.load(Ordering::SeqCst) >= 1);
        assert!(host.orphan_reaps.load(Ordering::SeqCst) >= 1);
        assert!(host.tombstone_expires.load(Ordering::SeqCst) >= 1);
        assert!(host.lease_checks.load(Ordering::SeqCst) >= 1);
    }
}
