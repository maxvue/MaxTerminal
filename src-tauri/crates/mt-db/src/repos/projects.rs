//! Repositório transacional das tabelas da Fase 06 (`projects`, `project_roots`,
//! `project_root_entries` e `project_discovery_dismissed`).
//!
//! Todos os títulos de projetos exigem [`SanitizedText`] na assinatura pública.

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use mt_core::ids::ProjectId;
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::sanitizer::SanitizedText;

/// Extensão interna invocada na mesma transação SQLite após cada inserção efetivamente nova em `projects`.
///
/// Na Fase 06 a lista de contribuidores está vazia; fases futuras (ex.: Fase 07 para templates padrão)
/// podem registrar um contribuidor sem expor `Connection` ou closures SQL fora de `mt-db`.
pub(crate) trait ProjectInsertContributor: Send + Sync + 'static {
    /// Executado dentro da transação imediatamente após um `INSERT INTO projects` efetivo.
    fn after_insert(&self, tx: &Transaction<'_>, project: &StoredProject) -> DbResult<()>;
}

/// Linha persistida da tabela `projects`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredProject {
    /// Identificador único do projeto (UUID v7).
    pub id: ProjectId,
    /// Título higienizado (`1..=120` caracteres).
    pub title: SanitizedText,
    /// Identificador Iconify (`prefixo:nome`).
    pub icon: String,
    /// Cor hexadecimal em caixa alta (`#RRGGBB`).
    pub color: String,
    /// Caminho absoluto canônico da pasta do projeto.
    pub path: String,
    /// Identificador opcional da pasta-mãe associada (UUID v7 textual).
    pub root_id: Option<String>,
    /// Identificador da stack (`tauri`, `vscode_extension`, `laravel`, `vue`, `rust`, `python`, `unknown`).
    pub stack: String,
    /// Fonte da identidade visual (`auto` ou `manual`).
    pub identity_source: String,
    /// Posição de ordenação (`0..N-1`).
    pub sort_order: i64,
    /// Indica se o diretório não foi encontrado na última verificação.
    pub missing: bool,
    /// Timestamp UNIX em ms de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms da última atualização.
    pub updated_at: i64,
}

/// Linha persistida da tabela `project_roots`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRoot {
    /// Identificador único da pasta-mãe (UUID v7 textual).
    pub id: String,
    /// Caminho absoluto canônico da pasta-mãe.
    pub path: String,
    /// Modo de descoberta (`suggest` ou `auto_import`).
    pub discovery_mode: String,
    /// Indica se o monitoramento automático via inotify está habilitado.
    pub watch_enabled: bool,
    /// Timestamp UNIX em ms de criação.
    pub created_at: i64,
    /// Timestamp UNIX em ms da última atualização.
    pub updated_at: i64,
}

/// Linha persistida da tabela `project_root_entries`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRootEntry {
    /// Identificador da pasta-mãe proprietária.
    pub root_id: String,
    /// Caminho lexical da entrada imediata dentro da pasta-mãe.
    pub entry_path: String,
    /// Caminho absoluto canônico resolvido da entrada.
    pub canonical_path: String,
    /// Timestamp UNIX em ms da observação.
    pub observed_at: i64,
}

/// Linha persistida da tabela `project_discovery_dismissed`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredDismissed {
    /// Identificador da pasta-mãe onde a sugestão foi dispensada.
    pub root_id: String,
    /// Caminho absoluto canônico dispensado.
    pub path: String,
    /// Timestamp UNIX em ms da dispensa.
    pub dismissed_at: i64,
}

/// Dados para inserção de um novo projeto na transação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredNewProject {
    /// Dados do projeto a inserir (`sort_order` será calculado na transação como `MAX(sort_order) + 1`).
    pub project: StoredProject,
    /// Caminhos canônicos a remover de `project_discovery_dismissed` em todas as raízes.
    pub clear_dismissed_paths: Vec<String>,
    /// Se `true`, caso `path` já exista em `projects`, relê a linha existente sem falhar e sem acionar contributors.
    pub if_exists_noop: bool,
}

