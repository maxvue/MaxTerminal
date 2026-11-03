//! Registro em memória thread-safe de estados de abas e painéis (`TabStateRegistry`)
//! com revisão monotônica estrita (`revision: u64`) por `TabId`.

use std::collections::BTreeMap;

use mt_core::ids::{PaneId, PtyId, SessionId, TabId};
use mt_core::shell::{
    AttentionSnapshot, ContinuousPolicy, FocusSnapshot, IntegrationMode, SeenRequest,
    TabStateRemovedEvent, TabStateSnapshot, TabStatus,
};
use parking_lot::RwLock;

use crate::state::daemon::{CONTINUOUS_PROMOTION_THRESHOLD_MS, RUNNING_HYSTERESIS_MS};
use crate::state::machine::{
    PaneInternalState, TransitionInput, aggregate_tab_snapshot, transition_pane,
};

#[derive(Debug, Clone)]
struct TabEntry {
    tab_id: TabId,
    revision: u64,
    panes: Vec<PaneInternalState>,
    attention: Option<AttentionSnapshot>,
    updated_at: i64,
}

#[derive(Debug, Default)]
struct RegistryInner {
    tabs: BTreeMap<TabId, TabEntry>,
    pty_to_tab: BTreeMap<PtyId, TabId>,
    tombstones: BTreeMap<TabId, u64>,
    focus: FocusSnapshot,
}

/// Registro concorrente de estados das abas e seus respectivos painéis.
#[derive(Debug, Default)]
pub struct TabStateRegistry {
    inner: RwLock<RegistryInner>,
}

impl TabStateRegistry {
    /// Cria uma nova instância vazia de [`TabStateRegistry`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra ou atualiza o vínculo de um painel dentro de uma aba, retornando o snapshot atualizado.
    #[allow(clippy::too_many_arguments)]
    pub fn register_pane(
        &self,
        tab_id: TabId,
        pane_id: Option<PaneId>,
        pty_id: PtyId,
        session_id: SessionId,
        integration_mode: IntegrationMode,
        continuous_policy: ContinuousPolicy,
        initial_cwd: Option<String>,
        now_ms: i64,
    ) -> TabStateSnapshot {
        let mut guard = self.inner.write();
        guard.pty_to_tab.insert(pty_id, tab_id);

        let focused = is_tab_focused(&guard.focus, tab_id);
        let min_rev = guard.tombstones.get(&tab_id).copied().unwrap_or(0);

        let entry = guard.tabs.entry(tab_id).or_insert_with(|| TabEntry {
            tab_id,
            revision: min_rev,
            panes: Vec::new(),
            attention: None,
            updated_at: now_ms,
        });

        let new_pane = PaneInternalState::new(
            pane_id,
            pty_id,
            session_id,
            integration_mode,
            continuous_policy,
            initial_cwd,
            now_ms,
        );

        if let Some(existing) = entry
            .panes
            .iter_mut()
            .find(|p| p.pty_id == pty_id || (pane_id.is_some() && p.pane_id == pane_id))
        {
            if existing.pty_id != pty_id {
                let old_pty = existing.pty_id;
                *existing = new_pane;
                entry.revision += 1;
                entry.updated_at = now_ms;
                let snapshot = build_snapshot(entry, focused, now_ms);
                guard.pty_to_tab.remove(&old_pty);
                return snapshot;
            }
            *existing = new_pane;
        } else {
            entry.panes.push(new_pane);
        }

        entry.revision += 1;
        entry.updated_at = now_ms;
        build_snapshot(entry, focused, now_ms)
    }

