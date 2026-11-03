//! Projeções sanitizadas (`SanitizedProjection`) para a tabela `search_metadata` e manutenção
//! do índice FTS5 `search_metadata_fts`.
#![allow(clippy::type_complexity, clippy::too_many_lines)]

use std::str::FromStr;

use mt_core::ids::{CommandGroupId, PaneId, ProjectId, SessionId, TabId, TemplateId};
use rusqlite::{Connection, OptionalExtension, params};

use crate::error::{DbError, DbResult};
use crate::fts::{FtsRegistry, FtsSpec, FtsTokenizer};
use crate::sanitizer::SanitizedText;
use crate::search::model::{SearchSourceKind, SearchTarget};

/// Especificação FTS5 canônica da tabela `search_metadata_fts` (Fase 17).
pub const SEARCH_METADATA_FTS_SPEC: FtsSpec = FtsSpec {
    content_table: "search_metadata",
    fts_table: "search_metadata_fts",
    columns: &["title", "body"],
    tokenizer: FtsTokenizer::Text,
    prefix: &[2, 3],
};

/// Alias compatível com `SEARCH_METADATA_FTS`.
pub const SEARCH_METADATA_FTS: FtsSpec = SEARCH_METADATA_FTS_SPEC;

/// Subconjunto de fontes que utilizam a projeção auxiliar `search_metadata`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProjectionSource {
    /// Sessões (`sessions`).
    Sessions,
    /// Projetos (`projects`).
    Projects,
    /// Templates de aba (`templates`).
    Templates,
    /// Grupos de comandos (`groups`).
    Groups,
    /// Execuções de grupos de comandos (`group_runs`).
    GroupRuns,
    /// Issues do GitHub (`issues`).
    Issues,
    /// Comentários de issues do GitHub (`comments`).
    Comments,
}

impl ProjectionSource {
    /// Retorna o valor literal gravado na coluna `search_metadata.source`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sessions => "sessions",
            Self::Projects => "projects",
            Self::Templates => "templates",
            Self::Groups => "groups",
            Self::GroupRuns => "group_runs",
            Self::Issues => "issues",
            Self::Comments => "comments",
        }
    }

    /// Converte `SearchSourceKind` em `Option<ProjectionSource>`.
    #[must_use]
    pub const fn from_source_kind(kind: SearchSourceKind) -> Option<Self> {
        match kind {
            SearchSourceKind::Commands | SearchSourceKind::Output => None,
            SearchSourceKind::Sessions => Some(Self::Sessions),
            SearchSourceKind::Projects => Some(Self::Projects),
            SearchSourceKind::Templates => Some(Self::Templates),
            SearchSourceKind::Groups => Some(Self::Groups),
            SearchSourceKind::GroupRuns => Some(Self::GroupRuns),
            SearchSourceKind::Issues => Some(Self::Issues),
            SearchSourceKind::Comments => Some(Self::Comments),
        }
    }
}

/// Registro higienizado pronto para persistência na tabela `search_metadata`.
///
/// Os campos textuais (`title`, `body`) exigem obrigatoriamente `SanitizedText`, garantindo por
/// construção que nenhum segredo em texto puro chegue ao índice `search_metadata_fts`.
#[derive(Debug, Clone)]
pub struct SanitizedProjection {
    /// Fonte dona da projeção.
    pub source: ProjectionSource,
    /// Chave única da entidade dentro da fonte.
    pub entity_key: String,
    /// Projeto associado (se aplicável).
    pub project_id: Option<ProjectId>,
    /// Aba associada (se aplicável).
    pub tab_id: Option<TabId>,
    /// Sessão associada (se aplicável).
    pub session_id: Option<SessionId>,
    /// Painel associado (se aplicável).
    pub pane_id: Option<PaneId>,
    /// Código de saída associado (aplicável a `group_runs`).
    pub exit_code: Option<i64>,
    /// Indica se o conteúdo original já estava truncado na origem.
    pub source_truncated: bool,
    /// Timestamp de sincronização externa (ex.: issues/comentários do GitHub).
    pub fetched_at: Option<i64>,
    /// Título já sanitizado.
    pub title: SanitizedText,
    /// Corpo/descrição já sanitizado.
    pub body: SanitizedText,
    /// Alvo de navegação serializado em `target_json`.
    pub target: SearchTarget,
    /// Timestamp UNIX em milissegundos de ocorrência/atualização.
    pub occurred_at: Option<i64>,
    /// Revisão ou hash da entidade de origem.
    pub source_revision: String,
}

