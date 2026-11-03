//! Suíte de testes de integração do pipeline de I/O: fluxo contínuo de 10 MiB com SHA-256,
//! preservação de UTF-8/ANSI/bytes arbitrários, replay/gap e `OutputTap` (CA-08, CA-09, CA-14, CA-33).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use bytes::Bytes;
use crossbeam_channel::unbounded;
use mt_core::ids::PtyId;
use mt_pty::io::{FrameKind, decode_end_payload, decode_frame, decode_gap_payload};
use mt_pty::{
    ExitInfo, OutputCursor, OutputSink, OutputTap, PtyConfig, PtyManager, SinkClosed, SpawnSpec,
    U64Decimal,
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
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

#[test]
fn streams_10mib_continuous_output_with_contiguous_seq_and_matching_sha256() {
    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();

    // Gera arquivo fixture de 10 MiB contendo bytes 0x00..=0xFF, UTF-8 multibyte e escapes ANSI
    let total_size = 10 * 1024 * 1024;
    let mut fixture = Vec::with_capacity(total_size);
    let mut snippet = "ação 🚀 日本語 \x1b[38;2;12;34;56mANSI\x1b[0m"
        .as_bytes()
        .to_vec();
    snippet.extend_from_slice(&[0x00, 0xFF]);
    while fixture.len() < total_size {
        let b = u8::try_from(fixture.len() % 256).unwrap();
        fixture.push(b);
        let rem = total_size - fixture.len();
        if rem > 0 {
            let take = rem.min(snippet.len());
            fixture.extend_from_slice(&snippet[..take]);
        }
    }
    assert_eq!(fixture.len(), total_size);
    let expected_sha256 = Sha256::digest(&fixture);

    let fixture_path = tmp.path().join("fixture_10mib.bin");
    fs::write(&fixture_path, &fixture).unwrap();

    let (tx, rx) = unbounded();
    let sink = Arc::new(ChannelSink { tx });

    let cmd = format!("stty raw -echo; cat '{}'", fixture_path.display());
    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), cmd],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    let attach = manager.attach(summary.id, sink, None).unwrap();

    let mut received = Vec::with_capacity(total_size);
    let mut expected_seq = 1u64;
    let mut consumed = 0u64;
    let deadline = Instant::now() + Duration::from_secs(15);

    while Instant::now() < deadline {
        if let Ok(raw) = rx.recv_timeout(Duration::from_millis(100)) {
            let decoded = decode_frame(raw).unwrap();
            match decoded.kind {
                FrameKind::Data | FrameKind::Replay => {
                    assert_eq!(
                        decoded.seq, expected_seq,
                        "Sequência de frame fora de ordem"
                    );
                    assert_eq!(decoded.byte_offset, consumed, "Offset de bytes desalinhado");
                    expected_seq += 1;
                    consumed += u64::try_from(decoded.payload.len()).unwrap();
                    received.extend_from_slice(&decoded.payload);
                    manager
                        .ack(summary.id, attach.attachment_id, consumed)
                        .unwrap();
                }
                FrameKind::Gap => {
                    panic!("Não deveria ocorrer Gap com consumidor ativo confirmando ACKs")
                }
                FrameKind::End => {
                    let exit = decode_end_payload(&decoded).unwrap();
                    assert_eq!(exit.code, Some(0));
                    break;
                }
            }
        }
    }

    assert_eq!(received.len(), total_size);
    assert_eq!(Sha256::digest(&received), expected_sha256);

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn reconnects_with_cursor_and_emits_gap_when_replay_ring_wraps() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.replay_capacity = 32 * 1024; // 32 KiB
    config.max_frame_bytes = 8 * 1024; // 8 KiB por frame
    config.read_buffer_size = 4 * 1024;
    let manager = PtyManager::new(config).unwrap();
    let events = manager.subscribe();

    // Emite 100 KiB sem consumidor anexado para forçar rotação do ring buffer de 32 KiB
    let total_bytes = 100_000usize;
    let payload_file = tmp.path().join("burst_100k.bin");
    let data: Vec<u8> = (0..total_bytes)
        .map(|i| u8::try_from((i % 26) + 65).unwrap())
        .collect();
    fs::write(&payload_file, &data).unwrap();

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec![
            "-c".to_string(),
            format!("stty raw -echo; cat '{}'", payload_file.display()),
        ],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();

    // Aguarda o término do PTY antes de anexar
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(mt_pty::PtyEvent::Exited { pty_id, .. }) =
            events.recv_timeout(Duration::from_millis(50))
            && pty_id == summary.id
        {
            break;
        }
    }

    let (tx1, rx1) = unbounded();
    let sink1 = Arc::new(ChannelSink { tx: tx1 });
    let info1 = manager.attach(summary.id, sink1, None).unwrap();
    assert!(
        info1.truncated,
        "Deve indicar truncamento quando 100 KiB > 32 KiB"
    );

    let mut saw_gap = false;
    let mut dropped_bytes = 0u64;
    let mut replayed = Vec::new();
    let mut consumed = 0u64;

    while let Ok(raw) = rx1.recv_timeout(Duration::from_millis(200)) {
        let decoded = decode_frame(raw).unwrap();
        match decoded.kind {
            FrameKind::Gap => {
                saw_gap = true;
                dropped_bytes = decode_gap_payload(&decoded).unwrap();
            }
            FrameKind::Replay => {
                consumed += u64::try_from(decoded.payload.len()).unwrap();
                replayed.extend_from_slice(&decoded.payload);
                let _ = manager.ack(summary.id, info1.attachment_id, consumed);
            }
            FrameKind::End => break,
            FrameKind::Data => panic!("Não esperado Data em tombstone"),
        }
    }

    assert!(saw_gap, "Primeiro frame deveria ser Gap");
    assert_eq!(
        usize::try_from(dropped_bytes).unwrap() + replayed.len(),
        total_bytes
    );
    assert_eq!(
        &replayed[..],
        &data[usize::try_from(dropped_bytes).unwrap()..]
    );

    // Reconexão usando o cursor exato `info1.replay_from` (que ainda está retido no ring)
    // não deve gerar truncamento nem Gap!
    let (tx2, rx2) = unbounded();
    let sink2 = Arc::new(ChannelSink { tx: tx2 });
    let info2 = manager
        .attach(summary.id, sink2, Some(info1.replay_from))
        .unwrap();
    assert!(!info2.truncated);

    let first_frame2 = decode_frame(rx2.recv_timeout(Duration::from_millis(200)).unwrap()).unwrap();
    assert_eq!(first_frame2.kind, FrameKind::Replay);

    // Cursor inválido no futuro -> pty.cursor_invalid
    let (tx3, _rx3) = unbounded();
    let bad_cursor = OutputCursor {
        next_seq: U64Decimal(999_999),
        byte_offset: U64Decimal(999_999_999),
    };
    let err = manager
        .attach(
            summary.id,
            Arc::new(ChannelSink { tx: tx3 }),
            Some(bad_cursor),
        )
        .unwrap_err();
    assert_eq!(err.code(), "pty.cursor_invalid");

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn output_tap_receives_live_bytes_once_even_detached_and_never_on_replay() {
    struct TestTap {
        bytes: Mutex<Vec<u8>>,
        seqs: Mutex<Vec<u64>>,
        exit_calls: AtomicUsize,
    }

    impl OutputTap for TestTap {
        fn on_output(&self, _pty_id: PtyId, seq: u64, bytes: &[u8]) {
            self.seqs.lock().push(seq);
            self.bytes.lock().extend_from_slice(bytes);
        }

        fn on_exit(&self, _pty_id: PtyId, _exit: &ExitInfo) {
            self.exit_calls.fetch_add(1, Ordering::SeqCst);
        }
    }

    let tmp = tempfile::tempdir().unwrap();
    let config = test_config(&tmp);
    let manager = PtyManager::new(config).unwrap();
    let events = manager.subscribe();

    let tap = Arc::new(TestTap {
        bytes: Mutex::new(Vec::new()),
        seqs: Mutex::new(Vec::new()),
        exit_calls: AtomicUsize::new(0),
    });
    let _tap_handle = manager.add_output_tap(tap.clone());

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec![
            "-c".to_string(),
            "stty raw -echo; printf 'tap-only-once-payload'".to_string(),
        ],
        login: false,
        ..SpawnSpec::default()
    };

    // Spawna e deixa concluir SEM nenhum OutputSink anexado
    let summary = manager.spawn(spec).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if let Ok(mt_pty::PtyEvent::Exited { pty_id, .. }) =
            events.recv_timeout(Duration::from_millis(50))
            && pty_id == summary.id
        {
            break;
        }
    }

    assert_eq!(&tap.bytes.lock()[..], b"tap-only-once-payload");
    assert_eq!(tap.exit_calls.load(Ordering::SeqCst), 1);
    let seq_count_before_replay = tap.seqs.lock().len();
    assert!(seq_count_before_replay >= 1);

    // Anexa agora para consumir o Replay: o OutputTap NÃO deve ser chamado novamente (CA-33)
    let (tx, rx) = unbounded();
    let sink = Arc::new(ChannelSink { tx });
    let _attach = manager.attach(summary.id, sink, None).unwrap();

    let mut replayed = Vec::new();
    while let Ok(raw) = rx.recv_timeout(Duration::from_millis(200)) {
        let decoded = decode_frame(raw).unwrap();
        if decoded.kind == FrameKind::Replay {
            replayed.extend_from_slice(&decoded.payload);
        } else if decoded.kind == FrameKind::End {
            break;
        }
    }

    assert_eq!(replayed, b"tap-only-once-payload");
    assert_eq!(tap.seqs.lock().len(), seq_count_before_replay);
    assert_eq!(tap.exit_calls.load(Ordering::SeqCst), 1);

    let _ = manager.shutdown(Duration::from_secs(2));
}
