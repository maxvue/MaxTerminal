//! Executor de grupos no modo Sequencial na mesma aba (`SequentialRunner`).

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mt_core::{MtResult, PtyId};
use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::groups::compose::SequentialComposer;
use crate::groups::events::{GroupEventSink, GroupRunEvent};
use crate::groups::model::{
    GroupRun, GroupRunId, GroupRunItem, GroupRunItemId, ItemStatus, SequentialSeparator,
};
use crate::groups::ports::{
    GroupPtyPort, GroupShellPort, PaneExecutionLease, RawOutputTap, ShellLifecycleEvent,
};
use crate::groups::repo::{CommandGroupRepo, RunItemPatch};
use crate::groups::runner::{GroupRunSink, ParallelRunner};

struct ActiveSequentialRun {
    pty_id: PtyId,
    generation: u64,
    current_item_id: Mutex<Option<GroupRunItemId>>,
    cancel_all: AtomicBool,
    cancelled_items: Mutex<HashSet<GroupRunItemId>>,
    done_notify: Arc<Notify>,
    finished: AtomicBool,
}

/// Executor do modo Sequencial na mesma aba sob reserva exclusiva (`PaneExecutionLease`).
pub struct SequentialRunner {
    pty_port: Arc<dyn GroupPtyPort>,
    shell_port: Arc<dyn GroupShellPort>,
    repo: Arc<dyn CommandGroupRepo>,
    events: Arc<dyn GroupEventSink>,
    sink_registry: Option<Arc<ParallelRunner>>,
    active_runs: Mutex<HashMap<GroupRunId, Arc<ActiveSequentialRun>>>,
}

