//! Testes de integração de telemetria (`TelemetryAggregator`, `JsonlTail`, parsers Claude e Codex).

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
use std::sync::atomic::{AtomicUsize, Ordering};

use mt_agents::ports::RunBinding;
use mt_agents::registry::AgentKind;
use mt_agents::settings::AgentsSettings;
use mt_agents::telemetry::aggregator::{TelemetryAggregator, TelemetrySink};
use mt_agents::telemetry::claude_statusline::ClaudeStatusLineParser;
use mt_agents::telemetry::claude_transcript::ClaudeTranscriptSource;
use mt_agents::telemetry::codex_session::CodexSessionSource;
use mt_agents::telemetry::{
    AgentTelemetry, JsonlTail, MeasurementQuality, MetricAvailability, MetricKey,
    TelemetrySourceKind,
};
use mt_core::{PaneId, ProjectId, PtyId, SessionId, TabId};
use parking_lot::RwLock;

struct CountingSink {
    emissions: AtomicUsize,
}

impl CountingSink {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            emissions: AtomicUsize::new(0),
        })
    }

    fn count(&self) -> usize {
        self.emissions.load(Ordering::SeqCst)
    }
}

impl TelemetrySink for CountingSink {
    fn emit_telemetry(&self, _telemetry: &AgentTelemetry) {
        self.emissions.fetch_add(1, Ordering::SeqCst);
    }
}

fn sample_binding(agent: AgentKind) -> RunBinding {
    RunBinding {
        tab_id: TabId::new(),
        pane_id: PaneId::new(),
        pty_id: PtyId::new(),
        session_id: SessionId::new(),
        generation: 1,
        project_id: ProjectId::new(),
        run_id: "run-001".to_string(),
        agent,
        cwd: PathBuf::from("/tmp"),
        started_at_ms: 1_700_000_000_000,
    }
}

#[tokio::test]
async fn claude_statusline_overrides_transcript_and_clears_on_null() {
    let sink = CountingSink::new();
    let settings = Arc::new(RwLock::new(AgentsSettings::default()));
    let agg = TelemetryAggregator::new(settings, sink.clone(), None);

    let binding = sample_binding(AgentKind::Claude);
    let now = 1_700_000_000_000;
    agg.register_run(binding.clone(), Some("claude-sonnet-4-6".to_string()), now);

    // Inicialmente o modelo é Configured
    let snap0 = agg.snapshot(binding.tab_id, now).unwrap();
    assert_eq!(
        snap0.model.as_ref().unwrap().quality,
        MeasurementQuality::Configured
    );

    // 1. Transcript (Derived) chega primeiro: 50% de contexto
    let transcript_raw = include_str!("fixtures/claude/transcript_sample.jsonl");
    let records: Vec<serde_json::Value> = transcript_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    let t_update =
        ClaudeTranscriptSource::parse_records(&binding, &records, false, now + 100).unwrap();
    let snap1 = agg.apply(t_update, now + 100).unwrap();
    assert_eq!(
        snap1.context.as_ref().unwrap().source,
        TelemetrySourceKind::ClaudeTranscript
    );
    assert!((snap1.context.as_ref().unwrap().value.percent - 50.0).abs() < 0.01);

    // 2. StatusLine (Reported) chega em seguida: 42.5% vence Transcript (prioridade 40 > 30)
    let sl_raw: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/claude/statusline.json")).unwrap();
    let (sl_update, hint) = ClaudeStatusLineParser::parse(&binding, &sl_raw, now + 200);
    assert_eq!(hint.unwrap().session_id, "sess-claude-001");
    let snap2 = agg.apply(sl_update, now + 200).unwrap();
    assert_eq!(
        snap2.context.as_ref().unwrap().source,
        TelemetrySourceKind::ClaudeStatusLine
    );
    assert!((snap2.context.as_ref().unwrap().value.percent - 42.5).abs() < 0.01);
    assert!((snap2.quota_5h.as_ref().unwrap().value.used_percent - 18.0).abs() < 0.01);

    // 3. StatusLine envia rate_limits: null -> limpa quota_5h da fonte ClaudeStatusLine (CA-34)
    let sl_null = serde_json::json!({
        "model": { "id": "claude-opus-4-6", "display_name": "Claude Opus 4.6" },
        "context_window": { "used_percentage": null },
        "rate_limits": null
    });
    let (sl_clear, _) = ClaudeStatusLineParser::parse(&binding, &sl_null, now + 300);
    let snap3 = agg.apply(sl_clear, now + 300).unwrap();
    assert!(snap3.quota_5h.is_none());
    // Como StatusLine limpou seu candidato de context, o candidato do Transcript (50%) volta a valer
    assert_eq!(
        snap3.context.as_ref().unwrap().source,
        TelemetrySourceKind::ClaudeTranscript
    );

    // 4. Compactação no transcript invalida todos os candidatos de contexto anteriores
    let compact_rec = vec![serde_json::json!({"type": "compact_boundary"})];
    let compact_upd =
        ClaudeTranscriptSource::parse_records(&binding, &compact_rec, false, now + 400).unwrap();
    let snap4 = agg.apply(compact_upd, now + 400).unwrap();
    assert!(snap4.context.is_none());
}

