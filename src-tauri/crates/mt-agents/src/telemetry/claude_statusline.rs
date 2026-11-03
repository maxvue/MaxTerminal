//! Parser estruturado do payload JSON da `statusLine` do Claude Code (`ClaudeStatusLineParser`).

use std::path::PathBuf;

use serde_json::Value;

use crate::ports::RunBinding;
use crate::telemetry::{
    ContextUsage, FieldUpdate, MeasurementQuality, ModelInfo, QuotaWindow, SessionHint,
    TelemetrySourceKind, TelemetryUpdate,
};

/// Parser para mensagens `RelayMessage::Statusline` do Claude Code.
pub struct ClaudeStatusLineParser;

impl ClaudeStatusLineParser {
    /// Converte o payload JSON do `statusLine` em `TelemetryUpdate` e `Option<SessionHint>`.
    #[must_use]
    #[allow(clippy::too_many_lines)]
    pub fn parse(
        binding: &RunBinding,
        payload: &Value,
        observed_at_ms: i64,
    ) -> (TelemetryUpdate, Option<SessionHint>) {
        // 1. session_id & transcript_path
        let session_ref = payload
            .get("session_id")
            .or_else(|| payload.get("sessionId"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);

        let transcript_path = payload
            .get("transcript_path")
            .or_else(|| payload.get("transcriptPath"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);

        let hint = session_ref.as_ref().map(|sid| SessionHint {
            session_id: sid.clone(),
            transcript_path,
        });

        // 2. model (snapshot completo: ausente ou null -> Clear)
        let model = match payload.get("model") {
            Some(Value::Object(m)) => {
                let id = m
                    .get("id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string);
                let display_name = m
                    .get("display_name")
                    .or_else(|| m.get("displayName"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .or_else(|| id.clone());

                if let Some(dn) = display_name {
                    FieldUpdate::Set {
                        value: ModelInfo {
                            id,
                            display_name: dn,
                        },
                        quality: MeasurementQuality::Reported,
                        expires_at_ms: None,
                    }
                } else {
                    FieldUpdate::Clear {
                        reason: Some("Modelo ausente no payload statusLine".to_string()),
                        invalid: false,
                    }
                }
            }
            Some(Value::String(s)) if !s.trim().is_empty() => FieldUpdate::Set {
                value: ModelInfo {
                    id: Some(s.trim().to_string()),
                    display_name: s.trim().to_string(),
                },
                quality: MeasurementQuality::Reported,
                expires_at_ms: None,
            },
            _ => FieldUpdate::Clear {
                reason: Some("Campo model ausente ou nulo no statusLine".to_string()),
                invalid: false,
            },
        };

        // 3. context_window
        let context = match payload
            .get("context_window")
            .or_else(|| payload.get("contextWindow"))
        {
            Some(Value::Object(cw)) => {
                let window_tokens = cw
                    .get("context_window_size")
                    .or_else(|| cw.get("contextWindowSize"))
                    .and_then(Value::as_u64);

                let used_tokens = cw
                    .get("current_usage")
                    .or_else(|| cw.get("currentUsage"))
                    .and_then(Value::as_object)
                    .map(|u| {
                        let input = u.get("input_tokens").and_then(Value::as_u64).unwrap_or(0);
                        let cache_create = u
                            .get("cache_creation_input_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        let cache_read = u
                            .get("cache_read_input_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0);
                        input + cache_create + cache_read
                    })
                    .or_else(|| cw.get("used_tokens").and_then(Value::as_u64));

                let raw_pct = cw
                    .get("used_percentage")
                    .or_else(|| cw.get("usedPercentage"))
                    .and_then(Value::as_f64);

                match raw_pct {
                    Some(pct) => {
                        #[allow(clippy::cast_possible_truncation)]
                        let pct_f32 = pct as f32;
                        if pct_f32.is_finite() && (0.0..=100.0).contains(&pct_f32) {
                            FieldUpdate::Set {
                                value: ContextUsage {
                                    used_tokens,
                                    window_tokens,
                                    percent: pct_f32,
                                },
                                quality: MeasurementQuality::Reported,
                                expires_at_ms: None,
                            }
                        } else {
                            FieldUpdate::Clear {
                                reason: Some(
                                    "Percentual de contexto fora do intervalo 0..=100".to_string(),
                                ),
                                invalid: true,
                            }
                        }
                    }
                    None => FieldUpdate::Clear {
                        reason: Some("used_percentage nulo ou ausente".to_string()),
                        invalid: false,
                    },
                }
            }
            _ => FieldUpdate::Clear {
                reason: Some("context_window nulo ou ausente".to_string()),
                invalid: false,
            },
        };

        // 4. rate_limits (five_hour e seven_day)
        let rate_limits = payload
            .get("rate_limits")
            .or_else(|| payload.get("rateLimits"));

        let quota_5h = parse_quota_window(
            rate_limits.and_then(|r| r.get("five_hour").or_else(|| r.get("fiveHour"))),
            300,
        );
        let quota_weekly = parse_quota_window(
            rate_limits.and_then(|r| r.get("seven_day").or_else(|| r.get("sevenDay"))),
            10_080,
        );

        // 5. cost.total_cost_usd
        let cost_usd = match payload
            .get("cost")
            .and_then(|c| c.get("total_cost_usd").or_else(|| c.get("totalCostUsd")))
        {
            Some(Value::Number(num)) => {
                if let Some(v) = num.as_f64()
                    && v.is_finite()
                    && v >= 0.0
                {
                    FieldUpdate::Set {
                        value: v,
                        quality: MeasurementQuality::Reported,
                        expires_at_ms: None,
                    }
                } else {
                    FieldUpdate::Clear {
                        reason: Some("Valor de custo inválido".to_string()),
                        invalid: true,
                    }
                }
            }
            _ => FieldUpdate::Clear {
                reason: None,
                invalid: false,
            },
        };

        let update = TelemetryUpdate {
            binding: binding.clone(),
            source: TelemetrySourceKind::ClaudeStatusLine,
            model,
            context,
            quota_5h,
            quota_weekly,
            cost_usd,
            session_ref,
            invalidate_previous_context: false,
            observed_at_ms,
        };

        (update, hint)
    }
}

fn parse_quota_window(raw_win: Option<&Value>, default_minutes: u32) -> FieldUpdate<QuotaWindow> {
    let Some(Value::Object(obj)) = raw_win else {
        return FieldUpdate::Clear {
            reason: None,
            invalid: false,
        };
    };

    let Some(pct_f64) = obj
        .get("used_percentage")
        .or_else(|| obj.get("usedPercentage"))
        .and_then(Value::as_f64)
    else {
        return FieldUpdate::Clear {
            reason: None,
            invalid: false,
        };
    };

    #[allow(clippy::cast_possible_truncation)]
    let pct = pct_f64 as f32;
    if !pct.is_finite() || !(0.0..=100.0).contains(&pct) {
        return FieldUpdate::Clear {
            reason: Some("Percentual de cota fora do intervalo 0..=100".to_string()),
            invalid: true,
        };
    }

    let resets_at_ms = obj
        .get("resets_at")
        .or_else(|| obj.get("resetsAt"))
        .and_then(Value::as_i64)
        .and_then(|v| {
            // Se menor que 10^11, está em segundos epoch; converte para ms
            if v < 100_000_000_000 {
                v.checked_mul(1000)
            } else {
                Some(v)
            }
        });

    let window_minutes = obj
        .get("window_minutes")
        .or_else(|| obj.get("windowMinutes"))
        .and_then(Value::as_u64)
        .and_then(|m| u32::try_from(m).ok())
        .unwrap_or(default_minutes);

    FieldUpdate::Set {
        value: QuotaWindow {
            used_percent: pct,
            resets_at_ms,
            window_minutes,
        },
        quality: MeasurementQuality::Reported,
        expires_at_ms: resets_at_ms,
    }
}
