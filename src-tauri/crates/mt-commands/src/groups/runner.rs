//! Executor de grupos no modo Paralelo em Background (`ParallelRunner`) e sink de saída
//! com ring buffer sanitizado de 256 KiB (`GroupRunSink`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mt_core::{MtResult, PtyId};
use mt_db::sanitizer::{Utf8Decoder, floor_char_boundary, sanitize};
use parking_lot::Mutex;
use tokio::sync::{Notify, Semaphore};

use crate::groups::events::{
    GroupEventSink, GroupOutputFrame, GroupOutputFrameKind, GroupRunEvent, OutputAttachReceipt,
    OutputFrameSubscriber,
};
use crate::groups::model::{
    GroupErrorCode, GroupRun, GroupRunId, GroupRunItem, GroupRunItemId, ItemStatus,
    OutputSubscriptionId,
};
use crate::groups::ports::{GroupPtyPort, HeadlessSpawnSpec, RawOutputTap};
use crate::groups::repo::{CommandGroupRepo, RunItemPatch};

/// Limite global de PTYs simultâneos em background no modo Paralelo.
pub const GLOBAL_PARALLEL_PTY_LIMIT: usize = 16;
/// Limite máximo da cauda de saída em memória e no SQLite por item (`256 KiB`).
pub const MAX_ITEM_OUTPUT_TAIL_BYTES: usize = 256 * 1024;
/// Limite máximo de uma linha sem `\n` antes de sofrer flush sanitizado conservador (`16 KiB`).
pub const MAX_UNTERMINATED_LINE_BYTES: usize = 16 * 1024;
/// Margem retida ao fazer flush conservador de linha sem `\n` para não partir segredos na fronteira.
const UNTERMINATED_SAFETY_OVERLAP: usize = 2048;

struct SubscriberEntry {
    subscription_id: OutputSubscriptionId,
    subscriber: Arc<dyn OutputFrameSubscriber>,
    pending_dropped_bytes: u64,
}

struct SinkInner {
    decoder: Utf8Decoder,
    pending_line: String,
    ring_buffer: String,
    ring_start_seq: u64,
    total_emitted_bytes: u64,
    truncated: bool,
    eof: bool,
    subscribers: HashMap<OutputSubscriptionId, SubscriberEntry>,
}

/// Sink de saída por item com sanitização incremental, ring buffer UTF-8 limitado a 256 KiB
/// e suporte a `attach`/`detach` atômico sem bloquear o leitor do PTY.
pub struct GroupRunSink {
    run_id: GroupRunId,
    item_id: GroupRunItemId,
    inner: Mutex<SinkInner>,
}

impl GroupRunSink {
    /// Cria um novo `GroupRunSink` para o par `(run_id, item_id)`.
    #[must_use]
    pub fn new(run_id: GroupRunId, item_id: GroupRunItemId) -> Arc<Self> {
        Arc::new(Self {
            run_id,
            item_id,
            inner: Mutex::new(SinkInner {
                decoder: Utf8Decoder::new(),
                pending_line: String::new(),
                ring_buffer: String::new(),
                ring_start_seq: 0,
                total_emitted_bytes: 0,
                truncated: false,
                eof: false,
                subscribers: HashMap::new(),
            }),
        })
    }

    /// Identificador da execução associada.
    #[must_use]
    pub const fn run_id(&self) -> &GroupRunId {
        &self.run_id
    }

    /// Identificador do item associado.
    #[must_use]
    pub const fn item_id(&self) -> &GroupRunItemId {
        &self.item_id
    }