/// Atualização de identidade automática (`stack` e opcionalmente `icon`/`color`) durante reconciliação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredIdentityUpdate {
    /// Identificador do projeto.
    pub id: ProjectId,
    /// Nova stack detectada.
    pub stack: String,
    /// Novo ícone automático (aplicado somente se `identity_source = 'auto'`).
    pub icon: String,
    /// Nova cor automática (aplicada somente se `identity_source = 'auto'`).
    pub color: String,
    /// Timestamp UNIX em ms da atualização.
    pub updated_at: i64,
}

/// Atualização do indicador `missing` de um projeto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredMissingUpdate {
    /// Identificador do projeto.
    pub id: ProjectId,
    /// Novo valor de `missing`.
    pub missing: bool,
    /// Timestamp UNIX em ms da atualização.
    pub updated_at: i64,
}

/// Lote transacional de reconciliação de origens, auto-imports, identidades e estados `missing`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoredReconcileBatch {
    /// Origens observadas a inserir ou atualizar em `project_root_entries`.
    pub upserted_entries: Vec<StoredRootEntry>,
    /// Origens confirmadamente removidas `(root_id, entry_path)`.
    pub removed_entries: Vec<(String, String)>,
    /// Projetos a importar automaticamente (idempotente por `path`).
    pub auto_imports: Vec<StoredNewProject>,
    /// Atualizações de stack/identidade para projetos `auto`.
    pub identity_updates: Vec<StoredIdentityUpdate>,
    /// Atualizações de `missing` para projetos que desapareceram ou reapareceram.
    pub missing_updates: Vec<StoredMissingUpdate>,
}

/// Snapshot consistente de todas as linhas persistidas do catálogo de projetos.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogRows {
    /// Lista de projetos ordenada por `(sort_order ASC, id ASC)`.
    pub projects: Vec<StoredProject>,
    /// Lista de pastas-mãe ordenada por `(created_at ASC, id ASC)`.
    pub roots: Vec<StoredRoot>,
    /// Lista de origens observadas ordenada por `(root_id ASC, entry_path ASC)`.
    pub entries: Vec<StoredRootEntry>,
    /// Lista de dispensas ordenada por `(root_id ASC, path ASC)`.
    pub dismissed: Vec<StoredDismissed>,
}

/// Mutação atômica executada em uma única transação `BEGIN IMMEDIATE`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectMutation {
    /// Cadastra um novo projeto manualmente.
    Add(StoredNewProject),
    /// Atualiza metadados de um projeto existente.
    Update(StoredProject),
    /// Remove um projeto do cadastro e opcionalmente grava dispensas nas raízes que o contêm.
    Remove {
        /// Identificador do projeto a remover.
        id: ProjectId,
        /// Dispensas a gravar na mesma transação.
        dismiss: Vec<StoredDismissed>,
    },
    /// Reordena o catálogo inteiro de projetos (`0..N-1`).
    Reorder(Vec<ProjectId>),
    /// Cadastra uma nova pasta-mãe junto com seu lote inicial de reconciliação/importação.
    AddRoot {
        /// Linha da pasta-mãe.
        root: StoredRoot,
        /// Reconciliação inicial (origens e projetos importados).
        reconcile: StoredReconcileBatch,
    },
    /// Atualiza configurações de uma pasta-mãe existente.
    UpdateRoot(StoredRoot),
    /// Remove uma pasta-mãe (`ON DELETE SET NULL` em `projects.root_id` e `ON DELETE CASCADE` em `entries`/`dismissed`).
    RemoveRoot(String),
    /// Aplica um lote de reconciliação de varredura/foco.
    Reconcile(StoredReconcileBatch),
    /// Aceita em lote sugestões descobertas, convertendo-as em projetos.
    Accept(Vec<StoredNewProject>),
    /// Dispensa em lote sugestões descobertas.
    Dismiss(Vec<StoredDismissed>),
}

