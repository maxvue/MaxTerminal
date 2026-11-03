//! Testes de integração do Detector de Atenção Correlacionada (`AttentionDetector`, `PatternSet`, `TailBuffer`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;
use mt_agents::attention::{
    AckReason, AttentionDetector, AttentionSignal, AttentionSink, AttentionSnapshot,
    AttentionSource, AttentionState, PatternSet, parse_osc_and_bel,
};
use mt_agents::ports::{
    AttentionKind, ForegroundProc, NotifyPort, PtyPort, RunBinding, TabStatePort,
};
use mt_agents::registry::{AgentKind, AgentRegistry};
use mt_agents::settings::AttentionSettings;
use mt_core::{AppPaths, PaneId, ProjectId, PtyId, SessionId, TabId};
use parking_lot::{Mutex, RwLock};

#[derive(Default)]
struct MockTabStatePort {
    required_calls: Mutex<Vec<(String, u64)>>,
    resolved_calls: Mutex<Vec<(String, u64)>>,
}

impl TabStatePort for MockTabStatePort {
    fn attention_required(&self, run: &RunBinding, episode: u64) -> bool {
        self.required_calls
            .lock()
            .push((run.run_id.clone(), episode));
        true
    }

    fn attention_resolved(&self, run: &RunBinding, episode: u64) {
        self.resolved_calls
            .lock()
            .push((run.run_id.clone(), episode));
    }
}

struct MockNotifyPort {
    focused: AtomicBool,
    notify_count: AtomicUsize,
    last_body: Mutex<Option<String>>,
}

impl MockNotifyPort {
    fn new(focused: bool) -> Arc<Self> {
        Arc::new(Self {
            focused: AtomicBool::new(focused),
            notify_count: AtomicUsize::new(0),
            last_body: Mutex::new(None),
        })
    }
}

#[async_trait]
impl NotifyPort for MockNotifyPort {
    fn set_attention(&self, _tab: TabId, _kind: AttentionKind) {}
    fn clear_attention(&self, _tab: TabId) {}
    fn is_tab_focused_and_active(&self, _tab: TabId) -> bool {
        self.focused.load(Ordering::SeqCst)
    }
    async fn notify(&self, _tab: TabId, _title: &str, body: &str) {
        self.notify_count.fetch_add(1, Ordering::SeqCst);
        *self.last_body.lock() = Some(body.to_string());
    }
}

struct MockPtyPort;
impl PtyPort for MockPtyPort {
    fn foreground(&self, _pty: PtyId) -> Option<ForegroundProc> {
        Some(ForegroundProc {
            pid: 1234,
            comm: "claude".to_string(),
            exe: None,
            cmdline: vec!["claude".to_string()],
        })
    }

    fn has_active_processes(&self, _tab: TabId) -> bool {
        true
    }
}

#[derive(Default)]
struct RecordingAttentionSink {
    snapshots: Mutex<Vec<AttentionSnapshot>>,
}

impl AttentionSink for RecordingAttentionSink {
    fn emit(&self, snapshot: AttentionSnapshot) {
        self.snapshots.lock().push(snapshot);
    }
}

fn sample_run(agent: AgentKind) -> RunBinding {
    RunBinding {
        tab_id: TabId::new(),
        pane_id: PaneId::new(),
        pty_id: PtyId::new(),
        session_id: SessionId::new(),
        generation: 1,
        project_id: ProjectId::new(),
        run_id: "run-att-01".to_string(),
        agent,
        cwd: PathBuf::from("/tmp"),
        started_at_ms: 1_700_000_000_000,
    }
}

fn test_registry() -> Arc<AgentRegistry> {
    let dir = tempfile::tempdir().unwrap();
    let paths = AppPaths {
        config_dir: dir.path().join("cfg"),
        data_dir: dir.path().join("data"),
        state_dir: dir.path().join("state"),
        cache_dir: dir.path().join("cache"),
        runtime_dir: Some(dir.path().join("run")),
        log_dir: dir.path().join("logs"),
        db_path: dir.path().join("db.sqlite"),
    };
    let (reg, _) = AgentRegistry::load(&paths);
    Arc::new(reg)
}

