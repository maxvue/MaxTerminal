//! Testes dos motores de execução `SequentialRunner`, `ParallelRunner`, `MultiTabPlanner`,
//! `MultiTabRunner` e do coletor `GroupRunSink` (`CA-06`, `CA-07`, `CA-08`, `CA-24`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use mt_commands::groups::events::{
    GroupChangedEvent, GroupEventSink, GroupOutputFrame, GroupRunEvent, OutputFrameSubscriber,
};
use mt_commands::groups::model::{
    CommandGroup, CommandGroupInput, CommandGroupItem, CommandGroupItemInput, ExecutionMode,
    GroupErrorCode, GroupItemId, GroupRequestId, GroupRun, GroupRunId, GroupRunItemId,
    GroupRunItemSnapshot, GroupRunSnapshot, GroupScope, ItemStatus, OutputSubscriptionId,
    RunStatus, SequentialSeparator, ShellDialect,
};
use mt_commands::groups::multi_tab::MultiTabRunner;
use mt_commands::groups::plan::MultiTabPlanner;
use mt_commands::groups::ports::{
    GroupPtyPort, GroupShellPort, GroupTabOpenSpec, GroupTabPort, HeadlessSpawnSpec,
    OpenedGroupTab, PaneExecutionLease, PaneShellState, ProjectShellTemplate, RawOutputTap,
    ShellLifecycleEvent, SupervisedExitInfo,
};
use mt_commands::groups::repo::{
    AcceptRunOutcome, CommandGroupRepo, CreateRunRecord, SqliteCommandGroupRepo,
};
use mt_commands::groups::runner::{GroupRunSink, MAX_ITEM_OUTPUT_TAIL_BYTES, ParallelRunner};
use mt_commands::groups::sequential::SequentialRunner;
use mt_core::error::{ErrorCode, MtError, MtResult};
use mt_core::ids::{
    CommandBlockId, CommandGroupId, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId,
};
use mt_db::{Db, DbConfig};
use parking_lot::Mutex;
use tempfile::TempDir;
use tokio::sync::mpsc;

#[derive(Default)]
struct RecordingEventSink {
    finished_events: Mutex<Vec<GroupRunEvent>>,
}

impl GroupEventSink for RecordingEventSink {
    fn emit_group_changed(&self, _event: GroupChangedEvent) {}
    fn emit_run_started(&self, _event: GroupRunEvent) {}
    fn emit_run_item_changed(&self, _event: GroupRunEvent) {}
    fn emit_run_finished(&self, event: GroupRunEvent) {
        self.finished_events.lock().push(event);
    }
}

#[derive(Default)]
struct RecordingFrameSubscriber {
    frames: Mutex<Vec<GroupOutputFrame>>,
}

impl OutputFrameSubscriber for RecordingFrameSubscriber {
    fn send_frame(&self, frame: GroupOutputFrame) -> bool {
        self.frames.lock().push(frame);
        true
    }
}

struct FakeLease {
    state: PaneShellState,
    events: VecDeque<ShellLifecycleEvent>,
}

#[async_trait]
impl PaneExecutionLease for FakeLease {
    fn state(&self) -> &PaneShellState {
        &self.state
    }

    async fn next_event(&mut self) -> Option<ShellLifecycleEvent> {
        self.events.pop_front()
    }

    async fn release(self: Box<Self>) {}
}

struct FakeShellPort {
    env_chdir: bool,
}

#[async_trait]
impl GroupShellPort for FakeShellPort {
    async fn inspect_pane(&self, pane_id: &PaneId) -> MtResult<PaneShellState> {
        Ok(PaneShellState {
            tab_id: TabId::new(),
            pane_id: *pane_id,
            pty_id: PtyId::new(),
            session_id: SessionId::new(),
            shell_path: PathBuf::from("/bin/bash"),
            generation: 1,
            prompt_ready: true,
            in_alt_screen: false,
            foreground_busy: false,
            tracking_available: true,
        })
    }