/// Resultado lido na mesma transação após aplicar [`ProjectMutation`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectCommit {
    /// Projetos inseridos ou atualizados na transação (na ordem em que foram afetados).
    pub upserted_projects: Vec<StoredProject>,
    /// Projetos efetivamente novos inseridos nesta transação.
    pub inserted_projects: Vec<StoredProject>,
    /// IDs de projetos excluídos nesta transação.
    pub deleted_project_ids: Vec<ProjectId>,
    /// Nova ordem completa de IDs quando a mutação foi `Reorder`.
    pub reordered_project_ids: Vec<ProjectId>,
    /// Pasta-mãe inserida ou atualizada nesta transação.
    pub upserted_root: Option<StoredRoot>,
    /// ID de pasta-mãe removida nesta transação.
    pub deleted_root_id: Option<String>,
    /// Visão completa e consistente do catálogo após o commit.
    pub catalog: CatalogRows,
}

/// Sentinela de constraint indicando projeto não encontrado.
pub const CONSTRAINT_PROJECT_NOT_FOUND: &str = "project.not_found";
/// Sentinela de constraint indicando pasta-mãe não encontrada.
pub const CONSTRAINT_PROJECT_ROOT_NOT_FOUND: &str = "project_root.not_found";
/// Sentinela de constraint indicando caminho de projeto duplicado.
pub const CONSTRAINT_PROJECT_DUPLICATE_PATH: &str = "project.duplicate_path";
/// Sentinela de constraint indicando caminho de pasta-mãe duplicado.
pub const CONSTRAINT_PROJECT_ROOT_DUPLICATE_PATH: &str = "project_root.duplicate_path";
/// Sentinela de constraint indicando divergência no conjunto de IDs de `Reorder`.
pub const CONSTRAINT_PROJECT_REORDER_MISMATCH: &str = "project.reorder_mismatch";

/// Repositório tipado de persistência de projetos e pastas de projetos (`mt-db`).
#[derive(Clone)]
pub struct ProjectsRepo {
    db: Db,
    contributors: Arc<Vec<Arc<dyn ProjectInsertContributor>>>,
}

impl std::fmt::Debug for ProjectsRepo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectsRepo")
            .field("contributors", &self.contributors.len())
            .finish_non_exhaustive()
    }
}

impl ProjectsRepo {
    /// Cria uma nova instância de [`ProjectsRepo`] acoplada à fachada [`Db`],
    /// registrando o [`crate::repos::templates::DefaultTemplateContributor`] da Fase 07.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self {
            db,
            contributors: Arc::new(vec![Arc::new(
                crate::repos::templates::DefaultTemplateContributor,
            )]),
        }
    }

    /// Carrega um snapshot consistente de todas as tabelas do catálogo de projetos.
    pub async fn load_catalog(&self) -> DbResult<CatalogRows> {
        self.db.read(load_catalog_conn).await
    }

    /// Busca um projeto pelo caminho canônico exato.
    pub async fn find_project_by_path(&self, path: &str) -> DbResult<Option<StoredProject>> {
        let path_owned = path.to_string();
        self.db
            .read(move |conn| find_project_by_path_conn(conn, &path_owned))
            .await
    }

    /// Executa uma [`ProjectMutation`] em uma única transação `BEGIN IMMEDIATE` e retorna o [`ProjectCommit`].
    pub async fn commit(&self, mutation: ProjectMutation) -> DbResult<ProjectCommit> {
        let contributors = Arc::clone(&self.contributors);
        self.db
            .write_sized("projects_commit", 2048, move |tx| {
                execute_mutation_tx(tx, mutation, &contributors)
            })
            .await
    }
}