#[tokio::test]
async fn permission_prompt_confirms_awaiting_and_notifies_once_per_episode() {
    let tab_state = Arc::new(MockTabStatePort::default());
    let notify = MockNotifyPort::new(false);
    let sink = Arc::new(RecordingAttentionSink::default());
    let detector = AttentionDetector::new(
        Arc::new(PatternSet::load_default()),
        test_registry(),
        tab_state.clone(),
        notify.clone(),
        Arc::new(MockPtyPort),
        sink,
        Arc::new(RwLock::new(AttentionSettings::default())),
    );

    let run = sample_run(AgentKind::Claude);
    let tab_id = run.tab_id;
    detector.register_run(run.clone(), 1000);

    // 1. auth_success nunca gera Awaiting
    detector
        .on_signal(
            tab_id,
            AttentionSignal::HookNotification {
                message: Some("Logged in".to_string()),
                notification_type: Some("auth_success".to_string()),
                subagent_id: None,
            },
            1100,
        )
        .await;
    assert!(matches!(
        detector.snapshot(tab_id).unwrap().state,
        AttentionState::Working
    ));
    assert_eq!(notify.notify_count.load(Ordering::SeqCst), 0);

    // 2. permission_prompt entra em AwaitingApproval (episode 1) e notifica 1 vez
    detector
        .on_signal(
            tab_id,
            AttentionSignal::HookNotification {
                message: Some("Run rm -rf?".to_string()),
                notification_type: Some("permission_prompt".to_string()),
                subagent_id: None,
            },
            1200,
        )
        .await;
    let snap = detector.snapshot(tab_id).unwrap();
    assert!(matches!(
        snap.state,
        AttentionState::Awaiting {
            kind: AttentionKind::AwaitingApproval,
            episode: 1,
            source: AttentionSource::NativeHook,
            ..
        }
    ));
    assert_eq!(notify.notify_count.load(Ordering::SeqCst), 1);
    // Texto da notificação nunca vaza o prompt bruto ("Run rm -rf?")
    assert!(!notify.last_body.lock().as_ref().unwrap().contains("rm -rf"));

    // Repetição do mesmo sinal para o mesmo episódio não incrementa episode nem reenvia notificação
    detector
        .on_signal(
            tab_id,
            AttentionSignal::HookNotification {
                message: Some("Run rm -rf?".to_string()),
                notification_type: Some("permission_prompt".to_string()),
                subagent_id: None,
            },
            1250,
        )
        .await;
    assert_eq!(notify.notify_count.load(Ordering::SeqCst), 1);

    // 3. UserInput ou ack apenas reconhece visualização (acknowledged = true), mas mantém Awaiting (CA-24)
    detector
        .on_signal(tab_id, AttentionSignal::UserInput, 1300)
        .await;
    let snap_ack = detector.snapshot(tab_id).unwrap();
    assert!(snap_ack.acknowledged);
    assert!(matches!(snap_ack.state, AttentionState::Awaiting { .. }));

    // 4. PreToolUse não resolve AwaitingApproval
    detector
        .on_signal(tab_id, AttentionSignal::HookPreToolUse, 1350)
        .await;
    assert!(matches!(
        detector.snapshot(tab_id).unwrap().state,
        AttentionState::Awaiting { .. }
    ));

    // 5. HookStop de subagente não resolve a conversa principal
    detector
        .on_signal(
            tab_id,
            AttentionSignal::HookStop { is_subagent: true },
            1380,
        )
        .await;
    assert!(matches!(
        detector.snapshot(tab_id).unwrap().state,
        AttentionState::Awaiting { .. }
    ));

    // 6. HookStop da conversa raiz resolve o episódio 1
    detector
        .on_signal(
            tab_id,
            AttentionSignal::HookStop { is_subagent: false },
            1400,
        )
        .await;
    assert!(matches!(
        detector.snapshot(tab_id).unwrap().state,
        AttentionState::Working
    ));
    assert_eq!(
        tab_state.resolved_calls.lock().as_slice(),
        &[("run-att-01".to_string(), 1)]
    );
}

