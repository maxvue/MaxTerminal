//! Fonte de telemetria derivada do transcript JSONL correlacionado do Claude Code (`ClaudeTranscriptSource`).

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::detect::current_unix_ms;
use crate::ports::RunBinding;
use crate::registry::AgentKind;
use crate::telemetry::{
    ContextUsage, FieldUpdate, JsonlTail, MeasurementQuality, ModelInfo, TelemetryContext,
    TelemetrySource, TelemetrySourceKind, TelemetryUpdate,
};

/// Fonte secundária que lê o transcript JSONL do Claude Code quando correlacionado pelo relay.
pub struct ClaudeTranscriptSource;

impl ClaudeTranscriptSource {
    /// Analisa um lote de registros JSONL do transcript do Claude Code e produz `Option<TelemetryUpdate>`.
    #[must_use]
    pub fn parse_records(
        binding: &RunBinding,
        records: &[Value],
        reset_detected: bool,
        observed_at_ms: i64,
    ) -> Option<TelemetryUpdate> {
        let mut model_update = FieldUpdate::Unchanged;
        let mut context_update = FieldUpdate::Unchanged;
        let mut invalidate_context = reset_detected;
        let mut any_change = reset_detected;

        for rec in records {
            let rec_type = rec.get("type").and_then(Value::as_str).unwrap_or_default();
            if rec_type == "summary"
                || rec_type == "compact_boundary"
                || rec.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
            {
                invalidate_context = true;
                context_update = FieldUpdate::Clear {
                    reason: Some("Contexto compactado na sessão".to_string()),
                    invalid: false,
                };
                any_change = true;
                continue;
            }

            let msg = rec.get("message").unwrap_or(rec);
            if let Some(model_str) = msg
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                model_update = FieldUpdate::Set {
                    value: ModelInfo {
                        id: Some(model_str.to_string()),
                        display_name: model_str.to_string(),
                    },
                    quality: MeasurementQuality::Derived,
                    expires_at_ms: None,
                };
                any_change = true;
            }

            let window_tokens = msg
                .get("context_window_size")
                .or_else(|| rec.get("context_window_size"))
                .and_then(Value::as_u64);

            if let Some(usage) = msg.get("usage").and_then(Value::as_object)
                && let Some(win) = window_tokens
                && win > 0
            {
                let input = usage
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let cache_create = usage
                    .get("cache_creation_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let cache_read = usage
                    .get("cache_read_input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let used = input + cache_create + cache_read;
                #[allow(clippy::cast_precision_loss)]
                let pct = ((used as f64 / win as f64) * 100.0).clamp(0.0, 100.0) as f32;

                context_update = FieldUpdate::Set {
                    value: ContextUsage {
                        used_tokens: Some(used),
                        window_tokens: Some(win),
                        percent: pct,
                    },
                    quality: MeasurementQuality::Derived,
                    expires_at_ms: None,
                };
                any_change = true;
            }
        }

        if !any_change {
            return None;
        }

        Some(TelemetryUpdate {
            binding: binding.clone(),
            source: TelemetrySourceKind::ClaudeTranscript,
            model: model_update,
            context: context_update,
            quota_5h: FieldUpdate::Unchanged,
            quota_weekly: FieldUpdate::Unchanged,
            cost_usd: FieldUpdate::Unchanged,
            session_ref: None,
            invalidate_previous_context: invalidate_context,
            observed_at_ms,
        })
    }
}

#[async_trait]
impl TelemetrySource for ClaudeTranscriptSource {
    fn kind(&self) -> TelemetrySourceKind {
        TelemetrySourceKind::ClaudeTranscript
    }

    fn supports(&self, agent: AgentKind) -> bool {
        agent == AgentKind::Claude
    }

    async fn run(
        &self,
        mut ctx: TelemetryContext,
        sink: mpsc::Sender<TelemetryUpdate>,
        cancel: CancellationToken,
    ) {
        let mut current_tail: Option<JsonlTail> = None;
        let mut current_path = None;
        let mut interval = tokio::time::interval(Duration::from_millis(500));

        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                _ = ctx.session_hint.changed() => {
                    let hint = ctx.session_hint.borrow().clone();
                    if let Some(h) = hint
                        && let Some(p) = h.transcript_path
                        && current_path.as_ref() != Some(&p)
                        && let Ok(tail) = JsonlTail::open_from_end(p.clone()).await
                    {
                        current_path = Some(p);
                        current_tail = Some(tail);
                    }
                }
                _ = interval.tick() => {
                    if let Some(ref mut tail) = current_tail
                        && let Ok(batch) = tail.read_new().await
                        && let Some(update) = Self::parse_records(
                            &ctx.binding,
                            &batch.records,
                            batch.reset_detected,
                            current_unix_ms(),
                        )
                    {
                        let _ = sink.send(update).await;
                    }
                }
            }
        }
    }
}