    /// Retorna `(cauda_sanitizada, foi_truncada)` para persistência em `command_group_run_items`.
    #[must_use]
    pub fn snapshot_tail(&self) -> (String, bool) {
        let guard = self.inner.lock();
        if guard.pending_line.is_empty() {
            return (guard.ring_buffer.clone(), guard.truncated);
        }
        let mut combined = guard.ring_buffer.clone();
        let clean_pending = sanitize(&guard.pending_line);
        combined.push_str(&clean_pending);
        let mut truncated = guard.truncated;
        if combined.len() > MAX_ITEM_OUTPUT_TAIL_BYTES {
            truncated = true;
            let mut cut_start = combined.len() - MAX_ITEM_OUTPUT_TAIL_BYTES;
            while cut_start < combined.len() && !combined.is_char_boundary(cut_start) {
                cut_start += 1;
            }
            combined.drain(..cut_start);
        }
        (combined, truncated)
    }

    /// Captura atomicamente o watermark do ring buffer, envia frames `Replay` (e `Gap` se truncado
    /// ou se `after_seq` já saiu da janela de 256 KiB) e registra o assinante para frames `Data`/`End`
    /// subsequentes sem duplicar nem perder bytes na fronteira replay/live.
    pub fn attach(
        &self,
        subscription_id: OutputSubscriptionId,
        after_seq: Option<u64>,
        subscriber: Arc<dyn OutputFrameSubscriber>,
    ) -> OutputAttachReceipt {
        let mut guard = self.inner.lock();
        let requested_after = after_seq.unwrap_or(0);
        let replay_start_seq = requested_after.max(guard.ring_start_seq);
        let replay_end_seq = guard.total_emitted_bytes;
        let truncated = guard.truncated;

        if requested_after < guard.ring_start_seq {
            let dropped = guard.ring_start_seq - requested_after;
            let _ = subscriber.send_frame(GroupOutputFrame {
                subscription_id: subscription_id.clone(),
                item_id: self.item_id.clone(),
                seq: guard.ring_start_seq,
                kind: GroupOutputFrameKind::Gap,
                text: None,
                dropped_bytes: Some(dropped),
            });
        }

        if replay_end_seq > replay_start_seq && !guard.ring_buffer.is_empty() {
            let rel_offset = usize::try_from(replay_start_seq - guard.ring_start_seq).unwrap_or(0);
            let safe_offset = ceil_char_boundary(&guard.ring_buffer, rel_offset);
            if safe_offset < guard.ring_buffer.len() {
                let slice = guard.ring_buffer[safe_offset..].to_string();
                let _ = subscriber.send_frame(GroupOutputFrame {
                    subscription_id: subscription_id.clone(),
                    item_id: self.item_id.clone(),
                    seq: replay_end_seq,
                    kind: GroupOutputFrameKind::Replay,
                    text: Some(slice),
                    dropped_bytes: None,
                });
            }
        }

        if guard.eof {
            let _ = subscriber.send_frame(GroupOutputFrame {
                subscription_id: subscription_id.clone(),
                item_id: self.item_id.clone(),
                seq: replay_end_seq,
                kind: GroupOutputFrameKind::End,
                text: None,
                dropped_bytes: None,
            });
        } else {
            guard.subscribers.insert(
                subscription_id.clone(),
                SubscriberEntry {
                    subscription_id: subscription_id.clone(),
                    subscriber,
                    pending_dropped_bytes: 0,
                },
            );
        }

        OutputAttachReceipt {
            subscription_id,
            replay_start_seq,
            replay_end_seq,
            truncated,
        }
    }

    /// Remove a assinatura sem cancelar o item em execução.
    pub fn detach(&self, subscription_id: &OutputSubscriptionId) {
        let mut guard = self.inner.lock();
        guard.subscribers.remove(subscription_id);
    }

