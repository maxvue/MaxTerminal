//! Máquina de estados de atenção por execução, `TailBuffer` de 8 KiB sem ANSI e parser OSC/BEL.

use std::collections::HashMap;
use std::sync::Arc;

use mt_core::TabId;
use parking_lot::RwLock;

use crate::attention::{
    AckReason, AttentionSignal, AttentionSnapshot, AttentionSource, AttentionState, PatternSet,
};
use crate::ports::{AttentionKind, NotifyPort, PtyPort, RunBinding, TabStatePort};
use crate::registry::{AgentDescriptor, AgentRegistry};
use crate::settings::{AttentionLevel, AttentionSettings};

/// Limite máximo do buffer de cauda sem ANSI por aba de agente (8 KiB).
pub const MAX_TAIL_BYTES: usize = 8 * 1024;

/// Sink para emissão de eventos `agent://attention` ao frontend.
pub trait AttentionSink: Send + Sync {
    /// Publica um novo snapshot de atenção.
    fn emit(&self, snapshot: AttentionSnapshot);
}

/// Remove sequências de escape ANSI (CSI, OSC, ESC de 2 bytes) de um slice de bytes.
pub fn strip_ansi_bytes(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == 0x1b {
            if i + 1 >= raw.len() {
                break;
            }
            match raw[i + 1] {
                b'[' => {
                    // CSI: ESC [ ... byte final em 0x40..=0x7E
                    i += 2;
                    while i < raw.len() && !(0x40..=0x7e).contains(&raw[i]) {
                        i += 1;
                    }
                    if i < raw.len() {
                        i += 1;
                    }
                }
                b']' => {
                    // OSC: ESC ] ... BEL (\x07) ou ST (ESC \)
                    i += 2;
                    while i < raw.len() {
                        if raw[i] == 0x07 {
                            i += 1;
                            break;
                        }
                        if raw[i] == 0x1b && i + 1 < raw.len() && raw[i + 1] == b'\\' {
                            i += 2;
                            break;
                        }
                        i += 1;
                    }
                }
                _ => {
                    // Sequência ESC curta de 2 bytes
                    i += 2;
                }
            }
        } else if raw[i] == 0x07 {
            // Ignora BEL no texto limpo
            i += 1;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    out
}

/// Buffer circular de cauda (até 8 KiB) com remoção de sequências ANSI.
#[derive(Debug, Clone, Default)]
pub struct TailBuffer {
    buf: String,
}

impl TailBuffer {
    /// Cria um novo buffer vazio.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(MAX_TAIL_BYTES),
        }
    }

    /// Anexa bytes brutos do PTY após remover sequências ANSI, preservando os últimos 8 KiB em fronteira UTF-8.
    pub fn push_bytes(&mut self, raw: &[u8]) {
        let stripped = strip_ansi_bytes(raw);
        let text = String::from_utf8_lossy(&stripped);
        self.buf.push_str(&text);
        if self.buf.len() > MAX_TAIL_BYTES {
            let excess = self.buf.len() - MAX_TAIL_BYTES;
            let mut cut = excess;
            while cut < self.buf.len() && !self.buf.is_char_boundary(cut) {
                cut += 1;
            }
            self.buf.drain(..cut);
        }
    }

    /// Limpa o buffer.
    pub fn clear(&mut self) {
        self.buf.clear();
    }

    /// Retorna o texto acumulado sem ANSI.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.buf
    }
}