    async fn acquire_pane_lease(&self, pane_id: &PaneId) -> MtResult<Box<dyn PaneExecutionLease>> {
        let state = self.inspect_pane(pane_id).await?;
        Ok(Box::new(FakeLease {
            state,
            events: VecDeque::new(),
        }))
    }

    async fn subscribe_session_events(
        &self,
        _session_id: &SessionId,
    ) -> MtResult<mpsc::UnboundedReceiver<ShellLifecycleEvent>> {
        let (tx, rx) = mpsc::unbounded_channel();
        let pty_id = PtyId::new();
        let sess_id = SessionId::new();
        let block_id = CommandBlockId::new();
        let _ = tx.send(ShellLifecycleEvent::BlockStarted {
            pty_id,
            session_id: sess_id,
            block_id,
            generation: 1,
        });
        let _ = tx.send(ShellLifecycleEvent::BlockFinished {
            pty_id,
            session_id: sess_id,
            block_id,
            generation: 1,
            exit_code: 0,
            signal: None,
        });
        Ok(rx)
    }

    fn supports_env_chdir(&self) -> bool {
        self.env_chdir
    }
}

struct ConcurrencyTrackingPtyPort {
    active_now: Arc<AtomicUsize>,
    max_observed: Arc<AtomicUsize>,
    sleep_ms: u64,
}