    /// Aplica uma transição a um painel identificado por `pty_id`.
    ///
    /// Retorna `Some(TabStateSnapshot)` se o estado da aba mudou (incrementando `revision`).
    pub fn apply_pty_transition(
        &self,
        pty_id: PtyId,
        input: TransitionInput,
        now_ms: i64,
    ) -> Option<TabStateSnapshot> {
        let mut guard = self.inner.write();
        let tab_id = *guard.pty_to_tab.get(&pty_id)?;
        let focused = is_tab_focused(&guard.focus, tab_id);

        let entry = guard.tabs.get_mut(&tab_id)?;
        let pane_idx = entry.panes.iter().position(|p| p.pty_id == pty_id)?;

        let out = transition_pane(&entry.panes[pane_idx], input);
        entry.panes[pane_idx] = out.state;

        if !out.state_changed {
            return None;
        }

        entry.revision += 1;
        entry.updated_at = now_ms;
        Some(build_snapshot(entry, focused, now_ms))
    }

    /// Verifica se uma aba/painel está efetivamente focada e visível no momento.
    #[must_use]
    pub fn is_pty_effectively_focused(&self, pty_id: PtyId) -> bool {
        let guard = self.inner.read();
        let Some(&tab_id) = guard.pty_to_tab.get(&pty_id) else {
            return false;
        };
        if !is_tab_focused(&guard.focus, tab_id) {
            return false;
        }
        // Se `active_pane_id` for especificado no foco, valida correspondência quando a aba tiver múltiplos painéis
        if let Some(active_pane) = guard.focus.active_pane_id
            && let Some(entry) = guard.tabs.get(&tab_id)
            && entry.panes.len() > 1
            && let Some(pane) = entry.panes.iter().find(|p| p.pty_id == pty_id)
            && let Some(pid) = pane.pane_id
        {
            return pid == active_pane;
        }
        true
    }

    /// Marca conclusões não vistas e alertas de uma aba (ou painel específico) como visualizados (`tab_mark_seen`).
    pub fn mark_seen(&self, request: &SeenRequest, now_ms: i64) -> Option<TabStateSnapshot> {
        let mut guard = self.inner.write();
        let focused = is_tab_focused(&guard.focus, request.tab_id);
        let entry = guard.tabs.get_mut(&request.tab_id)?;

        if let Some(up_to) = request.up_to_revision
            && entry.revision > up_to
        {
            // Se o pedido de seen referencia uma revisão mais antiga que uma transição subsequente,
            // ainda retorna o snapshot atual sem regredir a revisão.
            return Some(build_snapshot(entry, focused, now_ms));
        }

        let mut any_changed = false;
        for pane in &mut entry.panes {
            if request.pane_id.is_none() || pane.pane_id == request.pane_id {
                let out = transition_pane(pane, TransitionInput::MarkSeen { at_ms: now_ms });
                if out.state_changed {
                    *pane = out.state;
                    any_changed = true;
                }
            }
        }

        if request.pane_id.is_none()
            && let Some(att) = &mut entry.attention
            && !att.seen
        {
            att.seen = true;
            any_changed = true;
        }

        if any_changed {
            entry.revision += 1;
            entry.updated_at = now_ms;
        }

        Some(build_snapshot(entry, focused, now_ms))
    }

    /// Atualiza o foco global da janela/aba/painel (`tab_set_focus`) e retorna snapshots das abas cujo estado mudou.
    #[allow(clippy::needless_pass_by_value)]
    pub fn set_focus(&self, focus: FocusSnapshot, now_ms: i64) -> Vec<TabStateSnapshot> {
        let mut guard = self.inner.write();
        let prev_focus = guard.focus.clone();
        guard.focus = focus.clone();

        let mut updated = Vec::new();
        let affected_tabs: Vec<TabId> = [prev_focus.active_tab_id, focus.active_tab_id]
            .into_iter()
            .flatten()
            .collect();

        for tab_id in affected_tabs {
            let now_focused = is_tab_focused(&guard.focus, tab_id);
            let was_focused = is_tab_focused(&prev_focus, tab_id);

            if let Some(entry) = guard.tabs.get_mut(&tab_id) {
                let mut changed = now_focused != was_focused;

                if now_focused {
                    for pane in &mut entry.panes {
                        let pane_matches = focus.active_pane_id.is_none()
                            || pane.pane_id.is_none()
                            || pane.pane_id == focus.active_pane_id;
                        if pane_matches {
                            let out =
                                transition_pane(pane, TransitionInput::MarkSeen { at_ms: now_ms });
                            if out.state_changed {
                                *pane = out.state;
                                changed = true;
                            }
                        }
                    }
                    if let Some(att) = &mut entry.attention
                        && !att.seen
                    {
                        att.seen = true;
                        changed = true;
                    }
                }

                if changed {
                    entry.revision += 1;
                    entry.updated_at = now_ms;
                    updated.push(build_snapshot(entry, now_focused, now_ms));
                }
            }
        }

        updated
    }