/// Insere ou atualiza atomicamente uma projeção sanitizada em `search_metadata`.
pub fn upsert_projection(conn: &Connection, proj: &SanitizedProjection) -> DbResult<()> {
    let target_json =
        serde_json::to_string(&proj.target).map_err(|e| DbError::ConstraintViolation {
            constraint: format!("Falha ao serializar SearchTarget: {e}"),
        })?;

    let project_id_str = proj.project_id.map(|id| id.to_string());
    let tab_id_str = proj.tab_id.map(|id| id.to_string());
    let session_id_str = proj.session_id.map(|id| id.to_string());
    let pane_id_str = proj.pane_id.map(|id| id.to_string());
    let truncated_int: i64 = i64::from(proj.source_truncated);

    // Se project_id foi fornecido mas não existe em projects, grava NULL para não violar FK
    let valid_project_id: Option<String> = if let Some(ref pid) = project_id_str {
        let exists: i64 = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1);",
                [pid],
                |r| r.get(0),
            )
            .map_err(|e| DbError::from_rusqlite(e, "upsert_projection_check_project"))?;
        if exists != 0 { Some(pid.clone()) } else { None }
    } else {
        None
    };

    conn.execute(
        "INSERT INTO search_metadata (
            source, entity_key, project_id, tab_id, session_id, pane_id,
            exit_code, source_truncated, fetched_at, title, body, target_json,
            occurred_at, source_revision
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
         ON CONFLICT(source, entity_key) DO UPDATE SET
            project_id = excluded.project_id,
            tab_id = excluded.tab_id,
            session_id = excluded.session_id,
            pane_id = excluded.pane_id,
            exit_code = excluded.exit_code,
            source_truncated = excluded.source_truncated,
            fetched_at = excluded.fetched_at,
            title = excluded.title,
            body = excluded.body,
            target_json = excluded.target_json,
            occurred_at = excluded.occurred_at,
            source_revision = excluded.source_revision;",
        params![
            proj.source.as_str(),
            proj.entity_key,
            valid_project_id,
            tab_id_str,
            session_id_str,
            pane_id_str,
            proj.exit_code,
            truncated_int,
            proj.fetched_at,
            proj.title,
            proj.body,
            target_json,
            proj.occurred_at,
            proj.source_revision,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "upsert_search_projection"))?;

    Ok(())
}

/// Remove uma projeção específica de `search_metadata` (o trigger `_ad` atualiza `search_metadata_fts`).
pub fn delete_projection(
    conn: &Connection,
    source: ProjectionSource,
    entity_key: &str,
) -> DbResult<()> {
    conn.execute(
        "DELETE FROM search_metadata WHERE source = ?1 AND entity_key = ?2;",
        params![source.as_str(), entity_key],
    )
    .map_err(|e| DbError::from_rusqlite(e, "delete_search_projection"))?;
    Ok(())
}

