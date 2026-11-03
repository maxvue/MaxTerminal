//! Máquina de estados de encerramento de PTYs (`Graceful` e `Force`) e limpeza de cgroups.
//!
//! Implementa o escalonamento determinístico:
//! `SIGHUP (+SIGCONT)` → `hup_grace` → `SIGTERM (+SIGCONT a cada 50 ms)` → `term_grace` → `cgroup.kill` (`SIGKILL`).

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use nix::sys::signal::Signal;
use parking_lot::{Condvar, Mutex};

use crate::config::PtyConfig;
use crate::containment::ContainmentHandle;
use crate::error::PtyResult;
use crate::proc::{ProcSource, Signaler};
use crate::types::{ExitReason, KillMode, ProcessIdentity};

/// Plano parametrizado de encerramento de um PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KillPlan {
    /// Modo inicial solicitado (`Graceful` ou `Force`).
    pub mode: KillMode,
    /// Janela de espera após `SIGHUP` + `SIGCONT`.
    pub hup_grace: Duration,
    /// Janela de espera após `SIGTERM` + `SIGCONT`.
    pub term_grace: Duration,
    /// Janela máxima para esvaziamento do cgroup após `cgroup.kill`.
    pub force_wait: Duration,
    /// Intervalo de re-varredura de membros do cgroup durante `SIGTERM` (padrão: 50 ms).
    pub poll_interval: Duration,
}

impl KillPlan {
    /// Constrói um [`KillPlan`] a partir da configuração global e do modo solicitado.
    #[must_use]
    pub const fn from_config(config: &PtyConfig, mode: KillMode) -> Self {
        Self {
            mode,
            hup_grace: config.hup_grace,
            term_grace: config.term_grace,
            force_wait: Duration::from_secs(1),
            poll_interval: Duration::from_millis(50),
        }
    }

    /// Constrói um [`KillPlan`] abreviado para o desligamento global do aplicativo (`shutdown`).
    #[must_use]
    pub const fn for_shutdown(config: &PtyConfig) -> Self {
        Self {
            mode: KillMode::Graceful,
            hup_grace: config.shutdown_hup_grace,
            term_grace: config.shutdown_term_grace,
            force_wait: Duration::from_secs(1),
            poll_interval: Duration::from_millis(50),
        }
    }
}

/// Controle concorrente que permite promover uma operação `Graceful` em andamento para `Force`.
#[derive(Debug, Default)]
pub struct KillControl {
    force_requested: AtomicBool,
    mutex: Mutex<bool>,
    condvar: Condvar,
}

impl KillControl {
    /// Cria um novo controle de kill sem promoção ativa.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Solicita promoção imediata para `KillMode::Force`, acordando qualquer espera de graça.
    pub fn request_force(&self) {
        self.force_requested.store(true, Ordering::SeqCst);
        let mut guard = self.mutex.lock();
        *guard = true;
        self.condvar.notify_all();
    }

    /// Indica se a promoção para `Force` foi solicitada.
    #[must_use]
    pub fn is_force_requested(&self) -> bool {
        self.force_requested.load(Ordering::SeqCst)
    }

    /// Aguarda até `duration` ou até que `request_force()` seja invocado.
    pub fn wait_or_force(&self, duration: Duration) -> bool {
        if self.is_force_requested() {
            return true;
        }
        let mut guard = self.mutex.lock();
        if *guard {
            return true;
        }
        let _ = self.condvar.wait_for(&mut guard, duration);
        *guard || self.is_force_requested()
    }
}

/// Resultado final da execução da máquina de estados de encerramento.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KillOutcome {
    /// Razão semântica de saída (`KilledByUser` ou `Exited`).
    pub exit_reason: ExitReason,
    /// Indica se foi necessário escalar até o estágio `Force` (`cgroup.kill` / `SIGKILL`).
    pub escalated_to_force: bool,
    /// Indica se o cgroup ficou completamente vazio (`populated == 0`).
    pub cgroup_empty: bool,
    /// Lista de PIDs sobreviventes caso o cgroup não tenha esvaziado no prazo.
    pub survivors: Vec<u32>,
}

/// Executor da máquina de estados de encerramento e limpeza de cgroup.
#[derive(Clone)]
pub struct KillCoordinator {
    proc_source: Arc<dyn ProcSource>,
    signaler: Arc<dyn Signaler>,
}

impl std::fmt::Debug for KillCoordinator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KillCoordinator")
            .field("boot_id", &self.proc_source.boot_id())
            .finish_non_exhaustive()
    }
}

impl KillCoordinator {
    /// Cria um novo coordenador de encerramento.
    #[must_use]
    pub fn new(proc_source: Arc<dyn ProcSource>, signaler: Arc<dyn Signaler>) -> Self {
        Self {
            proc_source,
            signaler,
        }
    }

