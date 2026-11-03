//! Suíte de testes de integração de criação de PTY, resolução de shell/cwd/env,
//! redimensionamento deduplicado e retenção de tombstones (Critérios CA-04, CA-05, CA-06, CA-07, CA-32).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use crossbeam_channel::unbounded;
use mt_pty::io::{FrameKind, decode_end_payload, decode_frame};
use mt_pty::{
    ExitReason, OutputSink, PtyConfig, PtyEvent, PtyManager, PtyOwner, PtySize, PtyStatus,
    SinkClosed, SpawnSpec,
};
use tempfile::TempDir;

struct ChannelSink {
    tx: crossbeam_channel::Sender<Bytes>,
}

impl OutputSink for ChannelSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        self.tx.send(frame).map_err(|_| SinkClosed)
    }
}

fn test_config(tmp: &TempDir) -> PtyConfig {
    let instance_id = uuid::Uuid::now_v7().to_string();
    PtyConfig {
        registry_dir: tmp.path().join(format!("rt-{instance_id}")),
        orphan_sweep_on_startup: false,
        supervisor_interval: Duration::from_millis(40),
        ..PtyConfig::default()
    }
}

fn collect_until_end(
    manager: &PtyManager,
    pty_id: mt_core::ids::PtyId,
    attachment_id: mt_pty::AttachmentId,
    rx: &crossbeam_channel::Receiver<Bytes>,
    timeout: Duration,
) -> (Vec<u8>, mt_pty::ExitInfo) {
    let deadline = Instant::now() + timeout;
    let mut output = Vec::new();
    let mut consumed = 0u64;

    while Instant::now() < deadline {
        if let Ok(raw) = rx.recv_timeout(Duration::from_millis(50)) {
            let decoded = decode_frame(raw).unwrap();
            match decoded.kind {
                FrameKind::Data | FrameKind::Replay => {
                    consumed += u64::try_from(decoded.payload.len()).unwrap();
                    output.extend_from_slice(&decoded.payload);
                    let _ = manager.ack(pty_id, attachment_id, consumed);
                }
                FrameKind::Gap => {}
                FrameKind::End => {
                    let exit = decode_end_payload(&decoded).unwrap();
                    return (output, exit);
                }
            }
        }
    }
    panic!("Timeout aguardando frame End do PTY {pty_id}");
}