fn load_catalog_conn(conn: &Connection) -> DbResult<CatalogRows> {
    let projects = {
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, title, icon, color, path, root_id, stack, identity_source,
                        sort_order, missing, created_at, updated_at
                 FROM projects
                 ORDER BY sort_order ASC, id ASC;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_catalog"))?;
        let rows = stmt
            .query_map([], map_project_row)
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_catalog"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_catalog"))?
    };

    let roots = {
        let mut stmt = conn
            .prepare_cached(
                "SELECT id, path, discovery_mode, watch_enabled, created_at, updated_at
                 FROM project_roots
                 ORDER BY created_at ASC, id ASC;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_roots"))?;
        let rows = stmt
            .query_map([], map_root_row)
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_roots"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_roots"))?
    };

    let entries = {
        let mut stmt = conn
            .prepare_cached(
                "SELECT root_id, entry_path, canonical_path, observed_at
                 FROM project_root_entries
                 ORDER BY root_id ASC, entry_path ASC;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_entries"))?;
        let rows = stmt
            .query_map([], map_entry_row)
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_entries"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_entries"))?
    };

    let dismissed = {
        let mut stmt = conn
            .prepare_cached(
                "SELECT root_id, path, dismissed_at
                 FROM project_discovery_dismissed
                 ORDER BY root_id ASC, path ASC;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_dismissed"))?;
        let rows = stmt
            .query_map([], map_dismissed_row)
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_dismissed"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "projects_load_dismissed"))?
    };

    Ok(CatalogRows {
        projects,
        roots,
        entries,
        dismissed,
    })
}

fn find_project_by_path_conn(conn: &Connection, path: &str) -> DbResult<Option<StoredProject>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, title, icon, color, path, root_id, stack, identity_source,
                    sort_order, missing, created_at, updated_at
             FROM projects
             WHERE path = ?1;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "projects_find_by_path"))?;
    stmt.query_row([path], map_project_row)
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "projects_find_by_path"))
}

fn find_project_by_id_conn(conn: &Connection, id: ProjectId) -> DbResult<Option<StoredProject>> {
    let id_str = id.to_string();
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, title, icon, color, path, root_id, stack, identity_source,
                    sort_order, missing, created_at, updated_at
             FROM projects
             WHERE id = ?1;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "projects_find_by_id"))?;
    stmt.query_row([&id_str], map_project_row)
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "projects_find_by_id"))
}

fn find_root_by_id_conn(conn: &Connection, id: &str) -> DbResult<Option<StoredRoot>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, path, discovery_mode, watch_enabled, created_at, updated_at
             FROM project_roots
             WHERE id = ?1;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "projects_find_root_by_id"))?;
    stmt.query_row([id], map_root_row)
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "projects_find_root_by_id"))
}

fn next_sort_order_tx(tx: &Transaction<'_>) -> DbResult<i64> {
    let mut stmt = tx
        .prepare_cached("SELECT COALESCE(MAX(sort_order) + 1, 0) FROM projects;")
        .map_err(|e| DbError::from_rusqlite(e, "projects_next_sort_order"))?;
    stmt.query_row([], |r| r.get(0))
        .map_err(|e| DbError::from_rusqlite(e, "projects_next_sort_order"))
}

fn insert_new_project_tx(
    tx: &Transaction<'_>,
    new_proj: &StoredNewProject,
    contributors: &[Arc<dyn ProjectInsertContributor>],
) -> DbResult<(StoredProject, bool)> {
    if let Some(existing) = find_project_by_path_conn(tx, &new_proj.project.path)? {
        if new_proj.if_exists_noop {
            clear_dismissed_paths_tx(tx, &new_proj.clear_dismissed_paths)?;
            return Ok((existing, false));
        }
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PROJECT_DUPLICATE_PATH),
        });
    }

    let sort_order = next_sort_order_tx(tx)?;
    let mut stored = new_proj.project.clone();
    stored.sort_order = sort_order;

    let id_str = stored.id.to_string();
    let missing_int = i64::from(stored.missing);

    {
        let mut stmt = tx
            .prepare_cached(
                "INSERT INTO projects (
                    id, title, icon, color, path, root_id, stack, identity_source,
                    sort_order, missing, created_at, updated_at
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12);",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_insert"))?;
        stmt.execute(params![
            id_str,
            stored.title,
            stored.icon,
            stored.color,
            stored.path,
            stored.root_id,
            stored.stack,
            stored.identity_source,
            stored.sort_order,
            missing_int,
            stored.created_at,
            stored.updated_at,
        ])
        .map_err(|e| map_project_insert_err(e, "projects_insert"))?;
    }

    clear_dismissed_paths_tx(tx, &new_proj.clear_dismissed_paths)?;

    for contributor in contributors {
        contributor.after_insert(tx, &stored)?;
    }

    Ok((stored, true))
}

