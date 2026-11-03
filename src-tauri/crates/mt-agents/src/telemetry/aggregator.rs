//! Agregador multi-fonte de telemetria por execução (`TelemetryAggregator`) com precedência por fonte,
//! semântica `Unchanged`/`Clear`/`Set`, envelhecimento `stale` (60 s), throttle trailing de 1 Hz (`CA-20`)
//! e persistência em `agent_telemetry_snapshots`.

use std::collections::{BTreeMap, HashMap};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use mt_core::{MtError, MtResult, TabId};
use parking_lot::RwLock;
use rusqlite::{OptionalExtension, params};

use crate::detect::current_unix_ms;
use crate::ports::RunBinding;
use crate::registry::AgentKind;
use crate::settings::AgentsSettings;
use crate::telemetry::{
    AgentTelemetry, ContextUsage, Field, FieldUpdate, MeasurementQuality, MetricAvailability,
    MetricKey, MetricState, ModelInfo, QuotaWindow, TelemetrySourceKind, TelemetryUpdate,
};

const FIELD_MAX_TTL_MS: i64 = 300_000; // 300s
const THROTTLE_WINDOW_MS: i64 = 1_000; // 1 Hz
const DB_PERSIST_INTERVAL_MS: i64 = 30_000; // 30s

/// Sink de eventos de telemetria para emissão ao frontend (`agent://telemetry`).
pub trait TelemetrySink: Send + Sync {
    /// Emite um snapshot consolidado de telemetria.
    fn emit_telemetry(&self, telemetry: &AgentTelemetry);
}

/// Implementação no-op de `TelemetrySink` para testes.
pub struct NoopTelemetrySink;
impl TelemetrySink for NoopTelemetrySink {
    fn emit_telemetry(&self, _telemetry: &AgentTelemetry) {}
}

struct TabTelemetryState {
    binding: RunBinding,
    revision: u64,
    session_ref: Option<String>,
    agent_active: bool,
    updated_at_ms: i64,
    last_emitted_at_ms: i64,
    trailing_scheduled: bool,
    last_persisted_at_ms: i64,

    model_candidates: BTreeMap<TelemetrySourceKind, Field<ModelInfo>>,
    context_candidates: BTreeMap<TelemetrySourceKind, Field<ContextUsage>>,
    quota_5h_candidates: BTreeMap<TelemetrySourceKind, Field<QuotaWindow>>,
    quota_weekly_candidates: BTreeMap<TelemetrySourceKind, Field<QuotaWindow>>,
    cost_candidates: BTreeMap<TelemetrySourceKind, Field<f64>>,

    last_invalid_reason: BTreeMap<MetricKey, String>,
    hydrated_snapshot: Option<AgentTelemetry>,
}

/// Agregador central de telemetria por aba.
pub struct TelemetryAggregator {
    tabs: RwLock<HashMap<TabId, TabTelemetryState>>,
    settings: Arc<RwLock<AgentsSettings>>,
    sink: Arc<dyn TelemetrySink>,
    db: Option<mt_db::Db>,
}

