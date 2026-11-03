//! Encerramento cirúrgico de processos proprietários de portas com validação de `(pid, start_time_ticks)`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use mt_core::ids::{PaneId, PtyId};
use nix::sys::signal::Signal;

use crate::error::{PortsError, PortsResult};
use crate::types::{KillOutcome, PortKillResult};

/// Janela de espera pelo encerramento gracioso após `SIGTERM` (`1500 ms`).
pub const SIGTERM_GRACE_WINDOW: Duration = Duration::from_millis(1500);

/// Janela de espera após escalonamento para `SIGKILL` (`500 ms`).
pub const SIGKILL_WAIT_WINDOW: Duration = Duration::from_millis(500);

/// Intervalo de sondagem de morte do processo (`50 ms`).
pub const KILL_POLL_STEP: Duration = Duration::from_millis(50);

/// Trait para envio de sinais validados por identidade `(pid, start_time_ticks)` dentro do PTY.
#[async_trait]
pub trait ExactProcessSignaler: Send + Sync + 'static {
    /// Verifica se o processo `(pid, start_time_ticks)` existe e pertence ao PTY `pty_id`.
    fn verify_member(&self, pty_id: PtyId, pid: u32, start_time_ticks: u64) -> PortsResult<bool>;

    /// Envia `signal` ao processo `(pid, start_time_ticks)` pertencente ao PTY `pty_id`.
    fn send_signal(
        &self,
        pty_id: PtyId,
        pid: u32,
        start_time_ticks: u64,
        port: u16,
        signal: Signal,
    ) -> PortsResult<()>;
}

/// Executa o protocolo de encerramento seguro (`SIGTERM` -> espera 1.5s -> `SIGKILL` -> espera 500ms)
/// exclusivamente sobre o processo `(pid, start_time_ticks)` proprietário da porta.
pub async fn kill_port_process(
    signaler: &Arc<dyn ExactProcessSignaler>,
    pane_id: PaneId,
    pty_id: PtyId,
    port: u16,
    pid: u32,
    start_time_ticks: u64,
) -> PortsResult<PortKillResult> {
    let started = Instant::now();

    if !signaler.verify_member(pty_id, pid, start_time_ticks)? {
        return Ok(PortKillResult {
            pane_id,
            port,
            pid,
            outcome: KillOutcome::AlreadyExited,
            duration_ms: 0,
        });
    }

    signaler.send_signal(pty_id, pid, start_time_ticks, port, Signal::SIGTERM)?;

    let term_deadline = Instant::now() + SIGTERM_GRACE_WINDOW;
    while Instant::now() < term_deadline {
        tokio::time::sleep(KILL_POLL_STEP).await;
        if !signaler.verify_member(pty_id, pid, start_time_ticks)? {
            return Ok(PortKillResult {
                pane_id,
                port,
                pid,
                outcome: KillOutcome::GracefulTerm,
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            });
        }
    }

    // Processo ainda vivo após 1500 ms -> escalona para SIGKILL
    signaler.send_signal(pty_id, pid, start_time_ticks, port, Signal::SIGKILL)?;

    let kill_deadline = Instant::now() + SIGKILL_WAIT_WINDOW;
    while Instant::now() < kill_deadline {
        tokio::time::sleep(KILL_POLL_STEP).await;
        if !signaler.verify_member(pty_id, pid, start_time_ticks)? {
            return Ok(PortKillResult {
                pane_id,
                port,
                pid,
                outcome: KillOutcome::ForcedKill,
                duration_ms: u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX),
            });
        }
    }

    Err(PortsError::KillTimeout { port, pid })
}