    /// Executa o plano de encerramento sobre o cgroup e o processo líder do PTY.
    ///
    /// # Errors
    /// Retorna erro caso a leitura crítica do cgroup falhe. Erros individuais de processos que
    /// já saíram (`ESRCH`) são absorvidos durante a varredura.
    pub fn execute(
        &self,
        leader: &ProcessIdentity,
        containment: &dyn ContainmentHandle,
        plan: &KillPlan,
        control: &KillControl,
    ) -> PtyResult<KillOutcome> {
        if plan.mode == KillMode::Force || control.is_force_requested() {
            return self.execute_force_stage(leader, containment, plan.force_wait);
        }

        // Estágio 1: SIGHUP + SIGCONT em todos os membros atuais do cgroup (e no líder).
        self.signal_group_stage(leader, containment, &[Signal::SIGHUP, Signal::SIGCONT])?;

        if Self::wait_until_empty_or_force(
            containment,
            plan.hup_grace,
            plan.poll_interval,
            control,
        )? {
            let _ = containment.release();
            return Ok(KillOutcome {
                exit_reason: ExitReason::KilledByUser,
                escalated_to_force: false,
                cgroup_empty: true,
                survivors: Vec::new(),
            });
        }

        if control.is_force_requested() {
            return self.execute_force_stage(leader, containment, plan.force_wait);
        }

        // Estágio 2: SIGTERM + SIGCONT renovando a lista de membros a cada `poll_interval` (50 ms).
        let term_deadline = Instant::now() + plan.term_grace;
        loop {
            self.signal_group_stage(leader, containment, &[Signal::SIGTERM, Signal::SIGCONT])?;

            if containment.is_empty()? {
                let _ = containment.release();
                return Ok(KillOutcome {
                    exit_reason: ExitReason::KilledByUser,
                    escalated_to_force: false,
                    cgroup_empty: true,
                    survivors: Vec::new(),
                });
            }

            let now = Instant::now();
            if now >= term_deadline || control.is_force_requested() {
                break;
            }

            let step = term_deadline
                .saturating_duration_since(now)
                .min(plan.poll_interval.max(Duration::from_millis(5)));
            if control.wait_or_force(step) {
                break;
            }

            if containment.is_empty()? {
                let _ = containment.release();
                return Ok(KillOutcome {
                    exit_reason: ExitReason::KilledByUser,
                    escalated_to_force: false,
                    cgroup_empty: true,
                    survivors: Vec::new(),
                });
            }
        }

        // Estágio 3: Escalonamento para Force (cgroup.kill + SIGKILL).
        self.execute_force_stage(leader, containment, plan.force_wait)
    }

    /// Limpa processos descendentes remanescentes após o encerramento natural do shell líder (CA-12).
    ///
    /// # Errors
    /// Retorna erro se a consulta de estado do cgroup falhar.
    pub fn cleanup_after_leader_exit(
        &self,
        leader: &ProcessIdentity,
        containment: &dyn ContainmentHandle,
        config: &PtyConfig,
    ) -> PtyResult<KillOutcome> {
        if containment.is_empty()? {
            let _ = containment.release();
            return Ok(KillOutcome {
                exit_reason: ExitReason::Exited,
                escalated_to_force: false,
                cgroup_empty: true,
                survivors: Vec::new(),
            });
        }

        let plan = KillPlan::from_config(config, KillMode::Graceful);
        let control = KillControl::new();
        self.execute(leader, containment, &plan, &control)
    }

    fn execute_force_stage(
        &self,
        leader: &ProcessIdentity,
        containment: &dyn ContainmentHandle,
        force_wait: Duration,
    ) -> PtyResult<KillOutcome> {
        let _ = self.signal_group_stage(leader, containment, &[Signal::SIGKILL]);
        let _ = containment.kill_all();

        let empty = containment.wait_empty(force_wait)?;
        if empty {
            let _ = containment.release();
            Ok(KillOutcome {
                exit_reason: ExitReason::KilledByUser,
                escalated_to_force: true,
                cgroup_empty: true,
                survivors: Vec::new(),
            })
        } else {
            let survivors = containment.members().unwrap_or_default();
            Ok(KillOutcome {
                exit_reason: ExitReason::KilledByUser,
                escalated_to_force: true,
                cgroup_empty: false,
                survivors,
            })
        }
    }

    fn signal_group_stage(
        &self,
        leader: &ProcessIdentity,
        containment: &dyn ContainmentHandle,
        signals: &[Signal],
    ) -> PtyResult<()> {
        let cgroup_id = containment.cgroup_id();
        let mut target_pids = Vec::new();
        let mut seen = HashSet::new();

        if seen.insert(leader.pid) {
            target_pids.push(leader.pid);
        }
        if let Ok(members) = containment.members() {
            for pid in members {
                if seen.insert(pid) {
                    target_pids.push(pid);
                }
            }
        }

        for pid in target_pids {
            let identity = if pid == leader.pid {
                match self.proc_source.identity(pid)? {
                    Some(current) if current == *leader => Some(current),
                    _ => None,
                }
            } else {
                self.proc_source.identity(pid)?
            };

            if let Some(id) = identity {
                for &sig in signals {
                    let _ = self.signaler.signal_in_cgroup(&id, cgroup_id, sig);
                }
            }
        }

        Ok(())
    }

