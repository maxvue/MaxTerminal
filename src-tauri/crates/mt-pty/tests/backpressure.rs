//! Suíte de testes de integração de backpressure (`FlowControl`), expiração de lease (`ack_timeout`)
//! e limites da fila de escrita (`WriteQueue`) (Critérios CA-10, CA-11, CA-12, CA-13, CA-15).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use crossbeam_channel::unbounded;
use mt_pty::io::{FlowStatus, FrameKind, decode_end_payload, decode_frame};
use mt_pty::{KillMode, OutputSink, PtyConfig, PtyManager, SinkClosed, SpawnSpec};
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
fn pauses_reader_at_high_watermark_and_resumes_after_ack_without_loss() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.high_watermark = 64 * 1024; // 64 KiB
    config.low_watermark = 16 * 1024; // 16 KiB
    config.max_frame_bytes = 16 * 1024; // 16 KiB
    config.ack_timeout = Duration::from_secs(30);

    let manager = PtyManager::new(config).unwrap();

    let total_bytes = 256 * 1024;
    let payload: Vec<u8> = (0..total_bytes)
        .map(|i| u8::try_from(i % 251).unwrap())
        .collect();
    let expected_hash = Sha256::digest(&payload);
    let file_path = tmp.path().join("bp_256k.bin");
    fs::write(&file_path, &payload).unwrap();

    let (tx, rx) = unbounded();
    let sink = Arc::new(ChannelSink { tx });

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec![
            "-c".to_string(),
            format!("stty raw -echo; cat '{}'", file_path.display()),
        ],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    let attach = manager.attach(summary.id, sink, None).unwrap();
    let inst = manager.get_instance(summary.id).unwrap();

    // Aguarda até o controle de fluxo entrar em Paused por falta de ACKs
    let pause_deadline = Instant::now() + Duration::from_secs(5);
    while inst.output_state().flow().status() != FlowStatus::Paused
        && Instant::now() < pause_deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }

    assert_eq!(inst.output_state().flow().status(), FlowStatus::Paused);
    assert!(
        inst.output_state().flow().unacked_bytes() <= 64 * 1024,
        "Bytes em trânsito não podem exceder high_watermark (64 KiB)"
    );

    // Agora drena e envia ACKs para destravar o produtor até o fim dos 256 KiB
    let mut received = Vec::with_capacity(total_bytes);
    let mut consumed = 0u64;
    let mut expected_seq = 1u64;
    let finish_deadline = Instant::now() + Duration::from_secs(10);

    while Instant::now() < finish_deadline {
        if let Ok(raw) = rx.recv_timeout(Duration::from_millis(50)) {
            let decoded = decode_frame(raw).unwrap();
            match decoded.kind {
                FrameKind::Data | FrameKind::Replay => {
                    assert_eq!(decoded.seq, expected_seq);
                    expected_seq += 1;
                    consumed += u64::try_from(decoded.payload.len()).unwrap();
                    received.extend_from_slice(&decoded.payload);
                    manager
                        .ack(summary.id, attach.attachment_id, consumed)
                        .unwrap();
                }
                FrameKind::Gap => panic!("Gap inesperado"),
                FrameKind::End => {
                    let exit = decode_end_payload(&decoded).unwrap();
                    assert_eq!(exit.code, Some(0));
                    break;
                }
            }
        }
    }

    assert_eq!(received.len(), total_bytes);
    assert_eq!(Sha256::digest(&received), expected_hash);

    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn rejects_invalid_ack_and_stale_attachment_and_auto_detaches_on_timeout() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.ack_timeout = Duration::from_millis(180);
    config.supervisor_interval = Duration::from_millis(30);

    let manager = PtyManager::new(config).unwrap();

    let (tx1, rx1) = unbounded();
    let sink1 = Arc::new(ChannelSink { tx: tx1 });

    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), "printf 'chunk1'; sleep 5".to_string()],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();
    let att1 = manager.attach(summary.id, sink1, None).unwrap();

    // Aguarda receber o primeiro frame para que haja bytes em trânsito (sent_bytes > consumed_bytes)
    let first_raw = rx1.recv_timeout(Duration::from_secs(3)).unwrap();
    let first_dec = decode_frame(first_raw).unwrap();
    assert!(!first_dec.payload.is_empty());

    // ACK acima de sent_bytes -> pty.ack_invalid
    let err_invalid = manager
        .ack(summary.id, att1.attachment_id, 999_999)
        .unwrap_err();
    assert_eq!(err_invalid.code(), "pty.ack_invalid");

    // Não envia ACK válido e aguarda ack_timeout expirar pelo supervisor
    let timeout_deadline = Instant::now() + Duration::from_secs(3);
    while manager.get(summary.id).unwrap().attached && Instant::now() < timeout_deadline {
        std::thread::sleep(Duration::from_millis(30));
    }
    assert!(
        !manager.get(summary.id).unwrap().attached,
        "Conexão deveria ter sofrido auto-detach após ack_timeout"
    );

    // ACK na geração expirada -> pty.attachment_stale
    assert_eq!(
        manager
            .ack(summary.id, att1.attachment_id, 1)
            .unwrap_err()
            .code(),
        "pty.attachment_stale"
    );

    // Novo attach substitui geração e recebe replay
    let (tx2, _rx2) = unbounded();
    let sink2 = Arc::new(ChannelSink { tx: tx2 });
    let _att2 = manager.attach(summary.id, sink2, None).unwrap();
    assert!(manager.get(summary.id).unwrap().attached);

    let _ = manager.kill(summary.id, KillMode::Force);
    let _ = manager.shutdown(Duration::from_secs(2));
}

#[test]
fn write_queue_handles_chunked_writes_and_returns_write_queue_full_when_slave_stalls() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.write_queue_bytes = 64 * 1024; // 64 KiB
    config.max_write_bytes = 32 * 1024; // 32 KiB

    let manager = PtyManager::new(config).unwrap();

    // Processo que nunca lê do stdin
    let spec = SpawnSpec {
        shell: Some("/bin/sh".to_string()),
        args: vec!["-c".to_string(), "sleep 30".to_string()],
        login: false,
        ..SpawnSpec::default()
    };

    let summary = manager.spawn(spec).unwrap();

    // Pedido individual maior que max_write_bytes -> pty.input_invalid
    let too_large = vec![b'X'; 32 * 1024 + 1];
    assert_eq!(
        manager.write(summary.id, &too_large).unwrap_err().code(),
        "pty.input_invalid"
    );

    // Enche o buffer do kernel do PTY + WriteQueue de 64 KiB até retornar pty.write_queue_full
    let chunk = vec![b'A'; 16 * 1024];
    let mut saw_queue_full = false;
    for _ in 0..64 {
        match manager.write(summary.id, &chunk) {
            Ok(_) => {}
            Err(e) => {
                assert_eq!(e.code(), "pty.write_queue_full");
                saw_queue_full = true;
                break;
            }
        }
    }
    assert!(
        saw_queue_full,
        "Deveria retornar pty.write_queue_full quando o slave não drena o stdin"
    );

    // Kill deve cancelar imediatamente a escrita bloqueada sem travar
    let kill_start = Instant::now();
    let _ = manager.kill(summary.id, KillMode::Force).unwrap();
    assert!(kill_start.elapsed() < Duration::from_secs(2));

    let _ = manager.shutdown(Duration::from_secs(2));
}
