//! Teste de integração em subprocesso para limpeza de emergência via panic hook e watchdog
//! (`mt-pty-helper --mode watchdog`) quando o processo principal sofre `panic!` (Critério CA-21).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use mt_pty::{PtyConfig, PtyManager, SpawnSpec};

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

#[test]
fn panic_hook_and_watchdog_kill_all_pty_descendants_on_crash() {
    if std::env::var("MT_PTY_TEST_PANIC").as_deref() == Ok("1") {
        let rt_dir = std::env::var("MT_PTY_TEST_RT_DIR").expect("MT_PTY_TEST_RT_DIR ausente");
        let config = PtyConfig {
            registry_dir: PathBuf::from(rt_dir),
            orphan_sweep_on_startup: false,
            ..PtyConfig::default()
        };

        let manager = PtyManager::new(config).unwrap();
        let _guards = manager.install_guards().unwrap();

        let spec = SpawnSpec {
            shell: Some("/bin/sh".to_string()),
            args: vec!["-c".to_string(), "sleep 60".to_string()],
            login: false,
            ..SpawnSpec::default()
        };

        let summary = manager.spawn(spec).unwrap();
        std::thread::sleep(Duration::from_millis(100));

        let tracked = manager.tracked_processes(summary.id).unwrap();
        let pids_csv: String = tracked
            .iter()
            .map(|p| p.pid.to_string())
            .collect::<Vec<_>>()
            .join(",");

        println!("MT_PTY_CRASH_PIDS={pids_csv}");
        let _ = std::io::stdout().flush();

        // Força panic! sem chamar shutdown: o panic hook + EOF no watchdog devem matar o cgroup!
        panic!("Simulando crash abrupto do processo principal (CA-21)");
    }

    let tmp = tempfile::tempdir().unwrap();
    let rt_dir = tmp.path().join("panic-rt");

    let current_exe = std::env::current_exe().unwrap();
    let mut child = Command::new(current_exe)
        .arg("--exact")
        .arg("panic_hook_and_watchdog_kill_all_pty_descendants_on_crash")
        .arg("--nocapture")
        .env("MT_PTY_TEST_PANIC", "1")
        .env("MT_PTY_TEST_RT_DIR", &rt_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();

    let stdout = child.stdout.take().unwrap();
    let reader = BufReader::new(stdout);
    let mut tracked_pids: Vec<u32> = Vec::new();

    for line in reader.lines().map_while(Result::ok) {
        if let Some(rest) = line.strip_prefix("MT_PTY_CRASH_PIDS=") {
            tracked_pids = rest
                .split(',')
                .filter_map(|s| s.trim().parse::<u32>().ok())
                .collect();
            break;
        }
    }

    let status = child.wait().unwrap();
    assert!(
        !status.success(),
        "O subprocesso deveria ter encerrado com falha devido ao panic! intencional"
    );
    assert!(
        !tracked_pids.is_empty(),
        "O subprocesso deveria ter reportado pelo menos 1 PID ativo no cgroup antes do panic"
    );

    // Verifica que o watchdog matou todos os processos do PTY em < 2s (CA-21)
    let deadline = Instant::now() + Duration::from_secs(2);
    for &pid in &tracked_pids {
        while pid_exists(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            !pid_exists(pid),
            "Processo {pid} do PTY sobreviveu após crash do processo principal!"
        );
    }
}
