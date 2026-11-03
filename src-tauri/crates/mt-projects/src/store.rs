//! Abstração de persistência (`ProjectStore`), adaptador SQLite (`SqliteProjectStore`)
//! e implementação em memória para testes (`MemoryProjectStore`).

use std::collections::HashSet;
use std::str::FromStr;

use async_trait::async_trait;
use mt_core::error::codes;
use mt_core::ids::ProjectId;
use mt_core::{MtError, MtResult};
use mt_db::repos::projects::{
    CONSTRAINT_PROJECT_DUPLICATE_PATH, CONSTRAINT_PROJECT_NOT_FOUND,
    CONSTRAINT_PROJECT_REORDER_MISMATCH, CONSTRAINT_PROJECT_ROOT_DUPLICATE_PATH,
    CONSTRAINT_PROJECT_ROOT_NOT_FOUND, CatalogRows, ProjectCommit, ProjectMutation, ProjectsRepo,
    StoredDismissed, StoredIdentityUpdate, StoredMissingUpdate, StoredNewProject, StoredProject,
    StoredReconcileBatch, StoredRoot, StoredRootEntry,
};
use mt_db::sanitizer::SanitizedText;
use mt_db::{Db, DbError};
use parking_lot::Mutex;

use crate::model::{DiscoveryMode, IdentitySource, Project, ProjectRootId, StackKind};
use crate::scanner::RootEntryRow;

/// Linha persistida de uma Pasta de Projetos (sem `watch_status`, que é estado de runtime).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRootRow {
    /// Identificador único (UUID v7).
    pub id: ProjectRootId,
    /// Caminho absoluto canônico.
    pub path: String,
    /// Modo de descoberta configurado.
    pub discovery_mode: DiscoveryMode,
    /// Indica se o watcher automático está habilitado.
    pub watch_enabled: bool,
    /// Timestamp UNIX em ms de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms da última atualização.
    pub updated_at: i64,
}

/// Dados de um novo projeto a persistir na transação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewProjectRow {
    /// Projeto validado (`sort_order` será atribuído na transação).
    pub project: Project,
    /// Caminhos canônicos a remover de `project_discovery_dismissed` em todas as raízes.
    pub clear_dismissed_paths: Vec<String>,
    /// Quando `true`, caminho já existente é tratado como no-op idempotente.
    pub if_exists_noop: bool,
}

/// Linha de sugestão dispensada em `project_discovery_dismissed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DismissedRow {
    /// Identificador da Pasta de Projetos.
    pub root_id: ProjectRootId,
    /// Caminho absoluto canônico dispensado.
    pub path: String,
    /// Timestamp UNIX em ms da dispensa.
    pub dismissed_at: i64,
}

/// Atualização de identidade automática para projetos com `identity_source == Auto` e `stack == Unknown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdentityUpdateRow {
    /// Identificador do projeto.
    pub id: ProjectId,
    /// Nova stack detectada.
    pub stack: StackKind,
    /// Novo ícone automático.
    pub icon: String,
    /// Nova cor automática.
    pub color: String,
    /// Timestamp UNIX em ms da atualização.
    pub updated_at: i64,
}

/// Atualização do indicador `missing` de um projeto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingUpdateRow {
    /// Identificador do projeto.
    pub id: ProjectId,
    /// Novo valor de `missing`.
    pub missing: bool,
    /// Timestamp UNIX em ms da atualização.
    pub updated_at: i64,
}

/// Lote transacional de reconciliação de origens, auto-imports, identidades e `missing`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileBatch {
    /// Origens observadas a inserir/atualizar.
    pub upserted_entries: Vec<RootEntryRow>,
    /// Origens confirmadamente removidas `(root_id, entry_path)`.
    pub removed_entries: Vec<(ProjectRootId, String)>,
    /// Projetos autoimportados (idempotente por `path`).
    pub auto_imports: Vec<NewProjectRow>,
    /// Atualizações de stack/identidade para projetos `auto/unknown`.
    pub identity_updates: Vec<IdentityUpdateRow>,
    /// Atualizações de `missing`.
    pub missing_updates: Vec<MissingUpdateRow>,
}

