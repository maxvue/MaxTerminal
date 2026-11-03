//! Testes de integração de descoberta atômica de endpoints (`discovery_it.rs`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt;

use mt_bridge::{
    BridgeErrorCode, DiscoveryEndpoint, MAXTERMINAL_ENDPOINT_FILE, PROTOCOL_VERSION, PeerRole,
    default_maxterminal_capabilities, generate_token_hex, read_verified, remove_if_owned,
    resolve_discovery_dir, write_atomic,
};
use tempfile::TempDir;

fn sample_endpoint(pid: u32, host: &str) -> DiscoveryEndpoint {
    DiscoveryEndpoint {
        protocol_version: PROTOCOL_VERSION,
        role: PeerRole::MaxTerminal,
        instance_id: "inst-test-1".to_string(),
        pid,
        host: host.to_string(),
        port: 45123,
        ws_path: "/v1/ws".to_string(),
        http_rpc_path: "/v1/rpc".to_string(),
        token: generate_token_hex(),
        started_at_ms: 1_730_000_000_000,
        capabilities: default_maxterminal_capabilities(),
    }
}

#[test]
fn atomic_write_creates_0700_dir_and_0600_endpoint_file() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = resolve_discovery_dir(Some(&tmp.path().join("max-bridge"))).expect("resolve dir");
    let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700);

    let file_path = dir.join(MAXTERMINAL_ENDPOINT_FILE);
    let ep = sample_endpoint(std::process::id(), "127.0.0.1");
    write_atomic(&file_path, &ep).expect("write_atomic");

    let file_mode = fs::metadata(&file_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600);

    let loaded = read_verified(&file_path).expect("read_verified");
    assert_eq!(loaded, ep);
}

#[test]
fn rejects_insecure_0644_permissions() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = resolve_discovery_dir(Some(&tmp.path().join("max-bridge"))).unwrap();
    let file_path = dir.join(MAXTERMINAL_ENDPOINT_FILE);
    let ep = sample_endpoint(std::process::id(), "127.0.0.1");
    write_atomic(&file_path, &ep).unwrap();

    let mut perms = fs::metadata(&file_path).unwrap().permissions();
    perms.set_mode(0o644);
    fs::set_permissions(&file_path, perms).unwrap();

    let err = read_verified(&file_path).expect_err("deve rejeitar 0644");
    assert_eq!(err.code, BridgeErrorCode::DiscoveryInsecurePermissions);
}

#[test]
fn rejects_and_cleans_dead_pid() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = resolve_discovery_dir(Some(&tmp.path().join("max-bridge"))).unwrap();
    let file_path = dir.join(MAXTERMINAL_ENDPOINT_FILE);
    // PID inexistente (i32::MAX - 1)
    let ep = sample_endpoint(2_147_483_640, "127.0.0.1");
    write_atomic(&file_path, &ep).unwrap();

    let err = read_verified(&file_path).expect_err("deve rejeitar PID morto");
    assert_eq!(err.code, BridgeErrorCode::DiscoveryNotFound);
    assert!(!file_path.exists(), "arquivo órfão deve ser removido");
}

#[test]
fn rejects_non_loopback_host() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = resolve_discovery_dir(Some(&tmp.path().join("max-bridge"))).unwrap();
    let file_path = dir.join(MAXTERMINAL_ENDPOINT_FILE);
    let ep = sample_endpoint(std::process::id(), "0.0.0.0");

    let err = write_atomic(&file_path, &ep).expect_err("deve rejeitar 0.0.0.0");
    assert_eq!(err.code, BridgeErrorCode::InvalidEnvelope);
}

#[test]
fn remove_if_owned_only_removes_matching_instance() {
    let tmp = TempDir::new().expect("tempdir");
    let dir = resolve_discovery_dir(Some(&tmp.path().join("max-bridge"))).unwrap();
    let file_path = dir.join(MAXTERMINAL_ENDPOINT_FILE);
    let ep = sample_endpoint(std::process::id(), "127.0.0.1");
    write_atomic(&file_path, &ep).unwrap();

    remove_if_owned(&file_path, "outra-instancia");
    assert!(file_path.exists());

    remove_if_owned(&file_path, "inst-test-1");
    assert!(!file_path.exists());
}
