//! Subsistema de detecção de portas TCP em escuta (`/proc/net/tcp{,6}` + `/proc/<pid>/fd/*`),
//! correlação com árvores de processos por painel e encerramento seguro (`mt-ports` — Fase 08).

pub mod error;
pub mod fdmap;
pub mod killer;
pub mod opener;
pub mod poller;
pub mod procnet;
pub mod tree;
pub mod types;

pub use error::{PortsError, PortsResult};
pub use fdmap::{MAX_FDS_PER_PID, collect_process_socket_inodes, parse_socket_inode_link};
pub use killer::{ExactProcessSignaler, kill_port_process};
pub use opener::{PortUrlOpener, XdgPortUrlOpener, build_safe_loopback_url};
pub use poller::{
    CLOSE_HYSTERESIS_SOCKET_CYCLES, NoopPortsEventSink, OPEN_HYSTERESIS_CYCLES,
    POLL_INTERVAL_ACTIVE, POLL_INTERVAL_BACKGROUND, PortsEventSink, PortsService,
};
pub use procnet::{
    RawTcpListenEntry, TCP_STATE_LISTEN, decode_ipv4_hex, decode_ipv6_hex, parse_proc_net_tcp,
};
pub use tree::{
    ActivePaneTarget, MAX_CMDLINE_PREVIEW_CHARS, MAX_PIDS_PER_PTY, MAX_PROCESS_NAME_CHARS,
    ProcessTreeSource, ProcfsTreeSource, TrackedPaneProcess, format_and_sanitize_cmdline,
    parse_proc_stat_comm_and_starttime,
};
pub use types::{
    IpFamily, KillOutcome, ListeningPort, PollMode, PortAddress, PortClosed, PortId,
    PortKillResult, PortOpened, PortProtocol, PortSnapshot, PortUpdated,
};
