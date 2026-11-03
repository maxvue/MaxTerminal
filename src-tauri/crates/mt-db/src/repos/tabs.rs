//! Repositório transacional da tabela `tabs` e coordenação com `panes` (Fase 07).

use std::collections::HashSet;
use std::str::FromStr;

use mt_core::error::codes as core_codes;
use mt_core::ids::{PaneId, ProjectId, TabId, TemplateId};
use mt_core::workspace::{
    LaunchSnapshot, LayoutSizePatch, PaneLayoutNode, SplitDirection, TabOrigin,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::repos::panes::{
    CONSTRAINT_PANE_NOT_FOUND, StoredPaneRow, close_pane_row_tx, get_pane_conn, insert_pane_tx,
    list_open_panes_by_tab_conn,
};
use crate::repos::projects::CONSTRAINT_PROJECT_NOT_FOUND;
use crate::repos::templates::CONSTRAINT_TEMPLATE_NOT_FOUND;
use crate::sanitizer::SanitizedText;

/// Sentinela de constraint indicando aba não encontrada.
pub const CONSTRAINT_TAB_NOT_FOUND: &str = "tab.not_found";
/// Sentinela de constraint indicando divergência de lista em `tab_reorder`.
pub const CONSTRAINT_TAB_REORDER_MISMATCH: &str = "tab.reorder_mismatch";
/// Sentinela de constraint indicando árvore de layout inválida.
pub const CONSTRAINT_LAYOUT_INVALID: &str = "layout.invalid";
/// Sentinela de constraint indicando conflito de revisão da árvore de layout.
pub const CONSTRAINT_LAYOUT_CONFLICT: &str = "layout.conflict";
/// Sentinela de constraint indicando limite de painéis atingido (`MAX_PANES_PER_TAB`).
pub const CONSTRAINT_PANE_LIMIT_REACHED: &str = "pane.limit_reached";

/// Linha persistida da tabela `tabs`.
#[derive(Debug, Clone, PartialEq)]
pub struct StoredTabRow {
    /// Identificador único da aba (UUID v7).
    pub id: TabId,
    /// Identificador do projeto proprietário.
    pub project_id: ProjectId,
    /// Identificador opcional do template de origem.
    pub template_id: Option<TemplateId>,
    /// Origem da aba (`manual` ou `workspace`).
    pub origin: TabOrigin,
    /// Título higienizado da aba.
    pub title: String,
    /// Ícone Iconify da aba.
    pub icon: String,
    /// Posição de ordenação entre as abas do projeto.
    pub position: i64,
    /// Indica se a aba foi configurada como execução contínua.
    pub continuous: bool,
    /// Snapshot imutável de lançamento da aba.
    pub launch_snapshot: LaunchSnapshot,
    /// Árvore de layout de painéis da aba.
    pub layout: PaneLayoutNode,
    /// Painel atualmente com foco na aba.
    pub active_pane_id: PaneId,
    /// Painel primário (elegível ao comando inicial).
    pub primary_pane_id: PaneId,
    /// Revisão monotônica de metadados/estado da aba.
    pub revision: i64,
    /// Revisão monotônica da árvore de layout.
    pub layout_revision: i64,
    /// Última sequência alocada de agrupamento `tab_sessions` para esta aba.
    pub last_session_seq: i64,
    /// Timestamp UNIX em ms de abertura.
    pub opened_at: i64,
    /// Timestamp UNIX em ms de fechamento (`None` se aberta).
    pub closed_at: Option<i64>,
    /// Timestamp UNIX em ms do último foco recebido.
    pub last_focused_at: Option<i64>,
}

/// Dados de entrada para criação transacional de uma aba com sua folha inicial.
#[derive(Debug, Clone)]
pub struct NewTabTxInput {
    /// Identificador da aba.
    pub id: TabId,
    /// Projeto proprietário.
    pub project_id: ProjectId,
    /// Template opcional de origem.
    pub template_id: Option<TemplateId>,
    /// Origem (`Manual` ou `Workspace`).
    pub origin: TabOrigin,
    /// Título sanitizado.
    pub title: SanitizedText,
    /// Ícone Iconify.
    pub icon: String,
    /// Flag de execução contínua.
    pub continuous: bool,
    /// Snapshot imutável de lançamento (com valores de `env_vars` sanitizados se literais).
    pub launch_snapshot: LaunchSnapshot,
    /// Identificador da folha inicial.
    pub initial_pane_id: PaneId,
    /// Diretório inicial sanitizado da folha.
    pub initial_cwd: SanitizedText,
    /// Timestamp UNIX em ms de abertura.
    pub opened_at: i64,
}

/// Repositório tipado para operações sobre a tabela `tabs`.
#[derive(Clone, Debug)]
pub struct TabsRepo {
    db: Db,
}

impl TabsRepo {
    /// Cria uma nova instância de [`TabsRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Busca uma aba (aberta ou fechada) pelo seu [`TabId`].
    pub async fn get(&self, tab_id: TabId) -> DbResult<Option<StoredTabRow>> {
        self.db.read(move |conn| get_tab_conn(conn, tab_id)).await
    }

    /// Busca a aba única `origin = 'workspace'` associada a um `template_id` (aberta ou fechada).
    pub async fn find_workspace_tab_for_template(
        &self,
        template_id: TemplateId,
    ) -> DbResult<Option<StoredTabRow>> {
        self.db
            .read(move |conn| find_workspace_tab_for_template_conn(conn, template_id))
            .await
    }

    /// Lista as abas abertas (`closed_at IS NULL`) de um projeto em ordem `(position ASC, id ASC)`.
    pub async fn list_open_by_project(&self, project_id: ProjectId) -> DbResult<Vec<StoredTabRow>> {
        self.db
            .read(move |conn| list_open_tabs_by_project_conn(conn, project_id))
            .await
    }

    /// Lista todas as abas abertas (`closed_at IS NULL`) de todos os projetos.
    pub async fn list_all_open(&self) -> DbResult<Vec<StoredTabRow>> {
        self.db.read(list_all_open_tabs_conn).await
    }

    /// Reordena as abas abertas de um projeto de forma atômica, exigindo correspondência exata do conjunto.
    pub async fn reorder_open_tabs(
        &self,
        project_id: ProjectId,
        ordered_tab_ids: Vec<TabId>,
    ) -> DbResult<Vec<TabId>> {
        self.db
            .write_sized("tabs_reorder", 1024, move |tx| {
                reorder_open_tabs_tx(tx, project_id, ordered_tab_ids)
            })
            .await
    }

    /// Atualiza o foco da aba e opcionalmente o `active_pane_id`.
    pub async fn update_focus(
        &self,
        tab_id: TabId,
        pane_id: Option<PaneId>,
        now_ms: i64,
    ) -> DbResult<StoredTabRow> {
        self.db
            .write_sized("tabs_focus", 512, move |tx| {
                update_tab_focus_tx(tx, tab_id, pane_id, now_ms)
            })
            .await
    }

    /// Aplica patches de redimensionamento (`Splitpanes`) validando `expected_layout_revision`.
    pub async fn update_layout_with_revision(
        &self,
        tab_id: TabId,
        expected_layout_revision: i64,
        patches: Vec<LayoutSizePatch>,
    ) -> DbResult<StoredTabRow> {
        self.db
            .write_sized("tabs_update_layout", 1024, move |tx| {
                update_tab_layout_with_revision_tx(tx, tab_id, expected_layout_revision, &patches)
            })
            .await
    }
}

/// Busca uma aba na conexão/transação informada.
pub fn get_tab_conn(conn: &Connection, tab_id: TabId) -> DbResult<Option<StoredTabRow>> {
    let id_str = tab_id.to_string();
    conn.query_row(
        "SELECT id, project_id, template_id, origin, title, icon, position, continuous,
                launch_json, layout_json, active_pane_id, primary_pane_id, revision,
                layout_revision, last_session_seq, opened_at, closed_at, last_focused_at
         FROM tabs
         WHERE id = ?1;",
        [&id_str],
        map_tab_row,
    )
    .optional()
    .map_err(|e| DbError::from_rusqlite(e, "tabs_get"))
}

/// Busca a aba `origin = 'workspace'` associada a um `template_id` (aberta ou fechada).
pub fn find_workspace_tab_for_template_conn(
    conn: &Connection,
    template_id: TemplateId,
) -> DbResult<Option<StoredTabRow>> {
    let tpl_str = template_id.to_string();
    conn.query_row(
        "SELECT id, project_id, template_id, origin, title, icon, position, continuous,
                launch_json, layout_json, active_pane_id, primary_pane_id, revision,
                layout_revision, last_session_seq, opened_at, closed_at, last_focused_at
         FROM tabs
         WHERE origin = 'workspace' AND template_id = ?1;",
        [&tpl_str],
        map_tab_row,
    )
    .optional()
    .map_err(|e| DbError::from_rusqlite(e, "tabs_find_workspace_for_template"))
}

/// Lista as abas abertas de um projeto na conexão/transação informada.
pub fn list_open_tabs_by_project_conn(
    conn: &Connection,
    project_id: ProjectId,
) -> DbResult<Vec<StoredTabRow>> {
    let proj_str = project_id.to_string();
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, project_id, template_id, origin, title, icon, position, continuous,
                    launch_json, layout_json, active_pane_id, primary_pane_id, revision,
                    layout_revision, last_session_seq, opened_at, closed_at, last_focused_at
             FROM tabs
             WHERE project_id = ?1 AND closed_at IS NULL
             ORDER BY position ASC, id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "tabs_list_open_by_project"))?;
    let rows = stmt
        .query_map([&proj_str], map_tab_row)
        .map_err(|e| DbError::from_rusqlite(e, "tabs_list_open_by_project"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "tabs_list_open_by_project"))
}