    fn append_sanitized_locked(guard: &mut SinkInner, item_id: &GroupRunItemId, clean: &str) {
        if clean.is_empty() {
            return;
        }
        let chunk_bytes = clean.len() as u64;
        guard.total_emitted_bytes = guard.total_emitted_bytes.saturating_add(chunk_bytes);
        guard.ring_buffer.push_str(clean);

        if guard.ring_buffer.len() > MAX_ITEM_OUTPUT_TAIL_BYTES {
            guard.truncated = true;
            let mut drop_len = guard.ring_buffer.len() - MAX_ITEM_OUTPUT_TAIL_BYTES;
            while drop_len < guard.ring_buffer.len()
                && !guard.ring_buffer.is_char_boundary(drop_len)
            {
                drop_len += 1;
            }
            guard.ring_buffer.drain(..drop_len);
            guard.ring_start_seq = guard.total_emitted_bytes - (guard.ring_buffer.len() as u64);
        }

        let seq = guard.total_emitted_bytes;
        let text_owned = clean.to_string();

        guard.subscribers.retain(|_, sub| {
            if sub.pending_dropped_bytes > 0 {
                let gap_ok = sub.subscriber.send_frame(GroupOutputFrame {
                    subscription_id: sub.subscription_id.clone(),
                    item_id: item_id.clone(),
                    seq: seq.saturating_sub(chunk_bytes),
                    kind: GroupOutputFrameKind::Gap,
                    text: None,
                    dropped_bytes: Some(sub.pending_dropped_bytes),
                });
                if gap_ok {
                    sub.pending_dropped_bytes = 0;
                } else {
                    sub.pending_dropped_bytes =
                        sub.pending_dropped_bytes.saturating_add(chunk_bytes);
                    return true;
                }
            }

            let sent = sub.subscriber.send_frame(GroupOutputFrame {
                subscription_id: sub.subscription_id.clone(),
                item_id: item_id.clone(),
                seq,
                kind: GroupOutputFrameKind::Data,
                text: Some(text_owned.clone()),
                dropped_bytes: None,
            });
            if !sent {
                sub.pending_dropped_bytes = sub.pending_dropped_bytes.saturating_add(chunk_bytes);
            }
            true
        });
    }
}

impl RawOutputTap for GroupRunSink {
    fn on_bytes(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let mut guard = self.inner.lock();
        if guard.eof {
            return;
        }
        let decoded = guard.decoder.push(chunk).to_string();
        if !decoded.is_empty() {
            let normalized = decoded.replace("\r\n", "\n");
            guard.pending_line.push_str(&normalized);
        }

        if let Some(last_nl) = guard.pending_line.rfind('\n') {
            let complete_lines = guard.pending_line[..=last_nl].to_string();
            guard.pending_line.drain(..=last_nl);
            let sanitized = sanitize(&complete_lines).into_owned();
            Self::append_sanitized_locked(&mut guard, &self.item_id, &sanitized);
        }

        if guard.pending_line.len() > MAX_UNTERMINATED_LINE_BYTES {
            let safe_len = guard
                .pending_line
                .len()
                .saturating_sub(UNTERMINATED_SAFETY_OVERLAP);
            let mut cut = floor_char_boundary(&guard.pending_line, safe_len);
            if let Some(ws_pos) = guard.pending_line[..cut].rfind([' ', '\t']) {
                if ws_pos > 0 {
                    cut = ws_pos + 1;
                }
            }
            if cut > 0 {
                let prefix = guard.pending_line[..cut].to_string();
                guard.pending_line.drain(..cut);
                let sanitized = sanitize(&prefix).into_owned();
                Self::append_sanitized_locked(&mut guard, &self.item_id, &sanitized);
            }
        }
    }

    fn on_eof(&self) {
        let mut guard = self.inner.lock();
        if guard.eof {
            return;
        }
        let tail_utf8 = guard.decoder.finish().to_string();
        if !tail_utf8.is_empty() {
            let normalized = tail_utf8.replace("\r\n", "\n");
            guard.pending_line.push_str(&normalized);
        }
        if !guard.pending_line.is_empty() {
            let rem = std::mem::take(&mut guard.pending_line);
            let sanitized = sanitize(&rem).into_owned();
            Self::append_sanitized_locked(&mut guard, &self.item_id, &sanitized);
        }
        guard.eof = true;
        let final_seq = guard.total_emitted_bytes;
        for sub in guard.subscribers.values() {
            let _ = sub.subscriber.send_frame(GroupOutputFrame {
                subscription_id: sub.subscription_id.clone(),
                item_id: self.item_id.clone(),
                seq: final_seq,
                kind: GroupOutputFrameKind::End,
                text: None,
                dropped_bytes: None,
            });
        }
        guard.subscribers.clear();
    }
}