/// Extrai notificações `OSC 9;<texto>BEL/ST`, `OSC 777;notify;<titulo>;<corpo>BEL/ST`
/// e caracteres `BEL` (`\x07`) isolados (fora de sequências OSC).
#[must_use]
pub fn parse_osc_and_bel(bytes: &[u8]) -> (Vec<String>, bool) {
    let mut osc_messages = Vec::new();
    let mut standalone_bel = false;
    let mut i = 0;

    while i < bytes.len() {
        // Verifica início de OSC: ESC ] (\x1b\x5d)
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b']' {
            let start = i + 2;
            let mut end = start;
            let mut terminated_by_st = false;
            while end < bytes.len() {
                if bytes[end] == 0x07 {
                    break;
                }
                if bytes[end] == 0x1b && end + 1 < bytes.len() && bytes[end + 1] == b'\\' {
                    terminated_by_st = true;
                    break;
                }
                end += 1;
            }
            if end < bytes.len() {
                let payload = String::from_utf8_lossy(&bytes[start..end]);
                if let Some(rest) = payload.strip_prefix("9;") {
                    let msg = rest.trim().to_string();
                    if !msg.is_empty() {
                        osc_messages.push(msg);
                    }
                } else if let Some(rest) = payload.strip_prefix("777;notify;") {
                    let msg = rest.replace(';', ": ").trim().to_string();
                    if !msg.is_empty() {
                        osc_messages.push(msg);
                    }
                }
                i = if terminated_by_st { end + 2 } else { end + 1 };
                continue;
            }
            break;
        } else if bytes[i] == 0x07 {
            standalone_bel = true;
        }
        i += 1;
    }

    (osc_messages, standalone_bel)
}

struct TabAttentionEntry {
    run: RunBinding,
    state: AttentionState,
    revision: u64,
    episode: u64,
    active_episode_key: Option<String>,
    acknowledged: bool,
    foreground_is_agent: bool,
    tail: TailBuffer,
    last_output_ms: i64,
    evaluated_silence: bool,
}

/// Detector de atenção correlacionada por execução de agente.
pub struct AttentionDetector {
    tabs: RwLock<HashMap<TabId, TabAttentionEntry>>,
    patterns: Arc<PatternSet>,
    registry: Arc<AgentRegistry>,
    tab_state: Arc<dyn TabStatePort>,
    notify: Arc<dyn NotifyPort>,
    pty: Arc<dyn PtyPort>,
    sink: Arc<dyn AttentionSink>,
    settings: Arc<RwLock<AttentionSettings>>,
}

impl AttentionDetector {
    /// Cria uma nova instância do detector de atenção.
    #[must_use]
    pub fn new(
        patterns: Arc<PatternSet>,
        registry: Arc<AgentRegistry>,
        tab_state: Arc<dyn TabStatePort>,
        notify: Arc<dyn NotifyPort>,
        pty: Arc<dyn PtyPort>,
        sink: Arc<dyn AttentionSink>,
        settings: Arc<RwLock<AttentionSettings>>,
    ) -> Self {
        Self {
            tabs: RwLock::new(HashMap::new()),
            patterns,
            registry,
            tab_state,
            notify,
            pty,
            sink,
            settings,
        }
    }

    /// Registra o início de uma execução de agente em uma aba.
    pub fn register_run(&self, run: RunBinding, now_ms: i64) {
        let entry = TabAttentionEntry {
            run: run.clone(),
            state: AttentionState::Working,
            revision: 1,
            episode: 0,
            active_episode_key: None,
            acknowledged: false,
            foreground_is_agent: true,
            tail: TailBuffer::new(),
            last_output_ms: now_ms,
            evaluated_silence: false,
        };
        let snap = Self::to_snapshot(&entry);
        self.tabs.write().insert(run.tab_id, entry);
        self.sink.emit(snap);
    }

    /// Remove e limpa o rastreamento de atenção de uma aba, limpando eventual episódio ativo.
    pub fn unregister_tab(&self, tab_id: TabId) {
        let removed = self.tabs.write().remove(&tab_id);
        if let Some(entry) = removed
            && let AttentionState::Awaiting { episode, .. } = entry.state
        {
            self.tab_state.attention_resolved(&entry.run, episode);
            self.notify.clear_attention(tab_id);
        }
    }

    /// Obtém o snapshot atual de atenção da aba.
    #[must_use]
    pub fn snapshot(&self, tab_id: TabId) -> Option<AttentionSnapshot> {
        self.tabs.read().get(&tab_id).map(Self::to_snapshot)
    }

