//! Executor de grupos no modo Multi-Abas (`MultiTabRunner`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mt_core::{MtResult, PtyId, SessionId, TabId};
use parking_lot::Mutex;
use tokio::sync::Notify;

use crate::groups::events::{GroupEventSink, GroupRunEvent};
use crate::groups::model::{
    GroupErrorCode, GroupRun, GroupRunId, GroupRunItem, GroupRunItemId, ItemStatus,
};
use crate::groups::ports::{
    GroupPtyPort, GroupShellPort, GroupTabOpenSpec, GroupTabPort, OpenedGroupTab, RawOutputTap,
    ShellLifecycleEvent,
};
use crate::groups::repo::{CommandGroupRepo, RunItemPatch};
use crate::groups::runner::{GroupRunSink, ParallelRunner};

struct ActiveMultiTabItem {
    run_id: GroupRunId,
    item_id: GroupRunItemId,
    opened: Mutex<Option<(TabId, SessionId, PtyId)>>,
    cancel_flag: AtomicBool,
    done_notify: Arc<Notify>,
    finished: AtomicBool,
}

/// Executor do modo Multi-Abas com abertura sequencial, preservação de abas em falha parcial
/// e cancelamento seguro (`close_tab_if_running_initial`).
pub struct MultiTabRunner {
    tab_port: Arc<dyn GroupTabPort>,
    pty_port: Arc<dyn GroupPtyPort>,
    shell_port: Arc<dyn GroupShellPort>,
    repo: Arc<dyn CommandGroupRepo>,
    events: Arc<dyn GroupEventSink>,
    sink_registry: Option<Arc<ParallelRunner>>,
    active_items: Mutex<HashMap<(GroupRunId, GroupRunItemId), Arc<ActiveMultiTabItem>>>,
}

