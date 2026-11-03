//! Testes de histerese anti-flap, deduplicação dual-stack e eventos (`mt-ports::poller`).
#![allow(clippy::unwrap_used)]

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::Arc;

use async_trait::async_trait;
use mt_core::ids::{PaneId, PtyId, TabId};
use mt_ports::{
    ActivePaneTarget, ExactProcessSignaler, IpFamily, PortClosed, PortOpened, PortUpdated,
    PortUrlOpener, PortsEventSink, PortsResult, PortsService, ProcessTreeSource, RawTcpListenEntry,
    TrackedPaneProcess,
};
use nix::sys::signal::Signal;
use parking_lot::Mutex;

#[derive(Default)]
struct MockTreeSource {
    panes: Mutex<Vec<ActivePaneTarget>>,
    procs: Mutex<HashMap<PtyId, Vec<TrackedPaneProcess>>>,
    inodes: Mutex<HashMap<u32, HashSet<u64>>>,
    alive: Mutex<HashMap<u32, u64>>,
    listen: Mutex<Vec<RawTcpListenEntry>>,
}

impl ProcessTreeSource for MockTreeSource {
    fn active_panes(&self) -> Vec<ActivePaneTarget> {
        self.panes.lock().clone()
    }

    fn processes_for_pty(&self, pty_id: PtyId) -> Vec<TrackedPaneProcess> {
        self.procs.lock().get(&pty_id).cloned().unwrap_or_default()
    }

    fn socket_inodes_for_pid(&self, pid: u32) -> HashSet<u64> {
        self.inodes.lock().get(&pid).cloned().unwrap_or_default()
    }

    fn verify_process_alive(&self, pid: u32, expected_start_ticks: u64) -> bool {
        self.alive.lock().get(&pid).copied() == Some(expected_start_ticks)
    }

    fn read_listen_entries(&self) -> PortsResult<Vec<RawTcpListenEntry>> {
        Ok(self.listen.lock().clone())
    }
}

struct NoopSignaler;

#[async_trait]
impl ExactProcessSignaler for NoopSignaler {
    fn verify_member(
        &self,
        _pty_id: PtyId,
        _pid: u32,
        _start_time_ticks: u64,
    ) -> PortsResult<bool> {
        Ok(false)
    }

    fn send_signal(
        &self,
        _pty_id: PtyId,
        _pid: u32,
        _start_time_ticks: u64,
        _port: u16,
        _signal: Signal,
    ) -> PortsResult<()> {
        Ok(())
    }
}

struct NoopOpener;

#[async_trait]
impl PortUrlOpener for NoopOpener {
    async fn open_url(&self, _port: u16, _url: &str) -> PortsResult<()> {
        Ok(())
    }
}

#[derive(Default)]
struct CapturingPortEvents {
    opened: Mutex<Vec<PortOpened>>,
    updated: Mutex<Vec<PortUpdated>>,
    closed: Mutex<Vec<PortClosed>>,
}

impl PortsEventSink for CapturingPortEvents {
    fn emit_port_opened(&self, payload: PortOpened) {
        self.opened.lock().push(payload);
    }

    fn emit_port_updated(&self, payload: PortUpdated) {
        self.updated.lock().push(payload);
    }

    fn emit_port_closed(&self, payload: PortClosed) {
        self.closed.lock().push(payload);
    }
}

#[test]
fn enforces_two_cycle_open_hysteresis_deduplicates_dual_stack_and_closes_properly() {
    let tab_id = TabId::new();
    let pane_id = PaneId::new();
    let pty_id = PtyId::new();

    let source = Arc::new(MockTreeSource::default());
    source.panes.lock().push(ActivePaneTarget {
        tab_id,
        pane_id,
        pty_id,
    });
    source.procs.lock().insert(
        pty_id,
        vec![TrackedPaneProcess {
            pid: 5001,
            start_time_ticks: 100_000,
            process_name: "vite".to_string(),
            cmdline_preview: Some("vite --port 5177".to_string()),
            depth: 1,
        }],
    );
    source.inodes.lock().insert(5001, HashSet::from([101, 102]));
    source.alive.lock().insert(5001, 100_000);
    source.listen.lock().extend([
        RawTcpListenEntry {
            family: IpFamily::Ipv4,
            local_ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
            local_port: 5177,
            inode: 101,
            uid: 1000,
        },
        RawTcpListenEntry {
            family: IpFamily::Ipv6,
            local_ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
            local_port: 5177,
            inode: 102,
            uid: 1000,
        },
    ]);

    let sink = Arc::new(CapturingPortEvents::default());
    let service = PortsService::new(
        source.clone(),
        Arc::new(NoopSignaler),
        Arc::new(NoopOpener),
        sink.clone(),
    );

    // Ciclo 1: ainda em histerese (consecutive_seen == 1), 0 eventos emitidos
    service.tick_once().unwrap();
    assert!(sink.opened.lock().is_empty());
    assert_eq!(service.list_for_tab(tab_id).ports.len(), 0);

    // Ciclo 2: confirmado! Deve emitir exatamente 1 PortOpened com 2 endereços (IPv4 + IPv6)
    service.tick_once().unwrap();
    {
        let opened = sink.opened.lock();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened[0].port.port, 5177);
        assert_eq!(opened[0].port.addresses.len(), 2);
        assert_eq!(opened[0].port.browser_url, "http://127.0.0.1:5177/");
    }

    // Se o socket sumir por 1 ciclo mas o PID continuar vivo, não fecha no 1º ciclo
    source.listen.lock().clear();
    service.tick_once().unwrap();
    assert!(sink.closed.lock().is_empty());

    // No 2º ciclo sem socket, fecha e emite PortClosed
    service.tick_once().unwrap();
    assert_eq!(sink.closed.lock().len(), 1);
    assert_eq!(service.list_for_tab(tab_id).ports.len(), 0);
}