fn ceil_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

struct ActiveParallelItem {
    run_id: GroupRunId,
    item_id: GroupRunItemId,
    sink: Arc<GroupRunSink>,
    pty_id: Mutex<Option<PtyId>>,
    cancel_flag: AtomicBool,
    done_notify: Arc<Notify>,
    finished: AtomicBool,
}

/// Executor do modo Paralelo em Background com semáforo global de 16 PTYs.
pub struct ParallelRunner {
    semaphore: Arc<Semaphore>,
    pty_port: Arc<dyn GroupPtyPort>,
    repo: Arc<dyn CommandGroupRepo>,
    events: Arc<dyn GroupEventSink>,
    active_items: Mutex<HashMap<(GroupRunId, GroupRunItemId), Arc<ActiveParallelItem>>>,
    subscriptions: Mutex<HashMap<OutputSubscriptionId, Arc<GroupRunSink>>>,
}

impl ParallelRunner {
    /// Cria uma nova instância compartilhada de `ParallelRunner`.
    #[must_use]
    pub fn new(
        pty_port: Arc<dyn GroupPtyPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self> {
        Arc::new(Self {
            semaphore: Arc::new(Semaphore::new(GLOBAL_PARALLEL_PTY_LIMIT)),
            pty_port,
            repo,
            events,
            active_items: Mutex::new(HashMap::new()),
            subscriptions: Mutex::new(HashMap::new()),
        })
    }

    /// Registra um `GroupRunSink` externo (ex.: de `SequentialRunner` ou `MultiTabRunner`)
    /// para permitir `attach_output`/`detach_output` unificados.
    pub fn register_external_sink(
        &self,
        run_id: GroupRunId,
        item_id: GroupRunItemId,
        sink: Arc<GroupRunSink>,
    ) {
        let entry = Arc::new(ActiveParallelItem {
            run_id: run_id.clone(),
            item_id: item_id.clone(),
            sink,
            pty_id: Mutex::new(None),
            cancel_flag: AtomicBool::new(false),
            done_notify: Arc::new(Notify::new()),
            finished: AtomicBool::new(false),
        });
        self.active_items.lock().insert((run_id, item_id), entry);
    }

    /// Remove um `GroupRunSink` externo ao concluir o item.
    pub fn unregister_external_sink(&self, run_id: &GroupRunId, item_id: &GroupRunItemId) {
        if let Some(entry) = self
            .active_items
            .lock()
            .remove(&(run_id.clone(), item_id.clone()))
        {
            entry.finished.store(true, Ordering::SeqCst);
            entry.done_notify.notify_waiters();
        }
    }

    /// Inicia o processamento assíncrono dos itens da execução em background.
    pub async fn launch(self: &Arc<Self>, run: GroupRun) -> MtResult<()> {
        let run_id = run.id.clone();
        for item in run.items {
            let sink = GroupRunSink::new(run_id.clone(), item.id.clone());
            let active_entry = Arc::new(ActiveParallelItem {
                run_id: run_id.clone(),
                item_id: item.id.clone(),
                sink,
                pty_id: Mutex::new(None),
                cancel_flag: AtomicBool::new(false),
                done_notify: Arc::new(Notify::new()),
                finished: AtomicBool::new(false),
            });
            self.active_items
                .lock()
                .insert((run_id.clone(), item.id.clone()), Arc::clone(&active_entry));

            let runner = Arc::clone(self);
            tokio::spawn(async move {
                runner.run_single_item(item, active_entry).await;
            });
        }
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn run_single_item(&self, item: GroupRunItem, entry: Arc<ActiveParallelItem>) {
        let permit = match Arc::clone(&self.semaphore).acquire_owned().await {
            Ok(p) => p,
            Err(_) => {
                self.finish_entry(&entry);
                return;
            }
        };

        if entry.cancel_flag.load(Ordering::SeqCst) {
            drop(permit);
            self.finish_entry(&entry);
            return;
        }

        // Transiciona para Starting se ainda estiver Pending
        let now_start = mt_db::time::now_unix_ms();
        match self.repo.update_run_item(
            &entry.run_id,
            &entry.item_id,
            RunItemPatch {
                status: Some(ItemStatus::Starting),
                started_at: Some(Some(now_start)),
                ..RunItemPatch::default()
            },
            now_start,
        ) {
            Ok((updated_run, updated_item)) => {
                if updated_item.status.is_terminal() {
                    drop(permit);
                    self.finish_entry(&entry);
                    return;
                }
                self.emit_progress(&updated_run, &updated_item);
            }
            Err(_) => {
                drop(permit);
                self.finish_entry(&entry);
                return;
            }
        }

        let spec = HeadlessSpawnSpec {
            program: PathBuf::from(&item.snapshot.shell_path),
            args: vec!["-c".to_string(), item.snapshot.command.clone()],
            cwd: PathBuf::from(&item.snapshot.resolved_cwd),
            env: item.snapshot.env.clone(),
            login: false,
        };

        let tap: Arc<dyn RawOutputTap> = Arc::clone(&entry.sink) as Arc<dyn RawOutputTap>;
        let spawn_res = self.pty_port.spawn_headless(spec, tap).await;

        let pty_id = match spawn_res {
            Ok(pid) => pid,
            Err(spawn_err) => {
                entry.sink.on_eof();
                let (tail, truncated) = entry.sink.snapshot_tail();
                let now_fail = mt_db::time::now_unix_ms();
                if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                    &entry.run_id,
                    &entry.item_id,
                    RunItemPatch {
                        status: Some(ItemStatus::Failed),
                        error_code: Some(Some(GroupErrorCode::SpawnFailed.as_str().to_string())),
                        error_message: Some(Some(spawn_err.message.to_string())),
                        finished_at: Some(Some(now_fail)),
                        output_tail: Some((tail, truncated)),
                        ..RunItemPatch::default()
                    },
                    now_fail,
                ) {
                    self.emit_progress(&updated_run, &updated_item);
                }
                drop(permit);
                self.finish_entry(&entry);
                return;
            }
        };

        *entry.pty_id.lock() = Some(pty_id.clone());

        // Se cancelamento ocorreu enquanto `spawn_headless` estava em voo, mata imediatamente
        if entry.cancel_flag.load(Ordering::SeqCst) {
            let _ = self.pty_port.kill_graceful(&pty_id).await;
            entry.sink.on_eof();
            let (tail, truncated) = entry.sink.snapshot_tail();
            let now_cancel = mt_db::time::now_unix_ms();
            if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                &entry.run_id,
                &entry.item_id,
                RunItemPatch {
                    status: Some(ItemStatus::Cancelled),
                    pty_id: Some(Some(pty_id)),
                    finished_at: Some(Some(now_cancel)),
                    output_tail: Some((tail, truncated)),
                    ..RunItemPatch::default()
                },
                now_cancel,
            ) {
                self.emit_progress(&updated_run, &updated_item);
            }
            drop(permit);
            self.finish_entry(&entry);
            return;
        }

        // Marca Running com pty_id
        let now_running = mt_db::time::now_unix_ms();
        if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
            &entry.run_id,
            &entry.item_id,
            RunItemPatch {
                status: Some(ItemStatus::Running),
                pty_id: Some(Some(pty_id.clone())),
                ..RunItemPatch::default()
            },
            now_running,
        ) {
            self.emit_progress(&updated_run, &updated_item);
        }