/// Sincroniza a projeção de um projeto específico a partir da linha atual em `projects`.
pub fn sync_project_by_id(conn: &Connection, project_id: ProjectId) -> DbResult<()> {
    let pid_str = project_id.to_string();
    let row: Option<(String, String, String, i64)> = conn
        .query_row(
            "SELECT title, path, stack, updated_at FROM projects WHERE id = ?1;",
            [&pid_str],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "sync_project_by_id"))?;

    if let Some((title, path, stack, updated_at)) = row {
        upsert_projection(
            conn,
            &SanitizedProjection {
                source: ProjectionSource::Projects,
                entity_key: pid_str,
                project_id: Some(project_id),
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: None,
                source_truncated: false,
                fetched_at: None,
                title: SanitizedText::new(&title),
                body: SanitizedText::new(&format!("{path} {stack}")),
                target: SearchTarget::Project { project_id },
                occurred_at: Some(updated_at),
                source_revision: updated_at.to_string(),
            },
        )
    } else {
        delete_projection(conn, ProjectionSource::Projects, &pid_str)
    }
}

/// Sincroniza a projeção de um template de aba específico a partir da linha atual em `tab_templates`.
pub fn sync_template_by_id(conn: &Connection, template_id: TemplateId) -> DbResult<()> {
    let tid_str = template_id.to_string();
    let row: Option<(String, String, Option<String>, Option<String>, String, Option<String>, i64, i64)> =
        conn.query_row(
            "SELECT project_id, name, cwd, initial_command, environment_kind, agent_id, revision, updated_at
             FROM tab_templates WHERE id = ?1;",
            [&tid_str],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "sync_template_by_id"))?;

    if let Some((proj_raw, name, cwd, initial_cmd, env_kind, agent_id, revision, updated_at)) = row
        && let Ok(project_id) = ProjectId::from_str(&proj_raw)
    {
        let mut body_parts = Vec::new();
        if let Some(c) = initial_cmd {
            body_parts.push(c);
        }
        if let Some(d) = cwd {
            body_parts.push(d);
        }
        body_parts.push(env_kind);
        if let Some(a) = agent_id {
            body_parts.push(a);
        }

        upsert_projection(
            conn,
            &SanitizedProjection {
                source: ProjectionSource::Templates,
                entity_key: tid_str,
                project_id: Some(project_id),
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: None,
                source_truncated: false,
                fetched_at: None,
                title: SanitizedText::new(&name),
                body: SanitizedText::new(&body_parts.join(" ")),
                target: SearchTarget::Template {
                    project_id,
                    template_id,
                },
                occurred_at: Some(updated_at),
                source_revision: revision.to_string(),
            },
        )
    } else {
        delete_projection(conn, ProjectionSource::Templates, &tid_str)
    }
}

/// Sincroniza a projeção de uma sessão específica a partir de `sessions`.
pub fn sync_session_by_id(conn: &Connection, session_id: SessionId) -> DbResult<()> {
    let sid_str = session_id.to_string();
    let row: Option<(
        Option<String>,
        Option<String>,
        Option<String>,
        String,
        String,
        String,
        Option<String>,
        i64,
        Option<i64>,
        i64,
        Option<String>,
    )> = conn
        .query_row(
            "SELECT
                COALESCE(s.project_id, t.project_id),
                COALESCE(s.tab_id, ts.tab_id),
                COALESCE(s.pane_id, tsp.pane_id),
                s.shell_kind,
                s.shell_executable,
                s.initial_cwd,
                s.last_cwd,
                s.started_at,
                s.ended_at,
                COALESCE(s.scrollback_truncated, 0),
                t.title
             FROM sessions s
             LEFT JOIN tab_session_panes tsp ON tsp.session_id = s.id
             LEFT JOIN tab_sessions ts ON ts.id = COALESCE(s.tab_session_id, tsp.tab_session_id)
             LEFT JOIN tabs t ON t.id = COALESCE(s.tab_id, ts.tab_id)
             WHERE s.id = ?1;",
            [&sid_str],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                ))
            },
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "sync_session_by_id"))?;

    if let Some((
        proj_raw,
        tab_raw,
        pane_raw,
        shell_kind,
        shell_exec,
        initial_cwd,
        last_cwd,
        started_at,
        ended_at,
        truncated,
        tab_title,
    )) = row
    {
        let project_id = proj_raw
            .as_deref()
            .and_then(|s| ProjectId::from_str(s).ok());
        let tab_id = tab_raw.as_deref().and_then(|s| TabId::from_str(s).ok());
        let pane_id = pane_raw.as_deref().and_then(|s| PaneId::from_str(s).ok());
        let effective_cwd = last_cwd.as_deref().unwrap_or(&initial_cwd);
        let title_str = match tab_title {
            Some(ref t) if !t.trim().is_empty() => format!("{t} ({shell_kind}) — {effective_cwd}"),
            _ => format!("Sessão {shell_kind} — {effective_cwd}"),
        };
        let body_str = format!("{shell_kind} {shell_exec} {initial_cwd} {effective_cwd}");
        let occurred = ended_at.unwrap_or(started_at);

        upsert_projection(
            conn,
            &SanitizedProjection {
                source: ProjectionSource::Sessions,
                entity_key: sid_str,
                project_id,
                tab_id,
                session_id: Some(session_id),
                pane_id,
                exit_code: None,
                source_truncated: truncated != 0,
                fetched_at: None,
                title: SanitizedText::new(&title_str),
                body: SanitizedText::new(&body_str),
                target: SearchTarget::Session {
                    project_id,
                    tab_id,
                    session_id,
                },
                occurred_at: Some(occurred),
                source_revision: occurred.to_string(),
            },
        )
    } else {
        delete_projection(conn, ProjectionSource::Sessions, &sid_str)
    }
}