fn clear_dismissed_paths_tx(tx: &Transaction<'_>, paths: &[String]) -> DbResult<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let mut stmt = tx
        .prepare_cached("DELETE FROM project_discovery_dismissed WHERE path = ?1;")
        .map_err(|e| DbError::from_rusqlite(e, "projects_clear_dismissed"))?;
    for path in paths {
        stmt.execute([path])
            .map_err(|e| DbError::from_rusqlite(e, "projects_clear_dismissed"))?;
    }
    Ok(())
}

fn upsert_dismissed_tx(tx: &Transaction<'_>, items: &[StoredDismissed]) -> DbResult<()> {
    if items.is_empty() {
        return Ok(());
    }
    let mut stmt = tx
        .prepare_cached(
            "INSERT INTO project_discovery_dismissed (root_id, path, dismissed_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(root_id, path) DO UPDATE SET dismissed_at = excluded.dismissed_at;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "projects_upsert_dismissed"))?;
    for item in items {
        stmt.execute(params![item.root_id, item.path, item.dismissed_at])
            .map_err(|e| DbError::from_rusqlite(e, "projects_upsert_dismissed"))?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn apply_reconcile_batch_tx(
    tx: &Transaction<'_>,
    batch: &StoredReconcileBatch,
    contributors: &[Arc<dyn ProjectInsertContributor>],
    commit: &mut ProjectCommit,
) -> DbResult<()> {
    if !batch.removed_entries.is_empty() {
        let mut del_stmt = tx
            .prepare_cached(
                "DELETE FROM project_root_entries WHERE root_id = ?1 AND entry_path = ?2;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_delete_entry"))?;
        for (root_id, entry_path) in &batch.removed_entries {
            del_stmt
                .execute(params![root_id, entry_path])
                .map_err(|e| DbError::from_rusqlite(e, "projects_delete_entry"))?;
        }
    }

    if !batch.upserted_entries.is_empty() {
        let mut up_stmt = tx
            .prepare_cached(
                "INSERT INTO project_root_entries (root_id, entry_path, canonical_path, observed_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(root_id, entry_path) DO UPDATE SET
                    canonical_path = excluded.canonical_path,
                    observed_at = excluded.observed_at;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_upsert_entry"))?;
        for entry in &batch.upserted_entries {
            up_stmt
                .execute(params![
                    entry.root_id,
                    entry.entry_path,
                    entry.canonical_path,
                    entry.observed_at,
                ])
                .map_err(|e| DbError::from_rusqlite(e, "projects_upsert_entry"))?;
        }
    }

    for new_proj in &batch.auto_imports {
        let (stored, inserted) = insert_new_project_tx(tx, new_proj, contributors)?;
        if inserted {
            commit.inserted_projects.push(stored.clone());
            push_unique_project(&mut commit.upserted_projects, stored);
        } else if stored.root_id.is_none() && new_proj.project.root_id.is_some() {
            // Vincula root_id a projeto existente sem raiz quando descoberto sob uma raiz
            let id_str = stored.id.to_string();
            let mut stmt = tx
                .prepare_cached(
                    "UPDATE projects SET root_id = ?2, updated_at = ?3 WHERE id = ?1 AND root_id IS NULL;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "projects_link_root"))?;
            let changed = stmt
                .execute(params![
                    id_str,
                    new_proj.project.root_id,
                    new_proj.project.updated_at,
                ])
                .map_err(|e| DbError::from_rusqlite(e, "projects_link_root"))?;
            if changed > 0
                && let Some(updated) = find_project_by_id_conn(tx, stored.id)?
            {
                push_unique_project(&mut commit.upserted_projects, updated);
            }
        }
    }

    for ident in &batch.identity_updates {
        let id_str = ident.id.to_string();
        // Revalida na transação que identity_source ainda é 'auto' e stack ainda é 'unknown'
        let mut stmt = tx
            .prepare_cached(
                "UPDATE projects
                 SET stack = ?2,
                     icon = ?3,
                     color = ?4,
                     updated_at = ?5
                 WHERE id = ?1 AND identity_source = 'auto' AND stack = 'unknown';",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_reconcile_identity"))?;
        let changed = stmt
            .execute(params![
                id_str,
                ident.stack,
                ident.icon,
                ident.color,
                ident.updated_at,
            ])
            .map_err(|e| DbError::from_rusqlite(e, "projects_reconcile_identity"))?;
        if changed > 0
            && let Some(updated) = find_project_by_id_conn(tx, ident.id)?
        {
            push_unique_project(&mut commit.upserted_projects, updated);
        }
    }

    for miss in &batch.missing_updates {
        let id_str = miss.id.to_string();
        let miss_int = i64::from(miss.missing);
        let mut stmt = tx
            .prepare_cached(
                "UPDATE projects
                 SET missing = ?2,
                     updated_at = ?3
                 WHERE id = ?1 AND missing != ?2;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "projects_reconcile_missing"))?;
        let changed = stmt
            .execute(params![id_str, miss_int, miss.updated_at])
            .map_err(|e| DbError::from_rusqlite(e, "projects_reconcile_missing"))?;
        if changed > 0
            && let Some(updated) = find_project_by_id_conn(tx, miss.id)?
        {
            push_unique_project(&mut commit.upserted_projects, updated);
        }
    }

    Ok(())
}

fn push_unique_project(vec: &mut Vec<StoredProject>, proj: StoredProject) {
    if let Some(existing) = vec.iter_mut().find(|p| p.id == proj.id) {
        *existing = proj;
    } else {
        vec.push(proj);
    }
}

#[allow(clippy::too_many_lines)]
fn execute_mutation_tx(
    tx: &Transaction<'_>,
    mutation: ProjectMutation,
    contributors: &[Arc<dyn ProjectInsertContributor>],
) -> DbResult<ProjectCommit> {
    let mut commit = ProjectCommit::default();

    match mutation {
        ProjectMutation::Add(new_proj) => {
            let (stored, inserted) = insert_new_project_tx(tx, &new_proj, contributors)?;
            if inserted {
                commit.inserted_projects.push(stored.clone());
            }
            commit.upserted_projects.push(stored);
        }
        ProjectMutation::Update(proj) => {
            let id_str = proj.id.to_string();
            let missing_int = i64::from(proj.missing);
            let mut stmt = tx
                .prepare_cached(
                    "UPDATE projects
                     SET title = ?2,
                         icon = ?3,
                         color = ?4,
                         root_id = ?5,
                         stack = ?6,
                         identity_source = ?7,
                         missing = ?8,
                         updated_at = ?9
                     WHERE id = ?1;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "projects_update"))?;
            let changed = stmt
                .execute(params![
                    id_str,
                    proj.title,
                    proj.icon,
                    proj.color,
                    proj.root_id,
                    proj.stack,
                    proj.identity_source,
                    missing_int,
                    proj.updated_at,
                ])
                .map_err(|e| DbError::from_rusqlite(e, "projects_update"))?;
            if changed == 0 {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_PROJECT_NOT_FOUND),
                });
            }
            if let Some(updated) = find_project_by_id_conn(tx, proj.id)? {
                commit.upserted_projects.push(updated);
            }
        }
        ProjectMutation::Remove { id, dismiss } => {
            crate::repos::tabs::delete_project_workspace_tx(tx, id)?;
            let id_str = id.to_string();
            let mut stmt = tx
                .prepare_cached("DELETE FROM projects WHERE id = ?1;")
                .map_err(|e| DbError::from_rusqlite(e, "projects_remove"))?;
            let deleted = stmt
                .execute([&id_str])
                .map_err(|e| DbError::from_rusqlite(e, "projects_remove"))?;
            if deleted == 0 {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_PROJECT_NOT_FOUND),
                });
            }
            upsert_dismissed_tx(tx, &dismiss)?;
            commit.deleted_project_ids.push(id);
        }
        ProjectMutation::Reorder(ids) => {
            let current_ids: Vec<ProjectId> = {
                let mut stmt = tx
                    .prepare_cached("SELECT id FROM projects ORDER BY sort_order ASC, id ASC;")
                    .map_err(|e| DbError::from_rusqlite(e, "projects_reorder_check"))?;
                let rows = stmt
                    .query_map([], |r| {
                        let s: String = r.get(0)?;
                        ProjectId::from_str(&s).map_err(|_| {
                            rusqlite::Error::FromSqlConversionFailure(
                                0,
                                rusqlite::types::Type::Text,
                                Box::from("ProjectId inválido"),
                            )
                        })
                    })
                    .map_err(|e| DbError::from_rusqlite(e, "projects_reorder_check"))?;
                rows.collect::<Result<Vec<_>, _>>()
                    .map_err(|e| DbError::from_rusqlite(e, "projects_reorder_check"))?
            };

            if ids.len() != current_ids.len() {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_PROJECT_REORDER_MISMATCH),
                });
            }

            let current_set: HashSet<ProjectId> = current_ids.into_iter().collect();
            let mut seen_set = HashSet::with_capacity(ids.len());
            for id in &ids {
                if !current_set.contains(id) || !seen_set.insert(*id) {
                    return Err(DbError::ConstraintViolation {
                        constraint: String::from(CONSTRAINT_PROJECT_REORDER_MISMATCH),
                    });
                }
            }

            let mut stmt = tx
                .prepare_cached("UPDATE projects SET sort_order = ?2 WHERE id = ?1;")
                .map_err(|e| DbError::from_rusqlite(e, "projects_reorder_update"))?;
            for (idx, id) in ids.iter().enumerate() {
                let id_str = id.to_string();
                let order = i64::try_from(idx).unwrap_or(0);
                stmt.execute(params![id_str, order])
                    .map_err(|e| DbError::from_rusqlite(e, "projects_reorder_update"))?;
            }
            commit.reordered_project_ids = ids;
        }
        ProjectMutation::AddRoot { root, reconcile } => {
            let watch_int = i64::from(root.watch_enabled);
            {
                let mut stmt = tx
                    .prepare_cached(
                        "INSERT INTO project_roots (
                            id, path, discovery_mode, watch_enabled, created_at, updated_at
                        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6);",
                    )
                    .map_err(|e| DbError::from_rusqlite(e, "project_roots_insert"))?;
                stmt.execute(params![
                    root.id,
                    root.path,
                    root.discovery_mode,
                    watch_int,
                    root.created_at,
                    root.updated_at,
                ])
                .map_err(|e| map_root_insert_err(e, "project_roots_insert"))?;
            }
            commit.upserted_root = Some(root);
            apply_reconcile_batch_tx(tx, &reconcile, contributors, &mut commit)?;
        }
        ProjectMutation::UpdateRoot(root) => {
            let watch_int = i64::from(root.watch_enabled);
            let mut stmt = tx
                .prepare_cached(
                    "UPDATE project_roots
                     SET discovery_mode = ?2,
                         watch_enabled = ?3,
                         updated_at = ?4
                     WHERE id = ?1;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "project_roots_update"))?;
            let changed = stmt
                .execute(params![
                    root.id,
                    root.discovery_mode,
                    watch_int,
                    root.updated_at,
                ])
                .map_err(|e| DbError::from_rusqlite(e, "project_roots_update"))?;
            if changed == 0 {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_PROJECT_ROOT_NOT_FOUND),
                });
            }
            commit.upserted_root = find_root_by_id_conn(tx, &root.id)?;
        }
        ProjectMutation::RemoveRoot(root_id) => {
            let mut stmt = tx
                .prepare_cached("DELETE FROM project_roots WHERE id = ?1;")
                .map_err(|e| DbError::from_rusqlite(e, "project_roots_remove"))?;
            let deleted = stmt
                .execute([&root_id])
                .map_err(|e| DbError::from_rusqlite(e, "project_roots_remove"))?;
            if deleted == 0 {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_PROJECT_ROOT_NOT_FOUND),
                });
            }
            commit.deleted_root_id = Some(root_id);
        }
        ProjectMutation::Reconcile(batch) => {
            apply_reconcile_batch_tx(tx, &batch, contributors, &mut commit)?;
        }
        ProjectMutation::Accept(new_projects) => {
            for new_proj in new_projects {
                let (stored, inserted) = insert_new_project_tx(tx, &new_proj, contributors)?;
                if inserted {
                    commit.inserted_projects.push(stored.clone());
                }
                push_unique_project(&mut commit.upserted_projects, stored);
            }
        }
        ProjectMutation::Dismiss(dismissed) => {
            upsert_dismissed_tx(tx, &dismissed)?;
        }
    }

    for proj in &commit.upserted_projects {
        crate::search::projection::sync_project_if_ready(tx, proj.id)?;
    }
    for del_id in &commit.deleted_project_ids {
        crate::search::projection::delete_project_projections_if_ready(tx, *del_id)?;
    }

    commit.catalog = load_catalog_conn(tx)?;
    Ok(commit)
}

