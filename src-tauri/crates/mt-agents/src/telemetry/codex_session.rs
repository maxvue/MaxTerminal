//! Fonte de telemetria de sessão do Codex (`CodexSessionSource`) correlacionada por `thread-id`.

use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::detect::current_unix_ms;
use crate::ports::RunBinding;
use crate::registry::AgentKind;
use crate::telemetry::{
    ContextUsage, FieldUpdate, JsonlTail, MeasurementQuality, ModelInfo, QuotaWindow,
    TelemetryContext, TelemetrySource, TelemetrySourceKind, TelemetryUpdate,
};

/// Fonte que lê arquivos de rollout do Codex correlacionados pelo `thread-id`.
pub struct CodexSessionSource;

impl CodexSessionSource {
    /// Analisa registros JSONL de rollout do Codex.
    #[must_use]
    pub fn parse_records(
        binding: &RunBinding,
        expected_thread_id: Option<&str>,
        records: &[Value],
        reset_detected: bool,
        observed_at_ms: i64,
    ) -> Option<TelemetryUpdate> {
        let mut model_update = FieldUpdate::Unchanged;
        let mut context_update = FieldUpdate::Unchanged;
        let mut quota_5h = FieldUpdate::Unchanged;
        let mut quota_weekly = FieldUpdate::Unchanged;
        let mut any_change = reset_detected;

        for rec in records {
            if let Some(exp_tid) = expected_thread_id
                && let Some(rec_tid) = rec
                    .get("thread_id")
                    .or_else(|| rec.get("threadId"))
                    .or_else(|| rec.get("session_id"))
                    .and_then(Value::as_str)
                && rec_tid != exp_tid
            {
                // Ignora registros de outro thread_id
                continue;
            }

            if let Some(m) = rec
                .get("model")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                model_update = FieldUpdate::Set {
                    value: ModelInfo {
                        id: Some(m.to_string()),
                        display_name: m.to_string(),
                    },
                    quality: MeasurementQuality::Derived,
                    expires_at_ms: None,
                };
                any_change = true;
            }

            let payload = rec.get("payload").unwrap_or(rec);
            if let Some(info) = payload
                .get("info")
                .or_else(|| payload.get("token_usage"))
                .or_else(|| payload.get("token_count"))
            {
                let used = info
                    .get("total_tokens")
                    .or_else(|| info.get("input_tokens"))
                    .or_else(|| info.get("used_tokens"))
                    .and_then(Value::as_u64);
                let win = info
                    .get("model_context_window")
                    .or_else(|| info.get("context_window"))
                    .or_else(|| info.get("window_tokens"))
                    .and_then(Value::as_u64);

                if let (Some(u), Some(w)) = (used, win)
                    && w > 0
                {
                    #[allow(clippy::cast_precision_loss)]
                    let pct = ((u as f64 / w as f64) * 100.0).clamp(0.0, 100.0) as f32;
                    context_update = FieldUpdate::Set {
                        value: ContextUsage {
                            used_tokens: Some(u),
                            window_tokens: Some(w),
                            percent: pct,
                        },
                        quality: MeasurementQuality::Derived,
                        expires_at_ms: None,
                    };
                    any_change = true;
                }
            }

            let rl_items: Vec<&Value> = match payload.get("rate_limits") {
                Some(Value::Object(map)) => map.values().collect(),
                Some(Value::Array(arr)) => arr.iter().collect(),
                _ => Vec::new(),
            };
            for win_val in rl_items {
                let Some(w_mins) = win_val
                    .get("window_minutes")
                    .and_then(Value::as_u64)
                    .and_then(|v| u32::try_from(v).ok())
                else {
                    continue;
                };
                let Some(pct_f64) = win_val.get("used_percent").and_then(Value::as_f64) else {
                    continue;
                };
                #[allow(clippy::cast_possible_truncation)]
                let pct = pct_f64 as f32;
                if !pct.is_finite() || !(0.0..=100.0).contains(&pct) {
                    continue;
                }
                let resets_at_ms =
                    win_val
                        .get("resets_at_ms")
                        .and_then(Value::as_i64)
                        .or_else(|| {
                            win_val
                                .get("resets_at")
                                .and_then(Value::as_i64)
                                .and_then(|s| s.checked_mul(1000))
                        });

                let qw = QuotaWindow {
                    used_percent: pct,
                    resets_at_ms,
                    window_minutes: w_mins,
                };
                if w_mins == 300 {
                    quota_5h = FieldUpdate::Set {
                        value: qw,
                        quality: MeasurementQuality::Derived,
                        expires_at_ms: resets_at_ms,
                    };
                    any_change = true;
                } else if w_mins == 10_080 {
                    quota_weekly = FieldUpdate::Set {
                        value: qw,
                        quality: MeasurementQuality::Derived,
                        expires_at_ms: resets_at_ms,
                    };
                    any_change = true;
                }
            }
        }

        if !any_change {
            return None;
        }

        Some(TelemetryUpdate {
            binding: binding.clone(),
            source: TelemetrySourceKind::CodexSession,
            model: model_update,
            context: context_update,
            quota_5h,
            quota_weekly,
            cost_usd: FieldUpdate::Unchanged,
            session_ref: expected_thread_id.map(str::to_string),
            invalidate_previous_context: reset_detected,
            observed_at_ms,
        })
    }
}

#[async_trait]
impl TelemetrySource for CodexSessionSource {
    fn kind(&self) -> TelemetrySourceKind {
        TelemetrySourceKind::CodexSession
    }

    fn supports(&self, agent: AgentKind) -> bool {
        agent == AgentKind::Codex
    }

    async fn run(
        &self,
        mut ctx: TelemetryContext,
        sink: mpsc::Sender<TelemetryUpdate>,
        cancel: CancellationToken,
    ) {
        let mut current_tail: Option<JsonlTail> = None;
        let mut current_thread_id: Option<String> = None;
        let mut interval = tokio::time::interval(Duration::from_millis(500));

        loop {
            tokio::select! {
                () = cancel.cancelled() => break,
                _ = ctx.session_hint.changed() => {
                    let hint = ctx.session_hint.borrow().clone();
                    if let Some(h) = hint {
                        current_thread_id = Some(h.session_id);
                        if let Some(p) = h.transcript_path
                            && let Ok(tail) = JsonlTail::open_from_end(p).await
                        {
                            current_tail = Some(tail);
                        }
                    }
                }
                _ = interval.tick() => {
                    if let Some(ref mut tail) = current_tail
                        && let Ok(batch) = tail.read_new().await
                        && let Some(update) = Self::parse_records(
                            &ctx.binding,
                            current_thread_id.as_deref(),
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