/// Popula/atualiza integralmente `search_metadata` a partir de todas as tabelas de domínio existentes.
///
/// Usado tanto pela migration `0011_17_search` quanto por `search_index_rebuild` e fixtures de teste.
#[allow(clippy::too_many_lines)]
pub fn backfill_all_projections(conn: &Connection) -> DbResult<u64> {
    let mut total = 0u64;

    // 1. Projetos (Fase 06)
    if table_exists(conn, "projects")? {
        let rows: Vec<ProjectId> = {
            let mut stmt = conn
                .prepare("SELECT id FROM projects;")
                .map_err(|e| DbError::from_rusqlite(e, "backfill_projects_select"))?;
            let mapped = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| DbError::from_rusqlite(e, "backfill_projects_query"))?;
            let mut ids = Vec::new();
            for item in mapped {
                let raw = item.map_err(|e| DbError::from_rusqlite(e, "backfill_projects_row"))?;
                if let Ok(pid) = ProjectId::from_str(&raw) {
                    ids.push(pid);
                }
            }
            ids
        };
        for pid in rows {
            sync_project_by_id(conn, pid)?;
            total += 1;
        }
    }

    // 2. Templates (Fase 07)
    if table_exists(conn, "tab_templates")? {
        let rows: Vec<TemplateId> = {
            let mut stmt = conn
                .prepare("SELECT id FROM tab_templates;")
                .map_err(|e| DbError::from_rusqlite(e, "backfill_templates_select"))?;
            let mapped = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| DbError::from_rusqlite(e, "backfill_templates_query"))?;
            let mut ids = Vec::new();
            for item in mapped {
                let raw = item.map_err(|e| DbError::from_rusqlite(e, "backfill_templates_row"))?;
                if let Ok(tid) = TemplateId::from_str(&raw) {
                    ids.push(tid);
                }
            }
            ids
        };
        for tid in rows {
            sync_template_by_id(conn, tid)?;
            total += 1;
        }
    }

    // 3. Sessões (Fases 05/07)
    if table_exists(conn, "sessions")? {
        let rows: Vec<SessionId> = {
            let mut stmt = conn
                .prepare("SELECT id FROM sessions;")
                .map_err(|e| DbError::from_rusqlite(e, "backfill_sessions_select"))?;
            let mapped = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| DbError::from_rusqlite(e, "backfill_sessions_query"))?;
            let mut ids = Vec::new();
            for item in mapped {
                let raw = item.map_err(|e| DbError::from_rusqlite(e, "backfill_sessions_row"))?;
                if let Ok(sid) = SessionId::from_str(&raw) {
                    ids.push(sid);
                }
            }
            ids
        };
        for sid in rows {
            sync_session_by_id(conn, sid)?;
            total += 1;
        }
    }

    // 4. Grupos de Comandos e Execuções (Fase 15)
    if table_exists(conn, "command_groups")? {
        total += backfill_groups(conn)?;
    }
    if table_exists(conn, "command_group_runs")? {
        total += backfill_group_runs(conn)?;
    }

    // 5. Issues e Comentários do GitHub (Fase 16)
    if table_exists(conn, "github_issues")? {
        total += backfill_github_issues_and_comments(conn)?;
    }

    Ok(total)
}