        // Aguarda término supervisionado (ou kill acionado por cancel)
        let exit_res = self.pty_port.wait_exit(&pty_id).await;
        entry.sink.on_eof();
        let (tail, truncated) = entry.sink.snapshot_tail();
        let now_end = mt_db::time::now_unix_ms();
        let was_cancelled = entry.cancel_flag.load(Ordering::SeqCst);

        let patch = match exit_res {
            Ok(info) => {
                let final_status = if was_cancelled {
                    ItemStatus::Cancelled
                } else if info.exit_code == Some(0) && info.signal.is_none() {
                    ItemStatus::Succeeded
                } else {
                    ItemStatus::Failed
                };
                RunItemPatch {
                    status: Some(final_status),
                    pty_id: Some(Some(pty_id)),
                    exit_code: Some(info.exit_code),
                    signal: Some(info.signal),
                    finished_at: Some(Some(now_end)),
                    output_tail: Some((tail, truncated)),
                    ..RunItemPatch::default()
                }
            }
            Err(wait_err) => {
                let final_status = if was_cancelled {
                    ItemStatus::Cancelled
                } else {
                    ItemStatus::Interrupted
                };
                RunItemPatch {
                    status: Some(final_status),
                    pty_id: Some(Some(pty_id)),
                    error_code: Some(Some(wait_err.code.0.to_string())),
                    error_message: Some(Some(wait_err.message.to_string())),
                    finished_at: Some(Some(now_end)),
                    output_tail: Some((tail, truncated)),
                    ..RunItemPatch::default()
                }
            }
        };