fn map_project_insert_err(err: rusqlite::Error, op: &'static str) -> DbError {
    if let rusqlite::Error::SqliteFailure(ffi_err, ref msg) = err
        && ffi_err.code == rusqlite::ErrorCode::ConstraintViolation
        && msg.as_deref().is_some_and(|m| m.contains("projects.path"))
    {
        return DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PROJECT_DUPLICATE_PATH),
        };
    }
    DbError::from_rusqlite(err, op)
}

fn map_root_insert_err(err: rusqlite::Error, op: &'static str) -> DbError {
    if let rusqlite::Error::SqliteFailure(ffi_err, ref msg) = err
        && ffi_err.code == rusqlite::ErrorCode::ConstraintViolation
        && msg
            .as_deref()
            .is_some_and(|m| m.contains("project_roots.path"))
    {
        return DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PROJECT_ROOT_DUPLICATE_PATH),
        };
    }
    DbError::from_rusqlite(err, op)
}

fn map_project_row(row: &Row<'_>) -> rusqlite::Result<StoredProject> {
    let id_raw: String = row.get(0)?;
    let id = ProjectId::from_str(&id_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::from("ProjectId inválido"),
        )
    })?;
    let title_raw: String = row.get(1)?;
    let missing_int: i64 = row.get(9)?;

    Ok(StoredProject {
        id,
        title: SanitizedText::new(&title_raw),
        icon: row.get(2)?,
        color: row.get(3)?,
        path: row.get(4)?,
        root_id: row.get(5)?,
        stack: row.get(6)?,
        identity_source: row.get(7)?,
        sort_order: row.get(8)?,
        missing: missing_int != 0,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn map_root_row(row: &Row<'_>) -> rusqlite::Result<StoredRoot> {
    let watch_int: i64 = row.get(3)?;
    Ok(StoredRoot {
        id: row.get(0)?,
        path: row.get(1)?,
        discovery_mode: row.get(2)?,
        watch_enabled: watch_int != 0,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn map_entry_row(row: &Row<'_>) -> rusqlite::Result<StoredRootEntry> {
    Ok(StoredRootEntry {
        root_id: row.get(0)?,
        entry_path: row.get(1)?,
        canonical_path: row.get(2)?,
        observed_at: row.get(3)?,
    })
}

fn map_dismissed_row(row: &Row<'_>) -> rusqlite::Result<StoredDismissed> {
    Ok(StoredDismissed {
        root_id: row.get(0)?,
        path: row.get(1)?,
        dismissed_at: row.get(2)?,
    })
}
