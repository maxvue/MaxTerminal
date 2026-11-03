//! Suíte de testes de integração de supervisão via `/proc`, árvore de processos,
//! detecção de mudança de foreground (`tcgetpgrp`) e sinalização validada por `pidfd`
//! e cgroup (Critérios CA-25, CA-26, CA-27, CA-28).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::assert_is_empty)]

use std::time::{Duration, Instant};

use mt_pty::proc::current_boot_id;
use mt_pty::{KillMode, ProcessIdentity, PtyConfig, PtyEvent, PtyManager, SpawnSpec, U64Decimal};
use nix::sys::signal::Signal;
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> PtyConfig {
    let instance_id = uuid::Uuid::now_v7().to_string();
    PtyConfig {
        registry_dir: tmp.path().join(format!("rt-{instance_id}")),
        orphan_sweep_on_startup: false,
        supervisor_interval: Duration::from_millis(35),
        ..PtyConfig::default()
    }
}

#[test]
fn inspects_process_tree_pids_and_foreground_report() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), "sleep 30".to_string()],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();

    // Aguarda o processo filho (ou shell) estar populado no relatório
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut report = manager.process_report(summary.id).unwrap();
    while report.owned_processes.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
        report = manager.process_report(summary.id).unwrap();
    }

    assert_eq!(report.pty_id, summary.id);
    assert_eq!(report.pids.shell_pid, summary.pids.shell_pid);
    assert!(report.shell.is_some());
    assert!(report.tree.is_some());
    assert!(!report.owned_processes.is_empty());

    let tracked = manager.tracked_processes(summary.id).unwrap();
    assert!(!tracked.is_empty());
    for identity in &tracked {
        assert_eq!(manager.owner_of_process(identity), Some(summary.id));
    }

    let _ = manager.kill(summary.id, KillMode::Force);
    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn emits_foreground_changed_event_when_foreground_group_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.supervisor_interval = Duration::from_millis(30);
    let manager = PtyManager::new(config).unwrap();
    let events = manager.subscribe();

    // Inicia shell interativo com controle de jobs habilitado (-i -m)
    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-i".to_string(), "-m".to_string()],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    std::thread::sleep(Duration::from_millis(80));

    // Executa comando em foreground para criar um novo process group líder no terminal
    manager.write(summary.id, b"sleep 10\n").unwrap();

    let deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_fg_change = false;
    while Instant::now() < deadline {
        if let Ok(PtyEvent::ForegroundChanged { pty_id, pgid, .. }) =
            events.recv_timeout(Duration::from_millis(50))
            && pty_id == summary.id
            && pgid.is_some()
        {
            saw_fg_change = true;
            break;
        }
    }

    assert!(
        saw_fg_change,
        "Deveria ter emitido PtyEvent::ForegroundChanged ao iniciar subprocesso em foreground"
    );

    let _ = manager.kill(summary.id, KillMode::Force);
    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn signal_process_validates_identity_and_cgroup_ownership() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), "sleep 30".to_string()],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    std::thread::sleep(Duration::from_millis(80));

    let tracked = manager.tracked_processes(summary.id).unwrap();
    assert!(!tracked.is_empty());
    let valid_identity = tracked[0].clone();

    // 1. Rejeita envio de sinal para PID externo ao cgroup do PTY (ex.: o próprio processo de teste)
    let external_identity = ProcessIdentity {
        boot_id: current_boot_id().to_string(),
        pid: std::process::id(),
        start_time: U64Decimal(1),
    };
    let err_ext = manager
        .signal_process(summary.id, &external_identity, Signal::SIGCONT)
        .unwrap_err();
    assert_eq!(err_ext.code(), "pty.pid_not_owned");

    // 2. Rejeita envio de sinal quando start_time diverge (proteção contra reciclagem de PID — CA-28)
    let stale_identity = ProcessIdentity {
        boot_id: valid_identity.boot_id.clone(),
        pid: valid_identity.pid,
        start_time: U64Decimal(valid_identity.start_time.0.saturating_add(999_999)),
    };
    let err_stale = manager
        .signal_process(summary.id, &stale_identity, Signal::SIGCONT)
        .unwrap_err();
    assert_eq!(err_stale.code(), "pty.pid_not_owned");

    // 3. Permite envio de sinal com a identidade legítima validada por pidfd + cgroup
    manager
        .signal_process(summary.id, &valid_identity, Signal::SIGTERM)
        .unwrap();

    let _ = manager.kill(summary.id, KillMode::Force);
    let _ = manager.shutdown(Duration::from_secs(2));
}
