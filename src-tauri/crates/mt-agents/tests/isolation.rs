//! Testes de integração para `IsolationService`, semeadura de templates e ciclo de vida de worktrees (`CA-08`, `CA-12`..`CA-15`).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::useless_vec,
    clippy::assert_is_empty,
    clippy::needless_borrow
)]

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use async_trait::async_trait;
use mt_agents::error::codes;
use mt_agents::isolation::{
    IsolationService, WorktreeIsolation, WorktreeResolution, ensure_agent_templates_in_db,
};
use mt_agents::ports::{ForegroundProc, GitPort, ProjectRecord, PtyPort, WorkspacePort};
use mt_agents::registry::{AgentKind, AgentRegistry};
use mt_agents::settings::{AgentsSettings, WorktreeLocationDto};
use mt_core::{ErrorCode, MtError, MtResult, ProjectId, PtyId, TabId};
use mt_db::{
    Db, DbConfig, ProjectMutation, StoredNewProject, StoredProject, sanitizer::SanitizedText,
};
use mt_git::{
    CliGitRunner, CliWorktreeManager, GitContext, GitRunner, NoopActiveWorktreeGuard,
    WorktreeCreateRequest, WorktreeEntry, WorktreeManager,
};
use parking_lot::RwLock;
use tempfile::TempDir;

struct TestGitPort {
    wt_mgr: CliWorktreeManager,
}

impl TestGitPort {
    fn new() -> Self {
        let runner = Arc::new(CliGitRunner::default());
        let wt_mgr = CliWorktreeManager::new(
            runner as Arc<dyn GitRunner>,
            Arc::new(NoopActiveWorktreeGuard),
        );
        Self { wt_mgr }
    }
}

#[async_trait]
impl GitPort for TestGitPort {
    async fn create_worktree(&self, req: WorktreeCreateRequest) -> MtResult<WorktreeEntry> {
        self.wt_mgr
            .create(req)
            .await
            .map_err(|e| MtError::new(e.code(), e.to_string()))
    }

    async fn remove_worktree(
        &self,
        _repo_root: &Path,
        worktree_path: &Path,
        force: bool,
    ) -> MtResult<()> {
        self.wt_mgr
            .remove(worktree_path, force)
            .await
            .map_err(|e| MtError::new(e.code(), e.to_string()))
    }

    async fn status(&self, _cwd: &Path) -> MtResult<Option<GitContext>> {
        Ok(None)
    }

    async fn run(&self, cwd: &Path, args: &[&str]) -> MtResult<String> {
        let out = tokio::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .await
            .map_err(|e| MtError::new(ErrorCode("git.command_failed"), e.to_string()))?;
        if !out.status.success() {
            return Err(MtError::new(
                ErrorCode("git.command_failed"),
                String::from_utf8_lossy(&out.stderr).trim().to_string(),
            ));
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }
}

struct DummyPtyPort;
impl PtyPort for DummyPtyPort {
    fn foreground(&self, _pty: PtyId) -> Option<ForegroundProc> {
        None
    }
    fn has_active_processes(&self, _tab: TabId) -> bool {
        false
    }
}

struct DummyWorkspacePort {
    project: ProjectRecord,
}

#[async_trait]
impl WorkspacePort for DummyWorkspacePort {
    async fn project(&self, _id: ProjectId) -> MtResult<ProjectRecord> {
        Ok(self.project.clone())
    }
    fn project_of_tab(&self, _tab: TabId) -> Option<ProjectId> {
        Some(self.project.id)
    }
    fn tabs_using_cwd(&self, _dir: &Path) -> Vec<TabId> {
        Vec::new()
    }
    async fn ensure_agent_templates(&self, _project: ProjectId) -> MtResult<u32> {
        Ok(0)
    }
}

fn init_git_repo(dir: &Path) {
    fs::create_dir_all(dir).unwrap();
    let run = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} falhou");
    };
    run(&["init", "-b", "main"]);
    run(&["config", "user.email", "test@example.com"]);
    run(&["config", "user.name", "Test User"]);
    fs::write(dir.join("README.md"), "# Test Repo\n").unwrap();
    run(&["add", "README.md"]);
    run(&["commit", "-m", "Initial commit"]);
}

#[tokio::test]
async fn seeds_agent_templates_idempotently_and_respects_user_deletion() {
    let tmp = TempDir::new().unwrap();
    let db = Db::open(DbConfig::for_path(tmp.path().join("test.db"))).unwrap();
    let reg = AgentRegistry::with_builtins();

    let proj_id = ProjectId::new();
    db.projects()
        .commit(ProjectMutation::Add(StoredNewProject {
            project: StoredProject {
                id: proj_id,
                title: SanitizedText::new("Demo"),
                icon: "mdi:folder".to_string(),
                color: "#0EA5E9".to_string(),
                path: "/tmp/demo".to_string(),
                root_id: None,
                stack: "rust".to_string(),
                identity_source: "auto".to_string(),
                sort_order: 0,
                missing: false,
                created_at: 1_000,
                updated_at: 1_000,
            },
            clear_dismissed_paths: Vec::new(),
            if_exists_noop: false,
        }))
        .await
        .unwrap();

    // 1ª execução cria os 4 templates dedicados
    let created_1 = ensure_agent_templates_in_db(&db, &reg, proj_id, 2_000)
        .await
        .unwrap();
    assert_eq!(created_1, 4);

    // 2ª execução é no-op (retorna 0)
    let created_2 = ensure_agent_templates_in_db(&db, &reg, proj_id, 2_100)
        .await
        .unwrap();
    assert_eq!(created_2, 0);

    // Exclui manualmente 1 template de agente -> ensure_agent_templates_in_db NÃO o recria (CA-08)
    let all_tpls = db.templates().list_by_project(proj_id).await.unwrap();
    let claude_tpl = all_tpls
        .iter()
        .find(|t| t.config.environment.agent_id.as_deref() == Some("claude-code"))
        .unwrap();
    db.templates()
        .delete(claude_tpl.id, claude_tpl.revision)
        .await
        .unwrap();

    let created_3 = ensure_agent_templates_in_db(&db, &reg, proj_id, 2_200)
        .await
        .unwrap();
    assert_eq!(created_3, 0);

    let remaining = db.templates().list_by_project(proj_id).await.unwrap();
    assert!(
        !remaining
            .iter()
            .any(|t| t.config.environment.agent_id.as_deref() == Some("claude-code"))
    );

    db.shutdown().await.unwrap();
}