fn backfill_groups(conn: &Connection) -> DbResult<u64> {
    let groups: Vec<(
        String,
        Option<String>,
        String,
        String,
        Option<String>,
        i64,
        i64,
    )> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, name, description, cwd, revision, updated_at
                 FROM command_groups;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "backfill_groups_prepare"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            })
            .map_err(|e| DbError::from_rusqlite(e, "backfill_groups_query"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "backfill_groups_collect"))?
    };

    let mut count = 0u64;
    for (id_raw, proj_raw, name, description, cwd, revision, updated_at) in groups {
        let Ok(group_id) = CommandGroupId::from_str(&id_raw) else {
            continue;
        };
        let project_id = proj_raw
            .as_deref()
            .and_then(|s| ProjectId::from_str(s).ok());

        let mut body_parts = Vec::new();
        if !description.is_empty() {
            body_parts.push(description);
        }
        if let Some(c) = cwd {
            body_parts.push(c);
        }

        // Concatena comandos e títulos dos itens do grupo
        if table_exists(conn, "command_group_items")? {
            let mut item_stmt = conn
                .prepare(
                    "SELECT title, command, COALESCE(cwd, '') FROM command_group_items
                     WHERE group_id = ?1 ORDER BY position ASC;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "backfill_group_items_prepare"))?;
            let item_rows = item_stmt
                .query_map([&id_raw], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| DbError::from_rusqlite(e, "backfill_group_items_query"))?;
            for item in item_rows.flatten() {
                body_parts.push(item.0);
                body_parts.push(item.1);
                if !item.2.is_empty() {
                    body_parts.push(item.2);
                }
            }
        }

        // Concatena labels do grupo
        if table_exists(conn, "command_group_labels")? {
            let mut lbl_stmt = conn
                .prepare("SELECT label FROM command_group_labels WHERE group_id = ?1;")
                .map_err(|e| DbError::from_rusqlite(e, "backfill_group_labels_prepare"))?;
            let lbl_rows = lbl_stmt
                .query_map([&id_raw], |r| r.get::<_, String>(0))
                .map_err(|e| DbError::from_rusqlite(e, "backfill_group_labels_query"))?;
            for lbl in lbl_rows.flatten() {
                body_parts.push(lbl);
            }
        }

        upsert_projection(
            conn,
            &SanitizedProjection {
                source: ProjectionSource::Groups,
                entity_key: id_raw,
                project_id,
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: None,
                source_truncated: false,
                fetched_at: None,
                title: SanitizedText::new(&name),
                body: SanitizedText::new(&body_parts.join(" ")),
                target: SearchTarget::Group {
                    group_id,
                    project_id,
                },
                occurred_at: Some(updated_at),
                source_revision: revision.to_string(),
            },
        )?;
        count += 1;
    }

    Ok(count)
}