/// Visão consistente das linhas persistidas no banco.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogStateRows {
    /// Projetos ordenados por `(sort_order, id)`.
    pub projects: Vec<Project>,
    /// Pastas de projetos ordenadas por `(created_at, id)`.
    pub roots: Vec<ProjectRootRow>,
    /// Origens observadas ordenadas por `(root_id, entry_path)`.
    pub entries: Vec<RootEntryRow>,
    /// Dispensas ordenadas por `(root_id, path)`.
    pub dismissed: Vec<DismissedRow>,
}

/// Mutação transacional do catálogo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogMutation {
    /// Cadastro manual de projeto.
    Add(NewProjectRow),
    /// Atualização de metadados de projeto.
    Update(Project),
    /// Remoção de projeto e gravação de dispensas nas raízes associadas.
    Remove {
        /// Identificador do projeto.
        id: ProjectId,
        /// Dispensas a gravar na mesma transação.
        dismiss: Vec<DismissedRow>,
    },
    /// Reordenação completa dos projetos (`0..N-1`).
    Reorder(Vec<ProjectId>),
    /// Cadastro de Pasta de Projetos com reconciliação inicial na mesma transação.
    AddRoot {
        /// Linha da Pasta de Projetos.
        root: ProjectRootRow,
        /// Lote inicial de origens e projetos importados.
        reconciliation: ReconcileBatch,
    },
    /// Atualização de Pasta de Projetos.
    UpdateRoot(ProjectRootRow),
    /// Exclusão de Pasta de Projetos.
    RemoveRoot(ProjectRootId),
    /// Reconciliação periódica, de watcher ou de foco.
    Reconcile(ReconcileBatch),
    /// Aceitação em lote de sugestões descobertas.
    Accept(Vec<NewProjectRow>),
    /// Dispensa em lote de sugestões descobertas.
    Dismiss(Vec<DismissedRow>),
}

/// Resultado lido na mesma transação após o commit de [`CatalogMutation`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommitResult {
    /// Projetos inseridos ou modificados nesta transação.
    pub upserted_projects: Vec<Project>,
    /// Projetos efetivamente novos inseridos nesta transação.
    pub inserted_projects: Vec<Project>,
    /// IDs de projetos removidos nesta transação.
    pub deleted_project_ids: Vec<ProjectId>,
    /// Nova lista completa de IDs quando a mutação foi `Reorder`.
    pub reordered_project_ids: Vec<ProjectId>,
    /// Pasta de Projetos inserida ou atualizada.
    pub upserted_root: Option<ProjectRootRow>,
    /// ID de Pasta de Projetos excluída.
    pub deleted_root_id: Option<ProjectRootId>,
    /// Estado completo persistido lido na mesma transação.
    pub catalog: CatalogStateRows,
}

/// Contrato de persistência desacoplado de Tauri para o [`crate::service::ProjectService`].
#[async_trait]
pub trait ProjectStore: Send + Sync + 'static {
    /// Carrega todas as linhas persistidas do catálogo em uma leitura consistente.
    async fn load_catalog(&self) -> MtResult<CatalogStateRows>;

    /// Busca um projeto pelo caminho canônico exato.
    async fn find_project_by_path(&self, path: &str) -> MtResult<Option<Project>>;

    /// Executa uma mutação de domínio em uma única transação atômica.
    async fn commit(&self, mutation: CatalogMutation) -> MtResult<CommitResult>;
}

/// Implementação de [`ProjectStore`] apoiada em `mt_db::ProjectsRepo` (SQLite WAL).
#[derive(Clone, Debug)]
pub struct SqliteProjectStore {
    repo: ProjectsRepo,
}

