//! Isolamento de Agentes CLI em Git Worktree (`IsolationService`), semeadura idempotente de templates dedicados,
//! inspeção pós-fechamento e resolução conservadora validada por `inspection_token`.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use mt_core::{ErrorCode, MtError, MtResult, ProjectId, TabId, TemplateId};
use mt_git::{WorktreeCreateRequest, WorktreeLocation, discover_repository};
use parking_lot::RwLock;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::detect::current_unix_ms;
use crate::error;
use crate::launch::AgentTemplateOptions;
use crate::ports::{GitPort, ProjectRecord, PtyPort, WorkspacePort};
use crate::registry::{AgentKind, AgentRegistry};
use crate::settings::{AgentsSettings, WorktreeLocationDto};

const MAX_CHANGED_FILES_REPORTED: usize = 50;

/// Modo de isolamento em Git Worktree solicitado na abertura de uma aba de agente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum WorktreeIsolation {
    /// Sem isolamento; executa diretamente na pasta do projeto.
    None,
    /// Cria uma nova worktree dedicada para a execução.
    #[serde(rename_all = "camelCase")]
    Create {
        /// Localização da pasta de worktrees (`.worktrees/` ou `.max-code-worktrees/`).
        location: WorktreeLocationDto,
        /// Slug identificador da worktree.
        slug: String,
        /// Nome da branch Git a ser criada.
        branch: String,
        /// Referência base opcional (`HEAD` quando `None`).
        base: Option<String>,
        /// Quando `true`, garante que a pasta de worktrees esteja no `.gitignore` local/info/exclude.
        ensure_gitignore: bool,
    },
    /// Vincula uma worktree já existente pertencente ao mesmo repositório.
    #[serde(rename_all = "camelCase")]
    Existing {
        /// Caminho da worktree existente.
        path: String,
    },
}

/// Proposta somente-leitura de criação de worktree isolada (`agent_worktree_propose`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeProposal {
    /// Caminho canônico da raiz principal do repositório Git.
    pub repo_root: String,
    /// Slug único calculado.
    pub slug: String,
    /// Nome do diretório da worktree (`wt-<slug>`).
    pub name: String,
    /// Nome completo da branch proposta (`<prefix>/<short>/<slug>`).
    pub branch: String,
    /// Caminho completo onde a worktree será criada.
    pub path: String,
    /// Localização selecionada.
    pub location: WorktreeLocationDto,
    /// Indica se a pasta de worktrees está ignorada pelo Git (`git check-ignore`).
    pub root_ignored: bool,
    /// Avisos em `pt-BR` para exibição no modal.
    pub warnings: Vec<String>,
    /// OID completo (40 caracteres) do `HEAD` base atual.
    pub base_ref: String,
}

/// Requisição para abrir uma aba de Agente CLI (`agent_tab_open`).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabOpenRequest {
    /// Identificador do projeto.
    pub project_id: ProjectId,
    /// Tipo do agente CLI.
    pub agent: AgentKind,
    /// Template de aba opcional (se `None`, usa o template dedicado do agente no projeto).
    pub template_id: Option<TemplateId>,
    /// Modo de isolamento em Git Worktree.
    pub isolation: WorktreeIsolation,
    /// Modelo opcional a passar via flag CLI.
    pub model: Option<String>,
    /// Título customizado opcional para a aba.
    pub title: Option<String>,
    /// Se `true`, foca a aba imediatamente após abrir.
    pub focus: bool,
}

/// Ação de resolução pós-fechamento de uma worktree vinculada a aba de agente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeResolution {
    /// Mantém a worktree e a branch no disco.
    Keep,
    /// Remove a worktree limpa mantendo a branch.
    Remove,
    /// Remove a worktree limpa e apaga a branch já mesclada (`git branch -d`).
    RemoveAndDeleteBranch,
}

impl WorktreeResolution {
    /// Representação na coluna SQL `resolution`.
    #[must_use]
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Remove => "remove",
            Self::RemoveAndDeleteBranch => "remove_and_delete_branch",
        }
    }

    /// Converte o texto da coluna SQL `resolution`.
    #[must_use]
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "keep" => Some(Self::Keep),
            "remove" => Some(Self::Remove),
            "remove_and_delete_branch" => Some(Self::RemoveAndDeleteBranch),
            _ => None,
        }
    }
}

/// Vínculo persistido entre uma aba de agente e sua worktree (`agent_tab_links`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentTabLink {
    /// Identificador numérico do vínculo.
    pub id: i64,
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do projeto.
    pub project_id: ProjectId,
    /// Agente CLI executado na aba.
    pub agent: AgentKind,
    /// Caminho da worktree (quando isolada).
    pub worktree_path: Option<String>,
    /// Branch associada à worktree.
    pub branch: Option<String>,
    /// OID de referência base no momento da criação.
    pub base_ref: Option<String>,
    /// Indica se a worktree foi criada pelo próprio MaxTerminal (`true`) ou já existia (`false`).
    pub created_by_app: bool,
    /// Timestamp UNIX em ms da abertura da aba.
    pub opened_at_ms: i64,
    /// Timestamp UNIX em ms do fechamento da aba, se já fechada.
    pub tab_closed_at_ms: Option<i64>,
    /// Resolução aplicada pelo usuário após o fechamento.
    pub resolution: Option<WorktreeResolution>,
    /// Timestamp UNIX em ms da resolução.
    pub resolved_at_ms: Option<i64>,
}

/// Rascunho de vínculo antes da criação do `TabId` definitivo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentTabLinkDraft {
    /// Caminho da worktree, se houver.
    pub worktree_path: Option<String>,
    /// Branch da worktree, se houver.
    pub branch: Option<String>,
    /// OID base da worktree, se houver.
    pub base_ref: Option<String>,
    /// Indica se foi criada pelo app.
    pub created_by_app: bool,
}

/// Resultado da preparação de uma worktree antes de `open_tab`.
#[derive(Debug, Clone)]
pub struct PreparedWorktree {
    /// Identificador da operação durável em `agent_launch_operations`.
    pub launch_id: String,
    /// Raiz principal do repositório Git.
    pub repo_root: PathBuf,
    /// Caminho da worktree preparada.
    pub worktree_path: PathBuf,
    /// Branch da worktree, quando aplicável.
    pub branch: Option<String>,
    /// OID base da worktree, quando aplicável.
    pub base_oid: Option<String>,
    /// `true` quando a worktree foi criada agora pelo app.
    pub created_by_app: bool,
}