impl MultiTabRunner {
    /// Cria um novo `MultiTabRunner`.
    #[must_use]
    pub fn new(
        tab_port: Arc<dyn GroupTabPort>,
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tab_port,
            pty_port,
            shell_port,
            repo,
            events,
            sink_registry: None,
            active_items: Mutex::new(HashMap::new()),
        })
    }

    /// Cria um `MultiTabRunner` compartilhando o registro de sinks com `ParallelRunner`.
    #[must_use]
    pub fn with_sink_registry(
        tab_port: Arc<dyn GroupTabPort>,
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        repo: Arc<dyn CommandGroupRepo>,
        events: Arc<dyn GroupEventSink>,
        sink_registry: Arc<ParallelRunner>,
    ) -> Arc<Self> {
        Arc::new(Self {
            tab_port,
            pty_port,
            shell_port,
            repo,
            events,
            sink_registry: Some(sink_registry),
            active_items: Mutex::new(HashMap::new()),
        })
    }

    /// Inicia a abertura sequencial das abas e o monitoramento do comando inicial de cada item.
    pub async fn launch(self: &Arc<Self>, run: GroupRun) -> MtResult<()> {
        for item in &run.items {
            let entry = Arc::new(ActiveMultiTabItem {
                run_id: run.id.clone(),
                item_id: item.id.clone(),
                opened: Mutex::new(None),
                cancel_flag: AtomicBool::new(false),
                done_notify: Arc::new(Notify::new()),
                finished: AtomicBool::new(false),
            });
            self.active_items
                .lock()
                .insert((run.id.clone(), item.id.clone()), entry);
        }

        let runner = Arc::clone(self);
        tokio::spawn(async move {
            runner.open_all_tabs(run).await;
        });
        Ok(())
    }

    #[allow(clippy::too_many_lines)]
    async fn open_all_tabs(self: &Arc<Self>, run: GroupRun) {
        for (idx, item) in run.items.iter().enumerate() {
            let Some(entry) = self
                .active_items
                .lock()
                .get(&(run.id.clone(), item.id.clone()))
                .cloned()
            else {
                continue;
            };

            if entry.cancel_flag.load(Ordering::SeqCst) {
                self.finish_entry(&entry);
                continue;
            }

            let now_start = mt_db::time::now_unix_ms();
            match self.repo.update_run_item(
                &run.id,
                &item.id,
                RunItemPatch {
                    status: Some(ItemStatus::Starting),
                    started_at: Some(Some(now_start)),
                    ..RunItemPatch::default()
                },
                now_start,
            ) {
                Ok((updated_run, updated_item)) => {
                    if updated_item.status.is_terminal() {
                        self.finish_entry(&entry);
                        continue;
                    }
                    self.emit_progress(&updated_run, &updated_item);
                }
                Err(_) => {
                    self.finish_entry(&entry);
                    continue;
                }
            }

            let Some(ref template_id) = item.snapshot.template_id else {
                let now_err = mt_db::time::now_unix_ms();
                if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                    &run.id,
                    &item.id,
                    RunItemPatch {
                        status: Some(ItemStatus::Failed),
                        error_code: Some(Some(
                            GroupErrorCode::TemplateIncompatible.as_str().to_string(),
                        )),
                        error_message: Some(Some(
                            "Template de aba ausente no snapshot do item.".to_string(),
                        )),
                        finished_at: Some(Some(now_err)),
                        ..RunItemPatch::default()
                    },
                    now_err,
                ) {
                    self.emit_progress(&updated_run, &updated_item);
                }
                self.finish_entry(&entry);
                continue;
            };

            let spec = GroupTabOpenSpec {
                project_id: run.project_id.clone(),
                template_id: template_id.clone(),
                focus: idx == 0,
                title: format!("{} · {}", run.snapshot.name, item.snapshot.title),
                cwd: PathBuf::from(&item.snapshot.resolved_cwd),
                initial_command: item.snapshot.command.clone(),
                extra_env: item.snapshot.env.clone(),
                cwd_fallback_to_project: false,
            };

            match self.tab_port.open_tab(spec).await {
                Ok(opened) => {
                    *entry.opened.lock() = Some((
                        opened.tab_id.clone(),
                        opened.session_id.clone(),
                        opened.pty_id.clone(),
                    ));

                    // Se cancelamento ocorreu enquanto open_tab estava em voo, fecha a aba se ainda rodando o comando inicial
                    if entry.cancel_flag.load(Ordering::SeqCst) {
                        let _ = self
                            .tab_port
                            .close_tab_if_running_initial(
                                &opened.tab_id,
                                &opened.session_id,
                                &opened.pty_id,
                            )
                            .await;
                        let now_cancel = mt_db::time::now_unix_ms();
                        if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                            &run.id,
                            &item.id,
                            RunItemPatch {
                                status: Some(ItemStatus::Cancelled),
                                tab_id: Some(Some(opened.tab_id)),
                                pane_id: Some(Some(opened.pane_id)),
                                pty_id: Some(Some(opened.pty_id)),
                                session_id: Some(Some(opened.session_id)),
                                command_block_id: Some(opened.initial_block_id),
                                finished_at: Some(Some(now_cancel)),
                                ..RunItemPatch::default()
                            },
                            now_cancel,
                        ) {
                            self.emit_progress(&updated_run, &updated_item);
                        }
                        self.finish_entry(&entry);
                        continue;
                    }

                    // Assina eventos da sessão ANTES de marcar Running para não perder término rápido
                    let rx_opt = self
                        .shell_port
                        .subscribe_session_events(&opened.session_id)
                        .await
                        .ok();

                    let sink = GroupRunSink::new(run.id.clone(), item.id.clone());
                    if let Some(ref reg) = self.sink_registry {
                        reg.register_external_sink(
                            run.id.clone(),
                            item.id.clone(),
                            Arc::clone(&sink),
                        );
                    }
                    let tap_guard = self
                        .pty_port
                        .attach_item_tap(&opened.pty_id, Arc::clone(&sink) as Arc<dyn RawOutputTap>)
                        .await
                        .ok();

                    let now_run = mt_db::time::now_unix_ms();
                    if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                        &run.id,
                        &item.id,
                        RunItemPatch {
                            status: Some(ItemStatus::Running),
                            tab_id: Some(Some(opened.tab_id.clone())),
                            pane_id: Some(Some(opened.pane_id.clone())),
                            pty_id: Some(Some(opened.pty_id.clone())),
                            session_id: Some(Some(opened.session_id.clone())),
                            command_block_id: Some(opened.initial_block_id.clone()),
                            ..RunItemPatch::default()
                        },
                        now_run,
                    ) {
                        self.emit_progress(&updated_run, &updated_item);
                    }

                    let runner_clone = Arc::clone(self);
                    let item_clone = item.clone();
                    tokio::spawn(async move {
                        runner_clone
                            .monitor_opened_tab(item_clone, opened, entry, rx_opt, sink, tap_guard)
                            .await;
                    });
                }
                Err(open_err) => {
                    // Falha parcial: registra Failed neste item e prossegue para os demais sem destruir abas anteriores!
                    let now_fail = mt_db::time::now_unix_ms();
                    if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                        &run.id,
                        &item.id,
                        RunItemPatch {
                            status: Some(ItemStatus::Failed),
                            error_code: Some(Some(
                                GroupErrorCode::TabOpenFailed.as_str().to_string(),
                            )),
                            error_message: Some(Some(open_err.message.to_string())),
                            finished_at: Some(Some(now_fail)),
                            ..RunItemPatch::default()
                        },
                        now_fail,
                    ) {
                        self.emit_progress(&updated_run, &updated_item);
                    }
                    self.finish_entry(&entry);
                }
            }
        }
    }

    async fn monitor_opened_tab(
        &self,
        item: GroupRunItem,
        opened: OpenedGroupTab,
        entry: Arc<ActiveMultiTabItem>,
        rx_opt: Option<tokio::sync::mpsc::UnboundedReceiver<ShellLifecycleEvent>>,
        sink: Arc<GroupRunSink>,
        tap_guard: Option<Box<dyn Send + Sync>>,
    ) {
        if let Some(mut rx) = rx_opt {
            while let Some(ev) = rx.recv().await {
                match ev {
                    ShellLifecycleEvent::BlockStarted { block_id, .. } => {
                        let now_b = mt_db::time::now_unix_ms();
                        if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                            &entry.run_id,
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
                        exit_code,
                        signal,
                        ..
                    } => {
                        sink.on_eof();
                        drop(tap_guard);
                        if let Some(ref reg) = self.sink_registry {
                            reg.unregister_external_sink(&entry.run_id, &item.id);
                        }
                        let (tail, truncated) = sink.snapshot_tail();
                        let now_done = mt_db::time::now_unix_ms();
                        let was_cancelled = entry.cancel_flag.load(Ordering::SeqCst);
                        let final_status = if was_cancelled {
                            ItemStatus::Cancelled
                        } else if exit_code == 0 && signal.is_none() {
                            ItemStatus::Succeeded
                        } else {
                            ItemStatus::Failed
                        };
                        if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                            &entry.run_id,
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
                        self.finish_entry(&entry);
                        return;
                    }
                    ShellLifecycleEvent::GenerationInvalidated { reason, .. } => {
                        sink.on_eof();
                        drop(tap_guard);
                        if let Some(ref reg) = self.sink_registry {
                            reg.unregister_external_sink(&entry.run_id, &item.id);
                        }
                        let (tail, truncated) = sink.snapshot_tail();
                        let now_int = mt_db::time::now_unix_ms();
                        let was_cancelled = entry.cancel_flag.load(Ordering::SeqCst);
                        let final_status = if was_cancelled {
                            ItemStatus::Cancelled
                        } else {
                            ItemStatus::Interrupted
                        };
                        if let Ok((updated_run, updated_item)) = self.repo.update_run_item(
                            &entry.run_id,
                            &item.id,
                            RunItemPatch {
                                status: Some(final_status),
                                error_message: Some(Some(reason)),
                                finished_at: Some(Some(now_int)),
                                output_tail: Some((tail, truncated)),
                                ..RunItemPatch::default()
                            },
                            now_int,
                        ) {
                            self.emit_progress(&updated_run, &updated_item);
                        }
                        self.finish_entry(&entry);
                        return;
                    }
                    ShellLifecycleEvent::PromptReady { .. } => {}
                }
            }
        }

        // Fallback: aguarda wait_exit do PTY se o canal de eventos da sessão encerrar sem BlockFinished
        let exit_res = self.pty_port.wait_exit(&opened.pty_id).await;
        sink.on_eof();
        drop(tap_guard);
        if let Some(ref reg) = self.sink_registry {
            reg.unregister_external_sink(&entry.run_id, &item.id);
        }
        let (tail, truncated) = sink.snapshot_tail();
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
                    exit_code: Some(info.exit_code),
                    signal: Some(info.signal),
                    finished_at: Some(Some(now_end)),
                    output_tail: Some((tail, truncated)),
                    ..RunItemPatch::default()
                }
            }
            Err(err) => {
                let final_status = if was_cancelled {
                    ItemStatus::Cancelled
                } else {
                    ItemStatus::Interrupted
                };
                RunItemPatch {
                    status: Some(final_status),
                    error_message: Some(Some(err.message.to_string())),
                    finished_at: Some(Some(now_end)),
                    output_tail: Some((tail, truncated)),
                    ..RunItemPatch::default()
                }
            }
        };

        if let Ok((updated_run, updated_item)) =
            self.repo
                .update_run_item(&entry.run_id, &item.id, patch, now_end)
        {
            self.emit_progress(&updated_run, &updated_item);
        }
        self.finish_entry(&entry);
    }

    fn finish_entry(&self, entry: &Arc<ActiveMultiTabItem>) {
        entry.finished.store(true, Ordering::SeqCst);
        self.active_items
            .lock()
            .remove(&(entry.run_id.clone(), entry.item_id.clone()));
        entry.done_notify.notify_waiters();
    }

    fn emit_progress(&self, run: &GroupRun, item: &GroupRunItem) {
        self.events
            .emit_run_item_changed(GroupRunEvent::from_run(run, Some(item)));
        if run.status.is_terminal() {
            self.events
                .emit_run_finished(GroupRunEvent::from_run(run, None));
        }
    }

    /// Cancela toda a execução Multi-Abas ou um item específico:
    /// fecha a aba somente se ela ainda estiver executando o comando inicial do item.
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

        let targets: Vec<Arc<ActiveMultiTabItem>> = {
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
            let opened_opt = entry.opened.lock().clone();
            if let Some((tab_id, session_id, pty_id)) = opened_opt {
                let _ = self
                    .tab_port
                    .close_tab_if_running_initial(&tab_id, &session_id, &pty_id)
                    .await;
            }
            if !entry.finished.load(Ordering::SeqCst) {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    entry.done_notify.notified(),
                )
                .await;
            }
        }

        self.repo.get_run(run_id)
    }

    /// Cancela todas as execuções Multi-Abas ativas no shutdown do aplicativo.
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
