//! Adaptador `PtyExactProcessSignaler` que valida `(pid, start_time_ticks)` via `PtyManager`
//! antes de enviar sinais `SIGTERM` / `SIGKILL` a processos de portas.

use std::sync::Arc;

use async_trait::async_trait;
use mt_core::ids::PtyId;
use mt_ports::{ExactProcessSignaler, PortsError, PortsResult};
use mt_pty::{PtyError, PtyManager};
use nix::sys::signal::Signal;

/// Implementação de [`ExactProcessSignaler`] delegando ao [`PtyManager`].
pub struct PtyExactProcessSignaler {
    pty_manager: Arc<PtyManager>,
}

impl std::fmt::Debug for PtyExactProcessSignaler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtyExactProcessSignaler")
            .finish_non_exhaustive()
    }
}

impl PtyExactProcessSignaler {
    /// Cria uma nova instância conectada ao [`PtyManager`].
    #[must_use]
    pub fn new(pty_manager: Arc<PtyManager>) -> Arc<Self> {
        Arc::new(Self { pty_manager })
    }
}

#[async_trait]
impl ExactProcessSignaler for PtyExactProcessSignaler {
    fn verify_member(&self, pty_id: PtyId, pid: u32, start_time_ticks: u64) -> PortsResult<bool> {
        let Ok(members) = self.pty_manager.tracked_processes(pty_id) else {
            return Ok(false);
        };
        for member in members {
            if member.pid == pid {
                if member.start_time.0 != start_time_ticks {
                    return Err(PortsError::ProcessMismatch {
                        port: 0,
                        expected_pid: pid,
                    });
                }
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn send_signal(
        &self,
        pty_id: PtyId,
        pid: u32,
        start_time_ticks: u64,
        port: u16,
        signal: Signal,
    ) -> PortsResult<()> {
        let members = self.pty_manager.tracked_processes(pty_id).map_err(|_| {
            PortsError::ProcessMismatch {
                port,
                expected_pid: pid,
            }
        })?;

        let Some(target_identity) = members.into_iter().find(|m| m.pid == pid) else {
            return Ok(());
        };

        if target_identity.start_time.0 != start_time_ticks {
            return Err(PortsError::ProcessMismatch {
                port,
                expected_pid: pid,
            });
        }

        match self
            .pty_manager
            .signal_process(pty_id, &target_identity, signal)
        {
            Ok(()) | Err(PtyError::AlreadyExited) => Ok(()),
            Err(PtyError::PidNotOwned(_)) => Err(PortsError::ProcessMismatch {
                port,
                expected_pid: pid,
            }),
            Err(PtyError::Io(io_err)) if io_err.kind() == std::io::ErrorKind::PermissionDenied => {
                Err(PortsError::KillDenied { port, pid })
            }
            Err(_) => Err(PortsError::KillDenied { port, pid }),
        }
    }
}