/// Lista todas as abas abertas no banco, ordenadas por `(project.sort_order ASC, tabs.position ASC, tabs.id ASC)`.
pub fn list_all_open_tabs_conn(conn: &Connection) -> DbResult<Vec<StoredTabRow>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT t.id, t.project_id, t.template_id, t.origin, t.title, t.icon, t.position,
                    t.continuous, t.launch_json, t.layout_json, t.active_pane_id, t.primary_pane_id,
                    t.revision, t.layout_revision, t.last_session_seq, t.opened_at, t.closed_at,
                    t.last_focused_at
             FROM tabs t
             JOIN projects p ON p.id = t.project_id
             WHERE t.closed_at IS NULL
             ORDER BY p.sort_order ASC, t.position ASC, t.id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "tabs_list_all_open"))?;
    let rows = stmt
        .query_map([], map_tab_row)
        .map_err(|e| DbError::from_rusqlite(e, "tabs_list_all_open"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "tabs_list_all_open"))
}

/// Cria uma nova aba e sua folha inicial na mesma transação `BEGIN IMMEDIATE`.
pub fn create_tab_with_initial_pane_tx(
    tx: &Transaction<'_>,
    input: &NewTabTxInput,
) -> DbResult<(StoredTabRow, StoredPaneRow)> {
    ensure_project_exists_tx(tx, input.project_id)?;
    if let Some(tpl_id) = input.template_id {
        ensure_template_belongs_to_project_tx(tx, tpl_id, input.project_id)?;
    }

    let proj_str = input.project_id.to_string();
    let position: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM tabs WHERE project_id = ?1 AND closed_at IS NULL;",
            [&proj_str],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "tabs_next_position"))?;

    let layout = PaneLayoutNode::Leaf {
        pane_id: input.initial_pane_id,
    };
    layout
        .validate(&[input.initial_pane_id])
        .map_err(|_| DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_LAYOUT_INVALID),
        })?;

    let sanitized_launch = sanitize_launch_snapshot(&input.launch_snapshot);
    let launch_json =
        serde_json::to_string(&sanitized_launch).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tabs.launch_json"),
            reason: e.to_string(),
        })?;
    let layout_json = serde_json::to_string(&layout).map_err(|e| DbError::SettingInvalidValue {
        key: String::from("tabs.layout_json"),
        reason: e.to_string(),
    })?;

    let id_str = input.id.to_string();
    let tpl_str = input.template_id.map(|t| t.to_string());
    let origin_str = input.origin.as_db_str();
    let pane_str = input.initial_pane_id.to_string();

    tx.execute(
        "INSERT INTO tabs (
            id, project_id, template_id, origin, title, icon, position, continuous,
            launch_json, layout_json, active_pane_id, primary_pane_id,
            revision, layout_revision, last_session_seq, opened_at, closed_at, last_focused_at
        ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
            ?9, ?10, ?11, ?11,
            1, 1, 0, ?12, NULL, ?12
        );",
        params![
            id_str,
            proj_str,
            tpl_str,
            origin_str,
            input.title.as_str(),
            input.icon,
            position,
            i64::from(input.continuous),
            launch_json,
            layout_json,
            pane_str,
            input.opened_at,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_insert"))?;

    let pane_row = StoredPaneRow {
        id: input.initial_pane_id,
        tab_id: input.id,
        cwd: input.initial_cwd.as_str().to_string(),
        generation: 1,
        created_at: input.opened_at,
        closed_at: None,
    };
    insert_pane_tx(tx, &pane_row)?;

    let tab_row = StoredTabRow {
        id: input.id,
        project_id: input.project_id,
        template_id: input.template_id,
        origin: input.origin,
        title: input.title.as_str().to_string(),
        icon: input.icon.clone(),
        position,
        continuous: input.continuous,
        launch_snapshot: sanitized_launch,
        layout,
        active_pane_id: input.initial_pane_id,
        primary_pane_id: input.initial_pane_id,
        revision: 1,
        layout_revision: 1,
        last_session_seq: 0,
        opened_at: input.opened_at,
        closed_at: None,
        last_focused_at: Some(input.opened_at),
    };

    Ok((tab_row, pane_row))
}