    /// Reconhece visualmente o episódio atual (`UserInput` ou `Dismissed`) sem resolver a espera no agente.
    pub fn ack(
        &self,
        tab_id: TabId,
        run_id: &str,
        episode: u64,
        _reason: AckReason,
    ) -> Option<AttentionSnapshot> {
        let mut emit_snap = None;
        let result = {
            let mut guard = self.tabs.write();
            let entry = guard.get_mut(&tab_id)?;
            if entry.run.run_id != run_id {
                return Some(Self::to_snapshot(entry));
            }
            let matches_episode = match &entry.state {
                AttentionState::Awaiting {
                    episode: current_ep,
                    ..
                } => *current_ep == episode,
                AttentionState::Suspected { .. } => episode == entry.episode,
                _ => false,
            };
            if matches_episode && !entry.acknowledged {
                entry.acknowledged = true;
                entry.revision += 1;
                let snap = Self::to_snapshot(entry);
                emit_snap = Some(snap.clone());
                Some(snap)
            } else {
                Some(Self::to_snapshot(entry))
            }
        };
        if let Some(snap) = emit_snap {
            self.sink.emit(snap);
        }
        result
    }

    /// Processa um sinal de atenção para a aba informada.
    #[allow(clippy::too_many_lines)]
    pub async fn on_signal(&self, tab_id: TabId, sig: AttentionSignal, now_ms: i64) {
        let settings = self.settings.read().clone();
        if !settings.enabled {
            return;
        }

        enum PostAction {
            None,
            Emit(AttentionSnapshot),
            EnterAwaiting {
                snap: AttentionSnapshot,
                run: RunBinding,
                episode: u64,
                kind: AttentionKind,
                title: String,
                body: String,
            },
            ResolveAwaiting {
                snap: AttentionSnapshot,
                run: RunBinding,
                episode: u64,
                notify_turn_complete: Option<(String, String)>,
            },
            TurnCompleteOnly {
                snap: Option<AttentionSnapshot>,
                title: String,
                body: String,
            },
        }

        let action = {
            let mut guard = self.tabs.write();
            let Some(entry) = guard.get_mut(&tab_id) else {
                return;
            };
            let descriptor = self.registry.get(entry.run.agent);

            match sig {
                AttentionSignal::HookNotification {
                    message,
                    notification_type,
                    subagent_id,
                } => {
                    if subagent_id.is_some() {
                        PostAction::None
                    } else {
                        let kind_opt = match notification_type.as_deref() {
                            Some("permission_prompt") => Some(AttentionKind::AwaitingApproval),
                            Some("idle_prompt") => Some(AttentionKind::AwaitingInput),
                            _ => None,
                        };
                        if let Some(kind) = kind_opt {
                            let ep_key = format!(
                                "{}:{:?}:{}",
                                entry.run.run_id,
                                kind,
                                message.as_deref().unwrap_or("")
                            );
                            let already_same = matches!(
                                &entry.state,
                                AttentionState::Awaiting { kind: k, .. } if *k == kind
                            ) && entry.active_episode_key.as_deref()
                                == Some(ep_key.as_str());

                            if already_same {
                                PostAction::None
                            } else {
                                if let AttentionState::Awaiting {
                                    episode: prev_ep, ..
                                } = entry.state
                                {
                                    self.tab_state.attention_resolved(&entry.run, prev_ep);
                                }
                                entry.episode += 1;
                                let episode = entry.episode;
                                entry.active_episode_key = Some(ep_key);
                                entry.acknowledged = false;
                                let safe_msg = Some(Self::safe_local_message(&descriptor, kind));
                                entry.state = AttentionState::Awaiting {
                                    kind,
                                    episode,
                                    since_ms: now_ms,
                                    source: AttentionSource::NativeHook,
                                    message: safe_msg.clone(),
                                };
                                entry.revision += 1;
                                let snap = Self::to_snapshot(entry);
                                let title = descriptor.display_name.clone();
                                let body = safe_msg.unwrap_or_default();
                                PostAction::EnterAwaiting {
                                    snap,
                                    run: entry.run.clone(),
                                    episode,
                                    kind,
                                    title,
                                    body,
                                }
                            }
                        } else {
                            PostAction::None
                        }
                    }
                }

                AttentionSignal::HookStop { is_subagent } => {
                    if is_subagent {
                        PostAction::None
                    } else {
                        let notify_info =
                            if matches!(settings.on_turn_complete, AttentionLevel::Notify) {
                                Some((
                                    descriptor.display_name.clone(),
                                    format!("{} concluiu o turno", descriptor.display_name),
                                ))
                            } else {
                                None
                            };
                        match entry.state.clone() {
                            AttentionState::Awaiting { episode, .. } => {
                                entry.state = AttentionState::Working;
                                entry.active_episode_key = None;
                                entry.acknowledged = false;
                                entry.tail.clear();
                                entry.revision += 1;
                                let snap = Self::to_snapshot(entry);
                                PostAction::ResolveAwaiting {
                                    snap,
                                    run: entry.run.clone(),
                                    episode,
                                    notify_turn_complete: notify_info,
                                }
                            }
                            AttentionState::Suspected { .. } => {
                                entry.state = AttentionState::Working;
                                entry.acknowledged = false;
                                entry.tail.clear();
                                entry.revision += 1;
                                let snap = Self::to_snapshot(entry);
                                if let Some((title, body)) = notify_info {
                                    PostAction::TurnCompleteOnly {
                                        snap: Some(snap),
                                        title,
                                        body,
                                    }
                                } else {
                                    PostAction::Emit(snap)
                                }
                            }
                            _ => {
                                if let Some((title, body)) = notify_info {
                                    PostAction::TurnCompleteOnly {
                                        snap: None,
                                        title,
                                        body,
                                    }
                                } else {
                                    PostAction::None
                                }
                            }
                        }
                    }
                }

                AttentionSignal::CodexTurnComplete { .. } => {
                    let notify_info = if matches!(settings.on_turn_complete, AttentionLevel::Notify)
                    {
                        Some((
                            descriptor.display_name.clone(),
                            format!("{} concluiu o turno", descriptor.display_name),
                        ))
                    } else {
                        None
                    };
                    if matches!(entry.state, AttentionState::Suspected { .. }) {
                        entry.state = AttentionState::Working;
                        entry.acknowledged = false;
                        entry.tail.clear();
                        entry.revision += 1;
                        let snap = Self::to_snapshot(entry);
                        if let Some((title, body)) = notify_info {
                            PostAction::TurnCompleteOnly {
                                snap: Some(snap),
                                title,
                                body,
                            }
                        } else {
                            PostAction::Emit(snap)
                        }
                    } else if let Some((title, body)) = notify_info {
                        PostAction::TurnCompleteOnly {
                            snap: None,
                            title,
                            body,
                        }
                    } else {
                        PostAction::None
                    }
                }

                AttentionSignal::HookUserPromptSubmit => match entry.state.clone() {
                    AttentionState::Awaiting {
                        kind: AttentionKind::AwaitingInput,
                        episode,
                        ..
                    } => {
                        entry.state = AttentionState::Working;
                        entry.active_episode_key = None;
                        entry.acknowledged = false;
                        entry.tail.clear();
                        entry.revision += 1;
                        let snap = Self::to_snapshot(entry);
                        PostAction::ResolveAwaiting {
                            snap,
                            run: entry.run.clone(),
                            episode,
                            notify_turn_complete: None,
                        }
                    }
                    AttentionState::Suspected { .. } => {
                        entry.state = AttentionState::Working;
                        entry.acknowledged = false;
                        entry.tail.clear();
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    }
                    _ => PostAction::None,
                },

                AttentionSignal::HookPreToolUse => PostAction::None,

                AttentionSignal::ToolActivityConfirmed => match entry.state.clone() {
                    AttentionState::Awaiting { episode, .. } => {
                        entry.state = AttentionState::Working;
                        entry.active_episode_key = None;
                        entry.acknowledged = false;
                        entry.tail.clear();
                        entry.revision += 1;
                        let snap = Self::to_snapshot(entry);
                        PostAction::ResolveAwaiting {
                            snap,
                            run: entry.run.clone(),
                            episode,
                            notify_turn_complete: None,
                        }
                    }
                    AttentionState::Suspected { .. } => {
                        entry.state = AttentionState::Working;
                        entry.acknowledged = false;
                        entry.tail.clear();
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    }
                    _ => PostAction::None,
                },

                AttentionSignal::OscNotify { .. } | AttentionSignal::Bell => {
                    if matches!(entry.state, AttentionState::Working) && entry.foreground_is_agent {
                        entry.state = AttentionState::Suspected {
                            kind: AttentionKind::AwaitingInput,
                            since_ms: now_ms,
                            source: AttentionSource::OscNotification,
                            pattern_id: None,
                        };
                        entry.acknowledged = false;
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    } else {
                        PostAction::None
                    }
                }

                AttentionSignal::PatternMatch { pattern_id, kind } => {
                    if matches!(entry.state, AttentionState::Working) && entry.foreground_is_agent {
                        entry.state = AttentionState::Suspected {
                            kind,
                            since_ms: now_ms,
                            source: AttentionSource::Pattern,
                            pattern_id: Some(pattern_id),
                        };
                        entry.acknowledged = false;
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    } else {
                        PostAction::None
                    }
                }

                AttentionSignal::OutputBytes { bytes } => {
                    entry.last_output_ms = now_ms;
                    entry.evaluated_silence = false;
                    entry.tail.push_bytes(&bytes);
                    let (osc_msgs, has_bel) = parse_osc_and_bel(&bytes);
                    if (!osc_msgs.is_empty() || has_bel)
                        && matches!(entry.state, AttentionState::Working)
                        && entry.foreground_is_agent
                    {
                        entry.state = AttentionState::Suspected {
                            kind: AttentionKind::AwaitingInput,
                            since_ms: now_ms,
                            source: AttentionSource::OscNotification,
                            pattern_id: None,
                        };
                        entry.acknowledged = false;
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    } else {
                        PostAction::None
                    }
                }

                AttentionSignal::UserInput => {
                    if matches!(
                        entry.state,
                        AttentionState::Awaiting { .. } | AttentionState::Suspected { .. }
                    ) && !entry.acknowledged
                    {
                        entry.acknowledged = true;
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    } else {
                        PostAction::None
                    }
                }

                AttentionSignal::Resolved { run_id, episode } => {
                    if entry.run.run_id == run_id {
                        if let AttentionState::Awaiting {
                            episode: current_ep,
                            ..
                        } = entry.state
                        {
                            if current_ep == episode {
                                entry.state = AttentionState::Working;
                                entry.active_episode_key = None;
                                entry.acknowledged = false;
                                entry.tail.clear();
                                entry.revision += 1;
                                let snap = Self::to_snapshot(entry);
                                PostAction::ResolveAwaiting {
                                    snap,
                                    run: entry.run.clone(),
                                    episode,
                                    notify_turn_complete: None,
                                }
                            } else {
                                PostAction::None
                            }
                        } else {
                            PostAction::None
                        }
                    } else {
                        PostAction::None
                    }
                }

                AttentionSignal::Foreground { is_agent } => {
                    entry.foreground_is_agent = is_agent;
                    if !is_agent && matches!(entry.state, AttentionState::Working) {
                        entry.state = AttentionState::Inactive;
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    } else if is_agent && matches!(entry.state, AttentionState::Inactive) {
                        entry.state = AttentionState::Working;
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    } else {
                        PostAction::None
                    }
                }

                AttentionSignal::ExecutionEnded => match entry.state.clone() {
                    AttentionState::Awaiting { episode, .. } => {
                        entry.state = AttentionState::Inactive;
                        entry.active_episode_key = None;
                        entry.acknowledged = false;
                        entry.tail.clear();
                        entry.revision += 1;
                        let snap = Self::to_snapshot(entry);
                        PostAction::ResolveAwaiting {
                            snap,
                            run: entry.run.clone(),
                            episode,
                            notify_turn_complete: None,
                        }
                    }
                    _ => {
                        entry.state = AttentionState::Inactive;
                        entry.acknowledged = false;
                        entry.tail.clear();
                        entry.revision += 1;
                        PostAction::Emit(Self::to_snapshot(entry))
                    }
                },
            }
        };

        match action {
            PostAction::None => {}
            PostAction::Emit(snap) => {
                self.sink.emit(snap);
            }
            PostAction::EnterAwaiting {
                snap,
                run,
                episode,
                kind,
                title,
                body,
            } => {
                self.tab_state.attention_required(&run, episode);
                self.notify.set_attention(run.tab_id, kind);
                self.sink.emit(snap);
                if settings.notify && !self.notify.is_tab_focused_and_active(run.tab_id) {
                    self.notify.notify(run.tab_id, &title, &body).await;
                }
            }
            PostAction::ResolveAwaiting {
                snap,
                run,
                episode,
                notify_turn_complete,
            } => {
                self.tab_state.attention_resolved(&run, episode);
                self.notify.clear_attention(run.tab_id);
                self.sink.emit(snap);
                if let Some((title, body)) = notify_turn_complete
                    && settings.notify
                    && !self.notify.is_tab_focused_and_active(run.tab_id)
                {
                    self.notify.notify(run.tab_id, &title, &body).await;
                }
            }
            PostAction::TurnCompleteOnly { snap, title, body } => {
                if let Some(s) = snap {
                    self.sink.emit(s);
                }
                if settings.notify && !self.notify.is_tab_focused_and_active(tab_id) {
                    self.notify.notify(tab_id, &title, &body).await;
                }
            }
        }
    }