fn backfill_group_runs(conn: &Connection) -> DbResult<u64> {
    let runs: Vec<(
        String,
        String,
        String,
        String,
        Option<i64>,
        i64,
        Option<i64>,
        i64,
    )> = {
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, snapshot_json, status, last_exit_code, created_at, finished_at, version
                 FROM command_group_runs;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "backfill_group_runs_prepare"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            })
            .map_err(|e| DbError::from_rusqlite(e, "backfill_group_runs_query"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "backfill_group_runs_collect"))?
    };

    let mut count = 0u64;
    for (
        run_id,
        proj_raw,
        snapshot_json,
        status,
        last_exit_code,
        created_at,
        finished_at,
        version,
    ) in runs
    {
        let project_id = ProjectId::from_str(&proj_raw).ok();
        let group_name = serde_json::from_str::<serde_json::Value>(&snapshot_json)
            .ok()
            .and_then(|v| {
                v.get("name")
                    .or_else(|| v.get("groupName"))
                    .and_then(|n| n.as_str())
                    .map(str::to_owned)
            })
            .unwrap_or_else(|| format!("Execução {run_id}"));

        let mut body_parts = vec![status, snapshot_json];
        let mut any_truncated = false;
        let mut first_item_key: Option<String> = None;

        if table_exists(conn, "command_group_run_items")? {
            let mut item_stmt = conn
                .prepare(
                    "SELECT id, snapshot_json, output_tail, COALESCE(error_message, ''), output_truncated
                     FROM command_group_run_items WHERE run_id = ?1 ORDER BY position ASC;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "backfill_run_items_prepare"))?;
            let item_rows = item_stmt
                .query_map([&run_id], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, i64>(4)?,
                    ))
                })
                .map_err(|e| DbError::from_rusqlite(e, "backfill_run_items_query"))?;

            for item in item_rows.flatten() {
                if first_item_key.is_none() {
                    first_item_key = Some(item.0);
                }
                body_parts.push(item.1);
                if !item.2.is_empty() {
                    body_parts.push(item.2);
                }
                if !item.3.is_empty() {
                    body_parts.push(item.3);
                }
                if item.4 != 0 {
                    any_truncated = true;
                }
            }
        }

        let occurred = finished_at.unwrap_or(created_at);
        upsert_projection(
            conn,
            &SanitizedProjection {
                source: ProjectionSource::GroupRuns,
                entity_key: run_id.clone(),
                project_id,
                tab_id: None,
                session_id: None,
                pane_id: None,
                exit_code: last_exit_code,
                source_truncated: any_truncated,
                fetched_at: None,
                title: SanitizedText::new(&group_name),
                body: SanitizedText::new(&body_parts.join(" ")),
                target: SearchTarget::GroupRun {
                    run_key: run_id,
                    item_key: first_item_key,
                    project_id,
                },
                occurred_at: Some(occurred),
                source_revision: version.to_string(),
            },
        )?;
        count += 1;
    }

    Ok(count)
}