/// Reabre e atualiza uma aba `origin = 'workspace'` existente durante a restauração do boot.
///
/// Reutiliza `layout`, `active_pane_id` e `primary_pane_id` caso a árvore persistida seja válida
/// e todas as suas folhas existam na tabela `panes` para esta aba; caso contrário, substitui
/// por uma nova folha única `fallback_pane_id` sem apagar o histórico anterior.
#[allow(clippy::too_many_arguments)]
pub fn reopen_workspace_tab_tx(
    tx: &Transaction<'_>,
    tab_id: TabId,
    title: &SanitizedText,
    icon: &str,
    continuous: bool,
    launch_snapshot: &LaunchSnapshot,
    fallback_pane_id: PaneId,
    fallback_cwd: &SanitizedText,
    now_ms: i64,
) -> DbResult<(StoredTabRow, Vec<StoredPaneRow>, bool)> {
    let existing = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;

    let all_panes: Vec<StoredPaneRow> =
        crate::repos::panes::list_all_panes_by_tab_conn(tx, tab_id)?;
    let pane_by_id: std::collections::HashMap<PaneId, StoredPaneRow> =
        all_panes.into_iter().map(|p| (p.id, p)).collect();

    let leaves = existing.layout.leaves();
    let tree_valid = !leaves.is_empty()
        && existing.layout.validate(&leaves).is_ok()
        && leaves.iter().all(|pid| pane_by_id.contains_key(pid))
        && leaves.contains(&existing.active_pane_id)
        && leaves.contains(&existing.primary_pane_id);

    let mut layout_replaced = false;
    let (final_layout, active_pane_id, primary_pane_id, reopened_panes) = if tree_valid {
        for (pid, p) in &pane_by_id {
            if !leaves.contains(pid) && p.closed_at.is_none() {
                close_pane_row_tx(tx, *pid, now_ms)?;
            }
        }
        let mut panes_out = Vec::with_capacity(leaves.len());
        for pid in leaves {
            let bumped = crate::repos::panes::bump_pane_generation_tx(tx, pid, None)?;
            panes_out.push(bumped);
        }
        (
            existing.layout,
            existing.active_pane_id,
            existing.primary_pane_id,
            panes_out,
        )
    } else {
        layout_replaced = true;
        for (pid, p) in &pane_by_id {
            if p.closed_at.is_none() {
                close_pane_row_tx(tx, *pid, now_ms)?;
            }
        }
        let new_pane = StoredPaneRow {
            id: fallback_pane_id,
            tab_id,
            cwd: fallback_cwd.as_str().to_string(),
            generation: 1,
            created_at: now_ms,
            closed_at: None,
        };
        insert_pane_tx(tx, &new_pane)?;
        (
            PaneLayoutNode::Leaf {
                pane_id: fallback_pane_id,
            },
            fallback_pane_id,
            fallback_pane_id,
            vec![new_pane],
        )
    };

    let sanitized_launch = sanitize_launch_snapshot(launch_snapshot);
    let launch_json =
        serde_json::to_string(&sanitized_launch).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tabs.launch_json"),
            reason: e.to_string(),
        })?;
    let layout_json =
        serde_json::to_string(&final_layout).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tabs.layout_json"),
            reason: e.to_string(),
        })?;

    tx.execute(
        "UPDATE tabs
         SET title = ?2,
             icon = ?3,
             continuous = ?4,
             launch_json = ?5,
             layout_json = ?6,
             active_pane_id = ?7,
             primary_pane_id = ?8,
             revision = revision + 1,
             layout_revision = layout_revision + 1,
             opened_at = ?9,
             closed_at = NULL
         WHERE id = ?1;",
        params![
            tab_id.to_string(),
            title.as_str(),
            icon,
            i64::from(continuous),
            launch_json,
            layout_json,
            active_pane_id.to_string(),
            primary_pane_id.to_string(),
            now_ms,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_reopen_workspace"))?;

    let updated = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;

    Ok((updated, reopened_panes, layout_replaced))
}

/// Divide uma folha da árvore de uma aba aberta em uma transação `BEGIN IMMEDIATE`.
#[allow(clippy::too_many_arguments)]
pub fn split_pane_tx(
    tx: &Transaction<'_>,
    tab_id: TabId,
    target_pane_id: PaneId,
    new_pane_id: PaneId,
    new_node_id: String,
    direction: SplitDirection,
    expected_layout_revision: i64,
    new_cwd: &SanitizedText,
    now_ms: i64,
) -> DbResult<(StoredTabRow, StoredPaneRow)> {
    let tab = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;
    if tab.closed_at.is_some() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
        });
    }
    if tab.layout_revision != expected_layout_revision {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_LAYOUT_CONFLICT),
        });
    }

    let new_layout = tab
        .layout
        .split_leaf(target_pane_id, new_pane_id, direction, new_node_id)
        .map_err(|err| {
            let code = if err.code == core_codes::PANE_LIMIT_REACHED {
                CONSTRAINT_PANE_LIMIT_REACHED
            } else if err.code == core_codes::PANE_NOT_FOUND {
                CONSTRAINT_PANE_NOT_FOUND
            } else {
                CONSTRAINT_LAYOUT_INVALID
            };
            DbError::ConstraintViolation {
                constraint: String::from(code),
            }
        })?;

    let new_pane = StoredPaneRow {
        id: new_pane_id,
        tab_id,
        cwd: new_cwd.as_str().to_string(),
        generation: 1,
        created_at: now_ms,
        closed_at: None,
    };
    insert_pane_tx(tx, &new_pane)?;

    let layout_json =
        serde_json::to_string(&new_layout).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tabs.layout_json"),
            reason: e.to_string(),
        })?;

    tx.execute(
        "UPDATE tabs
         SET layout_json = ?2,
             active_pane_id = ?3,
             revision = revision + 1,
             layout_revision = layout_revision + 1,
             last_focused_at = ?4
         WHERE id = ?1;",
        params![
            tab_id.to_string(),
            layout_json,
            new_pane_id.to_string(),
            now_ms,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_split_update"))?;

    ensure_layout_matches_open_panes_tx(tx, tab_id, &new_layout)?;

    let updated = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;
    Ok((updated, new_pane))
}

/// Remove uma folha da árvore de uma aba aberta (fechando o painel em `panes`).
///
/// Retorna `Ok(Some(updated_tab))` se ainda restarem folhas na aba, ou `Ok(None)` se a folha
/// removida era a última da aba (caso em que o chamador deve arquivar a sessão da aba e fechar a aba).
pub fn remove_pane_from_layout_tx(
    tx: &Transaction<'_>,
    tab_id: TabId,
    pane_id: PaneId,
    now_ms: i64,
) -> DbResult<Option<StoredTabRow>> {
    let tab = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;
    if tab.closed_at.is_some() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
        });
    }

    let (maybe_next_layout, recommended_focus) =
        tab.layout
            .remove_leaf(pane_id)
            .map_err(|_| DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_PANE_NOT_FOUND),
            })?;

    close_pane_row_tx(tx, pane_id, now_ms)?;

    let Some(next_layout) = maybe_next_layout else {
        return Ok(None);
    };

    let remaining_leaves = next_layout.leaves();
    let fallback_leaf = recommended_focus
        .or_else(|| remaining_leaves.first().copied())
        .ok_or_else(|| DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_LAYOUT_INVALID),
        })?;

    let next_active = if remaining_leaves.contains(&tab.active_pane_id) {
        tab.active_pane_id
    } else {
        fallback_leaf
    };
    let next_primary = if remaining_leaves.contains(&tab.primary_pane_id) {
        tab.primary_pane_id
    } else {
        fallback_leaf
    };

    let layout_json =
        serde_json::to_string(&next_layout).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tabs.layout_json"),
            reason: e.to_string(),
        })?;

    tx.execute(
        "UPDATE tabs
         SET layout_json = ?2,
             active_pane_id = ?3,
             primary_pane_id = ?4,
             revision = revision + 1,
             layout_revision = layout_revision + 1
         WHERE id = ?1;",
        params![
            tab_id.to_string(),
            layout_json,
            next_active.to_string(),
            next_primary.to_string(),
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_remove_pane_update"))?;

    ensure_layout_matches_open_panes_tx(tx, tab_id, &next_layout)?;

    let updated = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;
    Ok(Some(updated))
}

/// Marca uma aba e todos os seus painéis como fechados (`closed_at = now_ms`).
pub fn close_tab_tx(
    tx: &Transaction<'_>,
    tab_id: TabId,
    now_ms: i64,
) -> DbResult<Option<StoredTabRow>> {
    let Some(existing) = get_tab_conn(tx, tab_id)? else {
        return Ok(None);
    };
    if existing.closed_at.is_some() {
        return Ok(Some(existing));
    }

    let id_str = tab_id.to_string();
    tx.execute(
        "UPDATE panes SET closed_at = COALESCE(closed_at, ?2) WHERE tab_id = ?1;",
        params![id_str, now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_close_panes"))?;

    tx.execute(
        "UPDATE tabs
         SET closed_at = ?2,
             revision = revision + 1
         WHERE id = ?1;",
        params![id_str, now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_close"))?;

    get_tab_conn(tx, tab_id)
}

/// Exclui na mesma transação todas as sessões base, blocos, scrollbacks, vínculos, agrupamentos,
/// painéis e abas pertencentes a um projeto que está sendo removido (conforme `spec.md` §5.3).
pub fn delete_project_workspace_tx(tx: &Transaction<'_>, project_id: ProjectId) -> DbResult<()> {
    let proj_str = project_id.to_string();

    // 1. Remove vínculos tab_session_panes antes de remover panes (devido a ON DELETE RESTRICT em pane_id)
    tx.execute(
        "DELETE FROM tab_session_panes
         WHERE tab_session_id IN (
             SELECT ts.id
             FROM tab_sessions ts
             JOIN tabs t ON t.id = ts.tab_id
             WHERE t.project_id = ?1
         ) OR pane_id IN (
             SELECT p.id
             FROM panes p
             JOIN tabs t ON t.id = p.tab_id
             WHERE t.project_id = ?1
         );",
        [&proj_str],
    )
    .map_err(|e| DbError::from_rusqlite(e, "projects_remove_tab_session_panes"))?;

    // 2. Remove sessões base associadas ao projeto ou às suas abas (cascateando command_blocks, FTS e scrollback)
    tx.execute(
        "DELETE FROM sessions
         WHERE project_id = ?1
            OR tab_id IN (SELECT id FROM tabs WHERE project_id = ?1);",
        [&proj_str],
    )
    .map_err(|e| DbError::from_rusqlite(e, "projects_remove_sessions"))?;

    // 3. Remove agrupamentos tab_sessions das abas do projeto
    tx.execute(
        "DELETE FROM tab_sessions
         WHERE tab_id IN (SELECT id FROM tabs WHERE project_id = ?1);",
        [&proj_str],
    )
    .map_err(|e| DbError::from_rusqlite(e, "projects_remove_tab_sessions"))?;

    // 4. Remove painéis e abas do projeto
    tx.execute(
        "DELETE FROM panes WHERE tab_id IN (SELECT id FROM tabs WHERE project_id = ?1);",
        [&proj_str],
    )
    .map_err(|e| DbError::from_rusqlite(e, "projects_remove_panes"))?;

    tx.execute("DELETE FROM tabs WHERE project_id = ?1;", [&proj_str])
        .map_err(|e| DbError::from_rusqlite(e, "projects_remove_tabs"))?;

    Ok(())
}

fn reorder_open_tabs_tx(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    ordered_tab_ids: Vec<TabId>,
) -> DbResult<Vec<TabId>> {
    let proj_str = project_id.to_string();
    let current_ids: Vec<TabId> = {
        let mut stmt = tx
            .prepare_cached(
                "SELECT id FROM tabs
                 WHERE project_id = ?1 AND closed_at IS NULL
                 ORDER BY position ASC, id ASC;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "tabs_reorder_check"))?;
        let rows = stmt
            .query_map([&proj_str], |r| {
                let s: String = r.get(0)?;
                TabId::from_str(&s).map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::from("TabId inválido"),
                    )
                })
            })
            .map_err(|e| DbError::from_rusqlite(e, "tabs_reorder_check"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "tabs_reorder_check"))?
    };

    if ordered_tab_ids.len() != current_ids.len() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_REORDER_MISMATCH),
        });
    }

    let current_set: HashSet<TabId> = current_ids.into_iter().collect();
    let mut seen = HashSet::with_capacity(ordered_tab_ids.len());
    for id in &ordered_tab_ids {
        if !current_set.contains(id) || !seen.insert(*id) {
            return Err(DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_TAB_REORDER_MISMATCH),
            });
        }
    }

    let mut stmt = tx
        .prepare_cached(
            "UPDATE tabs
             SET position = ?2,
                 revision = revision + 1
             WHERE id = ?1 AND project_id = ?3 AND closed_at IS NULL;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "tabs_reorder_update"))?;
    for (idx, id) in ordered_tab_ids.iter().enumerate() {
        let pos = i64::try_from(idx).unwrap_or(0);
        stmt.execute(params![id.to_string(), pos, proj_str])
            .map_err(|e| DbError::from_rusqlite(e, "tabs_reorder_update"))?;
    }

    Ok(ordered_tab_ids)
}

