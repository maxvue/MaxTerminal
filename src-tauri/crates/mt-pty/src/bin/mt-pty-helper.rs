//! Executável auxiliar `mt-pty-helper` para modos `launcher` e `watchdog`.
//!
//! - `--mode launcher --socket <path>`: conecta ao socket do [`LauncherServer`], aguarda
//!   inscrição no cgroup v2 pelo processo pai e só então executa `execve` no mesmo PID.
//! - `--mode watchdog --socket <path> --runtime-dir <dir>`: monitora o processo principal
//!   via EOF ou sinalização no socket de emergência e garante `cgroup.kill` em todas as
//!   árvores de processos de PTY caso o processo principal morra abruptamente (`SIGKILL` / panic).

use std::collections::HashMap;
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use mt_core::ids::PtyId;
use mt_pty::containment::{
    LauncherReleaseMsg, LauncherResponseMsg, WatchdogCommand, WatchdogResponse, recv_framed_json,
    send_framed_json,
};
use rustix::event::{PollFd, PollFlags, poll};
use rustix::io::{FdFlags, fcntl_setfd};
use serde::Deserialize;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut mode: Option<String> = None;
    let mut socket_path: Option<PathBuf> = None;
    let mut runtime_dir: Option<PathBuf> = None;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--mode" => {
                if i + 1 < args.len() {
                    mode = Some(args[i + 1].clone());
                    i += 2;
                } else {
                    std::process::exit(2);
                }
            }
            "--socket" => {
                if i + 1 < args.len() {
                    socket_path = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    std::process::exit(2);
                }
            }
            "--runtime-dir" => {
                if i + 1 < args.len() {
                    runtime_dir = Some(PathBuf::from(&args[i + 1]));
                    i += 2;
                } else {
                    std::process::exit(2);
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    match (mode.as_deref(), socket_path) {
        (Some("launcher"), Some(sock)) => run_launcher(&sock),
        (Some("watchdog"), Some(sock)) => {
            let dir = runtime_dir.unwrap_or_else(|| {
                sock.parent()
                    .map_or_else(|| PathBuf::from("/tmp"), Path::to_path_buf)
            });
            run_watchdog(&sock, &dir);
        }
        _ => std::process::exit(2),
    }
}

/// Executa o modo `launcher`: conecta ao socket do processo pai, aguarda liberação e faz `execve`.
fn run_launcher(socket_path: &Path) -> ! {
    let Ok(mut stream) = UnixStream::connect(socket_path) else {
        std::process::exit(126);
    };

    // Garante que o descritor do socket seja fechado automaticamente pelo kernel no execve bem-sucedido.
    let _ = fcntl_setfd(&stream, FdFlags::CLOEXEC);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let Ok(Some(release_msg)) = recv_framed_json::<_, LauncherReleaseMsg>(&mut stream) else {
        std::process::exit(126);
    };

    if let Err(e) = std::env::set_current_dir(&release_msg.cwd) {
        let _ = send_framed_json(
            &mut stream,
            &LauncherResponseMsg {
                ok: false,
                error: Some(format!(
                    "Falha ao alterar diretório para '{}': {e}",
                    release_msg.cwd
                )),
            },
        );
        std::process::exit(127);
    }

    let Ok(exec_cstr) = CString::new(release_msg.executable.clone()) else {
        let _ = send_framed_json(
            &mut stream,
            &LauncherResponseMsg {
                ok: false,
                error: Some("Caminho do executável contém byte nulo".to_string()),
            },
        );
        std::process::exit(127);
    };

    let mut argv_cstrs = Vec::with_capacity(1 + release_msg.argv.len());
    argv_cstrs.push(exec_cstr.clone());
    for arg in &release_msg.argv {
        let Ok(cstr) = CString::new(arg.as_str()) else {
            let _ = send_framed_json(
                &mut stream,
                &LauncherResponseMsg {
                    ok: false,
                    error: Some("Argumento do comando contém byte nulo".to_string()),
                },
            );
            std::process::exit(127);
        };
        argv_cstrs.push(cstr);
    }

    let mut env_cstrs = Vec::with_capacity(release_msg.env.len());
    for (k, v) in &release_msg.env {
        let entry = format!("{k}={v}");
        if let Ok(cstr) = CString::new(entry) {
            env_cstrs.push(cstr);
        }
    }

    match nix::unistd::execve(&exec_cstr, &argv_cstrs, &env_cstrs) {
        Ok(_) => unreachable!(),
        Err(err) => {
            let _ = send_framed_json(
                &mut stream,
                &LauncherResponseMsg {
                    ok: false,
                    error: Some(format!(
                        "Falha em execve para '{}': {err}",
                        release_msg.executable
                    )),
                },
            );
            std::process::exit(127);
        }
    }
}

/// Estrutura parcial de fallback para leitura de `pty-registry.json` pelo watchdog.
#[derive(Debug, Deserialize)]
struct FallbackRegistryFile {
    #[serde(default)]
    entries: Vec<FallbackRegistryEntry>,
}

#[derive(Debug, Deserialize)]
struct FallbackRegistryEntry {
    cgroup_path: PathBuf,
    cgroup_id: u64,
}

/// Executa o modo `watchdog`: monitora os sockets de controle e emergência e varre cgroups em caso de queda.
fn run_watchdog(socket_path: &Path, runtime_dir: &Path) -> ! {
    let Ok(mut control_stream) = UnixStream::connect(socket_path) else {
        std::process::exit(1);
    };
    let Ok(emergency_stream) = UnixStream::connect(socket_path) else {
        std::process::exit(1);
    };

    let mut registered: HashMap<PtyId, (PathBuf, u64)> = HashMap::new();

    loop {
        let mut fds = [
            PollFd::new(
                &control_stream,
                PollFlags::IN | PollFlags::HUP | PollFlags::ERR,
            ),
            PollFd::new(
                &emergency_stream,
                PollFlags::IN | PollFlags::HUP | PollFlags::ERR,
            ),
        ];

        match poll(&mut fds, -1) {
            Ok(_) => {}
            Err(err) if err == rustix::io::Errno::INTR => continue,
            Err(_) => {
                sweep_all_cgroups(&registered, runtime_dir);
                std::process::exit(0);
            }
        }

        let emergency_revents = fds[1].revents();
        if emergency_revents.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR) {
            sweep_all_cgroups(&registered, runtime_dir);
            std::process::exit(0);
        }

        let control_revents = fds[0].revents();
        if control_revents.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR) {
            match recv_framed_json::<_, WatchdogCommand>(&mut control_stream) {
                Ok(Some(WatchdogCommand::RegisterCgroup {
                    pty_id,
                    cgroup_path,
                    cgroup_id,
                })) => {
                    registered.insert(pty_id, (cgroup_path, cgroup_id));
                    let _ = send_framed_json(&mut control_stream, &WatchdogResponse::Ack);
                }
                Ok(Some(WatchdogCommand::UnregisterCgroup { pty_id })) => {
                    registered.remove(&pty_id);
                    let _ = send_framed_json(&mut control_stream, &WatchdogResponse::Ack);
                }
                Ok(Some(WatchdogCommand::Ping)) => {
                    let _ = send_framed_json(&mut control_stream, &WatchdogResponse::Pong);
                }
                Ok(Some(WatchdogCommand::Shutdown)) => {
                    let _ = send_framed_json(&mut control_stream, &WatchdogResponse::Ack);
                    if !registered.is_empty() {
                        sweep_all_cgroups(&registered, runtime_dir);
                    }
                    std::process::exit(0);
                }
                Ok(None) | Err(_) => {
                    // O processo pai morreu (EOF) ou o socket quebrou sem Shutdown.
                    sweep_all_cgroups(&registered, runtime_dir);
                    std::process::exit(0);
                }
            }
        }
    }
}