#[allow(clippy::too_many_lines)]
fn backfill_github_issues_and_comments(conn: &Connection) -> DbResult<u64> {
    // Mapeia repo_key -> Vec<ProjectId> (ou fallback para o primeiro projeto se não houver vínculo em github_project_repos)
    let mut repo_projects: std::collections::HashMap<String, Vec<ProjectId>> =
        std::collections::HashMap::new();
    if table_exists(conn, "github_project_repos")? {
        let mut stmt = conn
            .prepare(
                "SELECT repo_key, project_id FROM github_project_repos WHERE repo_key IS NOT NULL;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_repos_prepare"))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_repos_query"))?;
        for (rk, pid_raw) in rows.flatten() {
            if let Ok(pid) = ProjectId::from_str(&pid_raw) {
                repo_projects.entry(rk).or_default().push(pid);
            }
        }
    }

    let fallback_project: Option<ProjectId> = if table_exists(conn, "projects")? {
        conn.query_row(
            "SELECT id FROM projects ORDER BY sort_order ASC, id ASC LIMIT 1;",
            [],
            |r| r.get::<_, String>(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_fallback_proj"))?
        .and_then(|s| ProjectId::from_str(&s).ok())
    } else {
        None
    };

    let issues: Vec<(
        String,
        i64,
        String,
        String,
        String,
        String,
        Option<String>,
        i64,
        i64,
    )> = {
        let mut stmt = conn
            .prepare(
                "SELECT repo_key, issue_number, title, body_sanitized, state, author_login,
                        plan_markdown, updated_at_ms, synced_at_ms
                 FROM github_issues;",
            )
            .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_issues_prepare"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            })
            .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_issues_query"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_issues_collect"))?
    };

    let mut count = 0u64;
    for (
        repo_key,
        issue_number,
        title,
        body_sanitized,
        state,
        author_login,
        plan_markdown,
        updated_at_ms,
        synced_at_ms,
    ) in issues
    {
        let Ok(number_u32) = u32::try_from(issue_number) else {
            continue;
        };
        let target_projects: Vec<ProjectId> = repo_projects
            .get(&repo_key)
            .cloned()
            .or_else(|| fallback_project.map(|p| vec![p]))
            .unwrap_or_default();

        let mut labels_str = String::new();
        if table_exists(conn, "github_issue_labels")? {
            let mut lbl_stmt = conn
                .prepare(
                    "SELECT label_name FROM github_issue_labels
                     WHERE repo_key = ?1 AND issue_number = ?2;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_labels_prepare"))?;
            let lbl_rows = lbl_stmt
                .query_map(params![&repo_key, issue_number], |r| r.get::<_, String>(0))
                .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_labels_query"))?;
            for lbl in lbl_rows.flatten() {
                labels_str.push(' ');
                labels_str.push_str(&lbl);
            }
        }

        let plan_part = plan_markdown.unwrap_or_default();
        let full_title = format!("#{number_u32} {title}");
        let full_body = format!("{body_sanitized} {plan_part} {state} {author_login}{labels_str}");

        for project_id in target_projects {
            let entity_key = format!("{project_id}:{repo_key}#{number_u32}");
            upsert_projection(
                conn,
                &SanitizedProjection {
                    source: ProjectionSource::Issues,
                    entity_key,
                    project_id: Some(project_id),
                    tab_id: None,
                    session_id: None,
                    pane_id: None,
                    exit_code: None,
                    source_truncated: false,
                    fetched_at: Some(synced_at_ms),
                    title: SanitizedText::new(&full_title),
                    body: SanitizedText::new(&full_body),
                    target: SearchTarget::Issue {
                        repo_key: repo_key.clone(),
                        number: number_u32,
                        comment_key: None,
                        project_id,
                    },
                    occurred_at: Some(updated_at_ms),
                    source_revision: updated_at_ms.to_string(),
                },
            )?;
            count += 1;
        }
    }

    if table_exists(conn, "github_issue_comments")? {
        let comments: Vec<(String, i64, i64, String, String, i64, i64, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT c.repo_key, c.issue_number, c.comment_id, c.author_login,
                            c.body_sanitized, c.updated_at_ms, c.synced_at_ms,
                            COALESCE(i.title, '')
                     FROM github_issue_comments c
                     LEFT JOIN github_issues i
                       ON i.repo_key = c.repo_key AND i.issue_number = c.issue_number;",
                )
                .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_comments_prepare"))?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get(2)?,
                        r.get(3)?,
                        r.get(4)?,
                        r.get(5)?,
                        r.get(6)?,
                        r.get(7)?,
                    ))
                })
                .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_comments_query"))?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| DbError::from_rusqlite(e, "backfill_gh_comments_collect"))?
        };

        for (
            repo_key,
            issue_number,
            comment_id,
            author_login,
            body_sanitized,
            updated_at_ms,
            synced_at_ms,
            issue_title,
        ) in comments
        {
            let Ok(number_u32) = u32::try_from(issue_number) else {
                continue;
            };
            let target_projects: Vec<ProjectId> = repo_projects
                .get(&repo_key)
                .cloned()
                .or_else(|| fallback_project.map(|p| vec![p]))
                .unwrap_or_default();

            let full_title = format!("#{number_u32} {issue_title} — @{author_login}");
            let comment_key = comment_id.to_string();

            for project_id in target_projects {
                let entity_key = format!("{project_id}:{repo_key}#{number_u32}:c{comment_id}");
                upsert_projection(
                    conn,
                    &SanitizedProjection {
                        source: ProjectionSource::Comments,
                        entity_key,
                        project_id: Some(project_id),
                        tab_id: None,
                        session_id: None,
                        pane_id: None,
                        exit_code: None,
                        source_truncated: false,
                        fetched_at: Some(synced_at_ms),
                        title: SanitizedText::new(&full_title),
                        body: SanitizedText::new(&body_sanitized),
                        target: SearchTarget::Issue {
                            repo_key: repo_key.clone(),
                            number: number_u32,
                            comment_key: Some(comment_key.clone()),
                            project_id,
                        },
                        occurred_at: Some(updated_at_ms),
                        source_revision: updated_at_ms.to_string(),
                    },
                )?;
                count += 1;
            }
        }
    }

    Ok(count)
}

