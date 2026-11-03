//! Testes de encerramento cirúrgico de processo de porta com proteção contra PID reuse (`mt-ports::killer`).
#![allow(clippy::unwrap_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use mt_core::error::MtError;
use mt_core::error::codes;
use mt_core::ids::{PaneId, PtyId};
use mt_ports::{ExactProcessSignaler, KillOutcome, PortsError, PortsResult, kill_port_process};
use nix::sys::signal::Signal;

struct MockSignaler {
    alive_checks_remaining: AtomicUsize,
    expected_ticks: u64,
    actual_ticks: u64,
    signals_sent: parking_lot::Mutex<Vec<Signal>>,
}

#[async_trait]
impl ExactProcessSignaler for MockSignaler {
    fn verify_member(&self, _pty_id: PtyId, pid: u32, start_time_ticks: u64) -> PortsResult<bool> {
        if start_time_ticks != self.actual_ticks {
            return Err(PortsError::ProcessMismatch {
                port: 5177,
                expected_pid: pid,
            });
        }
        let rem = self.alive_checks_remaining.load(Ordering::SeqCst);
        if rem == 0 {
            return Ok(false);
        }
        self.alive_checks_remaining.fetch_sub(1, Ordering::SeqCst);
        Ok(true)
    }

    fn send_signal(
        &self,
        _pty_id: PtyId,
        pid: u32,
        start_time_ticks: u64,
        port: u16,
        signal: Signal,
    ) -> PortsResult<()> {
        if start_time_ticks != self.actual_ticks || start_time_ticks != self.expected_ticks {
            return Err(PortsError::ProcessMismatch {
                port,
                expected_pid: pid,
            });
        }
        self.signals_sent.lock().push(signal);
        Ok(())
    }
}

#[tokio::test]
async fn kills_gracefully_with_sigterm_when_process_exits_quickly() {
    let signaler = Arc::new(MockSignaler {
        alive_checks_remaining: AtomicUsize::new(1),
        expected_ticks: 555,
        actual_ticks: 555,
        signals_sent: parking_lot::Mutex::new(Vec::new()),
    });
    let dyn_sig: Arc<dyn ExactProcessSignaler> = signaler.clone();

    let res = kill_port_process(&dyn_sig, PaneId::new(), PtyId::new(), 5177, 1234, 555)
        .await
        .unwrap();

    assert_eq!(res.outcome, KillOutcome::GracefulTerm);
    assert_eq!(*signaler.signals_sent.lock(), vec![Signal::SIGTERM]);
}

#[tokio::test]
async fn rejects_kill_on_pid_reuse_mismatch() {
    let signaler = Arc::new(MockSignaler {
        alive_checks_remaining: AtomicUsize::new(5),
        expected_ticks: 555,
        actual_ticks: 999, // Ticks mudaram (PID reutilizado)
        signals_sent: parking_lot::Mutex::new(Vec::new()),
    });
    let dyn_sig: Arc<dyn ExactProcessSignaler> = signaler.clone();

    let err = kill_port_process(&dyn_sig, PaneId::new(), PtyId::new(), 5177, 1234, 555)
        .await
        .unwrap_err();
    let mt_err: MtError = err.into();
    assert_eq!(mt_err.code, codes::PORTS_PROCESS_MISMATCH);
    assert!(signaler.signals_sent.lock().is_empty());
}