impl PreparedWorktree {
    /// Converte em `AgentTabLinkDraft`.
    #[must_use]
    pub fn to_link_draft(&self) -> AgentTabLinkDraft {
        AgentTabLinkDraft {
            worktree_path: Some(self.worktree_path.to_string_lossy().to_string()),
            branch: self.branch.clone(),
            base_ref: self.base_oid.clone(),
            created_by_app: self.created_by_app,
        }
    }
}

/// Relatório detalhado de inspeção de uma worktree de agente após o fechamento da aba.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AgentCloseInspection {
    /// Dados do vínculo aba ↔ worktree.
    pub link: AgentTabLink,
    /// Indica se o diretório da worktree ainda existe no disco.
    pub exists: bool,
    /// Indica se há alterações rastreadas ou arquivos não rastreados.
    pub dirty: bool,
    /// Lista de até 50 arquivos modificados/não rastreados.
    pub changed_files: Vec<String>,
    /// `true` quando a lista de arquivos excedeu 50 itens.
    pub changed_files_truncated: bool,
    /// Quantidade de commits à frente de `base_ref`.
    pub ahead_of_base: u32,
    /// Indica se há arquivos ignorados (`!!`) dentro da worktree.
    pub has_ignored_files: bool,
    /// Indica se a inspeção Git completou sem erros/locks.
    pub inspection_complete: bool,
    /// Indica se a worktree ainda está em uso por processos vivos ou outra aba aberta.
    pub in_use: bool,
    /// Indica se a branch da worktree já está mesclada na base/HEAD principal.
    pub branch_merged: bool,
    /// Token SHA-256 hex do estado inspecionado (revalidado em `resolve`).
    pub inspection_token: String,
    /// Resoluções permitidas com segurança para este estado.
    pub allowed: Vec<WorktreeResolution>,
    /// Indica se a UI deve exibir o modal de confirmação ao usuário.
    pub requires_confirmation: bool,
}

/// Serviço de isolamento em Git Worktree e gestão de vínculos `agent_tab_links`.
pub struct IsolationService {
    git: Arc<dyn GitPort>,
    pty: Arc<dyn PtyPort>,
    workspace: Arc<dyn WorkspacePort>,
    db: mt_db::Db,
    settings: Arc<RwLock<AgentsSettings>>,
    repo_locks: RwLock<HashMap<PathBuf, Arc<Mutex<()>>>>,
}

impl IsolationService {
    /// Cria uma nova instância de `IsolationService`.
    #[must_use]
    pub fn new(
        git: Arc<dyn GitPort>,
        pty: Arc<dyn PtyPort>,
        workspace: Arc<dyn WorkspacePort>,
        db: mt_db::Db,
        settings: Arc<RwLock<AgentsSettings>>,
    ) -> Self {
        Self {
            git,
            pty,
            workspace,
            db,
            settings,
            repo_locks: RwLock::new(HashMap::new()),
        }
    }