        if let Ok((updated_run, updated_item)) =
            self.repo
                .update_run_item(&entry.run_id, &entry.item_id, patch, now_end)
        {
            self.emit_progress(&updated_run, &updated_item);
        }

        drop(permit);
        self.finish_entry(&entry);
    }

    fn finish_entry(&self, entry: &Arc<ActiveParallelItem>) {
        entry.finished.store(true, Ordering::SeqCst);
        self.active_items
            .lock()
            .remove(&(entry.run_id.clone(), entry.item_id.clone()));
        entry.done_notify.notify_waiters();
    }

    fn emit_progress(&self, run: &GroupRun, item: &GroupRunItem) {
        let ev = GroupRunEvent::from_run(run, Some(item));
        self.events.emit_run_item_changed(ev);
        if run.status.is_terminal() {
            self.events
                .emit_run_finished(GroupRunEvent::from_run(run, None));
        }
    }

    /// Cancela toda a execução ou um item específico de forma idempotente:
    /// itens `pending` na fila mudam imediatamente para `cancelled`;
    /// itens `starting`/`running` mudam para `cancelling`, invocam `kill_graceful` e só mudam para
    /// `cancelled` após confirmação de limpeza da árvore em `/proc`.
    pub async fn cancel(
        self: &Arc<Self>,
        run_id: &GroupRunId,
        item_id: Option<&GroupRunItemId>,
    ) -> MtResult<GroupRun> {
        let now_ms = mt_db::time::now_unix_ms();
        let (marked_run, changed_items) =
            self.repo.mark_cancel_requested(run_id, item_id, now_ms)?;

        for item in &changed_items {
            self.events
                .emit_run_item_changed(GroupRunEvent::from_run(&marked_run, Some(item)));
        }
        if marked_run.status.is_terminal() && !changed_items.is_empty() {
            self.events
                .emit_run_finished(GroupRunEvent::from_run(&marked_run, None));
            return Ok(marked_run);
        }

        let targets: Vec<Arc<ActiveParallelItem>> = {
            let guard = self.active_items.lock();
            guard
                .values()
                .filter(|e| {
                    &e.run_id == run_id && item_id.is_none_or(|target| &e.item_id == target)
                })
                .cloned()
                .collect()
        };

        for entry in targets {
            entry.cancel_flag.store(true, Ordering::SeqCst);
            let pty_opt = entry.pty_id.lock().clone();
            if let Some(pty_id) = pty_opt {
                let _ = self.pty_port.kill_graceful(&pty_id).await;
            }
            if !entry.finished.load(Ordering::SeqCst) {
                entry.done_notify.notified().await;
            }
        }

        self.repo.get_run(run_id)
    }

    /// Anexa um assinante ao `GroupRunSink` ativo ou faz replay a partir do `output_tail` persistido no SQLite
    /// caso o item já tenha terminado.
    pub fn attach_output(
        self: &Arc<Self>,
        run_id: &GroupRunId,
        item_id: &GroupRunItemId,
        after_seq: Option<u64>,
        subscriber: Arc<dyn OutputFrameSubscriber>,
    ) -> MtResult<OutputAttachReceipt> {
        let sub_id = OutputSubscriptionId::new();
        let active_sink = {
            let guard = self.active_items.lock();
            guard
                .get(&(run_id.clone(), item_id.clone()))
                .map(|e| Arc::clone(&e.sink))
        };

        if let Some(sink) = active_sink {
            let receipt = sink.attach(sub_id.clone(), after_seq, subscriber);
            self.subscriptions.lock().insert(sub_id, sink);
            return Ok(receipt);
        }

        // Item já finalizado: faz replay a partir do SQLite
        let (tail, truncated) = self.repo.get_run_item_output_tail(run_id, item_id)?;
        let total_len = tail.len() as u64;
        let requested_after = after_seq.unwrap_or(0);

        if truncated && requested_after == 0 {
            let _ = subscriber.send_frame(GroupOutputFrame {
                subscription_id: sub_id.clone(),
                item_id: item_id.clone(),
                seq: 0,
                kind: GroupOutputFrameKind::Gap,
                text: None,
                dropped_bytes: Some(1),
            });
        }

        if total_len > requested_after && !tail.is_empty() {
            let offset = usize::try_from(requested_after)
                .unwrap_or(0)
                .min(tail.len());
            let safe_offset = ceil_char_boundary(&tail, offset);
            if safe_offset < tail.len() {
                let _ = subscriber.send_frame(GroupOutputFrame {
                    subscription_id: sub_id.clone(),
                    item_id: item_id.clone(),
                    seq: total_len,
                    kind: GroupOutputFrameKind::Replay,
                    text: Some(tail[safe_offset..].to_string()),
                    dropped_bytes: None,
                });
            }
        }

        let _ = subscriber.send_frame(GroupOutputFrame {
            subscription_id: sub_id.clone(),
            item_id: item_id.clone(),
            seq: total_len,
            kind: GroupOutputFrameKind::End,
            text: None,
            dropped_bytes: None,
        });

        Ok(OutputAttachReceipt {
            subscription_id: sub_id,
            replay_start_seq: requested_after.min(total_len),
            replay_end_seq: total_len,
            truncated,
        })
    }

    /// Remove uma assinatura ativa de saída.
    pub fn detach_output(&self, subscription_id: &OutputSubscriptionId) {
        if let Some(sink) = self.subscriptions.lock().remove(subscription_id) {
            sink.detach(subscription_id);
        }
    }

    /// Cancela todas as execuções ativas e aguarda a limpeza supervisionada de todos os PTYs no shutdown explícito do app.
    pub async fn shutdown_all(self: &Arc<Self>) {
        let run_ids: Vec<GroupRunId> = {
            let guard = self.active_items.lock();
            let mut ids: Vec<GroupRunId> = guard.keys().map(|(r, _)| r.clone()).collect();
            ids.sort();
            ids.dedup();
            ids
        };
        for rid in run_ids {
            let _ = self.cancel(&rid, None).await;
        }
    }
}