#[tokio::test]
async fn stale_marking_and_1hz_throttle_work() {
    let sink = CountingSink::new();
    let settings = Arc::new(RwLock::new(AgentsSettings::default()));
    let agg = TelemetryAggregator::new(settings, sink.clone(), None);

    let binding = sample_binding(AgentKind::Claude);
    let t0 = 1_700_000_000_000;
    agg.register_run(binding.clone(), None, t0);
    let base_emissions = sink.count();

    let sl_raw: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/claude/statusline.json")).unwrap();

    // Envia 20 atualizações dentro de 200ms: throttle trailing deve emitir no máximo 1 vez síncrona
    for i in 0..20 {
        let (upd, _) = ClaudeStatusLineParser::parse(&binding, &sl_raw, t0 + 1000 + i * 5);
        agg.apply(upd, t0 + 1000 + i * 5);
    }
    assert_eq!(sink.count(), base_emissions + 1);

    // Após 65s sem novas observações (> stale_after_s = 60s), availability passa para Stale mantendo último valor
    let snap_stale = agg.snapshot(binding.tab_id, t0 + 66_500).unwrap();
    assert_eq!(
        snap_stale
            .availability
            .get(&MetricKey::Model)
            .unwrap()
            .availability,
        MetricAvailability::Stale
    );
    assert!(snap_stale.model.is_some());
}

#[tokio::test]
async fn jsonl_tail_handles_partial_invalid_and_truncation() {
    let dir = tempfile::tempdir().unwrap();
    let file_path = dir.path().join("session.jsonl");

    // Escreve linha inicial válida + linha malformada
    tokio::fs::write(&file_path, b"{\"ok\":1}\n{invalid json line\n{\"ok\":2}")
        .await
        .unwrap();

    let mut tail = JsonlTail::open_from_end(file_path.clone()).await.unwrap();
    let batch1 = tail.read_new().await.unwrap();
    // {"ok":1} foi lido, {invalid json line descartado, {"ok":2} ainda sem '\n' fica em partial
    assert_eq!(batch1.records.len(), 1);
    assert_eq!(batch1.records[0]["ok"], 1);
    assert!(!batch1.reset_detected);

    // Completa a linha parcial
    use tokio::io::AsyncWriteExt;
    let mut f = tokio::fs::OpenOptions::new()
        .append(true)
        .open(&file_path)
        .await
        .unwrap();
    f.write_all(b"\n{\"ok\":3}\n").await.unwrap();
    f.flush().await.unwrap();

    let batch2 = tail.read_new().await.unwrap();
    assert_eq!(batch2.records.len(), 2);
    assert_eq!(batch2.records[0]["ok"], 2);
    assert_eq!(batch2.records[1]["ok"], 3);

    // Trunca o arquivo para tamanho menor -> reset_detected = true (CA-34)
    tokio::fs::write(&file_path, b"{\"reset\":true}\n")
        .await
        .unwrap();
    let batch3 = tail.read_new().await.unwrap();
    assert!(batch3.reset_detected);
    assert_eq!(batch3.records.len(), 1);
    assert_eq!(batch3.records[0]["reset"], true);
}

#[test]
fn codex_session_correlates_by_thread_id() {
    let binding = sample_binding(AgentKind::Codex);
    let raw = include_str!("fixtures/codex/rollout_sample.jsonl");
    let records: Vec<serde_json::Value> = raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    // Thread ID divergente ignora o registro
    let ignored = CodexSessionSource::parse_records(
        &binding,
        Some("other-thread"),
        &records,
        false,
        1_700_000_000_000,
    );
    assert!(ignored.is_none());

    // Thread ID correlacionado extrai modelo, contexto e janelas 300/10080 min
    let matched = CodexSessionSource::parse_records(
        &binding,
        Some("thread-codex-001"),
        &records,
        false,
        1_700_000_000_000,
    )
    .unwrap();
    assert!(matches!(
        matched.context,
        mt_agents::telemetry::FieldUpdate::Set { .. }
    ));
    assert!(matches!(
        matched.quota_5h,
        mt_agents::telemetry::FieldUpdate::Set { .. }
    ));
    assert!(matches!(
        matched.quota_weekly,
        mt_agents::telemetry::FieldUpdate::Set { .. }
    ));
}