#[tokio::test]
async fn stop_and_codex_turn_complete_never_trigger_error_awaiting() {
    let tab_state = Arc::new(MockTabStatePort::default());
    let notify = MockNotifyPort::new(false);
    let sink = Arc::new(RecordingAttentionSink::default());
    let detector = AttentionDetector::new(
        Arc::new(PatternSet::load_default()),
        test_registry(),
        tab_state.clone(),
        notify,
        Arc::new(MockPtyPort),
        sink,
        Arc::new(RwLock::new(AttentionSettings::default())),
    );

    let run = sample_run(AgentKind::Codex);
    let tab_id = run.tab_id;
    detector.register_run(run, 1000);

    detector
        .on_signal(
            tab_id,
            AttentionSignal::CodexTurnComplete {
                last_message: Some("Done".to_string()),
            },
            1500,
        )
        .await;

    assert!(matches!(
        detector.snapshot(tab_id).unwrap().state,
        AttentionState::Working
    ));
    assert!(tab_state.required_calls.lock().is_empty());
}

#[tokio::test]
async fn two_anchor_patterns_and_quoted_line_filtering_work() {
    let pset = PatternSet::load_default();
    let positive = include_str!("fixtures/attention/positive_prompts.txt");
    let negative = include_str!("fixtures/attention/negative_outputs.txt");

    // Positivos casam para Claude e Codex
    let claude_match = pset.match_tail(AgentKind::Claude, positive);
    assert_eq!(
        claude_match,
        Some((
            "claude.permission".to_string(),
            AttentionKind::AwaitingApproval
        ))
    );
    let codex_match = pset.match_tail(AgentKind::Codex, positive);
    assert_eq!(
        codex_match,
        Some((
            "codex.approval".to_string(),
            AttentionKind::AwaitingApproval
        ))
    );

    // Negativos (dentro de ``` ou diffs/citações `>`, `+`) são filtrados e nunca casam (CA-22)
    assert!(pset.match_tail(AgentKind::Claude, negative).is_none());
    assert!(pset.match_tail(AgentKind::Codex, negative).is_none());

    // Avaliado após silence_ms no detector -> produz Suspected (nunca Awaiting)
    let tab_state = Arc::new(MockTabStatePort::default());
    let notify = MockNotifyPort::new(false);
    let sink = Arc::new(RecordingAttentionSink::default());
    let detector = AttentionDetector::new(
        Arc::new(pset),
        test_registry(),
        tab_state.clone(),
        notify,
        Arc::new(MockPtyPort),
        sink,
        Arc::new(RwLock::new(AttentionSettings::default())),
    );

    let run = sample_run(AgentKind::Claude);
    let tab_id = run.tab_id;
    detector.register_run(run.clone(), 1000);

    detector
        .on_signal(
            tab_id,
            AttentionSignal::OutputBytes {
                bytes: positive.as_bytes().to_vec(),
            },
            1100,
        )
        .await;

    // Antes de silence_ms (300ms): ainda Working
    detector.on_silence_tick(1200);
    assert!(matches!(
        detector.snapshot(tab_id).unwrap().state,
        AttentionState::Working
    ));

    // Após 300ms de silêncio: Suspected com pattern_id = claude.permission
    detector.on_silence_tick(1450);
    let snap = detector.snapshot(tab_id).unwrap();
    assert!(matches!(
        snap.state,
        AttentionState::Suspected {
            source: AttentionSource::Pattern,
            ..
        }
    ));
    // Suspected não aciona TabStatePort::attention_required
    assert!(tab_state.required_calls.lock().is_empty());

    // Ack manual funciona
    let acked = detector
        .ack(tab_id, &run.run_id, 0, AckReason::Dismissed)
        .unwrap();
    assert!(acked.acknowledged);
}

#[test]
fn osc_and_bel_parser_distinguishes_osc_terminator_from_standalone_bel() {
    // BEL terminador de OSC 9 não conta como standalone_bel, mas extrai a mensagem OSC 9
    let (osc, bel) = parse_osc_and_bel(b"\x1b]9;Approval needed\x07normal text");
    assert_eq!(osc, vec!["Approval needed".to_string()]);
    assert!(!bel);

    // BEL isolado fora de OSC
    let (osc2, bel2) = parse_osc_and_bel(b"Waiting for input\x07");
    assert!(osc2.is_empty());
    assert!(bel2);
}
