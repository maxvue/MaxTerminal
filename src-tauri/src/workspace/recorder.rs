//! Gravador assíncrono de scrollback sanitizado (`SessionRecorder`) para sessões do workspace.
//!
//! Implementa o contrato §3.7 da Fase 07:
//! - Sanitização em streaming obrigatória (`StreamSanitizer`, que já incorpora `Utf8Decoder` e
//!   `TerminalNormalizer`) antes de enfileirar no `BatchWriter<ScrollbackRecord>`;
//! - Orçamento de bytes por `tab_session_id` (default 8 MiB configurável em `workspace.scrollback.limit_mib`);
//! - Flush com barreira (`flush_pty` / `flush_tab_session` / `flush_all`) antes de arquivar sessões ou consultar scrollback.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use mt_core::ids::{PaneId, PtyId, SessionId, TabId, TabSessionId};
use mt_core::workspace::ScrollbackChunkKind;
use mt_db::sanitizer::{SanitizedText, StreamSanitizer};
use mt_db::{BatchPolicy, BatchWriter, Db, DbResult, ScrollbackRecord};
use mt_pty::OutputTap;
use parking_lot::Mutex;

/// Tamanho máximo de um chunk de scrollback antes de forçar emissão (16 KiB).
pub const MAX_CHUNK_ACCUM_BYTES: usize = 16 * 1024;
/// Intervalo máximo de retenção em memória antes do flush periódico (250 ms).
pub const CHUNK_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
/// Orçamento padrão de scrollback por sessão de aba (8 MiB).
pub const DEFAULT_TAB_SESSION_BUDGET_BYTES: i64 = 8 * 1024 * 1024;

#[derive(Debug)]
struct PaneRecordingState {
    #[allow(dead_code)]
    tab_id: TabId,
    #[allow(dead_code)]
    pane_id: PaneId,
    session_id: SessionId,
    tab_session_id: TabSessionId,
    #[allow(dead_code)]
    pty_id: PtyId,
    next_seq: i64,
    buffer: String,
    last_flush: Instant,
    sanitizer: StreamSanitizer,
}

#[derive(Debug)]
struct TabSessionBudgetState {
    emitted_bytes: i64,
    limit_bytes: i64,
    truncated_marker_emitted: bool,
}

/// Gravador de scrollback sanitizado acoplado ao `BatchWriter<ScrollbackRecord>`.
pub struct SessionRecorder {
    db: Db,
    batch_writer: BatchWriter<ScrollbackRecord>,
    panes_by_pty: Mutex<HashMap<PtyId, PaneRecordingState>>,
    pty_by_session: Mutex<HashMap<SessionId, PtyId>>,
    budgets: Mutex<HashMap<TabSessionId, TabSessionBudgetState>>,
}

impl std::fmt::Debug for SessionRecorder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionRecorder")
            .field("active_ptys", &self.panes_by_pty.lock().len())
            .finish_non_exhaustive()
    }
}

impl SessionRecorder {
    /// Cria um novo `SessionRecorder` registrando seu `BatchWriter` na fachada `Db`.
    pub fn new(db: Db) -> DbResult<Arc<Self>> {
        let policy = BatchPolicy {
            max_delay: Duration::from_millis(200),
            max_records: 128,
            max_bytes: 512 * 1024,
            queue_capacity: 1024,
            queue_bytes: 4 * 1024 * 1024,
        };
        let batch_writer = db.batch_writer::<ScrollbackRecord>(policy)?;

        let recorder = Arc::new(Self {
            db,
            batch_writer,
            panes_by_pty: Mutex::new(HashMap::new()),
            pty_by_session: Mutex::new(HashMap::new()),
            budgets: Mutex::new(HashMap::new()),
        });

        Self::spawn_periodic_flusher(Arc::clone(&recorder));
        Ok(recorder)
    }