#[tokio::test]
async fn proposes_creates_inspects_and_resolves_agent_worktree_safely() {
    let tmp = TempDir::new().unwrap();
    let repo_dir = tmp.path().join("repo");
    init_git_repo(&repo_dir);

    let db = Db::open(DbConfig::for_path(tmp.path().join("test.db"))).unwrap();
    let proj_id = ProjectId::new();
    let project = ProjectRecord {
        id: proj_id,
        title: "Repo".to_string(),
        path: repo_dir.clone(),
        stack: "rust".to_string(),
    };

    let settings = Arc::new(RwLock::new(AgentsSettings::default()));
    let iso_service = IsolationService::new(
        Arc::new(TestGitPort::new()),
        Arc::new(DummyPtyPort),
        Arc::new(DummyWorkspacePort {
            project: project.clone(),
        }),
        db.clone(),
        settings,
    );

    // 1. Proposta somente-leitura detecta que .worktrees/ ainda não está no .gitignore (CA-13)
    let proposal = iso_service
        .propose(&project, AgentKind::Claude, Some("Fix Auth Bug"))
        .await
        .unwrap();
    assert_eq!(proposal.slug, "fix-auth-bug");
    assert_eq!(proposal.branch, "agent/claude/fix-auth-bug");
    assert!(!proposal.root_ignored);
    assert!(!proposal.warnings.is_empty());

    // 2. Prepara worktree com ensure_gitignore = true
    let prepared = iso_service
        .prepare(
            &project,
            AgentKind::Claude,
            &WorktreeIsolation::Create {
                location: WorktreeLocationDto::Project,
                slug: proposal.slug.clone(),
                branch: proposal.branch.clone(),
                base: Some(proposal.base_ref.clone()),
                ensure_gitignore: true,
            },
        )
        .await
        .unwrap()
        .unwrap();
    assert!(prepared.worktree_path.exists());
    assert!(prepared.created_by_app);

    let tab_id = TabId::new();
    let link = iso_service
        .record_tab_link(
            tab_id,
            proj_id,
            AgentKind::Claude,
            Some(&prepared.to_link_draft()),
            Some(&prepared.launch_id),
            3_000,
        )
        .await
        .unwrap();

    // 3. Fecha a aba com a worktree limpa e sem commits extras -> permite Keep, Remove e RemoveAndDeleteBranch
    let inspection_clean = iso_service
        .mark_tab_closed(tab_id, 4_000)
        .await
        .unwrap()
        .unwrap();
    assert!(!inspection_clean.dirty);
    assert!(inspection_clean.branch_merged);
    assert!(
        inspection_clean
            .allowed
            .contains(&WorktreeResolution::Remove)
    );
    assert!(
        inspection_clean
            .allowed
            .contains(&WorktreeResolution::RemoveAndDeleteBranch)
    );

    // 4. Se um arquivo não rastreado surgir na worktree antes do resolve:
    //    - O token antigo falha com agents.inspection_stale
    //    - A nova inspeção bloqueia Remove (allowed = [Keep])
    fs::write(prepared.worktree_path.join("untracked.txt"), "dirty").unwrap();

    let stale_err = iso_service
        .resolve(
            link.id,
            WorktreeResolution::Remove,
            &inspection_clean.inspection_token,
        )
        .await
        .unwrap_err();
    assert_eq!(stale_err.code.0, codes::INSPECTION_STALE);

    let inspection_dirty = iso_service.inspect(link.id).await.unwrap();
    assert!(inspection_dirty.dirty);
    assert_eq!(inspection_dirty.allowed, vec![WorktreeResolution::Keep]);

    let dirty_err = iso_service
        .resolve(
            link.id,
            WorktreeResolution::Remove,
            &inspection_dirty.inspection_token,
        )
        .await
        .unwrap_err();
    assert_eq!(dirty_err.code.0, codes::WORKTREE_DIRTY);

    // 5. Limpa o arquivo e resolve com RemoveAndDeleteBranch
    fs::remove_file(prepared.worktree_path.join("untracked.txt")).unwrap();
    let inspection_clean_again = iso_service.inspect(link.id).await.unwrap();
    let resolved = iso_service
        .resolve(
            link.id,
            WorktreeResolution::RemoveAndDeleteBranch,
            &inspection_clean_again.inspection_token,
        )
        .await
        .unwrap();
    assert_eq!(
        resolved.resolution,
        Some(WorktreeResolution::RemoveAndDeleteBranch)
    );
    assert!(!prepared.worktree_path.exists());

    db.shutdown().await.unwrap();
}
