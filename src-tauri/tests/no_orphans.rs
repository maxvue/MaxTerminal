//! Teste de integração zero-órfãos (`CA-25` — Fase 17):
//! árvores reais de processos (filhos diretos, netos, `nohup`, `setsid`, double-fork e processos
//! que ignoram `SIGHUP`/`SIGTERM`), proteção contra reuso de PID via `(pid, start_time)` e
//! reconciliação de registros órfãos (`recover_stale_registries`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use mt_core::ids::PtyId;
use mt_pty::containment::{ContainmentProvider, SystemdCgroupProvider};
use mt_pty::guard::{
    REGISTRY_SCHEMA_VERSION, RegistryEntry, RegistryFile, RegistryHeader, recover_stale_registries,
};
use mt_pty::proc::{ProcSource, ProcfsSource};
use mt_pty::{KillMode, PtyConfig, PtyManager, SpawnSpec};
use uuid::Uuid;

struct TempTestDir(PathBuf);

impl TempTestDir {
    fn new() -> Self {
        let short_id = &Uuid::now_v7().simple().to_string()[20..];
        let dir = PathBuf::from("/tmp").join(format!("mto-{short_id}"));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn test_config(tmp: &TempTestDir) -> PtyConfig {
    PtyConfig {
        registry_dir: tmp.path().join("rt"),
        orphan_sweep_on_startup: false,
        supervisor_interval: Duration::from_millis(40),
        hup_grace: Duration::from_millis(150),
        term_grace: Duration::from_millis(200),
        ..PtyConfig::default()
    }
}

fn pid_alive_or_zombie(pid: u32) -> (bool, bool) {
    let proc_path = PathBuf::from(format!("/proc/{pid}"));
    if !proc_path.exists() {
        return (false, false);
    }
    if let Ok(stat) = fs::read_to_string(proc_path.join("stat"))
        && let Some(idx) = stat.rfind(')')
        && let Some(state_char) = stat.get(idx + 2..).and_then(|s| s.chars().next())
    {
        if state_char == 'Z' {
            return (false, true);
        }
        if state_char == 'X' {
            return (false, false);
        }
        return (true, false);
    }
    (false, false)
}

fn wait_for_pids(path: &Path, expected_count: usize, timeout: Duration) -> Vec<u32> {
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
fn eliminates_all_descendant_trees_without_zombies_and_guards_pid_reuse() {
    let tmp = TempTestDir::new();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    let pids_file = tmp.path().join("spawned_pids.txt");
    let script = format!(
        "sh -c 'sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> \"{0}\"; wait' </dev/null >/dev/null 2>&1 & echo $! >> \"{0}\"; \
         nohup sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> \"{0}\"; \
         (setsid sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> \"{0}\") 2>/dev/null || (sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> \"{0}\"); \
         ( (sleep 60 </dev/null >/dev/null 2>&1 & echo $! >> \"{0}\") & ); \
         sh -c 'trap \"\" HUP TERM; while :; do sleep 0.1; done' </dev/null >/dev/null 2>&1 & echo $! >> \"{0}\"; \
         sleep 60",
        pids_file.display()
    );

    let summary = manager
        .spawn(SpawnSpec {
            shell: Some("/bin/sh".to_string()),
            args: vec!["-c".to_string(), script],
            login: false,
            ..SpawnSpec::default()
        })
        .unwrap();

    let mut tracked_pids = wait_for_pids(&pids_file, 6, Duration::from_secs(5));
    tracked_pids.push(summary.pids.shell_pid);

    // Captura identidade (pid, start_time) via ProcfsSource antes do kill
    let proc_source = ProcfsSource::new();
    let shell_identity = proc_source
        .identity(summary.pids.shell_pid)
        .unwrap()
        .unwrap();
    assert!(shell_identity.start_time.0 > 0);

    // Encerra a aba/PTY (escalando até SIGKILL para o filho que ignora HUP/TERM)
    manager.kill(summary.id, KillMode::Graceful).unwrap();

    // Verifica que 100% dos processos morreram e que nenhum ficou zumbi (state != 'Z')
    let deadline = Instant::now() + Duration::from_secs(3);
    for &pid in &tracked_pids {
        while Instant::now() < deadline {
            let (alive, zombie) = pid_alive_or_zombie(pid);
            if !alive && !zombie {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let (alive, zombie) = pid_alive_or_zombie(pid);
        assert!(
            !alive && !zombie,
            "PID {pid} não deveria estar vivo ({alive}) nem zumbi ({zombie}) após kill do PTY"
        );
    }

    // 2. Proteção contra reuso de PID via (pid, start_time) e reconciliação de registro órfão
    let base_dir = tmp.path().join("stale-recovery");
    fs::create_dir_all(&base_dir).unwrap();
    let provider = SystemdCgroupProvider::discover(None, "ca25-stale");
    assert!(provider.is_available());

    let stale_pty = PtyId::new();
    let cg = provider.create(stale_pty).unwrap();
    let mut victim = Command::new("/bin/sleep").arg("60").spawn().unwrap();
    let victim_pid = victim.id();
    let victim_start = proc_source
        .identity(victim_pid)
        .unwrap()
        .unwrap()
        .start_time
        .0;
    cg.enroll_pid(victim_pid).unwrap();

    let stale_dir = base_dir.join("inst-crashed");
    {
        let header = RegistryHeader {
            version: REGISTRY_SCHEMA_VERSION,
            app_pid: 999_998,
            app_starttime_ticks: 1,
            watchdog_pid: None,
            watchdog_starttime_ticks: None,
            cgroup_subtree_root: provider.subtree_root().unwrap().to_path_buf(),
        };
        let reg = RegistryFile::open(&stale_dir, header).unwrap();
        reg.upsert(RegistryEntry {
            pty_id: stale_pty,
            cgroup_path: cg.cgroup_path().to_path_buf(),
            cgroup_id: cg.cgroup_id(),
            pgid: i32::try_from(victim_pid).unwrap(),
            sid: i32::try_from(victim_pid).unwrap(),
            shell_pid: victim_pid,
            shell_starttime_ticks: victim_start,
            created_at_epoch_ms: 1_000,
        })
        .unwrap();
    }

    let recovered = recover_stale_registries(&base_dir).unwrap();
    assert_eq!(recovered, 1);
    let _ = victim.wait();
    let (alive, zombie) = pid_alive_or_zombie(victim_pid);
    assert!(!alive && !zombie);

    let _ = manager.shutdown(Duration::from_secs(2));
}
