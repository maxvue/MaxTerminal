//! Testes unitários do parser `/proc/net/tcp` e `/proc/net/tcp6` e `/proc/<pid>/fd/*` (`mt-ports`).
#![allow(clippy::unwrap_used)]

use std::ffi::OsStr;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use mt_ports::{
    IpFamily, build_safe_loopback_url, format_and_sanitize_cmdline, parse_proc_net_tcp,
    parse_proc_stat_comm_and_starttime, parse_socket_inode_link,
};

#[test]
fn parses_proc_net_tcp_listen_entries_and_filters_state_and_uid() {
    let tcp4 = "\
  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1439 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 42424 1 0000000000000000 100 0 0 10 0
   1: 00000000:1F40 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 53535 1 0000000000000000 100 0 0 10 0
   2: 0100007F:1439 0100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 64646 1 0000000000000000 100 0 0 10 0
   3: 0100007F:0050 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 75757 1 0000000000000000 100 0 0 10 0
   4: 0100007F:0BB8 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 0     1 0000000000000000 100 0 0 10 0
";

    let entries = parse_proc_net_tcp(tcp4, IpFamily::Ipv4, Some(1000));
    assert_eq!(entries.len(), 2);

    // 0100007F:1439 -> 127.0.0.1:5177
    assert_eq!(entries[0].local_ip, IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_eq!(entries[0].local_port, 5177);
    assert_eq!(entries[0].inode, 42424);
    let addr0 = entries[0].to_port_address();
    assert!(addr0.is_loopback);
    assert!(!addr0.is_wildcard);

    // 00000000:1F40 -> 0.0.0.0:8000
    assert_eq!(entries[1].local_ip, IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    assert_eq!(entries[1].local_port, 8000);
    assert_eq!(entries[1].inode, 53535);
    let addr1 = entries[1].to_port_address();
    assert!(addr1.is_wildcard);
    assert!(!addr1.is_loopback);
}

#[test]
fn parses_proc_net_tcp6_loopback_and_wildcard() {
    let tcp6 = "\
  sl  local_address                         rem_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000001000000:1439 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 88888 1 0000000000000000 100 0 0 10 0
   1: 00000000000000000000000000000000:1F40 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 99999 1 0000000000000000 100 0 0 10 0
";

    let entries = parse_proc_net_tcp(tcp6, IpFamily::Ipv6, Some(1000));
    assert_eq!(entries.len(), 2);

    assert_eq!(entries[0].local_ip, IpAddr::V6(Ipv6Addr::LOCALHOST));
    assert_eq!(entries[0].local_port, 5177);
    assert!(entries[0].to_port_address().is_loopback);

    assert_eq!(entries[1].local_ip, IpAddr::V6(Ipv6Addr::UNSPECIFIED));
    assert_eq!(entries[1].local_port, 8000);
    assert!(entries[1].to_port_address().is_wildcard);
}

#[test]
fn parses_socket_links_stat_and_sanitizes_cmdline() {
    assert_eq!(
        parse_socket_inode_link(OsStr::new("socket:[123456]")),
        Some(123_456)
    );
    assert_eq!(
        parse_socket_inode_link(OsStr::new("[0000]:98765")),
        Some(98765)
    );
    assert_eq!(parse_socket_inode_link(OsStr::new("pipe:[111]")), None);

    let stat_line = "4321 (node (vite)) S 1000 4321 4321 0 -1 4194304 123 0 0 0 10 5 0 0 20 0 1 0 987654321 1000000 1000";
    let (comm, ticks) = parse_proc_stat_comm_and_starttime(stat_line).unwrap();
    assert_eq!(comm, "node (vite)");
    assert_eq!(ticks, 987_654_321);

    let cmdline = b"node\0server.js\0--token=Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.aaa.bbb\0";
    let preview = format_and_sanitize_cmdline(cmdline).unwrap();
    assert!(preview.contains("node server.js"));
    assert!(!preview.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));

    assert_eq!(
        build_safe_loopback_url(5177).unwrap(),
        "http://127.0.0.1:5177/"
    );
    assert!(build_safe_loopback_url(0).is_err());
}