    /// Altera a política `ContinuousPolicy` de uma aba/painel (`tab_set_continuous`).
    pub fn set_continuous_policy(
        &self,
        tab_id: TabId,
        pane_id: Option<PaneId>,
        policy: ContinuousPolicy,
        now_ms: i64,
    ) -> Option<TabStateSnapshot> {
        let mut guard = self.inner.write();
        let focused = is_tab_focused(&guard.focus, tab_id);
        let entry = guard.tabs.get_mut(&tab_id)?;

        let mut any_changed = false;
        for pane in &mut entry.panes {
            if pane_id.is_none() || pane.pane_id == pane_id {
                let out = transition_pane(
                    pane,
                    TransitionInput::SetContinuousPolicy {
                        policy,
                        at_ms: now_ms,
                    },
                );
                if out.state_changed {
                    *pane = out.state;
                    any_changed = true;
                }
            }
        }

        if any_changed {
            entry.revision += 1;
            entry.updated_at = now_ms;
        }

        Some(build_snapshot(entry, focused, now_ms))
    }

    /// Registra ou limpa um alerta de atenção de Agente CLI na aba.
    pub fn set_attention(
        &self,
        tab_id: TabId,
        attention: Option<AttentionSnapshot>,
        now_ms: i64,
    ) -> Option<TabStateSnapshot> {
        let mut guard = self.inner.write();
        let focused = is_tab_focused(&guard.focus, tab_id);
        let entry = guard.tabs.get_mut(&tab_id)?;

        if entry.attention == attention {
            return None;
        }

        entry.attention = attention;
        entry.revision += 1;
        entry.updated_at = now_ms;
        Some(build_snapshot(entry, focused, now_ms))
    }

    /// Avalia timers periódicos (histerese de 80 ms para `Running` e promoção de 15 s para `Continuous`)
    /// sobre todos os painéis com execução ativa, retornando os snapshots das abas alteradas.
    pub fn evaluate_timers(&self, now_ms: i64) -> Vec<TabStateSnapshot> {
        let mut guard = self.inner.write();
        let focus = guard.focus.clone();
        let mut changed_snapshots = Vec::new();

        for entry in guard.tabs.values_mut() {
            let mut tab_changed = false;
            for pane in &mut entry.panes {
                let Some(active) = pane.active_execution.clone() else {
                    continue;
                };
                let elapsed = now_ms.saturating_sub(active.started_at);

                if pane.pending_running_hysteresis && elapsed >= RUNNING_HYSTERESIS_MS {
                    let out = transition_pane(
                        pane,
                        TransitionInput::RunningHysteresisElapsed {
                            block_id: active.block_id,
                            at_ms: now_ms,
                        },
                    );
                    if out.state_changed {
                        *pane = out.state;
                        tab_changed = true;
                    }
                }

                if !pane.active_is_continuous && elapsed >= CONTINUOUS_PROMOTION_THRESHOLD_MS {
                    let out = transition_pane(
                        pane,
                        TransitionInput::ContinuousTimerElapsed {
                            block_id: active.block_id,
                            elapsed_ms: elapsed,
                            at_ms: now_ms,
                        },
                    );
                    if out.state_changed {
                        *pane = out.state;
                        tab_changed = true;
                    }
                }
            }

            if tab_changed {
                entry.revision += 1;
                entry.updated_at = now_ms;
                let focused = is_tab_focused(&focus, entry.tab_id);
                changed_snapshots.push(build_snapshot(entry, focused, now_ms));
            }
        }

        changed_snapshots
    }