#[test]
fn spawn_echo_and_exit_code_and_tombstone_retention() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();
    let events = manager.subscribe();

    let (tx, rx) = unbounded();
    let sink = Arc::new(ChannelSink { tx });

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), "printf hello; exit 3".to_string()],
        login: false,
        cwd: None,
        env: BTreeMap::new(),
        env_remove: Vec::new(),
        size: PtySize::default(),
        owner: PtyOwner::default(),
    };

    let summary = manager.spawn(spec).unwrap();
    let attach_info = manager.attach(summary.id, sink, None).unwrap();

    let (output, exit) = collect_until_end(
        &manager,
        summary.id,
        attach_info.attachment_id,
        &rx,
        Duration::from_secs(5),
    );

    assert_eq!(output, b"hello");
    assert_eq!(exit.code, Some(3));
    assert_eq!(exit.signal, None);
    assert_eq!(exit.reason, ExitReason::Exited);

    // Verifica que o evento global PtyEvent::Exited também foi emitido
    let mut saw_exited_event = false;
    while let Ok(ev) = events.recv_timeout(Duration::from_millis(200)) {
        if let PtyEvent::Exited {
            pty_id,
            exit: ev_exit,
            ..
        } = ev
            && pty_id == summary.id
        {
            assert_eq!(ev_exit.code, Some(3));
            saw_exited_event = true;
            break;
        }
    }
    assert!(saw_exited_event, "Esperado PtyEvent::Exited para o PTY");

    // CA-32: Tombstone permanece consultável em get() e list()
    let tombstone = manager
        .get(summary.id)
        .expect("Tombstone deve permanecer retido");
    assert_eq!(tombstone.status, PtyStatus::Exited { exit: exit.clone() });
    assert!(manager.list().iter().any(|s| s.id == summary.id));

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn resolves_shell_and_rejects_invalid_executable_and_cwd() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    // Executável inexistente -> pty.shell_not_found
    let bad_shell = SpawnSpec {
        shell: Some("/caminho/inexistente/shell-xyz".to_string()),
        login: false,
        ..SpawnSpec::default()
    };
    let err_shell = manager.spawn(bad_shell).unwrap_err();
    assert_eq!(err_shell.code(), "pty.shell_not_found");

    // Diretório inexistente -> pty.cwd_invalid
    let bad_cwd = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        cwd: Some("/diretorio/que/nao/existe/99999".to_string()),
        login: false,
        ..SpawnSpec::default()
    };
    let err_cwd = manager.spawn(bad_cwd).unwrap_err();
    assert_eq!(err_cwd.code(), "pty.cwd_invalid");

    // Caminho apontando para arquivo -> pty.cwd_invalid
    let regular_file = tmp.path().join("arquivo.txt");
    fs::write(&regular_file, b"conteudo").unwrap();
    let file_cwd = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        cwd: Some(regular_file.to_string_lossy().into_owned()),
        login: false,
        ..SpawnSpec::default()
    };
    assert_eq!(
        manager.spawn(file_cwd).unwrap_err().code(),
        "pty.cwd_invalid"
    );

    // Resolução via PATH ("sh") e cwd válido
    let work_dir = tmp.path().join("workspace");
    fs::create_dir_all(&work_dir).unwrap();
    let canon_work_dir = fs::canonicalize(&work_dir).unwrap();

    let (tx, rx) = unbounded();
    let sink = Arc::new(ChannelSink { tx });
    let ok_spec = SpawnSpec {
        shell: Some("sh".to_string()),
        args: vec!["-c".to_string(), "pwd".to_string()],
        login: false,
        cwd: Some(work_dir.to_string_lossy().into_owned()),
        ..SpawnSpec::default()
    };
    let summary = manager.spawn(ok_spec).unwrap();
    let attach = manager.attach(summary.id, sink, None).unwrap();
    let (out, exit) = collect_until_end(
        &manager,
        summary.id,
        attach.attachment_id,
        &rx,
        Duration::from_secs(5),
    );
    assert_eq!(exit.code, Some(0));
    let out_str = String::from_utf8_lossy(&out);
    assert_eq!(out_str.trim(), canon_work_dir.to_string_lossy());

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn applies_environment_precedence_and_validates_env_vars() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    // Chave inválida -> pty.env_invalid
    let mut bad_env_spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        login: false,
        ..SpawnSpec::default()
    };
    bad_env_spec
        .env
        .insert("1INVALID_KEY".to_string(), "val".to_string());
    assert_eq!(
        manager.spawn(bad_env_spec).unwrap_err().code(),
        "pty.env_invalid"
    );

    // Valor com byte nulo -> pty.env_invalid
    let mut null_val_spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        login: false,
        ..SpawnSpec::default()
    };
    null_val_spec
        .env
        .insert("VALID_KEY".to_string(), "bad\0val".to_string());
    assert_eq!(
        manager.spawn(null_val_spec).unwrap_err().code(),
        "pty.env_invalid"
    );

    // Verifica precedência de variáveis forçadas e customizadas
    let mut env = BTreeMap::new();
    env.insert("CUSTOM_KEY".to_string(), "custom_ok".to_string());
    env.insert("TERM".to_string(), "dumb".to_string());

    let (tx, rx) = unbounded();
    let sink = Arc::new(ChannelSink { tx });
    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec![
            "-c".to_string(),
            "printf '%s|%s|%s|%s|%s' \"$TERM\" \"$COLORTERM\" \"$TERM_PROGRAM\" \"$MAXTERMINAL\" \"$CUSTOM_KEY\""
                .to_string(),
        ],
        login: false,
        cwd: None,
        env,
        env_remove: vec!["HOME".to_string()],
        size: PtySize::default(),
        owner: PtyOwner::default(),
    };

    let summary = manager.spawn(spec).unwrap();
    let attach = manager.attach(summary.id, sink, None).unwrap();
    let (out, exit) = collect_until_end(
        &manager,
        summary.id,
        attach.attachment_id,
        &rx,
        Duration::from_secs(5),
    );
    assert_eq!(exit.code, Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out),
        "xterm-256color|truecolor|MaxTerminal|1|custom_ok"
    );

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn resizes_pty_deduplicates_identical_size_and_rejects_out_of_bounds() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    let (tx, rx) = unbounded();
    let sink = Arc::new(ChannelSink { tx });

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: Vec::new(),
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    let attach = manager.attach(summary.id, sink, None).unwrap();

    assert_eq!(manager.resize_ioctl_count(summary.id).unwrap(), 0);

    // Dimensões inválidas -> pty.resize_invalid
    assert_eq!(
        manager
            .resize(
                summary.id,
                PtySize {
                    rows: 0,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0
                }
            )
            .unwrap_err()
            .code(),
        "pty.resize_invalid"
    );
    assert_eq!(
        manager
            .resize(
                summary.id,
                PtySize {
                    rows: 24,
                    cols: 1001,
                    pixel_width: 0,
                    pixel_height: 0
                }
            )
            .unwrap_err()
            .code(),
        "pty.resize_invalid"
    );

    let new_size = PtySize {
        rows: 40,
        cols: 120,
        pixel_width: 0,
        pixel_height: 0,
    };
    manager.resize(summary.id, new_size).unwrap();
    assert_eq!(manager.resize_ioctl_count(summary.id).unwrap(), 1);

    // Chamada idêntica não deve disparar novo ioctl (CA-07)
    manager.resize(summary.id, new_size).unwrap();
    assert_eq!(manager.resize_ioctl_count(summary.id).unwrap(), 1);

    // Verifica via stty size dentro do PTY que o tamanho é 40 120
    manager.write(summary.id, b"stty size; exit 0\n").unwrap();

    let (out, exit) = collect_until_end(
        &manager,
        summary.id,
        attach.attachment_id,
        &rx,
        Duration::from_secs(5),
    );
    assert_eq!(exit.code, Some(0));
    let out_str = String::from_utf8_lossy(&out);
    assert!(
        out_str.contains("40 120"),
        "Saída de stty size deveria conter '40 120', recebido: {out_str:?}"
    );

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn expires_tombstone_after_ttl() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.tombstone_ttl = Duration::from_millis(80);
    config.supervisor_interval = Duration::from_millis(25);

    let manager = PtyManager::new(config).unwrap();
    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), "exit 0".to_string()],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if manager.get(summary.id).is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    assert!(
        manager.get(summary.id).is_none(),
        "Tombstone deveria ter sido removido após tombstone_ttl"
    );

    let _ = manager.shutdown(Duration::from_secs(2));
}