fn update_tab_focus_tx(
    tx: &Transaction<'_>,
    tab_id: TabId,
    pane_id: Option<PaneId>,
    now_ms: i64,
) -> DbResult<StoredTabRow> {
    let tab = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;
    if tab.closed_at.is_some() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
        });
    }

    let target_pane = match pane_id {
        Some(pid) => {
            if !tab.layout.leaves().contains(&pid) {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_PANE_NOT_FOUND),
                });
            }
            let pane_row = get_pane_conn(tx, pid)?.ok_or_else(|| DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_PANE_NOT_FOUND),
            })?;
            if pane_row.tab_id != tab_id || pane_row.closed_at.is_some() {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_PANE_NOT_FOUND),
                });
            }
            pid
        }
        None => tab.active_pane_id,
    };

    tx.execute(
        "UPDATE tabs
         SET active_pane_id = ?2,
             last_focused_at = ?3,
             revision = revision + 1
         WHERE id = ?1;",
        params![tab_id.to_string(), target_pane.to_string(), now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_update_focus"))?;

    get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })
}

fn update_tab_layout_with_revision_tx(
    tx: &Transaction<'_>,
    tab_id: TabId,
    expected_layout_revision: i64,
    patches: &[LayoutSizePatch],
) -> DbResult<StoredTabRow> {
    let mut tab = get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })?;
    if tab.closed_at.is_some() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
        });
    }
    if tab.layout_revision != expected_layout_revision {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_LAYOUT_CONFLICT),
        });
    }

    tab.layout =
        tab.layout
            .apply_size_patches(patches)
            .map_err(|_| DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_LAYOUT_INVALID),
            })?;

    ensure_layout_matches_open_panes_tx(tx, tab_id, &tab.layout)?;

    let layout_json =
        serde_json::to_string(&tab.layout).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tabs.layout_json"),
            reason: e.to_string(),
        })?;

    tx.execute(
        "UPDATE tabs
         SET layout_json = ?2,
             revision = revision + 1,
             layout_revision = layout_revision + 1
         WHERE id = ?1;",
        params![tab_id.to_string(), layout_json],
    )
    .map_err(|e| DbError::from_rusqlite(e, "tabs_update_layout"))?;

    get_tab_conn(tx, tab_id)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TAB_NOT_FOUND),
    })
}

