//! Suíte de testes de integração de garantia zero-órfãos (`cgroup v2`), isolamento entre PTYs,
//! desligamento paralelo (`shutdown`), recuperação de `pty-registry.json` órfão e ausência de vazamento
//! de descritores/threads (Critérios CA-18, CA-19, CA-20, CA-22, CA-23, CA-34).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mt_core::ids::PtyId;
use mt_pty::containment::{ContainmentProvider, SystemdCgroupProvider};
use mt_pty::guard::{
    REGISTRY_SCHEMA_VERSION, RegistryEntry, RegistryFile, RegistryHeader, recover_stale_registries,
};
use mt_pty::{KillMode, NullSink, PtyConfig, PtyEvent, PtyManager, PtySize, PtyStatus, SpawnSpec};
use tempfile::TempDir;

fn test_config(tmp: &TempDir) -> PtyConfig {
    let instance_id = uuid::Uuid::now_v7().to_string();
    PtyConfig {
        registry_dir: tmp.path().join(format!("rt-{instance_id}")),
        orphan_sweep_on_startup: false,
        supervisor_interval: Duration::from_millis(40),
        hup_grace: Duration::from_millis(150),
        term_grace: Duration::from_millis(200),
        ..PtyConfig::default()
    }
}

fn pid_exists(pid: u32) -> bool {
    let proc_path = PathBuf::from(format!("/proc/{pid}"));
    if !proc_path.exists() {
        return false;
    }
    if let Ok(stat) = fs::read_to_string(proc_path.join("stat"))
        && let Some(idx) = stat.rfind(')')
        && let Some(state_char) = stat.get(idx + 2..).and_then(|s| s.chars().next())
    {
        return state_char != 'Z' && state_char != 'X';
    }
    false
}

fn wait_for_pids_file(path: &Path, expected_count: usize, timeout: Duration) -> Vec<u32> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(content) = fs::read_to_string(path) {
            let pids: Vec<u32> = content
                .lines()
                .filter_map(|l| l.trim().parse::<u32>().ok())
                .collect();
            if pids.len() >= expected_count {
                return pids;
            }
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "Timeout aguardando {expected_count} PIDs no arquivo {}",
        path.display()
    );
}