impl SqliteProjectStore {
    /// Cria um novo [`SqliteProjectStore`] a partir de [`ProjectsRepo`].
    #[must_use]
    pub const fn new(repo: ProjectsRepo) -> Self {
        Self { repo }
    }

    /// Cria um novo [`SqliteProjectStore`] diretamente a partir da fachada [`Db`].
    #[must_use]
    pub fn from_db(db: &Db) -> Self {
        Self {
            repo: db.projects(),
        }
    }
}

#[async_trait]
impl ProjectStore for SqliteProjectStore {
    async fn load_catalog(&self) -> MtResult<CatalogStateRows> {
        let rows = self.repo.load_catalog().await.map_err(map_db_error)?;
        Ok(from_catalog_rows(rows))
    }

    async fn find_project_by_path(&self, path: &str) -> MtResult<Option<Project>> {
        let row = self
            .repo
            .find_project_by_path(path)
            .await
            .map_err(map_db_error)?;
        Ok(row.map(from_stored_project))
    }

    async fn commit(&self, mutation: CatalogMutation) -> MtResult<CommitResult> {
        let db_mutation = to_project_mutation(mutation);
        let commit = self.repo.commit(db_mutation).await.map_err(map_db_error)?;
        Ok(from_project_commit(commit))
    }
}

fn map_db_error(err: DbError) -> MtError {
    if let DbError::ConstraintViolation { ref constraint } = err {
        match constraint.as_str() {
            CONSTRAINT_PROJECT_DUPLICATE_PATH => {
                return MtError::new(
                    codes::PROJECT_DUPLICATE_PATH,
                    "A pasta informada já está cadastrada como projeto.",
                );
            }
            CONSTRAINT_PROJECT_ROOT_DUPLICATE_PATH => {
                return MtError::new(
                    codes::PROJECT_ROOT_DUPLICATE_PATH,
                    "A pasta informada já está cadastrada como Pasta de Projetos.",
                );
            }
            CONSTRAINT_PROJECT_NOT_FOUND => {
                return MtError::new(
                    codes::PROJECT_NOT_FOUND,
                    "O projeto solicitado não foi encontrado.",
                );
            }
            CONSTRAINT_PROJECT_ROOT_NOT_FOUND => {
                return MtError::new(
                    codes::PROJECT_ROOT_NOT_FOUND,
                    "A Pasta de Projetos solicitada não foi encontrada.",
                );
            }
            CONSTRAINT_PROJECT_REORDER_MISMATCH => {
                return MtError::new(
                    codes::PROJECT_REORDER_MISMATCH,
                    "A lista de projetos para reordenação diverge do catálogo atual.",
                );
            }
            _ => {}
        }
    }
    MtError::from(err)
}

fn to_stored_project(p: Project) -> StoredProject {
    StoredProject {
        id: p.id,
        title: SanitizedText::new(&p.title),
        icon: p.icon,
        color: p.color,
        path: p.path,
        root_id: p.root_id.map(|id| id.to_string()),
        stack: p.stack.as_db_str().to_string(),
        identity_source: p.identity_source.as_db_str().to_string(),
        sort_order: p.sort_order,
        missing: p.missing,
        created_at: p.created_at,
        updated_at: p.updated_at,
    }
}

fn from_stored_project(s: StoredProject) -> Project {
    Project {
        id: s.id,
        title: s.title.into_inner(),
        icon: s.icon,
        color: s.color,
        path: s.path,
        root_id: s.root_id.and_then(|raw| ProjectRootId::from_str(&raw).ok()),
        stack: StackKind::from_db_str(&s.stack),
        identity_source: IdentitySource::from_db_str(&s.identity_source),
        sort_order: s.sort_order,
        missing: s.missing,
        created_at: s.created_at,
        updated_at: s.updated_at,
    }
}