fn ensure_layout_matches_open_panes_tx(
    tx: &Transaction<'_>,
    tab_id: TabId,
    layout: &PaneLayoutNode,
) -> DbResult<()> {
    let open_panes: Vec<PaneId> = list_open_panes_by_tab_conn(tx, tab_id)?
        .into_iter()
        .map(|p| p.id)
        .collect();
    layout
        .validate(&open_panes)
        .map_err(|_| DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_LAYOUT_INVALID),
        })?;
    Ok(())
}

fn ensure_project_exists_tx(tx: &Transaction<'_>, project_id: ProjectId) -> DbResult<()> {
    let proj_str = project_id.to_string();
    let exists: Option<i64> = tx
        .query_row("SELECT 1 FROM projects WHERE id = ?1;", [&proj_str], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "tabs_check_project"))?;
    if exists.is_none() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PROJECT_NOT_FOUND),
        });
    }
    Ok(())
}

fn ensure_template_belongs_to_project_tx(
    tx: &Transaction<'_>,
    template_id: TemplateId,
    project_id: ProjectId,
) -> DbResult<()> {
    let tpl_str = template_id.to_string();
    let owner: Option<String> = tx
        .query_row(
            "SELECT project_id FROM tab_templates WHERE id = ?1;",
            [&tpl_str],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "tabs_check_template"))?;
    let Some(owner_proj) = owner else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
        });
    };
    if owner_proj != project_id.to_string() {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
        });
    }
    Ok(())
}