#[async_trait]
impl GroupPtyPort for ConcurrencyTrackingPtyPort {
    async fn spawn_headless(
        &self,
        _spec: HeadlessSpawnSpec,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<PtyId> {
        let cur = self.active_now.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_observed.fetch_max(cur, Ordering::SeqCst);
        output_tap.on_bytes(b"linha ok\n");
        Ok(PtyId::new())
    }

    async fn wait_exit(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo> {
        tokio::time::sleep(Duration::from_millis(self.sleep_ms)).await;
        self.active_now.fetch_sub(1, Ordering::SeqCst);
        Ok(SupervisedExitInfo {
            pty_id: *pty_id,
            exit_code: Some(0),
            signal: None,
            tree_cleaned: true,
        })
    }

    async fn write_to_pty(&self, _pty_id: &PtyId, _data: &[u8]) -> MtResult<()> {
        Ok(())
    }

    async fn kill_graceful(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo> {
        Ok(SupervisedExitInfo {
            pty_id: *pty_id,
            exit_code: None,
            signal: Some(15),
            tree_cleaned: true,
        })
    }

    async fn attach_item_tap(
        &self,
        _pty_id: &PtyId,
        _output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<Box<dyn Send + Sync>> {
        Ok(Box::new(()))
    }
}

struct PartialFailTabPort {
    fail_at_index: Option<usize>,
    opened_counter: AtomicUsize,
}

#[async_trait]
impl GroupTabPort for PartialFailTabPort {
    async fn resolve_project_root(&self, _project_id: &ProjectId) -> MtResult<PathBuf> {
        Ok(PathBuf::from("/tmp"))
    }

    async fn list_project_templates(
        &self,
        _project_id: &ProjectId,
    ) -> MtResult<Vec<ProjectShellTemplate>> {
        Ok(vec![])
    }

    async fn open_tab(&self, _spec: GroupTabOpenSpec) -> MtResult<OpenedGroupTab> {
        let idx = self.opened_counter.fetch_add(1, Ordering::SeqCst);
        if self.fail_at_index == Some(idx) {
            return Err(MtError::new(
                ErrorCode("workspace.tab_spawn_failed"),
                "Falha simulada ao abrir a 3ª aba",
            ));
        }
        Ok(OpenedGroupTab {
            tab_id: TabId::new(),
            pane_id: PaneId::new(),
            pty_id: PtyId::new(),
            session_id: SessionId::new(),
            initial_block_id: Some(CommandBlockId::new()),
        })
    }

    async fn close_tab_if_running_initial(
        &self,
        _tab_id: &TabId,
        _expected_session_id: &SessionId,
        _expected_pty_id: &PtyId,
    ) -> MtResult<bool> {
        Ok(true)
    }
}

async fn setup_repo_and_run(
    mode: ExecutionMode,
    separator: SequentialSeparator,
    cmds: &[&str],
    template_id: Option<TemplateId>,
) -> (
    TempDir,
    Db,
    Arc<SqliteCommandGroupRepo>,
    ProjectId,
    CommandGroup,
    GroupRun,
) {
    let tmp = TempDir::new().unwrap();
    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();
    let p1 = ProjectId::new();
    let p1_str = p1.to_string();

    db.write(move |tx| {
        tx.execute(
            "INSERT INTO projects (id, title, path, icon, color, stack, sort_order, created_at, updated_at)
             VALUES (?1, 'P1', '/tmp', 'lucide:box', '#22C55E', 'rust', 0, 1000, 1000);",
            rusqlite::params![p1_str],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let repo = Arc::new(SqliteCommandGroupRepo::new(db.clone()));
    let items: Vec<_> = cmds
        .iter()
        .enumerate()
        .map(|(i, c)| CommandGroupItemInput {
            id: None,
            title: format!("Item {i}"),
            command: (*c).to_string(),
            cwd: None,
            env: BTreeMap::new(),
            continuous: false,
        })
        .collect();

    let group = repo
        .create(&CommandGroupInput {
            scope: GroupScope::Local,
            project_id: Some(p1),
            name: "Grupo Runner".to_string(),
            description: String::new(),
            labels: vec![],
            mode,
            separator,
            cwd: None,
            env: BTreeMap::new(),
            items,
        })
        .unwrap();

    let item_snaps: Vec<(GroupRunItemId, GroupRunItemSnapshot)> = group
        .items
        .iter()
        .map(|it| {
            (
                GroupRunItemId::new(),
                GroupRunItemSnapshot {
                    source_item_id: it.id.clone(),
                    position: it.position,
                    title: it.title.clone(),
                    command: it.command.clone(),
                    defined_cwd: None,
                    resolved_cwd: "/tmp".to_string(),
                    env: BTreeMap::new(),
                    continuous: false,
                    template_id,
                    shell_path: "/bin/bash".to_string(),
                    shell_dialect: ShellDialect::Bash,
                },
            )
        })
        .collect();

    let rec = CreateRunRecord {
        run_id: GroupRunId::new(),
        request_id: GroupRequestId::new(),
        request_hash: "h".to_string(),
        group_id: group.id,
        project_id: p1,
        origin_session_id: None,
        snapshot: GroupRunSnapshot {
            group_id: group.id,
            group_revision: group.revision,
            scope: group.scope,
            name: group.name.clone(),
            description: group.description.clone(),
            labels: vec![],
            defined_mode: mode,
            effective_mode: mode,
            separator,
            cwd: None,
            env: BTreeMap::new(),
            project_root: "/tmp".to_string(),
            shell_path: "/bin/bash".to_string(),
            shell_dialect: ShellDialect::Bash,
            origin_tab_id: None,
            origin_pane_id: None,
            origin_pty_id: None,
            origin_session_id: None,
            origin_generation: Some(1),
        },
        mode,
        items: item_snaps,
        created_at: 1000,
    };

    let AcceptRunOutcome::Created(run) = repo.accept_run(rec).unwrap() else {
        panic!("esperava Created");
    };

    (tmp, db, repo, p1, group, run)
}

#[test]
fn group_run_sink_truncates_at_256_kib_preserving_utf8_and_redacts_secrets() {
    let run_id = GroupRunId::new();
    let item_id = GroupRunItemId::new();
    let sink = GroupRunSink::new(run_id, item_id);
    let sub = Arc::new(RecordingFrameSubscriber::default());
    let _receipt = sink.attach(
        OutputSubscriptionId::new(),
        None,
        Arc::clone(&sub) as Arc<dyn OutputFrameSubscriber>,
    );

    let chunk_unit = "çãõ-linha-de-log-extensa-0123456789\n".repeat(120);
    let chunk_bytes = chunk_unit.as_bytes();
    let mut sent = 0usize;
    while sent < 2 * 1024 * 1024 {
        sink.on_bytes(chunk_bytes);
        sent += chunk_bytes.len();
    }
    let secret_tail = b"token Final: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U\n";
    sink.on_bytes(secret_tail);
    sink.on_eof();

    let (tail_redacted, truncated) = sink.snapshot_tail();
    assert!(truncated);
    assert!(tail_redacted.len() <= MAX_ITEM_OUTPUT_TAIL_BYTES);
    assert!(!tail_redacted.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));

    let frames = sub.frames.lock();
    assert!(!frames.is_empty());
    for frame in frames.iter() {
        if let Some(ref txt) = frame.text {
            assert!(!txt.contains("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9"));
        }
    }
}

#[tokio::test]
async fn sequential_runner_and_vs_semicolon_policies() {
    // 1. SequentialSeparator::And: item 0 sucede (0), item 1 falha (2) -> item 2 é Skipped -> RunStatus::Partial
    let (_tmp1, db1, repo1, _p1, _group1, run1) = setup_repo_and_run(
        ExecutionMode::Sequential,
        SequentialSeparator::And,
        &["echo s0", "echo s1", "echo s2"],
        None,
    )
    .await;

    let pty_id = PtyId::new();
    let sess_id = SessionId::new();
    let b0 = CommandBlockId::new();
    let b1 = CommandBlockId::new();

    let lease1 = Box::new(FakeLease {
        state: PaneShellState {
            tab_id: TabId::new(),
            pane_id: PaneId::new(),
            pty_id,
            session_id: sess_id,
            shell_path: PathBuf::from("/bin/bash"),
            generation: 1,
            prompt_ready: true,
            in_alt_screen: false,
            foreground_busy: false,
            tracking_available: true,
        },
        events: VecDeque::from(vec![
            ShellLifecycleEvent::BlockStarted {
                pty_id,
                session_id: sess_id,
                block_id: b0,
                generation: 1,
            },
            ShellLifecycleEvent::BlockFinished {
                pty_id,
                session_id: sess_id,
                block_id: b0,
                generation: 1,
                exit_code: 0,
                signal: None,
            },
            ShellLifecycleEvent::PromptReady {
                pty_id,
                session_id: sess_id,
                generation: 1,
            },
            ShellLifecycleEvent::BlockStarted {
                pty_id,
                session_id: sess_id,
                block_id: b1,
                generation: 1,
            },
            ShellLifecycleEvent::BlockFinished {
                pty_id,
                session_id: sess_id,
                block_id: b1,
                generation: 1,
                exit_code: 2,
                signal: None,
            },
            ShellLifecycleEvent::PromptReady {
                pty_id,
                session_id: sess_id,
                generation: 1,
            },
        ]),
    });

    let pty_port1: Arc<dyn GroupPtyPort> = Arc::new(ConcurrencyTrackingPtyPort {
        active_now: Arc::new(AtomicUsize::new(0)),
        max_observed: Arc::new(AtomicUsize::new(0)),
        sleep_ms: 1,
    });
    let shell_port1: Arc<dyn GroupShellPort> = Arc::new(FakeShellPort { env_chdir: true });
    let events1 = Arc::new(RecordingEventSink::default());
    let runner1 = SequentialRunner::new(
        pty_port1,
        shell_port1,
        repo1.clone() as Arc<dyn CommandGroupRepo>,
        events1.clone() as Arc<dyn GroupEventSink>,
    );

    runner1
        .launch_with_lease(run1.clone(), lease1)
        .await
        .unwrap();
    for _ in 0..50 {
        let cur = repo1.get_run(&run1.id).unwrap();
        if cur.status.is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let detail1 = repo1.get_run(&run1.id).unwrap();
    assert_eq!(detail1.items[0].status, ItemStatus::Succeeded);
    assert_eq!(detail1.items[1].status, ItemStatus::Failed);
    assert_eq!(detail1.items[2].status, ItemStatus::Skipped);
    assert_eq!(detail1.status, RunStatus::Partial);
    db1.shutdown().await.unwrap();

    // 2. SequentialSeparator::Semicolon: item 0 falha (1), item 1 sucede (0) -> ambos executam -> RunStatus::Partial
    let (_tmp2, db2, repo2, _p2, _group2, run2) = setup_repo_and_run(
        ExecutionMode::Sequential,
        SequentialSeparator::Semicolon,
        &["echo s0", "echo s1"],
        None,
    )
    .await;

    let lease2 = Box::new(FakeLease {
        state: PaneShellState {
            tab_id: TabId::new(),
            pane_id: PaneId::new(),
            pty_id,
            session_id: sess_id,
            shell_path: PathBuf::from("/bin/bash"),
            generation: 1,
            prompt_ready: true,
            in_alt_screen: false,
            foreground_busy: false,
            tracking_available: true,
        },
        events: VecDeque::from(vec![
            ShellLifecycleEvent::BlockStarted {
                pty_id,
                session_id: sess_id,
                block_id: b0,
                generation: 1,
            },
            ShellLifecycleEvent::BlockFinished {
                pty_id,
                session_id: sess_id,
                block_id: b0,
                generation: 1,
                exit_code: 1,
                signal: None,
            },
            ShellLifecycleEvent::PromptReady {
                pty_id,
                session_id: sess_id,
                generation: 1,
            },
            ShellLifecycleEvent::BlockStarted {
                pty_id,
                session_id: sess_id,
                block_id: b1,
                generation: 1,
            },
            ShellLifecycleEvent::BlockFinished {
                pty_id,
                session_id: sess_id,
                block_id: b1,
                generation: 1,
                exit_code: 0,
                signal: None,
            },
            ShellLifecycleEvent::PromptReady {
                pty_id,
                session_id: sess_id,
                generation: 1,
            },
        ]),
    });

    let pty_port2: Arc<dyn GroupPtyPort> = Arc::new(ConcurrencyTrackingPtyPort {
        active_now: Arc::new(AtomicUsize::new(0)),
        max_observed: Arc::new(AtomicUsize::new(0)),
        sleep_ms: 1,
    });
    let shell_port2: Arc<dyn GroupShellPort> = Arc::new(FakeShellPort { env_chdir: true });
    let events2: Arc<dyn GroupEventSink> = Arc::new(RecordingEventSink::default());
    let runner2 = SequentialRunner::new(
        pty_port2,
        shell_port2,
        repo2.clone() as Arc<dyn CommandGroupRepo>,
        events2,
    );

    runner2
        .launch_with_lease(run2.clone(), lease2)
        .await
        .unwrap();
    for _ in 0..50 {
        let cur = repo2.get_run(&run2.id).unwrap();
        if cur.status.is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let detail2 = repo2.get_run(&run2.id).unwrap();
    assert_eq!(detail2.items[0].status, ItemStatus::Failed);
    assert_eq!(detail2.items[1].status, ItemStatus::Succeeded);
    assert_eq!(detail2.status, RunStatus::Partial);
    db2.shutdown().await.unwrap();
}

#[tokio::test]
async fn parallel_runner_enforces_16_pty_concurrency_cap() {
    let cmds: Vec<String> = (0..20).map(|i| format!("echo job-{i}")).collect();
    let cmd_refs: Vec<&str> = cmds.iter().map(String::as_str).collect();

    let (_tmp, db, repo, _p1, _group, run) = setup_repo_and_run(
        ExecutionMode::Parallel,
        SequentialSeparator::And,
        &cmd_refs,
        None,
    )
    .await;

    let active_now = Arc::new(AtomicUsize::new(0));
    let max_observed = Arc::new(AtomicUsize::new(0));
    let pty_port: Arc<dyn GroupPtyPort> = Arc::new(ConcurrencyTrackingPtyPort {
        active_now: Arc::clone(&active_now),
        max_observed: Arc::clone(&max_observed),
        sleep_ms: 40,
    });
    let events: Arc<dyn GroupEventSink> = Arc::new(RecordingEventSink::default());
    let runner = ParallelRunner::new(pty_port, repo.clone() as Arc<dyn CommandGroupRepo>, events);

    runner.launch(run.clone()).await.unwrap();
    for _ in 0..100 {
        let cur = repo.get_run(&run.id).unwrap();
        if cur.status.is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(15)).await;
    }

    let final_run = repo.get_run(&run.id).unwrap();
    assert_eq!(final_run.status, RunStatus::Succeeded);
    let observed = max_observed.load(Ordering::SeqCst);
    assert!(
        observed <= 16,
        "concorrência máxima observada ({observed}) excedeu 16"
    );
    assert!(
        observed >= 2,
        "esperava execução paralela real, observou {observed}"
    );

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn multi_tab_template_resolution_and_partial_failure_preservation() {
    let pid = ProjectId::new();
    let item_cont = CommandGroupItem {
        id: GroupItemId::new(),
        group_id: CommandGroupId::new(),
        position: 0,
        title: "Vite".to_string(),
        command: "npm run dev".to_string(),
        cwd: None,
        env: BTreeMap::new(),
        continuous: true,
    };

    // Sem template compatível com continuous == true deve retornar TemplateIncompatible
    let err = MultiTabPlanner::resolve_templates_for_items(
        &[item_cont],
        &[ProjectShellTemplate {
            id: TemplateId::new(),
            project_id: pid,
            name: "Shell Temp".to_string(),
            is_default: true,
            is_shell_environment: true,
            continuous: false,
            shell_path: PathBuf::from("/bin/bash"),
            base_env: BTreeMap::new(),
        }],
        &BTreeMap::new(),
    )
    .unwrap_err();
    assert_eq!(err.code.0, GroupErrorCode::TemplateIncompatible.as_str());

    // Falha parcial na abertura da 3ª aba (índice 2) preserva abas 0, 1 e 3
    let tpl_id = TemplateId::new();
    let (_tmp, db, repo, _p1, _group, run) = setup_repo_and_run(
        ExecutionMode::MultiTabs,
        SequentialSeparator::And,
        &["echo t0", "echo t1", "echo t2", "echo t3"],
        Some(tpl_id),
    )
    .await;

    let tab_port: Arc<dyn GroupTabPort> = Arc::new(PartialFailTabPort {
        fail_at_index: Some(2),
        opened_counter: AtomicUsize::new(0),
    });
    let pty_port: Arc<dyn GroupPtyPort> = Arc::new(ConcurrencyTrackingPtyPort {
        active_now: Arc::new(AtomicUsize::new(0)),
        max_observed: Arc::new(AtomicUsize::new(0)),
        sleep_ms: 5,
    });
    let shell_port: Arc<dyn GroupShellPort> = Arc::new(FakeShellPort { env_chdir: true });
    let events: Arc<dyn GroupEventSink> = Arc::new(RecordingEventSink::default());

    let runner = MultiTabRunner::new(
        tab_port,
        pty_port,
        shell_port,
        repo.clone() as Arc<dyn CommandGroupRepo>,
        events,
    );

    runner.launch(run.clone()).await.unwrap();
    for _ in 0..80 {
        let cur = repo.get_run(&run.id).unwrap();
        if cur.status.is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let detail = repo.get_run(&run.id).unwrap();
    assert_eq!(detail.status, RunStatus::Partial);
    assert_eq!(detail.items[0].status, ItemStatus::Succeeded);
    assert!(detail.items[0].tab_id.is_some());
    assert_eq!(detail.items[1].status, ItemStatus::Succeeded);
    assert!(detail.items[1].tab_id.is_some());
    assert_eq!(detail.items[2].status, ItemStatus::Failed);
    assert_eq!(detail.items[3].status, ItemStatus::Succeeded);

    db.shutdown().await.unwrap();
}