    fn wait_until_empty_or_force(
        containment: &dyn ContainmentHandle,
        grace: Duration,
        poll_interval: Duration,
        control: &KillControl,
    ) -> PtyResult<bool> {
        if containment.is_empty()? {
            return Ok(true);
        }
        let deadline = Instant::now() + grace;
        let step_max = poll_interval
            .min(Duration::from_millis(20))
            .max(Duration::from_millis(2));
        while Instant::now() < deadline {
            if control.is_force_requested() {
                return Ok(false);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let wait_slice = remaining.min(step_max);
            if control.wait_or_force(wait_slice) {
                return Ok(false);
            }
            if containment.is_empty()? {
                return Ok(true);
            }
        }
        containment.is_empty()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::containment::FakeContainmentHandle;
    use crate::proc::{FakeProcSource, RecordingSignaler};
    use crate::types::{ProcState, ProcessInfo, U64Decimal};

    fn sample_proc(pid: u32, start_time: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            ppid: 1,
            pgid: pid,
            sid: pid,
            tpgid: i32::try_from(pid).unwrap(),
            comm: "bash".to_string(),
            cmdline: vec!["bash".to_string()],
            exe: None,
            cwd: None,
            state: ProcState::Sleeping,
            start_time: U64Decimal(start_time),
            rss_bytes: None,
            threads: 1,
        }
    }

    #[test]
    fn graceful_escalation_sighup_sigterm_sigkill_order() {
        let proc_source = Arc::new(FakeProcSource::new("boot-test"));
        proc_source.insert(sample_proc(500, 1111));

        let signaler = Arc::new(RecordingSignaler::with_proc_source(proc_source.clone()));
        let handle = FakeContainmentHandle::new("/sys/fs/cgroup/fake-pty-1".into(), 42);
        handle.set_synthetic_pids(true);
        handle.enroll_pid(500).unwrap();
        signaler.set_cgroup(500, 42);

        let coordinator = KillCoordinator::new(proc_source, signaler.clone());
        let leader = ProcessIdentity {
            boot_id: "boot-test".to_string(),
            pid: 500,
            start_time: U64Decimal(1111),
        };
        let plan = KillPlan {
            mode: KillMode::Graceful,
            hup_grace: Duration::from_millis(15),
            term_grace: Duration::from_millis(25),
            force_wait: Duration::from_millis(20),
            poll_interval: Duration::from_millis(15),
        };
        let control = KillControl::new();

        let outcome = coordinator
            .execute(&leader, &handle, &plan, &control)
            .unwrap();

        assert_eq!(outcome.exit_reason, ExitReason::KilledByUser);
        assert!(outcome.escalated_to_force);
        assert!(outcome.cgroup_empty);

        let signals: Vec<Signal> = signaler
            .sent_signals()
            .into_iter()
            .map(|(_, s)| s)
            .collect();
        let first_hup = signals.iter().position(|&s| s == Signal::SIGHUP).unwrap();
        let first_term = signals.iter().position(|&s| s == Signal::SIGTERM).unwrap();
        let first_kill = signals.iter().position(|&s| s == Signal::SIGKILL).unwrap();

        assert!(first_hup < first_term);
        assert!(first_term < first_kill);
    }

    #[test]
    fn recycled_pid_is_never_signaled() {
        let proc_source = Arc::new(FakeProcSource::new("boot-test"));
        proc_source.insert(sample_proc(500, 9999));

        let signaler = Arc::new(RecordingSignaler::with_proc_source(proc_source.clone()));
        let handle = FakeContainmentHandle::new("/sys/fs/cgroup/fake-pty-2".into(), 77);
        signaler.set_cgroup(500, 77);

        let coordinator = KillCoordinator::new(proc_source, signaler.clone());
        let stale_leader = ProcessIdentity {
            boot_id: "boot-test".to_string(),
            pid: 500,
            start_time: U64Decimal(1111),
        };
        let plan = KillPlan {
            mode: KillMode::Graceful,
            hup_grace: Duration::from_millis(10),
            term_grace: Duration::from_millis(10),
            force_wait: Duration::from_millis(10),
            poll_interval: Duration::from_millis(5),
        };
        let control = KillControl::new();

        let outcome = coordinator
            .execute(&stale_leader, &handle, &plan, &control)
            .unwrap();

        assert!(outcome.cgroup_empty);
        assert_eq!(signaler.sent_signals(), Vec::new());
    }
}