/// Aciona `cgroup.kill` em todos os cgroups registrados e nos listados em `pty-registry.json`.
fn sweep_all_cgroups(registered: &HashMap<PtyId, (PathBuf, u64)>, runtime_dir: &Path) {
    let mut targets: Vec<(PathBuf, u64)> = registered.values().cloned().collect();

    let registry_path = runtime_dir.join("pty-registry.json");
    if let Ok(raw) = fs::read_to_string(&registry_path)
        && let Ok(parsed) = serde_json::from_str::<FallbackRegistryFile>(&raw)
    {
        for entry in parsed.entries {
            if !targets.iter().any(|(p, _)| p == &entry.cgroup_path) {
                targets.push((entry.cgroup_path, entry.cgroup_id));
            }
        }
    }

    for (cgroup_path, expected_id) in &targets {
        kill_and_cleanup_cgroup(cgroup_path, *expected_id);
    }

    let _ = fs::remove_file(&registry_path);
}

/// Mata atomicamente os processos de um cgroup v2 validando seu inode e remove o diretório.
fn kill_and_cleanup_cgroup(cgroup_path: &Path, expected_id: u64) {
    let Ok(meta) = fs::metadata(cgroup_path) else {
        return;
    };
    if expected_id != 0 && meta.ino() != expected_id {
        return;
    }

    let kill_file = cgroup_path.join("cgroup.kill");
    let _ = fs::write(&kill_file, b"1");

    // Também envia SIGKILL aos PIDs listados caso seja um diretório simulado em testes ou cgroup sem cgroup.kill.
    let procs_file = cgroup_path.join("cgroup.procs");
    if let Ok(content) = fs::read_to_string(&procs_file) {
        for line in content.lines() {
            if let Ok(pid) = line.trim().parse::<i32>()
                && pid > 1
            {
                let _ = nix::sys::signal::kill(
                    nix::unistd::Pid::from_raw(pid),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
        }
    }

    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if is_cgroup_empty(cgroup_path) {
            break;
        }
        let _ = fs::write(&kill_file, b"1");
        thread::sleep(Duration::from_millis(20));
    }

    let _ = fs::remove_dir(cgroup_path);
}

fn is_cgroup_empty(cgroup_path: &Path) -> bool {
    let events_file = cgroup_path.join("cgroup.events");
    if let Ok(events) = fs::read_to_string(&events_file) {
        for line in events.lines() {
            if let Some(val) = line.strip_prefix("populated ") {
                return val.trim() == "0";
            }
        }
    }

    let procs_file = cgroup_path.join("cgroup.procs");
    if let Ok(procs) = fs::read_to_string(&procs_file) {
        return procs.lines().all(|l| l.trim().is_empty());
    }

    true
}