impl SequentialRunner {
    /// Cria um novo `SequentialRunner`.
    #[must_use]
    pub fn new(
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pty_port,
            shell_port,
            repo,
            events,
            sink_registry: None,
            active_runs: Mutex::new(HashMap::new()),
        })
    }

    /// Cria um `SequentialRunner` compartilhando o registro de sinks de saída com `ParallelRunner`.
    #[must_use]
    pub fn with_sink_registry(
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
        sink_registry: Arc<ParallelRunner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pty_port,
            shell_port,
            repo,
            events,
            sink_registry: Some(sink_registry),
            active_runs: Mutex::new(HashMap::new()),
        })
    }

    /// Executa a fila sequencial sob um `PaneExecutionLease` já adquirido no preflight.
    pub async fn launch_with_lease(
        self: &Arc<Self>,
        run: GroupRun,
        lease: Box<dyn PaneExecutionLease>,
    ) -> MtResult<()> {
        let state = lease.state().clone();
        let active = Arc::new(ActiveSequentialRun {
            pty_id: state.pty_id.clone(),
            generation: state.generation,
            current_item_id: Mutex::new(None),
            cancel_all: AtomicBool::new(false),
            cancelled_items: Mutex::new(HashSet::new()),
            done_notify: Arc::new(Notify::new()),
            finished: AtomicBool::new(false),
        });
        self.active_runs
            .lock()
            .insert(run.id.clone(), Arc::clone(&active));

        let runner = Arc::clone(self);
        tokio::spawn(async move {
            runner.execute_queue(run, lease, active).await;
        });
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn execute_queue(
        &self,
        run: GroupRun,
        mut lease: Box<dyn PaneExecutionLease>,
        active: Arc<ActiveSequentialRun>,
    ) {
        let pane_state = lease.state().clone();
        let separator = run.snapshot.separator;
        let group_has_cwd = run.snapshot.cwd.is_some();
        let env_chdir_supported = self.shell_port.supports_env_chdir();

        for item in &run.items {
            if active.cancel_all.load(Ordering::SeqCst) {
                break;
            }
            if active.cancelled_items.lock().contains(&item.id) {
                continue;
            }

            // Verifica se ainda está Pending no banco
            if let Ok(current_run) = self.repo.get_run(&run.id) {
                if let Some(db_item) = current_run.items.iter().find(|i| i.id == item.id) {
                    if db_item.status.is_terminal() {
                        continue;
                    }
                }
            }

            *active.current_item_id.lock() = Some(item.id.clone());

            let sink = GroupRunSink::new(run.id.clone(), item.id.clone());
            if let Some(ref reg) = self.sink_registry {
                reg.register_external_sink(run.id.clone(), item.id.clone(), Arc::clone(&sink));
            }
            let tap_guard = self
                .pty_port
                .attach_item_tap(
                    &pane_state.pty_id,
                    Arc::clone(&sink) as Arc<dyn RawOutputTap>,
                )
                .await
                .ok();

            let now_start = mt_db::time::now_unix_ms();
            if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                &run.id,
                &item.id,
                RunItemPatch {
                    status: Some(ItemStatus::Starting),
                    pty_id: Some(Some(pane_state.pty_id.clone())),
                    tab_id: Some(Some(pane_state.tab_id.clone())),
                    pane_id: Some(Some(pane_state.pane_id.clone())),
                    session_id: Some(Some(pane_state.session_id.clone())),
                    started_at: Some(Some(now_start)),
                    ..RunItemPatch::default()
                },
                now_start,
            ) {
                self.emit_progress(&updated_run, &updated_item);
            }

            let cwd_override = if item.snapshot.defined_cwd.is_some() || group_has_cwd {
                Some(Path::new(&item.snapshot.resolved_cwd))
            } else {
                None
            };

            let composed_res = SequentialComposer::compose_submission(
                item.snapshot.shell_dialect,
                Path::new(&item.snapshot.shell_path),
                &item.snapshot.command,
                cwd_override,
                &item.snapshot.env,
                env_chdir_supported,
            );

            let submission_bytes = match composed_res {
                Ok(bytes) => bytes,
                Err(comp_err) => {
                    sink.on_eof();
                    drop(tap_guard);
                    if let Some(ref reg) = self.sink_registry {
                        reg.unregister_external_sink(&run.id, &item.id);
                    }
                    let (tail, truncated) = sink.snapshot_tail();
                    let now_err = mt_db::time::now_unix_ms();
                    if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                        &run.id,
                        &item.id,
                        RunItemPatch {
                            status: Some(ItemStatus::Failed),
                            error_code: Some(Some(comp_err.code.0.to_string())),
                            error_message: Some(Some(comp_err.message.to_string())),
                            finished_at: Some(Some(now_err)),
                            output_tail: Some((tail, truncated)),
                            ..RunItemPatch::default()
                        },
                        now_err,
                    ) {
                        self.emit_progress(&updated_run, &updated_item);
                    }
                    if separator == SequentialSeparator::And {
                        self.skip_rest(&run.id).await;
                        break;
                    }
                    continue;
                }
            };

            if let Err(write_err) = self
                .pty_port
                .write_to_pty(&pane_state.pty_id, &submission_bytes)
                .await
            {
                sink.on_eof();
                drop(tap_guard);
                if let Some(ref reg) = self.sink_registry {
                    reg.unregister_external_sink(&run.id, &item.id);
                }
                let (tail, truncated) = sink.snapshot_tail();
                let now_err = mt_db::time::now_unix_ms();
                if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                    &run.id,
                    &item.id,
                    RunItemPatch {
                        status: Some(ItemStatus::Failed),
                        error_code: Some(Some(write_err.code.0.to_string())),
                        error_message: Some(Some(write_err.message.to_string())),
                        finished_at: Some(Some(now_err)),
                        output_tail: Some((tail, truncated)),
                        ..RunItemPatch::default()
                    },
                    now_err,
                ) {
                    self.emit_progress(&updated_run, &updated_item);
                }
                if separator == SequentialSeparator::And {
                    self.skip_rest(&run.id).await;
                    break;
                }
                continue;
            }

            let now_run = mt_db::time::now_unix_ms();
            if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                &run.id,
                &item.id,
                RunItemPatch {
                    status: Some(ItemStatus::Running),
                    ..RunItemPatch::default()
                },
                now_run,
            ) {
                self.emit_progress(&updated_run, &updated_item);
            }

            // Aguarda ciclo OSC 133: BlockStarted -> BlockFinished -> PromptReady
            let mut finished_result: Option<(mt_core::CommandBlockId, i32, Option<i32>)> = None;
            let mut invalidated_reason: Option<String> = None;

            while let Some(ev) = lease.next_event().await {
                match ev {
                    ShellLifecycleEvent::BlockStarted {
                        block_id,
                        generation,
                        ..
                    } if generation == pane_state.generation => {
                        let now_b = mt_db::time::now_unix_ms();
                        if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                            &run.id,
                            &item.id,
                            RunItemPatch {
                                command_block_id: Some(Some(block_id)),
                                ..RunItemPatch::default()
                            },
                            now_b,
                        ) {
                            self.emit_progress(&updated_run, &updated_item);
                        }
                    }
                    ShellLifecycleEvent::BlockFinished {
                        block_id,
                        generation,
                        exit_code,
                        signal,
                        ..
                    } if generation == pane_state.generation => {
                        finished_result = Some((block_id, exit_code, signal));
                    }
                    ShellLifecycleEvent::PromptReady { generation, .. }
                        if generation == pane_state.generation && finished_result.is_some() =>
                    {
                        break;
                    }
                    ShellLifecycleEvent::GenerationInvalidated { reason, .. } => {
                        invalidated_reason = Some(reason);
                        break;
                    }
                    _ => {}
                }
            }

            sink.on_eof();
            drop(tap_guard);
            if let Some(ref reg) = self.sink_registry {
                reg.unregister_external_sink(&run.id, &item.id);
            }
            let (tail, truncated) = sink.snapshot_tail();
            *active.current_item_id.lock() = None;

            if let Some(reason) = invalidated_reason {
                let now_int = mt_db::time::now_unix_ms();
                let item_was_cancelled = active.cancel_all.load(Ordering::SeqCst)
                    || active.cancelled_items.lock().contains(&item.id);
                let status = if item_was_cancelled {
                    ItemStatus::Cancelled
                } else {
                    ItemStatus::Interrupted
                };
                if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                    &run.id,
                    &item.id,
                    RunItemPatch {
                        status: Some(status),
                        error_message: Some(Some(reason)),
                        finished_at: Some(Some(now_int)),
                        output_tail: Some((tail, truncated)),
                        ..RunItemPatch::default()
                    },
                    now_int,
                ) {
                    self.emit_progress(&updated_run, &updated_item);
                }
                // Interrompe quaisquer itens pendentes restantes
                let _ = self.repo.recover_interrupted_runs(now_int);
                if let Ok(final_run) = self.repo.get_run(&run.id) {
                    self.events
                        .emit_run_finished(GroupRunEvent::from_run(&final_run, None));
                }
                break;
            }

            if let Some((block_id, exit_code, signal)) = finished_result {
                let now_done = mt_db::time::now_unix_ms();
                let item_was_cancelled = active.cancel_all.load(Ordering::SeqCst)
                    || active.cancelled_items.lock().contains(&item.id);
                let final_status = if item_was_cancelled {
                    ItemStatus::Cancelled
                } else if exit_code == 0 && signal.is_none() {
                    ItemStatus::Succeeded
                } else {
                    ItemStatus::Failed
                };

                if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                    &run.id,
                    &item.id,
                    RunItemPatch {
                        status: Some(final_status),
                        command_block_id: Some(Some(block_id)),
                        exit_code: Some(Some(exit_code)),
                        signal: Some(signal),
                        finished_at: Some(Some(now_done)),
                        output_tail: Some((tail, truncated)),
                        ..RunItemPatch::default()
                    },
                    now_done,
                ) {
                    self.emit_progress(&updated_run, &updated_item);
                }

                if active.cancel_all.load(Ordering::SeqCst) {
                    break;
                }

                if final_status == ItemStatus::Failed && separator == SequentialSeparator::And {
                    self.skip_rest(&run.id).await;
                    break;
                }
            } else {
                // O canal do lease fechou sem BlockFinished
                let now_int = mt_db::time::now_unix_ms();
                let status = if active.cancel_all.load(Ordering::SeqCst) {
                    ItemStatus::Cancelled
                } else {
                    ItemStatus::Interrupted
                };
                if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                    &run.id,
                    &item.id,
                    RunItemPatch {
                        status: Some(status),
                        finished_at: Some(Some(now_int)),
                        output_tail: Some((tail, truncated)),
                        ..RunItemPatch::default()
                    },
                    now_int,
                ) {
                    self.emit_progress(&updated_run, &updated_item);
                }
                break;
            }
        }

        lease.release().await;
        active.finished.store(true, Ordering::SeqCst);
        self.active_runs.lock().remove(&run.id);
        active.done_notify.notify_waiters();
    }

    async fn skip_rest(&self, run_id: &GroupRunId) {
        let now_ms = mt_db::time::now_unix_ms();
        if let Ok((updated_run, skipped_items)) =
            self.repo.skip_remaining_pending_items(run_id, now_ms)
        {
            for item in &skipped_items {
                self.events
                    .emit_run_item_changed(GroupRunEvent::from_run(&updated_run, Some(item)));
            }
            if updated_run.status.is_terminal() {
                self.events
                    .emit_run_finished(GroupRunEvent::from_run(&updated_run, None));
            }
        }
    }

    fn emit_progress(&self, run: &GroupRun, item: &GroupRunItem) {
        self.events
            .emit_run_item_changed(GroupRunEvent::from_run(run, Some(item)));
        if run.status.is_terminal() {
            self.events
                .emit_run_finished(GroupRunEvent::from_run(run, None));
        }
    }

    /// Cancela a fila sequencial ou um item específico. Se cancelar o item ativo,
    /// envia `\x03` (Ctrl+C) para o PTY do lease e aguarda `BlockFinished` + `PromptReady`.
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

        let active_opt = self.active_runs.lock().get(run_id).cloned();
        if let Some(active) = active_opt {
            let should_interrupt_current = match item_id {
                None => {
                    active.cancel_all.store(true, Ordering::SeqCst);
                    true
                }
                Some(target_id) => {
                    active.cancelled_items.lock().insert(target_id.clone());
                    active.current_item_id.lock().as_ref() == Some(target_id)
                }
            };

            if should_interrupt_current {
                let current_state = self
                    .shell_port
                    .inspect_pane(
                        &marked_run
                            .snapshot
                            .origin_pane_id
                            .clone()
                            .unwrap_or_default(),
                    )
                    .await
                    .ok();
                if current_state
                    .as_ref()
                    .is_none_or(|s| s.generation == active.generation)
                {
                    let _ = self.pty_port.write_to_pty(&active.pty_id, b"\x03").await;
                }
            }

            if item_id.is_none() && !active.finished.load(Ordering::SeqCst) {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    active.done_notify.notified(),
                )
                .await;
            }
        }

        self.repo.get_run(run_id)
    }

    /// Cancela todas as filas sequenciais ativas no shutdown do aplicativo.
    pub async fn shutdown_all(self: &Arc<Self>) {
        let run_ids: Vec<GroupRunId> = self.active_runs.lock().keys().cloned().collect();
        for rid in run_ids {
            let _ = self.cancel(&rid, None).await;
        }
    }
}