    /// Avalia padrões regex de duas âncoras após `silence_ms` sem novo output.
    pub fn on_silence_tick(&self, now_ms: i64) {
        let settings = self.settings.read().clone();
        if !settings.enabled {
            return;
        }
        let silence_ms = i64::from(settings.silence_ms);

        let mut emissions = Vec::new();
        {
            let mut guard = self.tabs.write();
            for entry in guard.values_mut() {
                if entry.evaluated_silence || !matches!(entry.state, AttentionState::Working) {
                    continue;
                }
                if now_ms.saturating_sub(entry.last_output_ms) < silence_ms {
                    continue;
                }
                entry.evaluated_silence = true;

                let is_fg = if let Some(fg) = self.pty.foreground(entry.run.pty_id) {
                    let desc = self.registry.get(entry.run.agent);
                    desc.process_names.iter().any(|p| p == &fg.comm)
                        || fg.cmdline.iter().any(|arg| arg.contains(&desc.binary))
                } else {
                    entry.foreground_is_agent
                };

                if !is_fg {
                    continue;
                }

                if let Some((pattern_id, kind)) = self
                    .patterns
                    .match_tail(entry.run.agent, entry.tail.as_str())
                {
                    entry.state = AttentionState::Suspected {
                        kind,
                        since_ms: now_ms,
                        source: AttentionSource::Pattern,
                        pattern_id: Some(pattern_id),
                    };
                    entry.acknowledged = false;
                    entry.revision += 1;
                    emissions.push(Self::to_snapshot(entry));
                }
            }
        }

        for snap in emissions {
            self.sink.emit(snap);
        }
    }

    fn safe_local_message(descriptor: &AgentDescriptor, kind: AttentionKind) -> String {
        match kind {
            AttentionKind::AwaitingApproval => {
                format!("{} aguarda aprovação de permissão", descriptor.display_name)
            }
            AttentionKind::AwaitingInput => {
                format!("{} aguarda resposta do usuário", descriptor.display_name)
            }
        }
    }

    fn to_snapshot(entry: &TabAttentionEntry) -> AttentionSnapshot {
        AttentionSnapshot {
            tab_id: entry.run.tab_id,
            pane_id: entry.run.pane_id,
            project_id: entry.run.project_id,
            session_id: entry.run.session_id,
            run_id: entry.run.run_id.clone(),
            agent: entry.run.agent,
            revision: entry.revision,
            acknowledged: entry.acknowledged,
            state: entry.state.clone(),
        }
    }
}