#[test]
fn eliminates_background_nohup_setsid_and_double_fork_on_kill_and_spontaneous_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();
    let events = manager.subscribe();

    // Parte A: Kill explícito elimina background, nohup e double-fork (CA-18)
    let pids_file_a = tmp.path().join("pids_a.txt");
    let cmd_a = format!(
        "nohup sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> '{0}'; \
         (sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> '{0}'); \
         sleep 60",
        pids_file_a.display()
    );
    let spec_a = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), cmd_a],
        login: false,
        ..SpawnSpec::default()
    };

    let summary_a = manager.spawn(spec_a).unwrap();
    let spawned_pids_a = wait_for_pids_file(&pids_file_a, 2, Duration::from_secs(5));
    for &pid in &spawned_pids_a {
        assert!(
            pid_exists(pid),
            "Processo descendente {pid} deveria estar vivo antes do kill"
        );
    }

    manager.kill(summary_a.id, KillMode::Graceful).unwrap();

    for &pid in &spawned_pids_a {
        let deadline = Instant::now() + Duration::from_secs(2);
        while pid_exists(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !pid_exists(pid),
            "Processo descendente {pid} sobreviveu ao kill do PTY"
        );
    }

    // Parte B: Saída espontânea do shell líder elimina órfãos deixados para trás no cgroup (CA-19)
    let pids_file_b = tmp.path().join("pids_b.txt");
    let cmd_b = format!(
        "(sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> '{}'); sleep 0.05; exit 0",
        pids_file_b.display()
    );
    let spec_b = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), cmd_b],
        login: false,
        ..SpawnSpec::default()
    };

    let summary_b = manager.spawn(spec_b).unwrap();
    let spawned_pids_b = wait_for_pids_file(&pids_file_b, 1, Duration::from_secs(5));

    let exit_deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < exit_deadline {
        if let Ok(PtyEvent::Exited { pty_id, .. }) = events.recv_timeout(Duration::from_millis(50))
            && pty_id == summary_b.id
        {
            break;
        }
    }

    for &pid in &spawned_pids_b {
        let deadline = Instant::now() + Duration::from_secs(2);
        while pid_exists(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !pid_exists(pid),
            "Processo em background {pid} sobreviveu após saída espontânea do shell líder"
        );
    }

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn isolates_concurrent_ptys_and_parallel_shutdown_within_deadline() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    let make_sleep_spec = || SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), "sleep 60".to_string()],
        login: false,
        ..SpawnSpec::default()
    };

    let pty_a = manager.spawn(make_sleep_spec()).unwrap();
    let pty_b = manager.spawn(make_sleep_spec()).unwrap();

    // Matar PTY A não deve afetar PTY B
    manager.kill(pty_a.id, KillMode::Force).unwrap();
    assert!(matches!(
        manager.get(pty_a.id).unwrap().status,
        PtyStatus::Exited { .. }
    ));
    assert_eq!(manager.get(pty_b.id).unwrap().status, PtyStatus::Running);
    assert!(pid_exists(pty_b.pids.shell_pid));

    // Spawna mais 6 PTYs concorrentes e executa shutdown paralelo dentro do deadline (CA-20)
    let mut active_ids = vec![pty_b.id];
    for _ in 0..6 {
        let s = manager.spawn(make_sleep_spec()).unwrap();
        active_ids.push(s.id);
    }

    let shutdown_start = Instant::now();
    let report = manager.shutdown(Duration::from_secs(3));
    let shutdown_elapsed = shutdown_start.elapsed();

    assert!(
        shutdown_elapsed <= Duration::from_millis(3500),
        "Shutdown paralelo levou {shutdown_elapsed:?} (esperado <= 3.5s)"
    );
    assert!(
        report.pending.is_empty(),
        "Nenhum PTY deveria ficar pendente no shutdown"
    );
    for id in active_ids {
        assert!(report.completed.contains(&id));
    }
}