    fn lock_for_repo(&self, repo_root: &Path) -> Arc<Mutex<()>> {
        let mut guard = self.repo_locks.write();
        Arc::clone(
            guard
                .entry(repo_root.to_path_buf())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    /// Obtém os metadados de um projeto pelo `WorkspacePort`.
    pub async fn project_record(&self, project_id: ProjectId) -> MtResult<ProjectRecord> {
        self.workspace.project(project_id).await
    }

    /// Garante a semeadura idempotente dos 4 templates dedicados de agentes para o projeto.
    pub async fn ensure_templates(&self, project_id: ProjectId) -> MtResult<u32> {
        self.workspace.ensure_agent_templates(project_id).await
    }

    /// Calcula uma proposta somente-leitura de worktree isolada para o projeto e agente (`CA-12`, `CA-13`).
    pub async fn propose(
        &self,
        project: &ProjectRecord,
        agent: AgentKind,
        hint: Option<&str>,
    ) -> MtResult<WorktreeProposal> {
        let discovered = discover_repository(&project.path, None)
            .ok()
            .flatten()
            .ok_or_else(|| error::not_a_repo(&project.path.to_string_lossy()))?;

        let repo_root = discovered.main_worktree_path;
        let head_out = self
            .git
            .run(&repo_root, &["rev-parse", "--verify", "HEAD"])
            .await?;
        let base_ref = head_out.trim().to_string();

        let (location, branch_prefix) = {
            let s = self.settings.read();
            (
                s.isolation.default_location,
                s.isolation.branch_prefix.clone(),
            )
        };

        let base_slug = match hint.map(slugify).filter(|s| !s.is_empty()) {
            Some(s) => s,
            None => default_timestamp_slug(agent),
        };

        let location_dir = location.folder_name();
        let mut candidate_slug = base_slug.clone();
        let mut counter = 2u32;

        loop {
            let candidate_name = format!("wt-{candidate_slug}");
            let candidate_path = repo_root.join(location_dir).join(&candidate_name);
            let candidate_branch = format!("{branch_prefix}/{}/{candidate_slug}", agent.short());
            let ref_name = format!("refs/heads/{candidate_branch}");

            let path_exists = candidate_path.exists();
            let branch_exists = self
                .git
                .run(&repo_root, &["show-ref", "--verify", "--quiet", &ref_name])
                .await
                .is_ok();

            if !path_exists && !branch_exists {
                break;
            }
            candidate_slug = format!("{base_slug}-{counter}");
            counter += 1;
        }

        let name = format!("wt-{candidate_slug}");
        let branch = format!("{branch_prefix}/{}/{candidate_slug}", agent.short());
        let full_path = repo_root.join(location_dir).join(&name);

        let check_dir = format!("{location_dir}/");
        let root_ignored = self
            .git
            .run(&repo_root, &["check-ignore", "-q", "--", &check_dir])
            .await
            .is_ok();

        let mut warnings = Vec::new();
        if !root_ignored {
            warnings.push(format!(
                "A pasta de worktrees ({location_dir}/) não está no .gitignore deste repositório."
            ));
        }

        Ok(WorktreeProposal {
            repo_root: repo_root.to_string_lossy().to_string(),
            slug: candidate_slug,
            name,
            branch,
            path: full_path.to_string_lossy().to_string(),
            location,
            root_ignored,
            warnings,
            base_ref,
        })
    }

    /// Prepara o isolamento em worktree antes da abertura da aba, gravando `agent_launch_operations`.
    pub async fn prepare(
        &self,
        project: &ProjectRecord,
        agent: AgentKind,
        iso: &WorktreeIsolation,
    ) -> MtResult<Option<PreparedWorktree>> {
        match iso {
            WorktreeIsolation::None => Ok(None),
            WorktreeIsolation::Existing { path } => {
                let proj_disc = discover_repository(&project.path, None)
                    .ok()
                    .flatten()
                    .ok_or_else(|| error::not_a_repo(&project.path.to_string_lossy()))?;

                let target_path = PathBuf::from(path);
                let wt_disc = discover_repository(&target_path, None)
                    .ok()
                    .flatten()
                    .ok_or_else(|| error::worktree_foreign(path))?;

                if wt_disc.common_dir != proj_disc.common_dir {
                    return Err(error::worktree_foreign(path));
                }

                let head_oid = self
                    .git
                    .run(&wt_disc.worktree_root, &["rev-parse", "--verify", "HEAD"])
                    .await
                    .ok()
                    .map(|s| s.trim().to_string());

                let branch = self
                    .git
                    .run(
                        &wt_disc.worktree_root,
                        &[" symbolic-ref".trim(), "--short", "-q", "HEAD"],
                    )
                    .await
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty());

                let launch_id = Uuid::now_v7().to_string();
                let now_ms = current_unix_ms();
                let proj_id_str = project.id.to_string();
                let agent_str = agent.as_ref_id().to_string();
                let repo_str = proj_disc.main_worktree_path.to_string_lossy().to_string();
                let wt_str = wt_disc.worktree_root.to_string_lossy().to_string();
                let branch_cl = branch.clone();
                let base_cl = head_oid.clone();
                let lid_cl = launch_id.clone();

                self.db
                    .write(move |tx| {
                        tx.execute(
                            "INSERT INTO agent_launch_operations (
                                launch_id, project_id, tab_id, agent_id, repo_path,
                                worktree_path, branch, base_oid, stage, created_at, updated_at
                             ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, 'worktree_ready', ?8, ?8);",
                            params![
                                lid_cl,
                                proj_id_str,
                                agent_str,
                                repo_str,
                                wt_str,
                                branch_cl,
                                base_cl,
                                now_ms
                            ],
                        )
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_launch_insert_existing"))?;
                        Ok(())
                    })
                    .await
                    .map_err(MtError::from)?;

                Ok(Some(PreparedWorktree {
                    launch_id,
                    repo_root: proj_disc.main_worktree_path,
                    worktree_path: wt_disc.worktree_root,
                    branch,
                    base_oid: head_oid,
                    created_by_app: false,
                }))
            }
            WorktreeIsolation::Create {
                location,
                slug,
                branch,
                base,
                ensure_gitignore,
            } => {
                let proj_disc = discover_repository(&project.path, None)
                    .ok()
                    .flatten()
                    .ok_or_else(|| error::not_a_repo(&project.path.to_string_lossy()))?;

                let repo_root = proj_disc.main_worktree_path;
                let base_target = base.as_deref().unwrap_or("HEAD");
                let base_oid = self
                    .git
                    .run(&repo_root, &["rev-parse", "--verify", base_target])
                    .await?
                    .trim()
                    .to_string();

                let clean_slug = slugify(slug);
                if clean_slug.is_empty() {
                    return Err(error::settings_invalid(
                        "O slug da worktree não pode ser vazio.",
                        None,
                        None,
                    ));
                }
                let wt_name = if clean_slug.starts_with("wt-") {
                    clean_slug
                } else {
                    format!("wt-{clean_slug}")
                };

                let wt_location = match location {
                    WorktreeLocationDto::Project => WorktreeLocation::MaxTerminal,
                    WorktreeLocationDto::MaxCode => WorktreeLocation::MaxCode,
                };
                let expected_path = repo_root.join(location.folder_name()).join(&wt_name);

                let launch_id = Uuid::now_v7().to_string();
                let now_ms = current_unix_ms();
                {
                    let lid = launch_id.clone();
                    let pid = project.id.to_string();
                    let aid = agent.as_ref_id().to_string();
                    let rpath = repo_root.to_string_lossy().to_string();
                    let wpath = expected_path.to_string_lossy().to_string();
                    let br = branch.clone();
                    let boid = base_oid.clone();
                    self.db
                        .write(move |tx| {
                            tx.execute(
                                "INSERT INTO agent_launch_operations (
                                    launch_id, project_id, tab_id, agent_id, repo_path,
                                    worktree_path, branch, base_oid, stage, created_at, updated_at
                                 ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, 'reserved', ?8, ?8);",
                                params![lid, pid, aid, rpath, wpath, br, boid, now_ms],
                            )
                            .map_err(|e| {
                                mt_db::DbError::from_rusqlite(e, "agent_launch_reserve")
                            })?;
                            Ok(())
                        })
                        .await
                        .map_err(MtError::from)?;
                }

                if *ensure_gitignore {
                    let info_exclude = proj_disc.common_dir.join("info").join("exclude");
                    if let Some(parent) = info_exclude.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let entry_line = format!("/{}/", location.folder_name());
                    let existing_excl = fs::read_to_string(&info_exclude).unwrap_or_default();
                    if !existing_excl.lines().any(|l| l.trim() == entry_line) {
                        let mut updated = existing_excl;
                        if !updated.is_empty() && !updated.ends_with('\n') {
                            updated.push('\n');
                        }
                        updated.push_str(&entry_line);
                        updated.push('\n');
                        let _ = fs::write(&info_exclude, updated);
                    }
                }

                let created_entry = self
                    .git
                    .create_worktree(WorktreeCreateRequest {
                        project_root: repo_root.clone(),
                        name: wt_name,
                        branch: Some(branch.clone()),
                        base_ref: Some(base_oid.clone()),
                        location: wt_location,
                    })
                    .await?;

                let ready_ms = current_unix_ms();
                let lid_ready = launch_id.clone();
                let actual_wt_str = created_entry.path.to_string_lossy().to_string();
                self.db
                    .write(move |tx| {
                        tx.execute(
                            "UPDATE agent_launch_operations
                             SET worktree_path = ?2, stage = 'worktree_ready', updated_at = ?3
                             WHERE launch_id = ?1;",
                            params![lid_ready, actual_wt_str, ready_ms],
                        )
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_launch_ready"))?;
                        Ok(())
                    })
                    .await
                    .map_err(MtError::from)?;

                Ok(Some(PreparedWorktree {
                    launch_id,
                    repo_root,
                    worktree_path: created_entry.path,
                    branch: Some(branch.clone()),
                    base_oid: Some(base_oid),
                    created_by_app: true,
                }))
            }
        }
    }

    /// Registra o vínculo definitivo `agent_tab_links` após a abertura da aba e marca `agent_launch_operations` como `opened`.
    pub async fn record_tab_link(
        &self,
        tab_id: TabId,
        project_id: ProjectId,
        agent: AgentKind,
        draft: Option<&AgentTabLinkDraft>,
        launch_id: Option<&str>,
        now_ms: i64,
    ) -> MtResult<AgentTabLink> {
        let tab_str = tab_id.to_string();
        let proj_str = project_id.to_string();
        let agent_str = agent.as_ref_id().to_string();
        let wt_path = draft.and_then(|d| d.worktree_path.clone());
        let branch = draft.and_then(|d| d.branch.clone());
        let base_ref = draft.and_then(|d| d.base_ref.clone());
        let created_by_app = draft.is_some_and(|d| d.created_by_app);
        let launch_id_owned = launch_id.map(str::to_string);

        let wt_clone = wt_path.clone();
        let br_clone = branch.clone();
        let base_clone = base_ref.clone();

        let row_id = self
            .db
            .write(move |tx| {
                tx.execute(
                    "INSERT INTO agent_tab_links (
                        tab_id, project_id, agent_id, worktree_path, branch,
                        base_ref, created_by_app, opened_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8);",
                    params![
                        tab_str,
                        proj_str,
                        agent_str,
                        wt_clone,
                        br_clone,
                        base_clone,
                        i64::from(created_by_app),
                        now_ms
                    ],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_tab_links_insert"))?;
                let id = tx.last_insert_rowid();

                if let Some(lid) = launch_id_owned {
                    tx.execute(
                        "UPDATE agent_launch_operations
                         SET tab_id = ?2, stage = 'opened', updated_at = ?3
                         WHERE launch_id = ?1;",
                        params![lid, tab_str, now_ms],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_launch_opened"))?;
                }
                Ok(id)
            })
            .await
            .map_err(MtError::from)?;

        Ok(AgentTabLink {
            id: row_id,
            tab_id,
            project_id,
            agent,
            worktree_path: wt_path,
            branch,
            base_ref,
            created_by_app,
            opened_at_ms: now_ms,
            tab_closed_at_ms: None,
            resolution: None,
            resolved_at_ms: None,
        })
    }

    /// Busca o vínculo ativo ou mais recente de uma aba.
    pub async fn get_link_by_tab(&self, tab_id: TabId) -> MtResult<Option<AgentTabLink>> {
        let tab_str = tab_id.to_string();
        self.db
            .read(move |conn| {
                conn.query_row(
                    "SELECT id, tab_id, project_id, agent_id, worktree_path, branch,
                            base_ref, created_by_app, opened_at, tab_closed_at, resolution, resolved_at
                     FROM agent_tab_links
                     WHERE tab_id = ?1
                     ORDER BY id DESC
                     LIMIT 1;",
                    [&tab_str],
                    map_link_row,
                )
                .optional()
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_tab_links_by_tab"))
            })
            .await
            .map_err(MtError::from)
    }

    /// Desfaz com segurança uma worktree recém-criada quando `open_tab` falha (`02/spec.md` §4.2).
    pub async fn rollback(&self, prepared: &PreparedWorktree) -> MtResult<()> {
        let now_ms = current_unix_ms();
        if !prepared.created_by_app {
            let lid = prepared.launch_id.clone();
            self.db
                .write(move |tx| {
                    tx.execute(
                        "UPDATE agent_launch_operations SET stage = 'resolved', updated_at = ?2 WHERE launch_id = ?1;",
                        params![lid, now_ms],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_launch_rollback_existing"))?;
                    Ok(())
                })
                .await
                .map_err(MtError::from)?;
            return Ok(());
        }

        let is_clean = if prepared.worktree_path.exists() {
            let status_out = self
                .git
                .run(
                    &prepared.worktree_path,
                    &[
                        "status",
                        "--porcelain=v1",
                        "-z",
                        "--ignored=matching",
                        "--untracked-files=all",
                    ],
                )
                .await
                .ok();
            let head_out = self
                .git
                .run(&prepared.worktree_path, &["rev-parse", "--verify", "HEAD"])
                .await
                .ok()
                .map(|s| s.trim().to_string());

            status_out.is_some_and(|s| s.is_empty())
                && head_out.as_deref() == prepared.base_oid.as_deref()
        } else {
            true
        };

        if is_clean {
            if prepared.worktree_path.exists() {
                let _ = self
                    .git
                    .remove_worktree(&prepared.repo_root, &prepared.worktree_path, false)
                    .await;
            }
            if let Some(ref br) = prepared.branch {
                let _ = self
                    .git
                    .run(&prepared.repo_root, &["branch", "-d", "--", br])
                    .await;
            }
            let lid = prepared.launch_id.clone();
            self.db
                .write(move |tx| {
                    tx.execute(
                        "UPDATE agent_launch_operations SET stage = 'resolved', updated_at = ?2 WHERE launch_id = ?1;",
                        params![lid, now_ms],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_launch_rollback_clean"))?;
                    Ok(())
                })
                .await
                .map_err(MtError::from)?;
        } else {
            let lid = prepared.launch_id.clone();
            self.db
                .write(move |tx| {
                    tx.execute(
                        "UPDATE agent_launch_operations SET stage = 'recovery_required', updated_at = ?2 WHERE launch_id = ?1;",
                        params![lid, now_ms],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_launch_rollback_dirty"))?;
                    Ok(())
                })
                .await
                .map_err(MtError::from)?;
        }
        Ok(())
    }

    /// Marca a aba como fechada em `agent_tab_links` e retorna a inspeção caso possua worktree vinculada.
    pub async fn mark_tab_closed(
        &self,
        tab_id: TabId,
        now_ms: i64,
    ) -> MtResult<Option<AgentCloseInspection>> {
        let tab_str = tab_id.to_string();
        let link_opt = self
            .db
            .write(move |tx| {
                tx.execute(
                    "UPDATE agent_tab_links
                     SET tab_closed_at = COALESCE(tab_closed_at, ?2)
                     WHERE tab_id = ?1 AND tab_closed_at IS NULL;",
                    params![tab_str, now_ms],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_tab_links_mark_closed"))?;

                tx.query_row(
                    "SELECT id, tab_id, project_id, agent_id, worktree_path, branch,
                            base_ref, created_by_app, opened_at, tab_closed_at, resolution, resolved_at
                     FROM agent_tab_links
                     WHERE tab_id = ?1
                     ORDER BY id DESC
                     LIMIT 1;",
                    [&tab_str],
                    map_link_row,
                )
                .optional()
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_tab_links_select_closed"))
            })
            .await
            .map_err(MtError::from)?;

        let Some(link) = link_opt else {
            return Ok(None);
        };
        if link.worktree_path.is_none() || link.resolution.is_some() {
            return Ok(None);
        }

        let inspection = self.inspect(link.id).await?;
        Ok(Some(inspection))
    }

    /// Inspeciona o estado atual da worktree de um vínculo (`CA-14`, `CA-15`).
    pub async fn inspect(&self, link_id: i64) -> MtResult<AgentCloseInspection> {
        let link = self.get_link_by_id(link_id).await?;
        self.inspect_link(link).await
    }

    /// Inspeciona o vínculo de uma aba pelo seu `TabId`.
    pub async fn inspect_by_tab(&self, tab_id: TabId) -> MtResult<Option<AgentCloseInspection>> {
        let Some(link) = self.get_link_by_tab(tab_id).await? else {
            return Ok(None);
        };
        if link.worktree_path.is_none() {
            return Ok(None);
        }
        Ok(Some(self.inspect_link(link).await?))
    }

    async fn inspect_link(&self, link: AgentTabLink) -> MtResult<AgentCloseInspection> {
        let Some(ref wt_path_str) = link.worktree_path else {
            let token = compute_inspection_token(link.id, "", "", false, false, false, false);
            return Ok(AgentCloseInspection {
                link,
                exists: false,
                dirty: false,
                changed_files: Vec::new(),
                changed_files_truncated: false,
                ahead_of_base: 0,
                has_ignored_files: false,
                inspection_complete: true,
                in_use: false,
                branch_merged: false,
                inspection_token: token,
                allowed: vec![WorktreeResolution::Keep],
                requires_confirmation: false,
            });
        };

        let wt_path = PathBuf::from(wt_path_str);
        let exists = wt_path.exists();

        let tab_open = link.tab_closed_at_ms.is_none();
        let active_procs = self.pty.has_active_processes(link.tab_id);
        let other_tabs = self
            .workspace
            .tabs_using_cwd(&wt_path)
            .into_iter()
            .any(|t| t != link.tab_id);
        let in_use = tab_open || active_procs || other_tabs;

        if !exists {
            let token = compute_inspection_token(
                link.id,
                wt_path_str,
                "missing",
                false,
                false,
                in_use,
                false,
            );
            return Ok(AgentCloseInspection {
                link,
                exists: false,
                dirty: false,
                changed_files: Vec::new(),
                changed_files_truncated: false,
                ahead_of_base: 0,
                has_ignored_files: false,
                inspection_complete: true,
                in_use,
                branch_merged: false,
                inspection_token: token,
                allowed: vec![WorktreeResolution::Keep],
                requires_confirmation: false,
            });
        }

        let discovered = discover_repository(&wt_path, None).ok().flatten();
        let is_main_checkout = discovered
            .as_ref()
            .is_none_or(|d| !d.is_linked || d.worktree_root == d.main_worktree_path);

        let mut dirty = false;
        let mut has_ignored_files = false;
        let mut changed_files = Vec::new();
        let mut changed_files_truncated = false;
        let mut inspection_complete = discovered.is_some();

        if let Some(ref disc) = discovered {
            if disc.git_dir.join("index.lock").exists() || disc.locked {
                inspection_complete = false;
            }
        }

        match self
            .git
            .run(
                &wt_path,
                &[
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--ignored=matching",
                    "--untracked-files=all",
                ],
            )
            .await
        {
            Ok(porcelain) => {
                parse_porcelain_v1_z(
                    &porcelain,
                    &mut dirty,
                    &mut has_ignored_files,
                    &mut changed_files,
                    &mut changed_files_truncated,
                );
            }
            Err(_) => {
                inspection_complete = false;
            }
        }

        let head_oid = self
            .git
            .run(&wt_path, &["rev-parse", "--verify", "HEAD"])
            .await
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if head_oid.is_empty() {
            inspection_complete = false;
        }

        let ahead_of_base = if let Some(ref base) = link.base_ref {
            let range = format!("{base}..HEAD");
            self.git
                .run(&wt_path, &["rev-list", "--count", &range])
                .await
                .ok()
                .and_then(|s| s.trim().parse::<u32>().ok())
                .unwrap_or(0)
        } else {
            0
        };

        let branch_merged = if let Some(ref base) = link.base_ref {
            if head_oid == *base {
                true
            } else if let Some(ref disc) = discovered {
                // Verifica se HEAD da worktree é ancestral do HEAD do repositório principal
                self.git
                    .run(
                        &disc.main_worktree_path,
                        &["merge-base", "--is-ancestor", &head_oid, "HEAD"],
                    )
                    .await
                    .is_ok()
            } else {
                false
            }
        } else {
            false
        };

        let mut allowed = vec![WorktreeResolution::Keep];
        if link.created_by_app
            && !is_main_checkout
            && inspection_complete
            && !in_use
            && !dirty
            && !has_ignored_files
        {
            allowed.push(WorktreeResolution::Remove);
            if branch_merged && link.branch.is_some() {
                allowed.push(WorktreeResolution::RemoveAndDeleteBranch);
            }
        }

        let inspection_token = compute_inspection_token(
            link.id,
            wt_path_str,
            &head_oid,
            dirty,
            has_ignored_files,
            in_use,
            branch_merged,
        );
        let requires_confirmation = link.created_by_app && exists;

        Ok(AgentCloseInspection {
            link,
            exists,
            dirty,
            changed_files,
            changed_files_truncated,
            ahead_of_base,
            has_ignored_files,
            inspection_complete,
            in_use,
            branch_merged,
            inspection_token,
            allowed,
            requires_confirmation,
        })
    }

    /// Resolve o destino da worktree após o fechamento da aba (`Keep`, `Remove`, `RemoveAndDeleteBranch`).
    pub async fn resolve(
        &self,
        link_id: i64,
        action: WorktreeResolution,
        inspection_token: &str,
    ) -> MtResult<AgentTabLink> {
        let initial_link = self.get_link_by_id(link_id).await?;
        if initial_link.resolution.is_some() {
            return Err(error::link_resolved(link_id));
        }

        let repo_root = initial_link
            .worktree_path
            .as_deref()
            .and_then(|p| discover_repository(Path::new(p), None).ok().flatten())
            .map_or_else(
                || PathBuf::from(initial_link.worktree_path.as_deref().unwrap_or("")),
                |d| d.main_worktree_path,
            );

        let repo_mutex = self.lock_for_repo(&repo_root);
        let _repo_guard = repo_mutex.lock().await;

        let fresh = self.inspect(link_id).await?;
        if fresh.link.resolution.is_some() {
            return Err(error::link_resolved(link_id));
        }
        if fresh.inspection_token != inspection_token {
            return Err(error::inspection_stale(link_id));
        }

        match action {
            WorktreeResolution::Keep => {}
            WorktreeResolution::Remove | WorktreeResolution::RemoveAndDeleteBranch => {
                if fresh.in_use {
                    return Err(error::execution_in_use(
                        "A worktree ainda está em uso por processos ativos ou outra aba aberta.",
                    ));
                }
                if !fresh.link.created_by_app {
                    return Err(error::worktree_foreign(
                        fresh.link.worktree_path.as_deref().unwrap_or_default(),
                    ));
                }
                if fresh.dirty || fresh.has_ignored_files || !fresh.inspection_complete {
                    return Err(error::worktree_dirty(
                        "A worktree contém alterações locais ou arquivos ignorados e não pode ser removida automaticamente.",
                    ));
                }
                if action == WorktreeResolution::RemoveAndDeleteBranch && !fresh.branch_merged {
                    return Err(error::worktree_dirty(
                        "A branch possui commits exclusivos ainda não mesclados e não pode ser apagada com segurança.",
                    ));
                }

                if let Some(ref wt_str) = fresh.link.worktree_path {
                    let wt_path = PathBuf::from(wt_str);
                    if wt_path.exists() {
                        self.git
                            .remove_worktree(&repo_root, &wt_path, false)
                            .await?;
                    }
                }

                if action == WorktreeResolution::RemoveAndDeleteBranch
                    && let Some(ref branch_name) = fresh.link.branch
                {
                    self.git
                        .run(&repo_root, &["branch", "-d", "--", branch_name])
                        .await?;
                }
            }
        }

        let now_ms = current_unix_ms();
        let res_str = action.as_db_str().to_string();
        let updated_link = self
            .db
            .write(move |tx| {
                tx.execute(
                    "UPDATE agent_tab_links
                     SET tab_closed_at = COALESCE(tab_closed_at, ?3),
                         resolution = ?2,
                         resolved_at = ?3
                     WHERE id = ?1;",
                    params![link_id, res_str, now_ms],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_tab_links_resolve"))?;

                tx.query_row(
                    "SELECT id, tab_id, project_id, agent_id, worktree_path, branch,
                            base_ref, created_by_app, opened_at, tab_closed_at, resolution, resolved_at
                     FROM agent_tab_links
                     WHERE id = ?1;",
                    [link_id],
                    map_link_row,
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_tab_links_select_resolved"))
            })
            .await
            .map_err(MtError::from)?;

        Ok(updated_link)
    }

    /// Lista todas as inspeções de worktrees de abas já fechadas que aguardam decisão do usuário.
    pub async fn list_pending(
        &self,
        project_id: Option<ProjectId>,
    ) -> MtResult<Vec<AgentCloseInspection>> {
        let proj_filter = project_id.map(|p| p.to_string());
        let links: Vec<AgentTabLink> = self
            .db
            .read(move |conn| {
                let mut out = Vec::new();
                if let Some(ref pid) = proj_filter {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, tab_id, project_id, agent_id, worktree_path, branch,
                                    base_ref, created_by_app, opened_at, tab_closed_at, resolution, resolved_at
                             FROM agent_tab_links
                             WHERE project_id = ?1
                               AND tab_closed_at IS NOT NULL
                               AND resolution IS NULL
                               AND worktree_path IS NOT NULL
                             ORDER BY tab_closed_at DESC, id DESC;",
                        )
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_links_pending_by_proj"))?;
                    let rows = stmt
                        .query_map([pid], map_link_row)
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_links_pending_by_proj"))?;
                    for r in rows {
                        out.push(r.map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "agent_links_pending_by_proj")
                        })?);
                    }
                } else {
                    let mut stmt = conn
                        .prepare(
                            "SELECT id, tab_id, project_id, agent_id, worktree_path, branch,
                                    base_ref, created_by_app, opened_at, tab_closed_at, resolution, resolved_at
                             FROM agent_tab_links
                             WHERE tab_closed_at IS NOT NULL
                               AND resolution IS NULL
                               AND worktree_path IS NOT NULL
                             ORDER BY tab_closed_at DESC, id DESC;",
                        )
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_links_pending_all"))?;
                    let rows = stmt
                        .query_map([], map_link_row)
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_links_pending_all"))?;
                    for r in rows {
                        out.push(r.map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "agent_links_pending_all")
                        })?);
                    }
                }
                Ok(out)
            })
            .await
            .map_err(MtError::from)?;

        let mut inspections = Vec::with_capacity(links.len());
        for link in links {
            inspections.push(self.inspect_link(link).await?);
        }
        Ok(inspections)
    }

    /// Reconcilia no startup operações de lançamento interrompidas e abas órfãs.
    pub async fn reconcile_startup(&self) -> MtResult<u32> {
        let now_ms = current_unix_ms();
        let reconciled = self
            .db
            .write(move |tx| {
                // Marca como fechados vínculos cujas abas não existem mais em `tabs`
                let closed_orphans = tx
                    .execute(
                        "UPDATE agent_tab_links
                         SET tab_closed_at = ?1
                         WHERE tab_closed_at IS NULL
                           AND NOT EXISTS (SELECT 1 FROM tabs WHERE tabs.id = agent_tab_links.tab_id);",
                        [now_ms],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_reconcile_links"))?;

                let updated_ops = tx
                    .execute(
                        "UPDATE agent_launch_operations
                         SET stage = CASE
                             WHEN worktree_path IS NULL THEN 'resolved'
                             ELSE 'recovery_required'
                         END,
                         updated_at = ?1
                         WHERE stage IN ('reserved', 'worktree_ready', 'opening');",
                        [now_ms],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_reconcile_ops"))?;

                Ok(u32::try_from(closed_orphans + updated_ops).unwrap_or(0))
            })
            .await
            .map_err(MtError::from)?;

        Ok(reconciled)
    }

    async fn get_link_by_id(&self, link_id: i64) -> MtResult<AgentTabLink> {
        let opt = self
            .db
            .read(move |conn| {
                conn.query_row(
                    "SELECT id, tab_id, project_id, agent_id, worktree_path, branch,
                            base_ref, created_by_app, opened_at, tab_closed_at, resolution, resolved_at
                     FROM agent_tab_links
                     WHERE id = ?1;",
                    [link_id],
                    map_link_row,
                )
                .optional()
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_tab_links_by_id"))
            })
            .await
            .map_err(MtError::from)?;

        opt.ok_or_else(|| error::link_not_found(format!("id={link_id}")))
    }
}

/// Garante idempotentemente a criação dos 4 templates dedicados de Agentes CLI para `project_id`,
/// respeitando exclusões manuais do usuário registradas em `agent_template_seeds` (`template_id IS NULL`).
pub async fn ensure_agent_templates_in_db(
    db: &mt_db::Db,
    reg: &AgentRegistry,
    project_id: ProjectId,
    now_ms: i64,
) -> MtResult<u32> {
    let descriptors = reg.descriptors();
    let proj_str = project_id.to_string();

    db.write(move |tx| {
        let proj_exists: Option<i64> = tx
            .query_row(
                "SELECT 1 FROM projects WHERE id = ?1 LIMIT 1;",
                [&proj_str],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_templates_check_project"))?;

        if proj_exists.is_none() {
            return Err(mt_db::DbError::ConstraintViolation {
                constraint: String::from(mt_db::CONSTRAINT_PROJECT_NOT_FOUND),
            });
        }

        let _ = mt_db::TemplatesRepo::ensure_default_tx(tx, project_id, now_ms)?;

        let mut created_count = 0u32;
        for desc in descriptors.iter() {
            let agent_str = desc.kind.as_ref_id();
            let already_seeded: Option<Option<String>> = tx
                .query_row(
                    "SELECT template_id FROM agent_template_seeds
                     WHERE project_id = ?1 AND agent_id = ?2;",
                    params![proj_str, agent_str],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_templates_check_seed"))?;

            if already_seeded.is_some() {
                continue;
            }

            let existing_tpl_id: Option<String> = tx
                .query_row(
                    "SELECT id FROM tab_templates
                     WHERE project_id = ?1 AND environment_kind = 'agent' AND agent_id = ?2
                     ORDER BY position ASC, created_at ASC
                     LIMIT 1;",
                    params![proj_str, agent_str],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_templates_find_existing"))?;

            let tpl_id_str = if let Some(id_str) = existing_tpl_id {
                id_str
            } else {
                let next_pos: i64 = tx
                    .query_row(
                        "SELECT COALESCE(MAX(position) + 1, 0) FROM tab_templates WHERE project_id = ?1;",
                        [&proj_str],
                        |r| r.get(0),
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_templates_next_pos"))?;

                let new_id = TemplateId::new().to_string();
                tx.execute(
                    "INSERT INTO tab_templates (
                        id, project_id, name, icon, cwd, initial_command,
                        add_to_workspace, continuous, environment_kind, agent_id,
                        env_json, is_default, position, revision, created_at, updated_at
                     ) VALUES (
                        ?1, ?2, ?3, ?4, NULL, NULL,
                        0, 0, 'agent', ?5,
                        '{}', 0, ?6, 1, ?7, ?7
                     );",
                    params![
                        new_id,
                        proj_str,
                        desc.display_name,
                        desc.icon,
                        agent_str,
                        next_pos,
                        now_ms
                    ],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_templates_insert_tpl"))?;
                created_count += 1;
                new_id
            };

            tx.execute(
                "INSERT INTO agent_template_seeds (project_id, agent_id, template_id, seeded_at)
                 VALUES (?1, ?2, ?3, ?4);",
                params![proj_str, agent_str, tpl_id_str, now_ms],
            )
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_templates_insert_seed"))?;

            tx.execute(
                "INSERT OR IGNORE INTO agent_template_options (
                    template_id, model, extra_args_json, isolation_default, location, updated_at
                 ) VALUES (?1, NULL, '[]', 0, 'project', ?2);",
                params![tpl_id_str, now_ms],
            )
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_templates_insert_opts"))?;
        }

        Ok(created_count)
    })
    .await
    .map_err(MtError::from)
}

/// Obtém as `AgentTemplateOptions` de um template de agente.
pub async fn get_template_options_in_db(
    db: &mt_db::Db,
    template_id: TemplateId,
) -> MtResult<AgentTemplateOptions> {
    let tid_str = template_id.to_string();
    db.read(move |conn| {
        let row: Option<(Option<String>, String, i64, String)> = conn
            .query_row(
                "SELECT model, extra_args_json, isolation_default, location
                 FROM agent_template_options
                 WHERE template_id = ?1;",
                [&tid_str],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_template_options_get"))?;

        let Some((model, extra_json, iso_int, loc_str)) = row else {
            return Ok(AgentTemplateOptions::default());
        };
        let extra_args: Vec<String> = serde_json::from_str(&extra_json).unwrap_or_default();
        let location = WorktreeLocationDto::from_db_str(&loc_str).unwrap_or_default();

        Ok(AgentTemplateOptions {
            model,
            extra_args,
            isolation_default: iso_int != 0,
            location,
        })
    })
    .await
    .map_err(MtError::from)
}

/// Atualiza as `AgentTemplateOptions` de um template de agente no SQLite.
pub async fn set_template_options_in_db(
    db: &mt_db::Db,
    reg: &AgentRegistry,
    template_id: TemplateId,
    options: &AgentTemplateOptions,
    now_ms: i64,
) -> MtResult<AgentTemplateOptions> {
    let tpl = db
        .templates()
        .get(template_id)
        .await
        .map_err(MtError::from)?
        .ok_or_else(|| MtError::not_found("template", template_id))?;

    let Some(ref agent_id_str) = tpl.config.environment.agent_id else {
        return Err(MtError::new(
            ErrorCode(error::codes::TAB_NOT_AGENT),
            "O template informado não é um template de agente CLI.",
        ));
    };
    let agent_kind = AgentKind::from_ref_str(agent_id_str)?;
    let desc = reg.get(agent_kind);
    let validated_model =
        crate::launch::LaunchPlanner::resolve_cli_model(&desc, options.model.as_deref())?;

    let extra_json = serde_json::to_string(&options.extra_args).map_err(|e| {
        error::settings_invalid(format!("Argumentos extras inválidos: {e}"), None, None)
    })?;
    let loc_str = options.location.as_db_str().to_string();
    let iso_int = i64::from(options.isolation_default);
    let tid_str = template_id.to_string();
    let model_cl = validated_model.clone();

    db.write(move |tx| {
        tx.execute(
            "INSERT INTO agent_template_options (
                template_id, model, extra_args_json, isolation_default, location, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(template_id) DO UPDATE SET
                model = excluded.model,
                extra_args_json = excluded.extra_args_json,
                isolation_default = excluded.isolation_default,
                location = excluded.location,
                updated_at = excluded.updated_at;",
            params![tid_str, model_cl, extra_json, iso_int, loc_str, now_ms],
        )
        .map_err(|e| mt_db::DbError::from_rusqlite(e, "agent_template_options_set"))?;
        Ok(())
    })
    .await
    .map_err(MtError::from)?;

    Ok(AgentTemplateOptions {
        model: validated_model,
        extra_args: options.extra_args.clone(),
        isolation_default: options.isolation_default,
        location: options.location,
    })
}

fn map_link_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AgentTabLink> {
    let id: i64 = row.get(0)?;
    let tab_raw: String = row.get(1)?;
    let proj_raw: String = row.get(2)?;
    let agent_raw: String = row.get(3)?;
    let worktree_path: Option<String> = row.get(4)?;
    let branch: Option<String> = row.get(5)?;
    let base_ref: Option<String> = row.get(6)?;
    let created_by_app_int: i64 = row.get(7)?;
    let opened_at_ms: i64 = row.get(8)?;
    let tab_closed_at_ms: Option<i64> = row.get(9)?;
    let resolution_raw: Option<String> = row.get(10)?;
    let resolved_at_ms: Option<i64> = row.get(11)?;

    let tab_id = TabId::from_str(&tab_raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let project_id = ProjectId::from_str(&proj_raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let agent = AgentKind::from_ref_str(&agent_raw).unwrap_or(AgentKind::Claude);
    let resolution = resolution_raw
        .as_deref()
        .and_then(WorktreeResolution::from_db_str);

    Ok(AgentTabLink {
        id,
        tab_id,
        project_id,
        agent,
        worktree_path,
        branch,
        base_ref,
        created_by_app: created_by_app_int != 0,
        opened_at_ms,
        tab_closed_at_ms,
        resolution,
        resolved_at_ms,
    })
}

fn parse_porcelain_v1_z(
    raw: &str,
    dirty: &mut bool,
    has_ignored: &mut bool,
    changed_files: &mut Vec<String>,
    truncated: &mut bool,
) {
    let mut entries = raw.split('\0').peekable();
    while let Some(entry) = entries.next() {
        if entry.len() < 3 {
            continue;
        }
        let status = &entry[..2];
        let path_part = &entry[3..];
        if status == "!!" {
            *has_ignored = true;
            continue;
        }
        *dirty = true;
        if status.starts_with('R') || status.starts_with('C') {
            let _ = entries.next();
        }
        if changed_files.len() < MAX_CHANGED_FILES_REPORTED {
            changed_files.push(path_part.to_string());
        } else {
            *truncated = true;
        }
    }
}

fn compute_inspection_token(
    link_id: i64,
    wt_path: &str,
    head_oid: &str,
    dirty: bool,
    has_ignored: bool,
    in_use: bool,
    branch_merged: bool,
) -> String {
    let mut hasher = Sha256::new();
    let payload =
        format!("{link_id}|{wt_path}|{head_oid}|{dirty}|{has_ignored}|{in_use}|{branch_merged}");
    hasher.update(payload.as_bytes());
    let digest = hasher.finalize();
    let mut hex_out = String::with_capacity(64);
    for b in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex_out, "{b:02x}");
    }
    hex_out
}

/// Normaliza uma string arbitrária em slug ASCII `[a-z0-9-]` com até 40 caracteres.
#[must_use]
pub fn slugify(input: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in input.trim().chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            out.push(lower);
            prev_dash = false;
        } else if !out.is_empty() && !prev_dash {
            out.push('-');
            prev_dash = true;
        }
        if out.len() >= 40 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn default_timestamp_slug(agent: AgentKind) -> String {
    let now_secs = current_unix_ms() / 1000;
    let dt = time::OffsetDateTime::from_unix_timestamp(now_secs)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{}-{:04}{:02}{:02}-{:02}{:02}",
        agent.short(),
        dt.year(),
        u8::from(dt.month()),
        dt.day(),
        dt.hour(),
        dt.minute()
    )
}
