//! Testes do orquestrador `CommandGroupService` e `GroupSuggester` (`CA-09`, `CA-20`, `CA-23`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use mt_commands::groups::events::{GroupChangedEvent, GroupEventSink, GroupRunEvent};
use mt_commands::groups::model::{
    CommandGroupInput, CommandGroupItemInput, ExecutionMode, GroupErrorCode, GroupRequestId,
    GroupRunId, GroupRunItemId, GroupRunItemSnapshot, GroupRunRequest, GroupRunSnapshot,
    GroupScope, RunStatus, SequentialSeparator, ShellDialect,
};
use mt_commands::groups::ports::{
    GroupAiPort, GroupPtyPort, GroupShellPort, GroupTabOpenSpec, GroupTabPort, HeadlessSpawnSpec,
    OpenedGroupTab, PaneExecutionLease, PaneShellState, ProjectShellTemplate, RawOutputTap,
    RecurringCommandEntry, RecurringCommandsSource, ShellLifecycleEvent, SuggestPromptContext,
    SupervisedExitInfo,
};
use mt_commands::groups::repo::{CommandGroupRepo, CreateRunRecord, SqliteCommandGroupRepo};
use mt_commands::groups::service::CommandGroupService;
use mt_commands::groups::suggest::GroupSuggestRequest;
use mt_core::error::MtResult;
use mt_core::ids::{CommandBlockId, PaneId, ProjectId, PtyId, SessionId, TabId, TemplateId};
use mt_db::{Db, DbConfig};
use parking_lot::Mutex;
use tempfile::TempDir;
use tokio::sync::mpsc;

#[derive(Default)]
struct NoopEventSink {
    changed: Mutex<Vec<GroupChangedEvent>>,
}

impl GroupEventSink for NoopEventSink {
    fn emit_group_changed(&self, event: GroupChangedEvent) {
        self.changed.lock().push(event);
    }
    fn emit_run_started(&self, _event: GroupRunEvent) {}
    fn emit_run_item_changed(&self, _event: GroupRunEvent) {}
    fn emit_run_finished(&self, _event: GroupRunEvent) {}
}

struct MockLease {
    state: PaneShellState,
}

#[async_trait]
impl PaneExecutionLease for MockLease {
    fn state(&self) -> &PaneShellState {
        &self.state
    }
    async fn next_event(&mut self) -> Option<ShellLifecycleEvent> {
        None
    }
    async fn release(self: Box<Self>) {}
}

struct MockShellPort;

#[async_trait]
impl GroupShellPort for MockShellPort {
    async fn inspect_pane(&self, pane_id: &PaneId) -> MtResult<PaneShellState> {
        Ok(PaneShellState {
            tab_id: TabId::new(),
            pane_id: *pane_id,
            pty_id: PtyId::new(),
            session_id: SessionId::new(),
            shell_path: PathBuf::from("/bin/zsh"),
            generation: 1,
            prompt_ready: true,
            in_alt_screen: false,
            foreground_busy: false,
            tracking_available: true,
        })
    }

    async fn acquire_pane_lease(&self, pane_id: &PaneId) -> MtResult<Box<dyn PaneExecutionLease>> {
        let state = self.inspect_pane(pane_id).await?;
        Ok(Box::new(MockLease { state }))
    }

    async fn subscribe_session_events(
        &self,
        _session_id: &SessionId,
    ) -> MtResult<mpsc::UnboundedReceiver<ShellLifecycleEvent>> {
        let (_tx, rx) = mpsc::unbounded_channel();
        Ok(rx)
    }

    fn supports_env_chdir(&self) -> bool {
        true
    }
}

struct MockPtyPort;

#[async_trait]
impl GroupPtyPort for MockPtyPort {
    async fn spawn_headless(
        &self,
        _spec: HeadlessSpawnSpec,
        output_tap: Arc<dyn RawOutputTap>,
    ) -> MtResult<PtyId> {
        output_tap.on_bytes(b"done\n");
        Ok(PtyId::new())
    }