impl TelemetryAggregator {
    /// Cria um novo `TelemetryAggregator`.
    #[must_use]
    pub fn new(
        settings: Arc<RwLock<AgentsSettings>>,
        sink: Arc<dyn TelemetrySink>,
        db: Option<mt_db::Db>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tabs: RwLock::new(HashMap::new()),
            settings,
            sink,
            db,
        })
    }

    /// Registra o início de uma nova execução de agente na aba, descartando candidatos de execuções anteriores.
    pub fn register_run(
        self: &Arc<Self>,
        binding: RunBinding,
        configured_model: Option<String>,
        now_ms: i64,
    ) -> AgentTelemetry {
        let mut model_candidates = BTreeMap::new();
        if let Some(m) = configured_model.filter(|s| !s.trim().is_empty()) {
            model_candidates.insert(
                TelemetrySourceKind::Configured,
                Field {
                    value: ModelInfo {
                        id: Some(m.clone()),
                        display_name: m,
                    },
                    source: TelemetrySourceKind::Configured,
                    observed_at_ms: now_ms,
                    received_at_ms: now_ms,
                    quality: MeasurementQuality::Configured,
                    expires_at_ms: None,
                },
            );
        }

        let state = TabTelemetryState {
            binding: binding.clone(),
            revision: 1,
            session_ref: None,
            agent_active: true,
            updated_at_ms: now_ms,
            last_emitted_at_ms: now_ms,
            trailing_scheduled: false,
            last_persisted_at_ms: 0,
            model_candidates,
            context_candidates: BTreeMap::new(),
            quota_5h_candidates: BTreeMap::new(),
            quota_weekly_candidates: BTreeMap::new(),
            cost_candidates: BTreeMap::new(),
            last_invalid_reason: BTreeMap::new(),
            hydrated_snapshot: None,
        };

        let snap = self.build_snapshot(&state, now_ms);
        self.tabs.write().insert(binding.tab_id, state);
        self.sink.emit_telemetry(&snap);
        self.maybe_persist_snapshot(&snap, true);
        snap
    }

    /// Aplica um `TelemetryUpdate` de uma fonte se pertencer ao `run_id` ativo da aba.
    /// Retorna `Some(AgentTelemetry)` se o atualização foi aceita.
    pub fn apply(self: &Arc<Self>, update: TelemetryUpdate, now_ms: i64) -> Option<AgentTelemetry> {
        let mut emit_now: Option<AgentTelemetry> = None;
        let mut schedule_trailing_for: Option<(TabId, u64)> = None;
        let mut persist_snap: Option<(AgentTelemetry, bool)> = None;

        {
            let mut guard = self.tabs.write();
            let state = guard.get_mut(&update.binding.tab_id)?;

            // Descarta eventos de execuções antigas (run_id ou generation divergentes)
            if state.binding.run_id != update.binding.run_id
                || state.binding.generation != update.binding.generation
            {
                return None;
            }

            state.hydrated_snapshot = None;

            if update.invalidate_previous_context {
                state.context_candidates.clear();
            }

            // Se o modelo ativo mudou para outro ID, invalida contexto anterior
            if let FieldUpdate::Set { ref value, .. } = update.model
                && let Some((_, prev_model)) = best_candidate(&state.model_candidates, now_ms)
                && prev_model.value.id.is_some()
                && value.id.is_some()
                && prev_model.value.id != value.id
            {
                state.context_candidates.clear();
            }

            apply_field_update(
                &mut state.model_candidates,
                &mut state.last_invalid_reason,
                MetricKey::Model,
                update.source,
                update.model,
                update.observed_at_ms,
                now_ms,
                |_| true,
            );
            apply_field_update(
                &mut state.context_candidates,
                &mut state.last_invalid_reason,
                MetricKey::Context,
                update.source,
                update.context,
                update.observed_at_ms,
                now_ms,
                |c| c.percent.is_finite() && (0.0..=100.0).contains(&c.percent),
            );
            apply_field_update(
                &mut state.quota_5h_candidates,
                &mut state.last_invalid_reason,
                MetricKey::Quota5h,
                update.source,
                update.quota_5h,
                update.observed_at_ms,
                now_ms,
                |q| q.used_percent.is_finite() && (0.0..=100.0).contains(&q.used_percent),
            );
            apply_field_update(
                &mut state.quota_weekly_candidates,
                &mut state.last_invalid_reason,
                MetricKey::QuotaWeekly,
                update.source,
                update.quota_weekly,
                update.observed_at_ms,
                now_ms,
                |q| q.used_percent.is_finite() && (0.0..=100.0).contains(&q.used_percent),
            );
            apply_field_update(
                &mut state.cost_candidates,
                &mut state.last_invalid_reason,
                MetricKey::CostUsd,
                update.source,
                update.cost_usd,
                update.observed_at_ms,
                now_ms,
                |&v| v.is_finite() && v >= 0.0,
            );

            if let Some(sref) = update.session_ref {
                state.session_ref = Some(sref);
            }

            state.revision += 1;
            state.updated_at_ms = now_ms;
            let snap = self.build_snapshot(state, now_ms);

            if now_ms.saturating_sub(state.last_emitted_at_ms) >= THROTTLE_WINDOW_MS {
                state.last_emitted_at_ms = now_ms;
                state.trailing_scheduled = false;
                emit_now = Some(snap.clone());
            } else if !state.trailing_scheduled {
                state.trailing_scheduled = true;
                let wait_ms = THROTTLE_WINDOW_MS
                    .saturating_sub(now_ms.saturating_sub(state.last_emitted_at_ms))
                    .max(10);
                schedule_trailing_for =
                    Some((update.binding.tab_id, u64::try_from(wait_ms).unwrap_or(100)));
            }

            if now_ms.saturating_sub(state.last_persisted_at_ms) >= DB_PERSIST_INTERVAL_MS {
                state.last_persisted_at_ms = now_ms;
                persist_snap = Some((snap.clone(), false));
            }

            drop(guard);
            if let Some(ref to_emit) = emit_now {
                self.sink.emit_telemetry(to_emit);
            }
            if let Some((tab_id, delay_ms)) = schedule_trailing_for {
                let agg = Arc::clone(self);
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    agg.flush_trailing(tab_id);
                });
            }
            if let Some((ref psnap, force)) = persist_snap {
                self.maybe_persist_snapshot(psnap, force);
            }
            Some(snap)
        }
    }

    fn flush_trailing(&self, tab_id: TabId) {
        let now_ms = current_unix_ms();
        let snap_opt = {
            let mut guard = self.tabs.write();
            let Some(state) = guard.get_mut(&tab_id) else {
                return;
            };
            if !state.trailing_scheduled {
                return;
            }
            state.trailing_scheduled = false;
            state.last_emitted_at_ms = now_ms;
            Some(self.build_snapshot(state, now_ms))
        };
        if let Some(snap) = snap_opt {
            self.sink.emit_telemetry(&snap);
        }
    }

    /// Marca a execução da aba como inativa quando o PTY encerra, preservando o último snapshot conhecido.
    pub fn mark_inactive(&self, tab_id: TabId, run_id: &str, now_ms: i64) {
        let snap_opt = {
            let mut guard = self.tabs.write();
            let Some(state) = guard.get_mut(&tab_id) else {
                return;
            };
            if state.binding.run_id != run_id {
                return;
            }
            state.agent_active = false;
            state.revision += 1;
            state.updated_at_ms = now_ms;
            Some(self.build_snapshot(state, now_ms))
        };
        if let Some(snap) = snap_opt {
            self.sink.emit_telemetry(&snap);
            self.maybe_persist_snapshot(&snap, true);
        }
    }

    /// Altera o estado `agent_active` da execução atual de uma aba.
    pub fn set_active(&self, tab_id: TabId, active: bool, now_ms: i64) {
        let snap_opt = {
            let mut guard = self.tabs.write();
            let Some(state) = guard.get_mut(&tab_id) else {
                return;
            };
            if state.agent_active == active {
                return;
            }
            state.agent_active = active;
            state.revision += 1;
            state.updated_at_ms = now_ms;
            Some(self.build_snapshot(state, now_ms))
        };
        if let Some(snap) = snap_opt {
            self.sink.emit_telemetry(&snap);
            if !active {
                self.maybe_persist_snapshot(&snap, true);
            }
        }
    }

    /// Remove o estado de telemetria de uma aba fechada.
    pub fn remove_tab(&self, tab_id: TabId) {
        self.tabs.write().remove(&tab_id);
    }

    /// Obtém o snapshot de telemetria mais recente da aba informada.
    #[must_use]
    pub fn snapshot(&self, tab_id: TabId, now_ms: i64) -> Option<AgentTelemetry> {
        let guard = self.tabs.read();
        let state = guard.get(&tab_id)?;
        if let Some(ref hydrated) = state.hydrated_snapshot {
            return Some(hydrated.clone());
        }
        Some(self.build_snapshot(state, now_ms))
    }

    /// Lista todos os snapshots de telemetria ativos ou hidratados em memória.
    #[must_use]
    pub fn list_snapshots(&self, now_ms: i64) -> Vec<AgentTelemetry> {
        let guard = self.tabs.read();
        guard
            .values()
            .map(|state| {
                if let Some(ref hydrated) = state.hydrated_snapshot {
                    hydrated.clone()
                } else {
                    self.build_snapshot(state, now_ms)
                }
            })
            .collect()
    }

    /// Hidrata do SQLite (`agent_telemetry_snapshots`) os últimos snapshots de abas ainda abertas,
    /// marcando `agent_active = false` até que um novo processo emita telemetria.
    pub async fn hydrate_from_db(&self) -> MtResult<u32> {
        let Some(ref db) = self.db else {
            return Ok(0);
        };
        let rows: Vec<(String, String)> = db
            .read(|conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT s.tab_id, s.payload
                         FROM agent_telemetry_snapshots s
                         INNER JOIN tabs t ON t.id = s.tab_id;",
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "telemetry_hydrate_query"))?;
                let iter = stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "telemetry_hydrate_query"))?;
                let mut out = Vec::new();
                for item in iter {
                    out.push(
                        item.map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "telemetry_hydrate_row")
                        })?,
                    );
                }
                Ok(out)
            })
            .await
            .map_err(MtError::from)?;

        let mut count = 0u32;
        let mut guard = self.tabs.write();
        for (tab_str, payload) in rows {
            let Ok(tab_id) = TabId::from_str(&tab_str) else {
                continue;
            };
            if guard.contains_key(&tab_id) {
                continue;
            }
            let Ok(mut snap) = serde_json::from_str::<AgentTelemetry>(&payload) else {
                continue;
            };
            snap.agent_active = false;
            let binding = RunBinding {
                tab_id: snap.tab_id,
                pane_id: snap.pane_id,
                pty_id: mt_core::PtyId::new(),
                session_id: snap.session_id,
                generation: 0,
                project_id: mt_core::ProjectId::new(),
                run_id: snap.run_id.clone(),
                agent: snap.agent,
                cwd: std::path::PathBuf::new(),
                started_at_ms: snap.updated_at_ms,
            };
            guard.insert(
                tab_id,
                TabTelemetryState {
                    binding,
                    revision: snap.revision,
                    session_ref: snap.session_ref.clone(),
                    agent_active: false,
                    updated_at_ms: snap.updated_at_ms,
                    last_emitted_at_ms: snap.updated_at_ms,
                    trailing_scheduled: false,
                    last_persisted_at_ms: snap.updated_at_ms,
                    model_candidates: BTreeMap::new(),
                    context_candidates: BTreeMap::new(),
                    quota_5h_candidates: BTreeMap::new(),
                    quota_weekly_candidates: BTreeMap::new(),
                    cost_candidates: BTreeMap::new(),
                    last_invalid_reason: BTreeMap::new(),
                    hydrated_snapshot: Some(snap),
                },
            );
            count += 1;
        }
        Ok(count)
    }

    fn maybe_persist_snapshot(&self, snap: &AgentTelemetry, _force: bool) {
        let Some(ref db) = self.db else {
            return;
        };
        let Ok(raw_json) = serde_json::to_string(snap) else {
            return;
        };
        let sanitized = mt_db::sanitizer::sanitize(&raw_json).into_owned();
        let tab_str = snap.tab_id.to_string();
        let agent_str = snap.agent.as_ref_id().to_string();
        let updated_at = snap.updated_at_ms;
        let db_clone = db.clone();

        tokio::spawn(async move {
            let _ = db_clone
                .write(move |tx| {
                    let tab_exists: Option<i64> = tx
                        .query_row(
                            "SELECT 1 FROM tabs WHERE id = ?1 LIMIT 1;",
                            [&tab_str],
                            |r| r.get(0),
                        )
                        .optional()
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "telemetry_check_tab"))?;
                    if tab_exists.is_some() {
                        tx.execute(
                            "INSERT INTO agent_telemetry_snapshots (tab_id, agent_id, payload, updated_at)
                             VALUES (?1, ?2, ?3, ?4)
                             ON CONFLICT(tab_id) DO UPDATE SET
                                agent_id = excluded.agent_id,
                                payload = excluded.payload,
                                updated_at = excluded.updated_at;",
                            params![tab_str, agent_str, sanitized, updated_at],
                        )
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "telemetry_upsert_snap"))?;
                    }
                    Ok(())
                })
                .await;
        });
    }

    fn build_snapshot(&self, state: &TabTelemetryState, now_ms: i64) -> AgentTelemetry {
        let (telem_enabled, stale_after_ms) = {
            let s = self.settings.read();
            (
                s.telemetry.enabled,
                i64::from(s.telemetry.stale_after_s) * 1000,
            )
        };

        let model = best_candidate(&state.model_candidates, now_ms).map(|(_, f)| f.clone());
        let context = best_candidate(&state.context_candidates, now_ms).map(|(_, f)| f.clone());
        let quota_5h = best_candidate(&state.quota_5h_candidates, now_ms).map(|(_, f)| f.clone());
        let quota_weekly =
            best_candidate(&state.quota_weekly_candidates, now_ms).map(|(_, f)| f.clone());
        let cost_usd = best_candidate(&state.cost_candidates, now_ms).map(|(_, f)| f.clone());

        let mut availability = BTreeMap::new();
        for key in MetricKey::ALL {
            let field_obs = match key {
                MetricKey::Model => model.as_ref().map(|f| (f.observed_at_ms, f.quality)),
                MetricKey::Context => context.as_ref().map(|f| (f.observed_at_ms, f.quality)),
                MetricKey::Quota5h => quota_5h.as_ref().map(|f| (f.observed_at_ms, f.quality)),
                MetricKey::QuotaWeekly => {
                    quota_weekly.as_ref().map(|f| (f.observed_at_ms, f.quality))
                }
                MetricKey::CostUsd => cost_usd.as_ref().map(|f| (f.observed_at_ms, f.quality)),
            };

            let metric_state = if !telem_enabled {
                MetricState {
                    availability: MetricAvailability::Disabled,
                    reason: Some("Telemetria desativada nas configurações.".to_string()),
                    checked_at_ms: now_ms,
                }
            } else if let Some((obs_ms, quality)) = field_obs {
                if quality != MeasurementQuality::Configured
                    && now_ms.saturating_sub(obs_ms) > stale_after_ms
                {
                    MetricState {
                        availability: MetricAvailability::Stale,
                        reason: Some("Dado sem atualização recente (> 60s).".to_string()),
                        checked_at_ms: now_ms,
                    }
                } else {
                    MetricState {
                        availability: MetricAvailability::Available,
                        reason: None,
                        checked_at_ms: now_ms,
                    }
                }
            } else if let Some(inv_reason) = state.last_invalid_reason.get(&key) {
                MetricState {
                    availability: MetricAvailability::Invalid,
                    reason: Some(inv_reason.clone()),
                    checked_at_ms: now_ms,
                }
            } else {
                default_empty_metric_state(state.binding.agent, key, now_ms)
            };

            availability.insert(key, metric_state);
        }

        AgentTelemetry {
            tab_id: state.binding.tab_id,
            pane_id: state.binding.pane_id,
            session_id: state.binding.session_id,
            run_id: state.binding.run_id.clone(),
            revision: state.revision,
            agent: state.binding.agent,
            session_ref: state.session_ref.clone(),
            model,
            context,
            quota_5h,
            quota_weekly,
            cost_usd,
            availability,
            agent_active: state.agent_active,
            updated_at_ms: state.updated_at_ms,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_field_update<T: Clone>(
    candidates: &mut BTreeMap<TelemetrySourceKind, Field<T>>,
    invalid_reasons: &mut BTreeMap<MetricKey, String>,
    key: MetricKey,
    source: TelemetrySourceKind,
    update: FieldUpdate<T>,
    observed_at_ms: i64,
    received_at_ms: i64,
    validator: impl FnOnce(&T) -> bool,
) {
    match update {
        FieldUpdate::Unchanged => {}
        FieldUpdate::Clear { reason, invalid } => {
            candidates.remove(&source);
            if invalid {
                invalid_reasons.insert(
                    key,
                    reason.unwrap_or_else(|| "Valor inválido reportado pela fonte.".to_string()),
                );
            } else {
                invalid_reasons.remove(&key);
            }
        }
        FieldUpdate::Set {
            value,
            quality,
            expires_at_ms,
        } => {
            if !validator(&value) {
                candidates.remove(&source);
                invalid_reasons.insert(key, "Valor fora dos limites permitidos.".to_string());
                return;
            }
            if let Some(existing) = candidates.get(&source)
                && observed_at_ms < existing.observed_at_ms
            {
                // Ignora observação regressiva para a mesma fonte
                return;
            }
            invalid_reasons.remove(&key);
            candidates.insert(
                source,
                Field {
                    value,
                    source,
                    observed_at_ms,
                    received_at_ms,
                    quality,
                    expires_at_ms,
                },
            );
        }
    }
}

fn best_candidate<T>(
    candidates: &BTreeMap<TelemetrySourceKind, Field<T>>,
    now_ms: i64,
) -> Option<(TelemetrySourceKind, &Field<T>)> {
    candidates
        .iter()
        .filter(|(_, field)| {
            if let Some(exp) = field.expires_at_ms
                && now_ms >= exp
            {
                return false;
            }
            if field.quality != MeasurementQuality::Configured
                && now_ms.saturating_sub(field.observed_at_ms) > FIELD_MAX_TTL_MS
            {
                return false;
            }
            true
        })
        .max_by_key(|(src, field)| (src.priority(), field.observed_at_ms))
        .map(|(k, v)| (*k, v))
}

fn default_empty_metric_state(agent: AgentKind, key: MetricKey, now_ms: i64) -> MetricState {
    match agent {
        AgentKind::Claude => MetricState {
            availability: MetricAvailability::AwaitingData,
            reason: Some("Aguardando dados do agente".to_string()),
            checked_at_ms: now_ms,
        },
        AgentKind::Codex => match key {
            MetricKey::Model | MetricKey::Context | MetricKey::Quota5h | MetricKey::QuotaWeekly => {
                MetricState {
                    availability: MetricAvailability::AwaitingData,
                    reason: Some("Indisponível para este agente ou aguardando sessão".to_string()),
                    checked_at_ms: now_ms,
                }
            }
            MetricKey::CostUsd => MetricState {
                availability: MetricAvailability::Unsupported,
                reason: Some("Indisponível para este agente".to_string()),
                checked_at_ms: now_ms,
            },
        },
        AgentKind::Agy | AgentKind::OpenCode => MetricState {
            availability: MetricAvailability::Unverified,
            reason: Some("Indisponível para este agente".to_string()),
            checked_at_ms: now_ms,
        },
    }
}