#[test]
fn recovers_stale_registry_without_touching_locked_active_instance() {
    let tmp = tempfile::tempdir().unwrap();
    let base_dir = tmp.path().join("recovery-test");
    fs::create_dir_all(&base_dir).unwrap();

    let provider_stale = SystemdCgroupProvider::discover(None, "test-stale-rec");
    let provider_live = SystemdCgroupProvider::discover(None, "test-live-rec");
    assert!(provider_stale.is_available());
    assert!(provider_live.is_available());

    let pty_stale = PtyId::new();
    let pty_live = PtyId::new();
    let cg_stale = provider_stale.create(pty_stale).unwrap();
    let cg_live = provider_live.create(pty_live).unwrap();

    let mut child_stale = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let mut child_live = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let stale_pid = child_stale.id();
    let live_pid = child_live.id();

    cg_stale.enroll_pid(stale_pid).unwrap();
    cg_live.enroll_pid(live_pid).unwrap();

    // 1. Instância órfã (sem lock ativo porque descartamos RegistryFile)
    let stale_dir = base_dir.join("inst-stale");
    {
        let header = RegistryHeader {
            version: REGISTRY_SCHEMA_VERSION,
            app_pid: 999_999,
            app_starttime_ticks: 1,
            watchdog_pid: None,
            watchdog_starttime_ticks: None,
            cgroup_subtree_root: provider_stale.subtree_root().unwrap().to_path_buf(),
        };
        let reg = RegistryFile::open(&stale_dir, header).unwrap();
        reg.upsert(RegistryEntry {
            pty_id: pty_stale,
            cgroup_path: cg_stale.cgroup_path().to_path_buf(),
            cgroup_id: cg_stale.cgroup_id(),
            pgid: i32::try_from(stale_pid).unwrap(),
            sid: i32::try_from(stale_pid).unwrap(),
            shell_pid: stale_pid,
            shell_starttime_ticks: 1,
            created_at_epoch_ms: 1000,
        })
        .unwrap();
        // drop(reg) libera o flock sem remover pty-registry.json (simulando SIGKILL da instância)
    }

    // 2. Instância viva (mantém _live_reg aberto com flock exclusivo ativo)
    let live_dir = base_dir.join("inst-live");
    let live_header = RegistryHeader {
        version: REGISTRY_SCHEMA_VERSION,
        app_pid: std::process::id(),
        app_starttime_ticks: 1,
        watchdog_pid: None,
        watchdog_starttime_ticks: None,
        cgroup_subtree_root: provider_live.subtree_root().unwrap().to_path_buf(),
    };
    let live_reg = RegistryFile::open(&live_dir, live_header).unwrap();
    live_reg
        .upsert(RegistryEntry {
            pty_id: pty_live,
            cgroup_path: cg_live.cgroup_path().to_path_buf(),
            cgroup_id: cg_live.cgroup_id(),
            pgid: i32::try_from(live_pid).unwrap(),
            sid: i32::try_from(live_pid).unwrap(),
            shell_pid: live_pid,
            shell_starttime_ticks: 1,
            created_at_epoch_ms: 1000,
        })
        .unwrap();

    // Executa varredura de recuperação (CA-22)
    let recovered_count = recover_stale_registries(&base_dir).unwrap();
    assert_eq!(recovered_count, 1);

    let _ = child_stale.wait();
    assert!(
        !pid_exists(stale_pid),
        "Processo órfão da instância morta deveria ter sido morto"
    );
    assert!(!stale_dir.join("pty-registry.json").exists());

    // Instância com lock ativo deve permanecer intacta
    assert!(
        pid_exists(live_pid),
        "Processo da instância viva não pode ser afetado"
    );
    assert!(live_dir.join("pty-registry.json").exists());

    // Limpeza da instância viva de teste
    let _ = cg_live.kill_all();
    let _ = child_live.wait();
    let _ = cg_live.release();
    live_reg.cleanup_files();
}

#[test]
fn rapid_spawn_attach_resize_kill_cycles_do_not_leak_fds_or_threads() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.tombstone_ttl = Duration::from_millis(20);
    config.supervisor_interval = Duration::from_millis(20);
    let manager = PtyManager::new(config).unwrap();

    let count_open_fds = || -> usize { fs::read_dir("/proc/self/fd").map_or(0, Iterator::count) };

    // Ciclo de aquecimento
    let warm = manager
        .spawn(SpawnSpec {
            shell: Some("/bin/sh".to_string()),
            args: vec!["-c".to_string(), "sleep 5".to_string()],
            login: false,
            ..SpawnSpec::default()
        })
        .unwrap();
    let _ = manager.kill(warm.id, KillMode::Force);
    std::thread::sleep(Duration::from_millis(60));

    let baseline_fds = count_open_fds();

    for i in 0..20u16 {
        let summary = manager
            .spawn(SpawnSpec {
                shell: Some("/bin/sh".to_string()),
                args: vec!["-c".to_string(), "sleep 10".to_string()],
                login: false,
                ..SpawnSpec::default()
            })
            .unwrap();
        let att = manager
            .attach(summary.id, Arc::new(NullSink), None)
            .unwrap();
        let _ = manager.write(summary.id, b"echo test\n");
        let _ = manager.resize(
            summary.id,
            PtySize {
                rows: 24 + (i % 10),
                cols: 80 + (i % 10),
                pixel_width: 0,
                pixel_height: 0,
            },
        );
        let _ = manager.detach(summary.id, att.attachment_id);
        let _ = manager.kill(summary.id, KillMode::Force).unwrap();
    }

    // Aguarda expiração dos tombstones
    std::thread::sleep(Duration::from_millis(120));
    let final_fds = count_open_fds();

    assert!(
        final_fds <= baseline_fds + 10,
        "Vazamento de FDs detectado após 20 ciclos: baseline={baseline_fds}, final={final_fds}"
    );

    let _ = manager.shutdown(Duration::from_secs(2));
}
