//! Testes do `CommandTracker` cobrindo ciclos nominais, comandos sem `A`/`B`,
//! fechamento implícito, múltiplos comandos no mesmo chunk, `Ctrl+C` (130), sinais e nonces inválidos.

#![allow(clippy::unwrap_used)]

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use mt_core::ids::{PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    BlockEndReason, BlockStatus, ContinuousPolicy, IntegrationMode, ObservationConfidence,
    RuntimeBinding, ShellKind,
};
use mt_db::sanitizer::SanitizedText;
use mt_shell::{CommandTracker, OscScanner, TrackerEvent};

fn new_tracker() -> CommandTracker {
    CommandTracker::new(
        RuntimeBinding {
            pty_id: PtyId::new(),
            session_id: SessionId::new(),
            tab_id: Some(TabId::new()),
            pane_id: Some(PaneId::new()),
        },
        ShellKind::Zsh,
        Some("nonce-valido".to_string()),
        IntegrationMode::Pending,
        ContinuousPolicy::Auto,
        Some(SanitizedText::new("/home/dev")),
    )
}

#[test]
fn separates_multiple_commands_and_outputs_within_single_chunk() {
    let mut scanner = OscScanner::new();
    let mut tracker = new_tracker();

    let cmd1 = STANDARD.encode("echo primeiro");
    let cmd2 = STANDARD.encode("echo segundo && exit 2");
    let cwd = STANDARD.encode("/home/dev");

    // Dois comandos completos dentro do MESMO chunk de bytes!
    let chunk = format!(
        "lixo_antes\x1b]133;A;nonce=nonce-valido\x07\x1b]133;B;nonce=nonce-valido\x07\x1b]133;C;nonce=nonce-valido;cmd_b64={cmd1};cwd_b64={cwd}\x07primeiro\r\n\x1b]133;D;0;nonce=nonce-valido;cwd_b64={cwd}\x07\x1b]133;A;nonce=nonce-valido\x07\x1b]133;B;nonce=nonce-valido\x07\x1b]133;C;nonce=nonce-valido;cmd_b64={cmd2};cwd_b64={cwd}\x07erro_segundo\r\n\x1b]133;D;2;nonce=nonce-valido;cwd_b64={cwd}\x07prompt_final"
    );

    let scan = scanner.scan_chunk(1, chunk.as_bytes());
    let events = tracker.process_scan_output(&scan, 2_000);

    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            TrackerEvent::BlockFinished {
                finished, output, ..
            } => Some((finished, output)),
            _ => None,
        })
        .collect();

    assert_eq!(finished.len(), 2);

    // Bloco 1 recebeu apenas "primeiro\n" (sem "lixo_antes" e sem "erro_segundo")
    assert_eq!(finished[0].0.command_seq, 1);
    assert_eq!(finished[0].0.status, BlockStatus::Succeeded);
    assert!(!finished[0].0.inspectable);
    assert_eq!(
        finished[0].1.output_text.as_ref().unwrap().as_str(),
        "primeiro\n"
    );

    // Bloco 2 recebeu apenas "erro_segundo\n" e é inspecionável (exit 2)
    assert_eq!(finished[1].0.command_seq, 2);
    assert_eq!(finished[1].0.status, BlockStatus::Failed);
    assert_eq!(finished[1].0.exit_code, Some(2));
    assert!(finished[1].0.inspectable);
    assert_eq!(
        finished[1].1.output_text.as_ref().unwrap().as_str(),
        "erro_segundo\n"
    );
}

#[test]
fn handles_synthetic_c_without_a_b_and_implicit_close_on_next_a() {
    let mut scanner = OscScanner::new();
    let mut tracker = new_tracker();
    let cmd = STANDARD.encode("ls");
    let cwd = STANDARD.encode("/tmp");

    // C chega sem A/B anterior, e depois A chega sem D
    let chunk = format!(
        "\x1b]133;C;nonce=nonce-valido;cmd_b64={cmd};cwd_b64={cwd}\x07arq1\r\n\x1b]133;A;nonce=nonce-valido\x07"
    );
    let scan = scanner.scan_chunk(1, chunk.as_bytes());
    let events = tracker.process_scan_output(&scan, 3_000);

    let finished = events
        .iter()
        .find_map(|e| match e {
            TrackerEvent::BlockFinished { finished, .. } => Some(finished),
            _ => None,
        })
        .unwrap();

    assert_eq!(finished.confidence, ObservationConfidence::Degraded);
    assert_eq!(finished.end_reason, BlockEndReason::ImplicitNextPrompt);
    assert_eq!(finished.exit_code, None);
    assert!(!finished.inspectable);
}

#[test]
fn classifies_ctrl_c_130_and_sigkill_137_correctly() {
    let mut scanner = OscScanner::new();
    let mut tracker = new_tracker();
    let cmd = STANDARD.encode("sleep 60");
    let cwd = STANDARD.encode("/tmp");

    let ctrl_c_chunk = format!(
        "\x1b]133;A;nonce=nonce-valido\x07\x1b]133;B;nonce=nonce-valido\x07\x1b]133;C;nonce=nonce-valido;cmd_b64={cmd};cwd_b64={cwd}\x07^C\r\n\x1b]133;D;130;nonce=nonce-valido;cwd_b64={cwd}\x07"
    );
    let evs1 = tracker.process_scan_output(&scanner.scan_chunk(1, ctrl_c_chunk.as_bytes()), 1000);
    let fin1 = evs1
        .iter()
        .find_map(|e| match e {
            TrackerEvent::BlockFinished { finished, .. } => Some(finished),
            _ => None,
        })
        .unwrap();
    assert_eq!(fin1.status, BlockStatus::Cancelled);
    assert_eq!(fin1.end_reason, BlockEndReason::CtrlC);
    assert_eq!(fin1.signal.as_deref(), Some("SIGINT"));
    assert!(!fin1.inspectable);

    let sigkill_chunk = format!(
        "\x1b]133;A;nonce=nonce-valido\x07\x1b]133;B;nonce=nonce-valido\x07\x1b]133;C;nonce=nonce-valido;cmd_b64={cmd};cwd_b64={cwd}\x07Killed\r\n\x1b]133;D;137;nonce=nonce-valido;cwd_b64={cwd}\x07"
    );
    let evs2 = tracker.process_scan_output(&scanner.scan_chunk(2, sigkill_chunk.as_bytes()), 2000);
    let fin2 = evs2
        .iter()
        .find_map(|e| match e {
            TrackerEvent::BlockFinished { finished, .. } => Some(finished),
            _ => None,
        })
        .unwrap();
    assert_eq!(fin2.status, BlockStatus::TerminatedBySignal);
    assert_eq!(fin2.end_reason, BlockEndReason::KilledBySignal);
    assert_eq!(fin2.signal.as_deref(), Some("SIGKILL"));
    assert!(fin2.inspectable);
}
