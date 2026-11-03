//! Utilitário de fumaça CLI (`pty_smoke`) para validar `PtyManager` sem Tauri (CA-31).
//!
//! Exemplo de uso:
//! `cargo run -p mt-pty --example pty_smoke -- --cmd 'ls -la'`

use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use crossbeam_channel::unbounded;
use mt_pty::io::{FrameKind, decode_end_payload, decode_frame};
use mt_pty::{
    OutputSink, PtyConfig, PtyEvent, PtyManager, PtyOwner, PtySize, SinkClosed, SpawnSpec,
};

struct ChannelForwardSink {
    tx: crossbeam_channel::Sender<Bytes>,
}

impl OutputSink for ChannelForwardSink {
    fn send(&self, frame: Bytes) -> Result<(), SinkClosed> {
        self.tx.send(frame).map_err(|_| SinkClosed)
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut cmd_str = String::from("echo 'MaxTerminal PTY Smoke OK'");

    let mut i = 1;
    while i < args.len() {
        if args[i] == "--cmd" && i + 1 < args.len() {
            cmd_str.clone_from(&args[i + 1]);
            i += 2;
        } else {
            i += 1;
        }
    }

    let config = PtyConfig::default();
    let manager = match PtyManager::new(config) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Falha ao inicializar PtyManager: {e}");
            std::process::exit(1);
        }
    };

    let event_rx = manager.subscribe();
    let (frame_tx, frame_rx) = unbounded::<Bytes>();
    let sink = Arc::new(ChannelForwardSink { tx: frame_tx });

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), cmd_str],
        login: false,
        cwd: None,
        env: std::collections::BTreeMap::new(),
        env_remove: Vec::new(),
        size: PtySize::default(),
        owner: PtyOwner::default(),
    };

    let summary = match manager.spawn(spec) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Falha ao iniciar PTY: {e}");
            std::process::exit(1);
        }
    };

    let attach_info = match manager.attach(summary.id, sink, None) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("Falha ao anexar ao PTY: {e}");
            std::process::exit(1);
        }
    };

    let mut exit_code = 0i32;

    loop {
        if let Ok(raw_frame) = frame_rx.recv_timeout(Duration::from_millis(50))
            && let Ok(decoded) = decode_frame(raw_frame)
        {
            match decoded.kind {
                FrameKind::Data | FrameKind::Replay => {
                    let _ = std::io::stdout().write_all(&decoded.payload);
                    let _ = std::io::stdout().flush();
                    let consumed_bytes = decoded
                        .byte_offset
                        .saturating_add(u64::try_from(decoded.payload.len()).unwrap_or(0));
                    let _ = manager.ack(summary.id, attach_info.attachment_id, consumed_bytes);
                }
                FrameKind::Gap => {}
                FrameKind::End => {
                    if let Ok(exit_info) = decode_end_payload(&decoded) {
                        println!(
                            "\n[pty_smoke] Encerrado: code={:?}, signal={:?}, reason={:?}",
                            exit_info.code, exit_info.signal, exit_info.reason
                        );
                        exit_code = exit_info.code.unwrap_or(0);
                    }
                    break;
                }
            }
        }

        while let Ok(ev) = event_rx.try_recv() {
            if let PtyEvent::Exited { pty_id, exit, .. } = ev
                && pty_id == summary.id
            {
                while let Ok(raw_frame) = frame_rx.try_recv() {
                    if let Ok(decoded) = decode_frame(raw_frame)
                        && matches!(decoded.kind, FrameKind::Data | FrameKind::Replay)
                    {
                        let _ = std::io::stdout().write_all(&decoded.payload);
                        let _ = std::io::stdout().flush();
                    }
                }
                println!(
                    "\n[pty_smoke] Encerrado: code={:?}, signal={:?}, reason={:?}",
                    exit.code, exit.signal, exit.reason
                );
                exit_code = exit.code.unwrap_or(0);
                let _ = manager.shutdown(Duration::from_secs(2));
                std::process::exit(exit_code);
            }
        }
    }

    let _ = manager.shutdown(Duration::from_secs(2));
    std::process::exit(exit_code);
}