    /// Remove uma aba inteira do registro, emitindo [`TabStateRemovedEvent`] com tombstone de revisão.
    pub fn remove_tab(&self, tab_id: TabId) -> Option<TabStateRemovedEvent> {
        let mut guard = self.inner.write();
        let entry = guard.tabs.remove(&tab_id)?;
        for pane in &entry.panes {
            guard.pty_to_tab.remove(&pane.pty_id);
        }
        let final_rev = entry.revision + 1;
        guard.tombstones.insert(tab_id, final_rev);
        Some(TabStateRemovedEvent {
            tab_id,
            revision: final_rev,
        })
    }

    /// Remove um painel individual de uma aba que permanece aberta (ex.: `pane_close` em split),
    /// recalculando o snapshot agregado da aba.
    pub fn remove_pane(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        now_ms: i64,
    ) -> Option<TabStateSnapshot> {
        let mut guard = self.inner.write();
        let focused = is_tab_focused(&guard.focus, tab_id);
        let (removed_pty, snap) = {
            let entry = guard.tabs.get_mut(&tab_id)?;
            let idx = entry
                .panes
                .iter()
                .position(|p| p.pane_id == Some(pane_id))?;
            let removed = entry.panes.remove(idx);
            entry.revision += 1;
            entry.updated_at = now_ms;
            (removed.pty_id, build_snapshot(entry, focused, now_ms))
        };
        guard.pty_to_tab.remove(&removed_pty);
        Some(snap)
    }

    /// Limpa alertas e estados de painéis antigos de uma aba ao iniciar uma nova sessão (`session_new`),
    /// preservando a monotonicidade de `revision`.
    pub fn clear_tab_panes(&self, tab_id: TabId, now_ms: i64) {
        let mut guard = self.inner.write();
        let removed_ptys: Vec<PtyId> = if let Some(entry) = guard.tabs.get_mut(&tab_id) {
            let ptys = entry.panes.iter().map(|p| p.pty_id).collect();
            entry.panes.clear();
            entry.attention = None;
            entry.revision += 1;
            entry.updated_at = now_ms;
            ptys
        } else {
            Vec::new()
        };
        for pty_id in removed_ptys {
            guard.pty_to_tab.remove(&pty_id);
        }
    }

    /// Retorna o snapshot atual de uma aba pelo seu `TabId`.
    #[must_use]
    pub fn get_tab(&self, tab_id: TabId) -> Option<TabStateSnapshot> {
        let guard = self.inner.read();
        let entry = guard.tabs.get(&tab_id)?;
        let focused = is_tab_focused(&guard.focus, tab_id);
        Some(build_snapshot(entry, focused, entry.updated_at))
    }

    /// Lista os snapshots de todas as abas registradas, ordenados por `TabId`.
    #[must_use]
    pub fn list_tabs(&self) -> Vec<TabStateSnapshot> {
        let guard = self.inner.read();
        guard
            .tabs
            .values()
            .map(|entry| {
                let focused = is_tab_focused(&guard.focus, entry.tab_id);
                build_snapshot(entry, focused, entry.updated_at)
            })
            .collect()
    }

    /// Retorna o estado atual do painel correspondente a um `PtyId`.
    #[must_use]
    pub fn get_pane_by_pty(&self, pty_id: PtyId) -> Option<PaneInternalState> {
        let guard = self.inner.read();
        let tab_id = guard.pty_to_tab.get(&pty_id)?;
        let entry = guard.tabs.get(tab_id)?;
        entry.panes.iter().find(|p| p.pty_id == pty_id).cloned()
    }

    /// Indica se existe ao menos um painel em estado `Running` ou `Continuous` em qualquer aba.
    #[must_use]
    pub fn has_active_processes(&self) -> bool {
        let guard = self.inner.read();
        guard.tabs.values().any(|entry| {
            entry.panes.iter().any(|p| {
                p.active_execution.is_some()
                    || matches!(p.status, TabStatus::Running | TabStatus::Continuous)
            })
        })
    }
}