    async fn wait_exit(&self, pty_id: &PtyId) -> MtResult<SupervisedExitInfo> {
        tokio::time::sleep(Duration::from_millis(5)).await;
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

struct MockTabPort {
    root: PathBuf,
}

#[async_trait]
impl GroupTabPort for MockTabPort {
    async fn resolve_project_root(&self, _project_id: &ProjectId) -> MtResult<PathBuf> {
        Ok(self.root.clone())
    }

    async fn list_project_templates(
        &self,
        project_id: &ProjectId,
    ) -> MtResult<Vec<ProjectShellTemplate>> {
        Ok(vec![ProjectShellTemplate {
            id: TemplateId::new(),
            project_id: *project_id,
            name: "Shell Padrão".to_string(),
            is_default: true,
            is_shell_environment: true,
            continuous: true,
            shell_path: PathBuf::from("/bin/zsh"),
            base_env: BTreeMap::new(),
        }])
    }

    async fn open_tab(&self, _spec: GroupTabOpenSpec) -> MtResult<OpenedGroupTab> {
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

struct MockRecurringSource;

#[async_trait]
impl RecurringCommandsSource for MockRecurringSource {
    fn is_available(&self) -> bool {
        true
    }

    async fn list_recurring(
        &self,
        _project_id: &ProjectId,
        _limit: u32,
    ) -> MtResult<Vec<RecurringCommandEntry>> {
        Ok(vec![
            RecurringCommandEntry {
                command: "npm run dev".to_string(),
                cwd: None,
                frequency: 15,
                last_used_at: 1000,
            },
            RecurringCommandEntry {
                command: "php artisan horizon".to_string(),
                cwd: None,
                frequency: 12,
                last_used_at: 2000,
            },
        ])
    }
}

struct MockAiPort {
    suggestion: CommandGroupInput,
}

#[async_trait]
impl GroupAiPort for MockAiPort {
    async fn generate_group_suggestion(
        &self,
        _ai_request_id: &str,
        _context: SuggestPromptContext,
    ) -> MtResult<CommandGroupInput> {
        Ok(self.suggestion.clone())
    }
}

async fn build_service(
    suggestion: CommandGroupInput,
) -> (TempDir, Db, ProjectId, Arc<CommandGroupService>) {
    let tmp = TempDir::new().unwrap();
    let proj_dir = tmp.path().join("proj");
    std::fs::create_dir_all(&proj_dir).unwrap();

    let cfg = DbConfig::for_path(tmp.path().join("maxterminal.db"));
    let db = Db::open(cfg).unwrap();
    let p1 = ProjectId::new();
    let p1_str = p1.to_string();
    let proj_path_str = proj_dir.to_string_lossy().into_owned();

    db.write(move |tx| {
        tx.execute(
            "INSERT INTO projects (id, title, path, icon, color, stack, sort_order, created_at, updated_at)
             VALUES (?1, 'P1', ?2, 'lucide:box', '#22C55E', 'laravel', 0, 1000, 1000);",
            rusqlite::params![p1_str, proj_path_str],
        )?;
        Ok(())
    })
    .await
    .unwrap();

    let repo: Arc<dyn CommandGroupRepo> = Arc::new(SqliteCommandGroupRepo::new(db.clone()));
    let pty_port: Arc<dyn GroupPtyPort> = Arc::new(MockPtyPort);
    let shell_port: Arc<dyn GroupShellPort> = Arc::new(MockShellPort);
    let tab_port: Arc<dyn GroupTabPort> = Arc::new(MockTabPort { root: proj_dir });
    let ai_port: Arc<dyn GroupAiPort> = Arc::new(MockAiPort { suggestion });
    let recurring_source: Option<Arc<dyn RecurringCommandsSource>> =
        Some(Arc::new(MockRecurringSource));
    let events: Arc<dyn GroupEventSink> = Arc::new(NoopEventSink::default());

    let svc = CommandGroupService::new(
        repo,
        pty_port,
        shell_port,
        tab_port,
        ai_port,
        recurring_source,
        events,
    );

    (tmp, db, p1, svc)
}

fn default_ai_suggestion() -> CommandGroupInput {
    CommandGroupInput {
        scope: GroupScope::Local,
        project_id: None,
        name: "Ambiente Laravel + Vite".to_string(),
        description: "Sobe os serviços principais".to_string(),
        labels: vec!["dev".to_string(), "laravel".to_string()],
        mode: ExecutionMode::MultiTabs,
        separator: SequentialSeparator::And,
        cwd: None,
        env: BTreeMap::new(),
        items: vec![
            CommandGroupItemInput {
                id: None,
                title: "Servidor Vite".to_string(),
                command: "npm run dev".to_string(),
                cwd: None,
                env: BTreeMap::new(),
                continuous: true,
            },
            CommandGroupItemInput {
                id: None,
                title: "Horizon Queue".to_string(),
                command: "php artisan horizon".to_string(),
                cwd: None,
                env: BTreeMap::new(),
                continuous: true,
            },
        ],
    }
}

#[tokio::test]
async fn recover_interrupted_and_request_id_idempotency_before_revision_check() {
    let (_tmp, db, p1, svc) = build_service(default_ai_suggestion()).await;

    let created = svc
        .create(CommandGroupInput {
            scope: GroupScope::Local,
            project_id: Some(p1),
            name: "Dev Stack".to_string(),
            description: String::new(),
            labels: vec![],
            mode: ExecutionMode::Parallel,
            separator: SequentialSeparator::And,
            cwd: None,
            env: BTreeMap::new(),
            items: vec![CommandGroupItemInput {
                id: None,
                title: "Vite".to_string(),
                command: "npm run dev".to_string(),
                cwd: None,
                env: BTreeMap::new(),
                continuous: false,
            }],
        })
        .await
        .unwrap();

    let req_id = GroupRequestId("req-idemp-1".to_string());
    let run_req = GroupRunRequest {
        request_id: req_id.clone(),
        group_id: created.id,
        expected_revision: created.revision,
        project_id: p1,
        origin_pane_id: None,
        mode_override: None,
        template_by_item: BTreeMap::new(),
    };
    let run_first = svc.run(run_req.clone()).await.unwrap();

    // Atualiza o grupo para incrementar revision (1 -> 2)
    let updated = svc
        .update(
            created.id,
            1,
            CommandGroupInput {
                scope: GroupScope::Local,
                project_id: Some(p1),
                name: "Dev Stack v2".to_string(),
                description: String::new(),
                labels: vec![],
                mode: ExecutionMode::Parallel,
                separator: SequentialSeparator::And,
                cwd: None,
                env: BTreeMap::new(),
                items: vec![CommandGroupItemInput {
                    id: None,
                    title: "Vite".to_string(),
                    command: "npm run dev -- --host".to_string(),
                    cwd: None,
                    env: BTreeMap::new(),
                    continuous: false,
                }],
            },
        )
        .await
        .unwrap();
    assert_eq!(updated.revision, 2);

    // Repetir exatamente o mesmo GroupRunRequest (mesmo request_id e mesmo expected_revision = 1)
    // deve retornar o run original por idempotência ANTES de checar revisão!
    let run_repeat = svc.run(run_req).await.unwrap();
    assert_eq!(run_repeat.id, run_first.id);

    // Reutilizar o mesmo request_id com payload diferente retorna RequestConflict
    let conflict_err = svc
        .run(GroupRunRequest {
            request_id: req_id,
            group_id: created.id,
            expected_revision: 2,
            project_id: p1,
            origin_pane_id: None,
            mode_override: None,
            template_by_item: BTreeMap::new(),
        })
        .await
        .unwrap_err();
    assert_eq!(
        conflict_err.code.0,
        GroupErrorCode::RequestConflict.as_str()
    );

    // Cria um run órfão (Queued/Pending) e executa recover_interrupted()
    let orphan_id = GroupRunId::new();
    let repo = SqliteCommandGroupRepo::new(db.clone());
    repo.accept_run(CreateRunRecord {
        run_id: orphan_id.clone(),
        request_id: GroupRequestId("req-orphan".to_string()),
        request_hash: "h-orphan".to_string(),
        group_id: created.id,
        project_id: p1,
        origin_session_id: None,
        snapshot: GroupRunSnapshot {
            group_id: created.id,
            group_revision: 2,
            scope: created.scope,
            name: created.name.clone(),
            description: String::new(),
            labels: vec![],
            defined_mode: ExecutionMode::Parallel,
            effective_mode: ExecutionMode::Parallel,
            separator: SequentialSeparator::And,
            cwd: None,
            env: BTreeMap::new(),
            project_root: "/tmp".to_string(),
            shell_path: "/bin/zsh".to_string(),
            shell_dialect: ShellDialect::Zsh,
            origin_tab_id: None,
            origin_pane_id: None,
            origin_pty_id: None,
            origin_session_id: None,
            origin_generation: None,
        },
        mode: ExecutionMode::Parallel,
        items: vec![(
            GroupRunItemId::new(),
            GroupRunItemSnapshot {
                source_item_id: created.items[0].id.clone(),
                position: 0,
                title: "Orphan".to_string(),
                command: "sleep 10".to_string(),
                defined_cwd: None,
                resolved_cwd: "/tmp".to_string(),
                env: BTreeMap::new(),
                continuous: false,
                template_id: None,
                shell_path: "/bin/zsh".to_string(),
                shell_dialect: ShellDialect::Zsh,
            },
        )],
        created_at: 5000,
    })
    .unwrap();

    let recovered = svc.recover_interrupted().await.unwrap();
    assert!(recovered.iter().any(|r| r.id == orphan_id));
    let orphan_detail = svc.get_run(orphan_id).await.unwrap();
    assert_eq!(orphan_detail.status, RunStatus::Interrupted);
    assert!(orphan_detail.finished_at.is_some());

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn ai_suggestion_and_recurring_commands_work_and_block_secrets() {
    let (_tmp, db, p1, svc) = build_service(default_ai_suggestion()).await;

    let recurring = svc.recurring_commands(p1, 10).await.unwrap();
    assert!(recurring.available);
    assert_eq!(recurring.items.len(), 2);

    let resp = svc
        .suggest(GroupSuggestRequest {
            request_id: "ai-req-1".to_string(),
            project_id: Some(p1),
            pane_id: None,
            scope: GroupScope::Local,
            goal: "Subir servidores de desenvolvimento".to_string(),
            draft: None,
            draft_revision: 3,
        })
        .await
        .unwrap();

    assert_eq!(resp.draft_revision, 3);
    assert_eq!(resp.suggestion.name, "Ambiente Laravel + Vite");
    assert_eq!(resp.suggestion.project_id, Some(p1));
    assert_eq!(resp.suggestion.items.len(), 2);
    db.shutdown().await.unwrap();

    // Sugestão com segredo deve ser bloqueada por validate_group_input
    let mut bad_sugg = default_ai_suggestion();
    bad_sugg.items[0].command =
        "curl -H 'Authorization: Bearer eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U' https://example.com"
            .to_string();
    let (_tmp2, db2, p2, svc2) = build_service(bad_sugg).await;
    let err = svc2
        .suggest(GroupSuggestRequest {
            request_id: "ai-req-2".to_string(),
            project_id: Some(p2),
            pane_id: None,
            scope: GroupScope::Local,
            goal: "Teste segredo".to_string(),
            draft: None,
            draft_revision: 1,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code.0, GroupErrorCode::SecretDetected.as_str());
    db2.shutdown().await.unwrap();
}