/// Reconstrói um índice FTS5 validando que `fts_table` pertence estritamente à allowlist do `FtsRegistry`.
pub fn rebuild_search_fts(
    conn: &Connection,
    registry: &FtsRegistry,
    fts_table: &str,
) -> DbResult<()> {
    let spec = registry
        .specs()
        .iter()
        .find(|s| s.fts_table == fts_table)
        .ok_or_else(|| DbError::ConstraintViolation {
            constraint: format!("Tabela FTS5 '{fts_table}' não registrada no FtsRegistry"),
        })?;

    if table_exists(conn, spec.fts_table)? {
        conn.execute_batch(&spec.rebuild_sql())
            .map_err(|e| DbError::from_rusqlite(e, "rebuild_search_fts"))?;
    }
    Ok(())
}

fn table_exists(conn: &Connection, table_name: &str) -> DbResult<bool> {
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1);",
            [table_name],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "projection_table_exists"))?;
    Ok(exists != 0)
}

/// Sincroniza a projeção de um projeto caso a tabela `search_metadata` já esteja migrada.
pub fn sync_project_if_ready(conn: &Connection, project_id: ProjectId) -> DbResult<()> {
    if table_exists(conn, "search_metadata")? {
        sync_project_by_id(conn, project_id)?;
    }
    Ok(())
}

/// Sincroniza a projeção de um template de aba caso a tabela `search_metadata` já esteja migrada.
pub fn sync_template_if_ready(conn: &Connection, template_id: TemplateId) -> DbResult<()> {
    if table_exists(conn, "search_metadata")? {
        sync_template_by_id(conn, template_id)?;
    }
    Ok(())
}

/// Sincroniza a projeção de uma sessão caso a tabela `search_metadata` já esteja migrada.
pub fn sync_session_if_ready(conn: &Connection, session_id: SessionId) -> DbResult<()> {
    if table_exists(conn, "search_metadata")? {
        sync_session_by_id(conn, session_id)?;
    }
    Ok(())
}

/// Remove uma projeção caso a tabela `search_metadata` já esteja migrada.
pub fn delete_projection_if_ready(
    conn: &Connection,
    source: ProjectionSource,
    entity_key: &str,
) -> DbResult<()> {
    if table_exists(conn, "search_metadata")? {
        delete_projection(conn, source, entity_key)?;
    }
    Ok(())
}

/// Remove todas as projeções associadas a um projeto em `search_metadata` quando o projeto é excluído.
pub fn delete_project_projections_if_ready(
    conn: &Connection,
    project_id: ProjectId,
) -> DbResult<()> {
    if table_exists(conn, "search_metadata")? {
        let pid_str = project_id.to_string();
        conn.execute(
            "DELETE FROM search_metadata
             WHERE project_id = ?1
                OR (source = 'projects' AND entity_key = ?1);",
            [&pid_str],
        )
        .map_err(|e| DbError::from_rusqlite(e, "delete_project_projections"))?;
    }
    Ok(())
}