fn is_tab_focused(focus: &FocusSnapshot, tab_id: TabId) -> bool {
    focus.window_focused && focus.active_tab_id == Some(tab_id)
}

fn build_snapshot(entry: &TabEntry, focused: bool, now_ms: i64) -> TabStateSnapshot {
    aggregate_tab_snapshot(
        entry.tab_id,
        entry.revision,
        focused,
        &entry.panes,
        entry.attention.as_ref(),
        now_ms,
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use mt_core::ids::CommandBlockId;
    use mt_core::shell::{BlockEndReason, BlockStatus, ExecutionRef, ObservationSource};

    use super::*;

    #[test]
    fn aggregates_multipane_precedence_and_monotonic_revisions() {
        let reg = TabStateRegistry::new();
        let tab_id = TabId::new();
        let pane1 = PaneId::new();
        let pane2 = PaneId::new();
        let pty1 = PtyId::new();
        let pty2 = PtyId::new();

        let s1 = reg.register_pane(
            tab_id,
            Some(pane1),
            pty1,
            SessionId::new(),
            IntegrationMode::Osc133,
            ContinuousPolicy::Auto,
            Some("/tmp".to_string()),
            100,
        );
        assert_eq!(s1.revision, 1);
        assert_eq!(s1.status, TabStatus::Idle);

        let s2 = reg.register_pane(
            tab_id,
            Some(pane2),
            pty2,
            SessionId::new(),
            IntegrationMode::Osc133,
            ContinuousPolicy::Always,
            Some("/tmp".to_string()),
            110,
        );
        assert_eq!(s2.revision, 2);

        // Pane 2 entra em Continuous
        let block2 = CommandBlockId::new();
        let s3 = reg
            .apply_pty_transition(
                pty2,
                TransitionInput::CommandStarted {
                    execution: ExecutionRef {
                        block_id: block2,
                        command_seq: 1,
                        started_at: 120,
                        command_preview: Some("npm run dev".to_string()),
                        source: ObservationSource::Osc133,
                    },
                    command_text: Some("npm run dev".to_string()),
                    cwd: None,
                    apply_hysteresis: false,
                    at_ms: 120,
                },
                120,
            )
            .unwrap();
        assert_eq!(s3.status, TabStatus::Continuous);
        assert_eq!(s3.revision, 3);

        // Pane 1 falha enquanto aba não está focada -> ErrorUnseen vence Continuous!
        let block1 = CommandBlockId::new();
        reg.apply_pty_transition(
            pty1,
            TransitionInput::CommandStarted {
                execution: ExecutionRef {
                    block_id: block1,
                    command_seq: 1,
                    started_at: 130,
                    command_preview: Some("false".to_string()),
                    source: ObservationSource::Osc133,
                },
                command_text: Some("false".to_string()),
                cwd: None,
                apply_hysteresis: false,
                at_ms: 130,
            },
            130,
        );
        let s4 = reg
            .apply_pty_transition(
                pty1,
                TransitionInput::CommandFinished {
                    block_id: block1,
                    block_status: BlockStatus::Failed,
                    end_reason: BlockEndReason::ExitNonZero,
                    exit_code: Some(1),
                    signal: None,
                    duration_ms: 10,
                    is_effectively_focused: false,
                    at_ms: 140,
                },
                140,
            )
            .unwrap();
        assert_eq!(s4.status, TabStatus::ErrorUnseen);
        assert!(reg.has_active_processes());

        // Ao marcar como visto, ErrorUnseen do Pane 1 vira Idle e a aba volta a mostrar Continuous do Pane 2!
        let s5 = reg
            .mark_seen(
                &SeenRequest {
                    tab_id,
                    pane_id: None,
                    up_to_revision: None,
                },
                150,
            )
            .unwrap();
        assert_eq!(s5.status, TabStatus::Continuous);
        assert!(s5.revision > s4.revision);
    }
}