    fn spawn_periodic_flusher(recorder: Arc<Self>) {
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                let mut ticker = tokio::time::interval(CHUNK_FLUSH_INTERVAL);
                ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    ticker.tick().await;
                    recorder.flush_expired_buffers();
                }
            });
        }
    }

    fn resolve_budget_limit(&self) -> i64 {
        if let Ok(settings) = self.db.settings()
            && let Ok(snap) = settings.get_snapshot("workspace.scrollback.limit_mib")
            && let Some(mib) = snap.value.as_i64()
            && (1..=256).contains(&mib)
        {
            return mib * 1024 * 1024;
        }
        DEFAULT_TAB_SESSION_BUDGET_BYTES
    }

    /// Registra o início da gravação de um PTY vinculado a `(tab_id, pane_id, session_id, tab_session_id)`.
    pub fn register_pty(
        &self,
        tab_id: TabId,
        pane_id: PaneId,
        session_id: SessionId,
        tab_session_id: TabSessionId,
        pty_id: PtyId,
    ) {
        let limit_bytes = self.resolve_budget_limit();
        {
            let mut budgets = self.budgets.lock();
            budgets
                .entry(tab_session_id)
                .or_insert(TabSessionBudgetState {
                    emitted_bytes: 0,
                    limit_bytes,
                    truncated_marker_emitted: false,
                });
        }

        let state = PaneRecordingState {
            tab_id,
            pane_id,
            session_id,
            tab_session_id,
            pty_id,
            next_seq: 1,
            buffer: String::new(),
            last_flush: Instant::now(),
            sanitizer: StreamSanitizer::new(),
        };

        self.panes_by_pty.lock().insert(pty_id, state);
        self.pty_by_session.lock().insert(session_id, pty_id);
    }

    /// Ingere bytes brutos emitidos pelo PTY (`on_output`).
    pub fn on_pty_output(&self, pty_id: PtyId, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let mut to_enqueue = Vec::new();
        let mut truncated_sessions = Vec::new();

        {
            let mut panes = self.panes_by_pty.lock();
            let Some(state) = panes.get_mut(&pty_id) else {
                return;
            };

            let sanitized = state.sanitizer.push(chunk);
            if !sanitized.is_empty() {
                state.buffer.push_str(&sanitized);
            }

            while state.buffer.len() >= MAX_CHUNK_ACCUM_BYTES {
                let mut split_pos = state.buffer[..MAX_CHUNK_ACCUM_BYTES]
                    .rfind('\n')
                    .map_or(MAX_CHUNK_ACCUM_BYTES, |idx| idx + 1);
                while split_pos > 0 && !state.buffer.is_char_boundary(split_pos) {
                    split_pos -= 1;
                }
                if split_pos == 0 {
                    break;
                }
                let piece: String = state.buffer.drain(..split_pos).collect();

                let (rec_opt, newly_truncated) = self.build_record_under_budget(
                    state.session_id,
                    state.tab_session_id,
                    &mut state.next_seq,
                    ScrollbackChunkKind::Output,
                    piece,
                );
                if newly_truncated {
                    truncated_sessions.push(state.session_id);
                }
                if let Some(rec) = rec_opt {
                    to_enqueue.push(rec);
                }
                state.last_flush = Instant::now();
            }
        }

        for rec in to_enqueue {
            let _ = self.batch_writer.push(rec);
        }
        self.mark_truncated_sessions(truncated_sessions);
    }

    /// Registra um marcador explícito (ex.: comando inicial despachado) no scrollback da sessão.
    pub fn record_system_marker(&self, session_id: SessionId, marker_text: String) {
        let pty_id = {
            let map = self.pty_by_session.lock();
            map.get(&session_id).copied()
        };
        let Some(pty_id) = pty_id else {
            return;
        };

        let mut to_enqueue = Vec::new();
        let mut truncated_sessions = Vec::new();
        {
            let mut panes = self.panes_by_pty.lock();
            let Some(state) = panes.get_mut(&pty_id) else {
                return;
            };

            if !state.buffer.is_empty() {
                let pending = std::mem::take(&mut state.buffer);
                let (rec_opt, newly_truncated) = self.build_record_under_budget(
                    state.session_id,
                    state.tab_session_id,
                    &mut state.next_seq,
                    ScrollbackChunkKind::Output,
                    pending,
                );
                if newly_truncated {
                    truncated_sessions.push(state.session_id);
                }
                if let Some(rec) = rec_opt {
                    to_enqueue.push(rec);
                }
            }

            let mut marker_with_nl = marker_text;
            if !marker_with_nl.ends_with('\n') {
                marker_with_nl.push('\n');
            }
            let (rec_opt, newly_truncated) = self.build_record_under_budget(
                state.session_id,
                state.tab_session_id,
                &mut state.next_seq,
                ScrollbackChunkKind::Output,
                marker_with_nl,
            );
            if newly_truncated {
                truncated_sessions.push(state.session_id);
            }
            if let Some(rec) = rec_opt {
                to_enqueue.push(rec);
            }
            state.last_flush = Instant::now();
        }

        for rec in to_enqueue {
            let _ = self.batch_writer.push(rec);
        }
        self.mark_truncated_sessions(truncated_sessions);
    }

    fn flush_expired_buffers(&self) {
        let now = Instant::now();
        let mut to_enqueue = Vec::new();
        let mut truncated_sessions = Vec::new();

        {
            let mut panes = self.panes_by_pty.lock();
            for state in panes.values_mut() {
                if !state.buffer.is_empty()
                    && now.duration_since(state.last_flush) >= CHUNK_FLUSH_INTERVAL
                {
                    let pending = std::mem::take(&mut state.buffer);
                    let (rec_opt, newly_truncated) = self.build_record_under_budget(
                        state.session_id,
                        state.tab_session_id,
                        &mut state.next_seq,
                        ScrollbackChunkKind::Output,
                        pending,
                    );
                    if newly_truncated {
                        truncated_sessions.push(state.session_id);
                    }
                    if let Some(rec) = rec_opt {
                        to_enqueue.push(rec);
                    }
                    state.last_flush = now;
                }
            }
        }

        for rec in to_enqueue {
            let _ = self.batch_writer.push(rec);
        }
        self.mark_truncated_sessions(truncated_sessions);
    }

    fn drain_pty_state(
        &self,
        mut state: PaneRecordingState,
    ) -> (Vec<ScrollbackRecord>, Vec<SessionId>) {
        let mut records = Vec::new();
        let mut truncated_sessions = Vec::new();

        let tail_san = state.sanitizer.finish();
        if !tail_san.is_empty() {
            state.buffer.push_str(&tail_san);
        }

        if !state.buffer.is_empty() {
            let pending = std::mem::take(&mut state.buffer);
            let (rec_opt, newly_truncated) = self.build_record_under_budget(
                state.session_id,
                state.tab_session_id,
                &mut state.next_seq,
                ScrollbackChunkKind::Output,
                pending,
            );
            if newly_truncated {
                truncated_sessions.push(state.session_id);
            }
            if let Some(rec) = rec_opt {
                records.push(rec);
            }
        }
        (records, truncated_sessions)
    }

    /// Finaliza o estado de gravação de um PTY e enfileira o resíduo final no `BatchWriter`.
    pub fn unregister_pty(&self, pty_id: PtyId) {
        let removed = self.panes_by_pty.lock().remove(&pty_id);
        if let Some(state) = removed {
            self.pty_by_session.lock().remove(&state.session_id);
            let (recs, truncs) = self.drain_pty_state(state);
            for rec in recs {
                let _ = self.batch_writer.push(rec);
            }
            self.mark_truncated_sessions(truncs);
        }
    }

    /// Drena todos os painéis de uma `tab_session_id` e aguarda a barreira do `BatchWriter`.
    pub async fn flush_tab_session(&self, tab_session_id: TabSessionId) -> DbResult<()> {
        let mut to_enqueue = Vec::new();
        let mut truncated_sessions = Vec::new();
        {
            let mut panes = self.panes_by_pty.lock();
            let mut sessions = self.pty_by_session.lock();
            let matching_ptys: Vec<PtyId> = panes
                .iter()
                .filter(|(_, s)| s.tab_session_id == tab_session_id)
                .map(|(k, _)| *k)
                .collect();

            for pty_id in matching_ptys {
                if let Some(state) = panes.remove(&pty_id) {
                    sessions.remove(&state.session_id);
                    let (recs, truncs) = self.drain_pty_state(state);
                    to_enqueue.extend(recs);
                    truncated_sessions.extend(truncs);
                }
            }
        }

        for rec in to_enqueue {
            let _ = self.batch_writer.push(rec);
        }
        for sid in truncated_sessions {
            let _ = self.db.scrollback().mark_session_truncated(sid).await;
        }

        self.budgets.lock().remove(&tab_session_id);
        self.batch_writer.flush().await
    }

    /// Drena um PTY específico e aguarda a barreira do `BatchWriter`.
    pub async fn flush_pty(&self, pty_id: PtyId) -> DbResult<()> {
        self.unregister_pty(pty_id);
        self.batch_writer.flush().await
    }

    /// Força o flush imediato de todos os buffers em memória para o SQLite.
    pub async fn flush_all(&self) -> DbResult<()> {
        let mut to_enqueue = Vec::new();
        let mut truncated_sessions = Vec::new();
        {
            let mut panes = self.panes_by_pty.lock();
            for state in panes.values_mut() {
                if !state.buffer.is_empty() {
                    let pending = std::mem::take(&mut state.buffer);
                    let (rec_opt, newly_truncated) = self.build_record_under_budget(
                        state.session_id,
                        state.tab_session_id,
                        &mut state.next_seq,
                        ScrollbackChunkKind::Output,
                        pending,
                    );
                    if newly_truncated {
                        truncated_sessions.push(state.session_id);
                    }
                    if let Some(rec) = rec_opt {
                        to_enqueue.push(rec);
                    }
                    state.last_flush = Instant::now();
                }
            }
        }

        for rec in to_enqueue {
            let _ = self.batch_writer.push(rec);
        }
        for sid in truncated_sessions {
            let _ = self.db.scrollback().mark_session_truncated(sid).await;
        }
        self.batch_writer.flush().await
    }

    fn mark_truncated_sessions(&self, sessions: Vec<SessionId>) {
        if sessions.is_empty() {
            return;
        }
        let db = self.db.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                for sid in sessions {
                    let _ = db.scrollback().mark_session_truncated(sid).await;
                }
            });
        }
    }

    #[allow(clippy::needless_pass_by_value)]
    fn build_record_under_budget(
        &self,
        session_id: SessionId,
        tab_session_id: TabSessionId,
        next_seq: &mut i64,
        kind: ScrollbackChunkKind,
        content: String,
    ) -> (Option<ScrollbackRecord>, bool) {
        let now_ms = mt_db::time::now_unix_ms();
        let mut budgets = self.budgets.lock();
        let budget = budgets
            .entry(tab_session_id)
            .or_insert_with(|| TabSessionBudgetState {
                emitted_bytes: 0,
                limit_bytes: DEFAULT_TAB_SESSION_BUDGET_BYTES,
                truncated_marker_emitted: false,
            });

        let sanitized_text = SanitizedText::new(&content);
        let content_bytes = i64::try_from(sanitized_text.len()).unwrap_or(i64::MAX);

        if budget.emitted_bytes < budget.limit_bytes {
            budget.emitted_bytes = budget.emitted_bytes.saturating_add(content_bytes);
            let seq = *next_seq;
            *next_seq += 1;
            return (
                Some(ScrollbackRecord::new(
                    session_id,
                    seq,
                    now_ms,
                    sanitized_text,
                    kind,
                )),
                false,
            );
        }

        if !budget.truncated_marker_emitted {
            budget.truncated_marker_emitted = true;
            let marker = SanitizedText::new(
                "[Scrollback truncado: limite máximo de armazenamento da sessão atingido]\n",
            );
            let seq = *next_seq;
            *next_seq += 1;
            return (
                Some(ScrollbackRecord::new(
                    session_id,
                    seq,
                    now_ms,
                    marker,
                    ScrollbackChunkKind::Gap,
                )),
                true,
            );
        }

        (None, false)
    }
}

impl OutputTap for SessionRecorder {
    fn on_output(&self, pty_id: PtyId, _seq: u64, bytes: &[u8]) {
        self.on_pty_output(pty_id, bytes);
    }
}