fn to_stored_root(r: ProjectRootRow) -> StoredRoot {
    StoredRoot {
        id: r.id.to_string(),
        path: r.path,
        discovery_mode: r.discovery_mode.as_db_str().to_string(),
        watch_enabled: r.watch_enabled,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

fn from_stored_root(s: StoredRoot) -> Option<ProjectRootRow> {
    let id = ProjectRootId::from_str(&s.id).ok()?;
    Some(ProjectRootRow {
        id,
        path: s.path,
        discovery_mode: DiscoveryMode::from_db_str(&s.discovery_mode),
        watch_enabled: s.watch_enabled,
        created_at: s.created_at,
        updated_at: s.updated_at,
    })
}

fn to_stored_new_project(n: NewProjectRow) -> StoredNewProject {
    StoredNewProject {
        project: to_stored_project(n.project),
        clear_dismissed_paths: n.clear_dismissed_paths,
        if_exists_noop: n.if_exists_noop,
    }
}

fn to_stored_dismissed(d: DismissedRow) -> StoredDismissed {
    StoredDismissed {
        root_id: d.root_id.to_string(),
        path: d.path,
        dismissed_at: d.dismissed_at,
    }
}

fn to_stored_reconcile(b: ReconcileBatch) -> StoredReconcileBatch {
    StoredReconcileBatch {
        upserted_entries: b
            .upserted_entries
            .into_iter()
            .map(|e| StoredRootEntry {
                root_id: e.root_id.to_string(),
                entry_path: e.entry_path,
                canonical_path: e.canonical_path,
                observed_at: e.observed_at,
            })
            .collect(),
        removed_entries: b
            .removed_entries
            .into_iter()
            .map(|(r, p)| (r.to_string(), p))
            .collect(),
        auto_imports: b
            .auto_imports
            .into_iter()
            .map(to_stored_new_project)
            .collect(),
        identity_updates: b
            .identity_updates
            .into_iter()
            .map(|u| StoredIdentityUpdate {
                id: u.id,
                stack: u.stack.as_db_str().to_string(),
                icon: u.icon,
                color: u.color,
                updated_at: u.updated_at,
            })
            .collect(),
        missing_updates: b
            .missing_updates
            .into_iter()
            .map(|m| StoredMissingUpdate {
                id: m.id,
                missing: m.missing,
                updated_at: m.updated_at,
            })
            .collect(),
    }
}

fn to_project_mutation(m: CatalogMutation) -> ProjectMutation {
    match m {
        CatalogMutation::Add(n) => ProjectMutation::Add(to_stored_new_project(n)),
        CatalogMutation::Update(p) => ProjectMutation::Update(to_stored_project(p)),
        CatalogMutation::Remove { id, dismiss } => ProjectMutation::Remove {
            id,
            dismiss: dismiss.into_iter().map(to_stored_dismissed).collect(),
        },
        CatalogMutation::Reorder(ids) => ProjectMutation::Reorder(ids),
        CatalogMutation::AddRoot {
            root,
            reconciliation,
        } => ProjectMutation::AddRoot {
            root: to_stored_root(root),
            reconcile: to_stored_reconcile(reconciliation),
        },
        CatalogMutation::UpdateRoot(r) => ProjectMutation::UpdateRoot(to_stored_root(r)),
        CatalogMutation::RemoveRoot(id) => ProjectMutation::RemoveRoot(id.to_string()),
        CatalogMutation::Reconcile(b) => ProjectMutation::Reconcile(to_stored_reconcile(b)),
        CatalogMutation::Accept(items) => {
            ProjectMutation::Accept(items.into_iter().map(to_stored_new_project).collect())
        }
        CatalogMutation::Dismiss(items) => {
            ProjectMutation::Dismiss(items.into_iter().map(to_stored_dismissed).collect())
        }
    }
}

fn from_catalog_rows(rows: CatalogRows) -> CatalogStateRows {
    CatalogStateRows {
        projects: rows.projects.into_iter().map(from_stored_project).collect(),
        roots: rows
            .roots
            .into_iter()
            .filter_map(from_stored_root)
            .collect(),
        entries: rows
            .entries
            .into_iter()
            .filter_map(|e| {
                let root_id = ProjectRootId::from_str(&e.root_id).ok()?;
                Some(RootEntryRow {
                    root_id,
                    entry_path: e.entry_path,
                    canonical_path: e.canonical_path,
                    observed_at: e.observed_at,
                })
            })
            .collect(),
        dismissed: rows
            .dismissed
            .into_iter()
            .filter_map(|d| {
                let root_id = ProjectRootId::from_str(&d.root_id).ok()?;
                Some(DismissedRow {
                    root_id,
                    path: d.path,
                    dismissed_at: d.dismissed_at,
                })
            })
            .collect(),
    }
}

fn from_project_commit(c: ProjectCommit) -> CommitResult {
    CommitResult {
        upserted_projects: c
            .upserted_projects
            .into_iter()
            .map(from_stored_project)
            .collect(),
        inserted_projects: c
            .inserted_projects
            .into_iter()
            .map(from_stored_project)
            .collect(),
        deleted_project_ids: c.deleted_project_ids,
        reordered_project_ids: c.reordered_project_ids,
        upserted_root: c.upserted_root.and_then(from_stored_root),
        deleted_root_id: c
            .deleted_root_id
            .and_then(|s| ProjectRootId::from_str(&s).ok()),
        catalog: from_catalog_rows(c.catalog),
    }
}

/// Implementação de [`ProjectStore`] em memória para testes isolados sem SQLite.
#[derive(Debug, Default)]
pub struct MemoryProjectStore {
    state: Mutex<CatalogStateRows>,
}

impl MemoryProjectStore {
    /// Cria um novo [`MemoryProjectStore`] vazio.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProjectStore for MemoryProjectStore {
    async fn load_catalog(&self) -> MtResult<CatalogStateRows> {
        Ok(self.state.lock().clone())
    }

    async fn find_project_by_path(&self, path: &str) -> MtResult<Option<Project>> {
        let guard = self.state.lock();
        Ok(guard.projects.iter().find(|p| p.path == path).cloned())
    }

    #[allow(clippy::too_many_lines)]
    async fn commit(&self, mutation: CatalogMutation) -> MtResult<CommitResult> {
        let mut guard = self.state.lock();
        let mut res = CommitResult::default();

        match mutation {
            CatalogMutation::Add(new_proj) => {
                let (p, inserted) = memory_insert_project(&mut guard, new_proj)?;
                if inserted {
                    res.inserted_projects.push(p.clone());
                }
                res.upserted_projects.push(p);
            }
            CatalogMutation::Update(proj) => {
                let Some(existing) = guard.projects.iter_mut().find(|p| p.id == proj.id) else {
                    return Err(MtError::new(
                        codes::PROJECT_NOT_FOUND,
                        "O projeto solicitado não foi encontrado.",
                    ));
                };
                *existing = proj.clone();
                res.upserted_projects.push(proj);
            }
            CatalogMutation::Remove { id, dismiss } => {
                let before = guard.projects.len();
                guard.projects.retain(|p| p.id != id);
                if guard.projects.len() == before {
                    return Err(MtError::new(
                        codes::PROJECT_NOT_FOUND,
                        "O projeto solicitado não foi encontrado.",
                    ));
                }
                memory_upsert_dismissed(&mut guard, dismiss);
                res.deleted_project_ids.push(id);
            }
            CatalogMutation::Reorder(ids) => {
                if ids.len() != guard.projects.len() {
                    return Err(MtError::new(
                        codes::PROJECT_REORDER_MISMATCH,
                        "A lista de projetos para reordenação diverge do catálogo atual.",
                    ));
                }
                let current_ids: HashSet<ProjectId> = guard.projects.iter().map(|p| p.id).collect();
                let mut seen = HashSet::with_capacity(ids.len());
                for id in &ids {
                    if !current_ids.contains(id) || !seen.insert(*id) {
                        return Err(MtError::new(
                            codes::PROJECT_REORDER_MISMATCH,
                            "A lista de projetos para reordenação diverge do catálogo atual.",
                        ));
                    }
                }
                for (idx, id) in ids.iter().enumerate() {
                    if let Some(p) = guard.projects.iter_mut().find(|p| p.id == *id) {
                        p.sort_order = i64::try_from(idx).unwrap_or(0);
                    }
                }
                guard.projects.sort_by(|a, b| {
                    a.sort_order
                        .cmp(&b.sort_order)
                        .then_with(|| a.id.cmp(&b.id))
                });
                res.reordered_project_ids = ids;
            }
            CatalogMutation::AddRoot {
                root,
                reconciliation,
            } => {
                if guard.roots.iter().any(|r| r.path == root.path) {
                    return Err(MtError::new(
                        codes::PROJECT_ROOT_DUPLICATE_PATH,
                        "A pasta informada já está cadastrada como Pasta de Projetos.",
                    ));
                }
                guard.roots.push(root.clone());
                guard.roots.sort_by(|a, b| {
                    a.created_at
                        .cmp(&b.created_at)
                        .then_with(|| a.id.cmp(&b.id))
                });
                res.upserted_root = Some(root);
                memory_apply_reconcile(&mut guard, reconciliation, &mut res)?;
            }
            CatalogMutation::UpdateRoot(root) => {
                let Some(existing) = guard.roots.iter_mut().find(|r| r.id == root.id) else {
                    return Err(MtError::new(
                        codes::PROJECT_ROOT_NOT_FOUND,
                        "A Pasta de Projetos solicitada não foi encontrada.",
                    ));
                };
                existing.discovery_mode = root.discovery_mode;
                existing.watch_enabled = root.watch_enabled;
                existing.updated_at = root.updated_at;
                res.upserted_root = Some(existing.clone());
            }
            CatalogMutation::RemoveRoot(id) => {
                let before = guard.roots.len();
                guard.roots.retain(|r| r.id != id);
                if guard.roots.len() == before {
                    return Err(MtError::new(
                        codes::PROJECT_ROOT_NOT_FOUND,
                        "A Pasta de Projetos solicitada não foi encontrada.",
                    ));
                }
                for p in &mut guard.projects {
                    if p.root_id == Some(id) {
                        p.root_id = None;
                    }
                }
                guard.entries.retain(|e| e.root_id != id);
                guard.dismissed.retain(|d| d.root_id != id);
                res.deleted_root_id = Some(id);
            }
            CatalogMutation::Reconcile(batch) => {
                memory_apply_reconcile(&mut guard, batch, &mut res)?;
            }
            CatalogMutation::Accept(items) => {
                for item in items {
                    let (p, inserted) = memory_insert_project(&mut guard, item)?;
                    if inserted {
                        res.inserted_projects.push(p.clone());
                    }
                    memory_push_unique_project(&mut res.upserted_projects, p);
                }
            }
            CatalogMutation::Dismiss(items) => {
                memory_upsert_dismissed(&mut guard, items);
            }
        }

        res.catalog = guard.clone();
        Ok(res)
    }
}

fn memory_insert_project(
    state: &mut CatalogStateRows,
    new_proj: NewProjectRow,
) -> MtResult<(Project, bool)> {
    if let Some(existing) = state
        .projects
        .iter()
        .find(|p| p.path == new_proj.project.path)
        .cloned()
    {
        if new_proj.if_exists_noop {
            state
                .dismissed
                .retain(|d| !new_proj.clear_dismissed_paths.contains(&d.path));
            return Ok((existing, false));
        }
        return Err(MtError::new(
            codes::PROJECT_DUPLICATE_PATH,
            "A pasta informada já está cadastrada como projeto.",
        ));
    }

    let next_order = state
        .projects
        .iter()
        .map(|p| p.sort_order + 1)
        .max()
        .unwrap_or(0);
    let mut p = new_proj.project;
    p.title = SanitizedText::new(&p.title).into_inner();
    p.sort_order = next_order;
    state.projects.push(p.clone());
    state.projects.sort_by(|a, b| {
        a.sort_order
            .cmp(&b.sort_order)
            .then_with(|| a.id.cmp(&b.id))
    });
    state
        .dismissed
        .retain(|d| !new_proj.clear_dismissed_paths.contains(&d.path));

    Ok((p, true))
}

fn memory_upsert_dismissed(state: &mut CatalogStateRows, items: Vec<DismissedRow>) {
    for item in items {
        if let Some(existing) = state
            .dismissed
            .iter_mut()
            .find(|d| d.root_id == item.root_id && d.path == item.path)
        {
            existing.dismissed_at = item.dismissed_at;
        } else {
            state.dismissed.push(item);
        }
    }
    state
        .dismissed
        .sort_by(|a, b| a.root_id.cmp(&b.root_id).then_with(|| a.path.cmp(&b.path)));
}

fn memory_apply_reconcile(
    state: &mut CatalogStateRows,
    batch: ReconcileBatch,
    res: &mut CommitResult,
) -> MtResult<()> {
    for (root_id, entry_path) in &batch.removed_entries {
        state
            .entries
            .retain(|e| !(e.root_id == *root_id && e.entry_path == *entry_path));
    }
    for entry in batch.upserted_entries {
        if let Some(existing) = state
            .entries
            .iter_mut()
            .find(|e| e.root_id == entry.root_id && e.entry_path == entry.entry_path)
        {
            existing.canonical_path = entry.canonical_path;
            existing.observed_at = entry.observed_at;
        } else {
            state.entries.push(entry);
        }
    }
    state.entries.sort_by(|a, b| {
        a.root_id
            .cmp(&b.root_id)
            .then_with(|| a.entry_path.cmp(&b.entry_path))
    });

    for new_proj in batch.auto_imports {
        let root_to_link = new_proj.project.root_id;
        let updated_ts = new_proj.project.updated_at;
        let (p, inserted) = memory_insert_project(state, new_proj)?;
        if inserted {
            res.inserted_projects.push(p.clone());
            memory_push_unique_project(&mut res.upserted_projects, p);
        } else if p.root_id.is_none()
            && root_to_link.is_some()
            && let Some(existing) = state.projects.iter_mut().find(|x| x.id == p.id)
        {
            existing.root_id = root_to_link;
            existing.updated_at = updated_ts;
            memory_push_unique_project(&mut res.upserted_projects, existing.clone());
        }
    }

    for ident in batch.identity_updates {
        if let Some(p) = state.projects.iter_mut().find(|p| p.id == ident.id)
            && p.identity_source == IdentitySource::Auto
            && p.stack == StackKind::Unknown
        {
            p.stack = ident.stack;
            p.icon = ident.icon;
            p.color = ident.color;
            p.updated_at = ident.updated_at;
            memory_push_unique_project(&mut res.upserted_projects, p.clone());
        }
    }

    for miss in batch.missing_updates {
        if let Some(p) = state.projects.iter_mut().find(|p| p.id == miss.id)
            && p.missing != miss.missing
        {
            p.missing = miss.missing;
            p.updated_at = miss.updated_at;
            memory_push_unique_project(&mut res.upserted_projects, p.clone());
        }
    }

    Ok(())
}

fn memory_push_unique_project(vec: &mut Vec<Project>, proj: Project) {
    if let Some(existing) = vec.iter_mut().find(|p| p.id == proj.id) {
        *existing = proj;
    } else {
        vec.push(proj);
    }
}