fn sanitize_launch_snapshot(snapshot: &LaunchSnapshot) -> LaunchSnapshot {
    let mut clean = snapshot.clone();
    clean.title = SanitizedText::new(&clean.title).into_inner();
    clean.resolved_cwd = SanitizedText::new(&clean.resolved_cwd).into_inner();
    clean.configured_cwd = clean
        .configured_cwd
        .as_deref()
        .map(SanitizedText::new)
        .map(SanitizedText::into_inner);
    clean.initial_command = clean
        .initial_command
        .as_deref()
        .map(SanitizedText::new)
        .map(SanitizedText::into_inner);
    clean.env_vars = clean
        .env_vars
        .into_iter()
        .map(|(k, v)| {
            let clean_val = match v {
                mt_core::workspace::EnvValue::Literal { value } => {
                    mt_core::workspace::EnvValue::Literal {
                        value: SanitizedText::new(&value).into_inner(),
                    }
                }
                mt_core::workspace::EnvValue::Inherited { source } => {
                    mt_core::workspace::EnvValue::Inherited { source }
                }
            };
            (k, clean_val)
        })
        .collect();
    clean
}

pub(crate) fn map_tab_row(row: &Row<'_>) -> rusqlite::Result<StoredTabRow> {
    let id_raw: String = row.get(0)?;
    let proj_raw: String = row.get(1)?;
    let tpl_raw: Option<String> = row.get(2)?;
    let origin_raw: String = row.get(3)?;
    let title: String = row.get(4)?;
    let icon: String = row.get(5)?;
    let position: i64 = row.get(6)?;
    let continuous_int: i64 = row.get(7)?;
    let launch_json: String = row.get(8)?;
    let layout_json: String = row.get(9)?;
    let active_pane_raw: String = row.get(10)?;
    let primary_pane_raw: String = row.get(11)?;
    let revision: i64 = row.get(12)?;
    let layout_revision: i64 = row.get(13)?;
    let last_session_seq: i64 = row.get(14)?;
    let opened_at: i64 = row.get(15)?;
    let closed_at: Option<i64> = row.get(16)?;
    let last_focused_at: Option<i64> = row.get(17)?;

    let id = TabId::from_str(&id_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::from("TabId inválido"),
        )
    })?;
    let project_id = ProjectId::from_str(&proj_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::from("ProjectId inválido"),
        )
    })?;
    let template_id = match tpl_raw {
        Some(s) => Some(TemplateId::from_str(&s).map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                2,
                rusqlite::types::Type::Text,
                Box::from("TemplateId inválido"),
            )
        })?),
        None => None,
    };
    let origin = TabOrigin::from_db_str(&origin_raw).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            3,
            rusqlite::types::Type::Text,
            Box::from("TabOrigin inválido"),
        )
    })?;
    let launch_snapshot: LaunchSnapshot = serde_json::from_str(&launch_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let layout: PaneLayoutNode = serde_json::from_str(&layout_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(9, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let active_pane_id = PaneId::from_str(&active_pane_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            10,
            rusqlite::types::Type::Text,
            Box::from("PaneId ativo inválido"),
        )
    })?;
    let primary_pane_id = PaneId::from_str(&primary_pane_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            11,
            rusqlite::types::Type::Text,
            Box::from("PaneId primário inválido"),
        )
    })?;

    Ok(StoredTabRow {
        id,
        project_id,
        template_id,
        origin,
        title,
        icon,
        position,
        continuous: continuous_int != 0,
        launch_snapshot,
        layout,
        active_pane_id,
        primary_pane_id,
        revision,
        layout_revision,
        last_session_seq,
        opened_at,
        closed_at,
        last_focused_at,
    })
}
