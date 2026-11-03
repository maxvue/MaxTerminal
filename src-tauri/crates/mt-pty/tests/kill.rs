//! Suíte de testes de integração da máquina de estados de encerramento (`kill.rs`):
//! encerramento gracioso rápido (`< 500 ms`), escalada `SIGHUP -> SIGTERM -> SIGKILL` (`<= 3.5 s`)
//! e promoção imediata para `KillMode::Force` (Critérios CA-16, CA-17).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use mt_pty::{ExitReason, KillMode, PtyConfig, PtyManager, SpawnSpec};
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> PtyConfig {
    let instance_id = uuid::Uuid::now_v7().to_string();
    PtyConfig {
        registry_dir: tmp.path().join(format!("rt-{instance_id}")),
        orphan_sweep_on_startup: false,
        supervisor_interval: Duration::from_millis(40),
        ..PtyConfig::default()
    }
}

#[test]
fn graceful_kill_terminates_idle_shell_under_500ms_with_killed_by_user() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: Vec::new(),
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    std::thread::sleep(Duration::from_millis(50));

    let start = Instant::now();
    let exit = manager.kill(summary.id, KillMode::Graceful).unwrap();
    let elapsed = start.elapsed();

    assert!(
        elapsed < Duration::from_millis(500),
        "Kill gracioso de shell ocioso levou {elapsed:?} (esperado < 500ms)"
    );
    assert_eq!(exit.reason, ExitReason::KilledByUser);

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn escalates_sighup_to_sigterm_and_sigkill_within_deadline() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.hup_grace = Duration::from_millis(200);
    config.term_grace = Duration::from_millis(300);
    let manager = PtyManager::new(config).unwrap();

    // 1. Processo que ignora SIGHUP mas encerra com SIGTERM
    let spec_ignore_hup = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec![
            "-c".to_string(),
            "trap '' HUP; while :; do sleep 0.1; done".to_string(),
        ],
        login: false,
        ..SpawnSpec::default()
    };
    let s1 = manager.spawn(spec_ignore_hup).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let t1 = Instant::now();
    let exit1 = manager.kill(s1.id, KillMode::Graceful).unwrap();
    let d1 = t1.elapsed();
    assert_eq!(exit1.reason, ExitReason::KilledByUser);
    assert!(
        d1 >= Duration::from_millis(180) && d1 < Duration::from_millis(1500),
        "Esperado escalar após hup_grace (200ms), levou {d1:?}"
    );

    // 2. Processo que ignora SIGHUP e SIGTERM -> morre via cgroup.kill (SIGKILL) em <= 3.5s
    let spec_ignore_both = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec![
            "-c".to_string(),
            "trap '' HUP TERM; while :; do sleep 0.1; done".to_string(),
        ],
        login: false,
        ..SpawnSpec::default()
    };
    let s2 = manager.spawn(spec_ignore_both).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let t2 = Instant::now();
    let exit2 = manager.kill(s2.id, KillMode::Graceful).unwrap();
    let d2 = t2.elapsed();
    assert_eq!(exit2.reason, ExitReason::KilledByUser);
    assert!(
        d2 >= Duration::from_millis(450) && d2 <= Duration::from_millis(3500),
        "Esperado escalar até SIGKILL após hup+term grace, levou {d2:?}"
    );

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn promotes_in_flight_graceful_kill_to_force_immediately() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.hup_grace = Duration::from_secs(5);
    config.term_grace = Duration::from_secs(5);
    let manager = PtyManager::new(config).unwrap();

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec![
            "-c".to_string(),
            "trap '' HUP TERM; while :; do sleep 0.1; done".to_string(),
        ],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    std::thread::sleep(Duration::from_millis(100));

    let start = Instant::now();
    let mgr_clone = Arc::clone(&manager);
    let pty_id = summary.id;

    let graceful_thread = std::thread::spawn(move || mgr_clone.kill(pty_id, KillMode::Graceful));

    std::thread::sleep(Duration::from_millis(100));
    let force_exit = manager.kill(pty_id, KillMode::Force).unwrap();
    let graceful_exit = graceful_thread.join().unwrap().unwrap();

    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_millis(1500),
        "Promoção para Force deveria abortar esperas de 10s e concluir rapidamente, levou {elapsed:?}"
    );
    assert_eq!(force_exit.reason, ExitReason::KilledByUser);
    assert_eq!(graceful_exit.reason, ExitReason::KilledByUser);

    let _ = manager.shutdown(Duration::from_secs(2));
}
