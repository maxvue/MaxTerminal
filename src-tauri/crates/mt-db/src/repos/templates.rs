//! Repositório transacional da tabela `tab_templates` e contribuidor `DefaultTemplateContributor` (Fase 07).
//!
//! Garante que todo projeto possua **exatamente um template padrão** (`is_default = 1`) em todas
//! as mutações, bem como na inserção transacional de projetos via `ProjectInsertContributor`.

use std::collections::BTreeMap;
use std::str::FromStr;

use mt_core::ids::{ProjectId, TemplateId};
use mt_core::workspace::{
    EnvValue, EnvironmentKind, TabTemplate, TabTemplateInput, TemplateConfig, TemplateEnvironment,
    validate_template_input,
};
use rusqlite::{Connection, OptionalExtension, Row, Transaction, params};

use crate::Db;
use crate::error::{DbError, DbResult};
use crate::repos::projects::{
    CONSTRAINT_PROJECT_NOT_FOUND, ProjectInsertContributor, StoredProject,
};
use crate::sanitizer::SanitizedText;

/// Sentinela de constraint indicando template não encontrado.
pub const CONSTRAINT_TEMPLATE_NOT_FOUND: &str = "template.not_found";
/// Sentinela de constraint indicando que o template padrão não pode ser removido ou ficar ausente.
pub const CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED: &str = "template.default_required";
/// Sentinela de constraint indicando literal com segredo no comando ou variáveis de ambiente.
pub const CONSTRAINT_TEMPLATE_SECRET_LITERAL: &str = "template.secret_literal";
/// Sentinela de constraint indicando conflito de revisão otimista (`expected_revision`).
pub const CONSTRAINT_TEMPLATE_CONFLICT: &str = "template.conflict";
/// Sentinela de constraint indicando parâmetro inválido de template.
pub const CONSTRAINT_TEMPLATE_INVALID: &str = "template.invalid";

/// Contribuidor transacional interno que garante a criação do template padrão no mesmo `COMMIT`
/// de qualquer inserção em `projects` (avulso, importação de raiz ou aceite de descoberta).
#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultTemplateContributor;

impl ProjectInsertContributor for DefaultTemplateContributor {
    fn after_insert(&self, tx: &Transaction<'_>, project: &StoredProject) -> DbResult<()> {
        let _ = TemplatesRepo::ensure_default_tx(tx, project.id, project.created_at)?;
        Ok(())
    }
}

/// Alias compatível com `TemplateRepo` mencionado na especificação.
pub type TemplateRepo = TemplatesRepo;

/// Repositório tipado de persistência de Templates de Abas (`tab_templates`).
#[derive(Clone, Debug)]
pub struct TemplatesRepo {
    db: Db,
}

impl TemplatesRepo {
    /// Cria uma nova instância de [`TemplatesRepo`] acoplada à fachada [`Db`].
    #[must_use]
    pub const fn new(db: Db) -> Self {
        Self { db }
    }

    /// Primitivo transacional que garante que `project_id` possua exatamente um template padrão:
    /// - Se já houver `is_default = 1`, retorna seu [`TemplateId`] sem modificações.
    /// - Se não houver `is_default = 1`, mas existirem templates no projeto, elege o menor `(position, created_at, id)`.
    /// - Se não houver nenhum template no projeto, cria o template padrão `"Terminal"` (`mdi:console`, `shell`).
    pub fn ensure_default_tx(
        tx: &Transaction<'_>,
        project_id: ProjectId,
        now_ms: i64,
    ) -> DbResult<TemplateId> {
        let proj_str = project_id.to_string();

        if let Some(existing_default) = find_default_id_conn(tx, &proj_str)? {
            return Ok(existing_default);
        }

        // Verifica se existe algum template no projeto para eleger em ordem estável
        let candidate_id: Option<String> = tx
            .query_row(
                "SELECT id FROM tab_templates
                 WHERE project_id = ?1
                 ORDER BY position ASC, created_at ASC, id ASC
                 LIMIT 1;",
                [&proj_str],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| DbError::from_rusqlite(e, "templates_find_candidate_default"))?;

        if let Some(raw_id) = candidate_id {
            let parsed_id = parse_template_id(&raw_id)?;
            tx.execute(
                "UPDATE tab_templates SET is_default = 0 WHERE project_id = ?1 AND is_default = 1;",
                [&proj_str],
            )
            .map_err(|e| DbError::from_rusqlite(e, "templates_clear_default"))?;

            let updated = tx
                .execute(
                    "UPDATE tab_templates
                     SET is_default = 1, revision = revision + 1, updated_at = ?3
                     WHERE id = ?1 AND project_id = ?2;",
                    params![raw_id, proj_str, now_ms],
                )
                .map_err(|e| DbError::from_rusqlite(e, "templates_elect_default"))?;

            if updated != 1 {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED),
                });
            }
            return Ok(parsed_id);
        }

        // Cria o template padrão "Terminal"
        let new_id = TemplateId::new();
        let new_id_str = new_id.to_string();
        tx.execute(
            "INSERT INTO tab_templates (
                id, project_id, name, icon, cwd, initial_command,
                add_to_workspace, continuous, environment_kind, agent_id,
                env_json, is_default, position, revision, created_at, updated_at,
                connection_json
             ) VALUES (
                ?1, ?2, 'Terminal', 'mdi:console', NULL, NULL,
                0, 0, 'shell', NULL,
                '{}', 1, 0, 1, ?3, ?3,
                '{\"kind\":\"local\"}'
             );",
            params![new_id_str, proj_str, now_ms],
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_insert_default"))?;

        crate::search::projection::sync_template_if_ready(tx, new_id)?;
        Ok(new_id)
    }

    /// Garante que todos os projetos cadastrados possuam exatamente um template padrão.
    /// Retorna a quantidade de projetos que precisaram de reparo/criação.
    pub async fn ensure_all_projects_have_default(&self, now_ms: i64) -> DbResult<usize> {
        self.db
            .write_sized("templates_ensure_all_defaults", 1024, move |tx| {
                ensure_all_projects_have_default_tx(tx, now_ms)
            })
            .await
    }

    /// Lista todos os templates de um projeto ordenados por `(is_default DESC, position ASC, name ASC, id ASC)`.
    pub async fn list_by_project(&self, project_id: ProjectId) -> DbResult<Vec<TabTemplate>> {
        let proj_str = project_id.to_string();
        self.db
            .read(move |conn| list_by_project_conn(conn, &proj_str))
            .await
    }

    /// Lista todos os templates marcados com `add_to_workspace = 1` ordenados por
    /// `(projects.sort_order ASC, tab_templates.position ASC, tab_templates.id ASC)`.
    pub async fn list_workspace_templates(&self) -> DbResult<Vec<TabTemplate>> {
        self.db.read(list_workspace_templates_conn).await
    }

    /// Busca um template pelo seu [`TemplateId`].
    pub async fn get(&self, template_id: TemplateId) -> DbResult<Option<TabTemplate>> {
        let id_str = template_id.to_string();
        self.db
            .read(move |conn| get_template_by_id_conn(conn, &id_str))
            .await
    }

    /// Busca o template padrão (`is_default = 1`) de um projeto.
    pub async fn get_default(&self, project_id: ProjectId) -> DbResult<Option<TabTemplate>> {
        let proj_str = project_id.to_string();
        self.db
            .read(move |conn| get_default_template_conn(conn, &proj_str))
            .await
    }

    /// Cria um novo template para `project_id`, promovendo-o a padrão se `input.make_default == true`.
    pub async fn create(
        &self,
        project_id: ProjectId,
        input: TabTemplateInput,
        now_ms: i64,
    ) -> DbResult<TabTemplate> {
        let config = validate_and_check_secrets(&input)?;
        let make_default = input.make_default;

        self.db
            .write_sized("templates_create", 1024, move |tx| {
                create_template_tx(tx, project_id, &config, make_default, now_ms)
            })
            .await
    }

    /// Atualiza um template existente validando `expected_revision` e preservando a invariante de 1 padrão por projeto.
    pub async fn update(
        &self,
        template_id: TemplateId,
        expected_revision: i64,
        input: TabTemplateInput,
        now_ms: i64,
    ) -> DbResult<TabTemplate> {
        let config = validate_and_check_secrets(&input)?;
        let make_default = input.make_default;

        self.db
            .write_sized("templates_update", 1024, move |tx| {
                update_template_tx(
                    tx,
                    template_id,
                    expected_revision,
                    &config,
                    make_default,
                    now_ms,
                )
            })
            .await
    }

    /// Exclui um template não-padrão validando `expected_revision`.
    /// Recusa a exclusão do template padrão com `template.default_required`.
    pub async fn delete(
        &self,
        template_id: TemplateId,
        expected_revision: i64,
    ) -> DbResult<ProjectId> {
        self.db
            .write_sized("templates_delete", 512, move |tx| {
                delete_template_tx(tx, template_id, expected_revision)
            })
            .await
    }

    /// Promove atomicamente `template_id` a template padrão do seu projeto e retorna a lista atualizada.
    pub async fn set_default(
        &self,
        template_id: TemplateId,
        now_ms: i64,
    ) -> DbResult<Vec<TabTemplate>> {
        self.db
            .write_sized("templates_set_default", 512, move |tx| {
                set_default_tx(tx, template_id, now_ms)
            })
            .await
    }
}

/// Valida o `TabTemplateInput` e verifica que nenhum literal contém segredos detectados pelo sanitizador da Fase 03.
pub fn validate_and_check_secrets(input: &TabTemplateInput) -> DbResult<TemplateConfig> {
    let config = validate_template_input(input).map_err(|err| DbError::ConstraintViolation {
        constraint: format!("{}|{}", err.code.0, err.message),
    })?;

    let clean_name = SanitizedText::new(&config.name);
    if clean_name.as_str() != config.name {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_SECRET_LITERAL),
        });
    }

    if let Some(ref cwd) = config.cwd {
        let clean_cwd = SanitizedText::new(cwd);
        if clean_cwd.as_str() != cwd {
            return Err(DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_TEMPLATE_SECRET_LITERAL),
            });
        }
    }

    if let Some(ref cmd) = config.initial_command {
        let clean_cmd = SanitizedText::new(cmd);
        if clean_cmd.as_str() != cmd {
            return Err(DbError::ConstraintViolation {
                constraint: String::from(CONSTRAINT_TEMPLATE_SECRET_LITERAL),
            });
        }
    }

    for val in config.env_vars.values() {
        if let EnvValue::Literal { value } = val {
            let clean_val = SanitizedText::new(value);
            if clean_val.as_str() != value {
                return Err(DbError::ConstraintViolation {
                    constraint: String::from(CONSTRAINT_TEMPLATE_SECRET_LITERAL),
                });
            }
        }
    }

    Ok(config)
}

pub(crate) fn ensure_all_projects_have_default_tx(
    tx: &Transaction<'_>,
    now_ms: i64,
) -> DbResult<usize> {
    let projects: Vec<(ProjectId, i64)> = {
        let mut stmt = tx
            .prepare_cached("SELECT id, created_at FROM projects ORDER BY sort_order ASC, id ASC;")
            .map_err(|e| DbError::from_rusqlite(e, "templates_list_projects_for_default"))?;
        let rows = stmt
            .query_map([], |r| {
                let raw_id: String = r.get(0)?;
                let created_at: i64 = r.get(1)?;
                let pid = ProjectId::from_str(&raw_id).map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::from("ProjectId inválido"),
                    )
                })?;
                Ok((pid, created_at))
            })
            .map_err(|e| DbError::from_rusqlite(e, "templates_list_projects_for_default"))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| DbError::from_rusqlite(e, "templates_list_projects_for_default"))?
    };

    let mut repaired = 0usize;
    for (pid, created_at) in projects {
        let pid_str = pid.to_string();
        if find_default_id_conn(tx, &pid_str)?.is_none() {
            let ts = if created_at > 0 { created_at } else { now_ms };
            let _ = TemplatesRepo::ensure_default_tx(tx, pid, ts)?;
            repaired += 1;
        }
    }
    Ok(repaired)
}

fn create_template_tx(
    tx: &Transaction<'_>,
    project_id: ProjectId,
    config: &TemplateConfig,
    make_default: bool,
    now_ms: i64,
) -> DbResult<TabTemplate> {
    let proj_str = project_id.to_string();
    if !project_exists_conn(tx, &proj_str)? {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_PROJECT_NOT_FOUND),
        });
    }

    let has_default = find_default_id_conn(tx, &proj_str)?.is_some();
    let effective_default = make_default || !has_default;

    if effective_default {
        tx.execute(
            "UPDATE tab_templates SET is_default = 0 WHERE project_id = ?1 AND is_default = 1;",
            [&proj_str],
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_create_clear_default"))?;
    }

    let next_pos: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM tab_templates WHERE project_id = ?1;",
            [&proj_str],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_next_position"))?;

    let id = TemplateId::new();
    let id_str = id.to_string();
    let env_json =
        serde_json::to_string(&config.env_vars).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tab_templates.env_json"),
            reason: format!("env_vars: {e}"),
        })?;
    let connection_json =
        serde_json::to_string(&config.connection).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tab_templates.connection_json"),
            reason: format!("connection: {e}"),
        })?;

    tx.execute(
        "INSERT INTO tab_templates (
            id, project_id, name, icon, cwd, initial_command,
            add_to_workspace, continuous, environment_kind, agent_id,
            env_json, is_default, position, revision, created_at, updated_at,
            connection_json
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6,
            ?7, ?8, ?9, ?10,
            ?11, ?12, ?13, 1, ?14, ?14,
            ?15
         );",
        params![
            id_str,
            proj_str,
            config.name,
            config.icon,
            config.cwd,
            config.initial_command,
            i64::from(config.add_to_workspace),
            i64::from(config.continuous),
            config.environment.kind.as_db_str(),
            config.environment.agent_id,
            env_json,
            i64::from(effective_default),
            next_pos,
            now_ms,
            connection_json,
        ],
    )
    .map_err(|e| DbError::from_rusqlite(e, "templates_insert"))?;

    assert_single_default_tx(tx, &proj_str)?;
    crate::search::projection::sync_template_if_ready(tx, id)?;

    get_template_by_id_conn(tx, &id_str)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
    })
}

fn update_template_tx(
    tx: &Transaction<'_>,
    template_id: TemplateId,
    expected_revision: i64,
    config: &TemplateConfig,
    make_default: bool,
    now_ms: i64,
) -> DbResult<TabTemplate> {
    let id_str = template_id.to_string();
    let Some(existing) = get_template_by_id_conn(tx, &id_str)? else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
        });
    };

    if existing.revision != expected_revision {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_CONFLICT),
        });
    }

    let proj_str = existing.project_id.to_string();
    // Se já era default, permanece default mesmo que make_default venha false (spec §5.1)
    let effective_default = existing.is_default || make_default;

    if effective_default && !existing.is_default {
        tx.execute(
            "UPDATE tab_templates SET is_default = 0 WHERE project_id = ?1 AND is_default = 1;",
            [&proj_str],
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_update_clear_default"))?;
    }

    let env_json =
        serde_json::to_string(&config.env_vars).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tab_templates.env_json"),
            reason: format!("env_vars: {e}"),
        })?;
    let connection_json =
        serde_json::to_string(&config.connection).map_err(|e| DbError::SettingInvalidValue {
            key: String::from("tab_templates.connection_json"),
            reason: format!("connection: {e}"),
        })?;

    let updated = tx
        .execute(
            "UPDATE tab_templates
             SET name = ?2,
                 icon = ?3,
                 cwd = ?4,
                 initial_command = ?5,
                 add_to_workspace = ?6,
                 continuous = ?7,
                 environment_kind = ?8,
                 agent_id = ?9,
                 env_json = ?10,
                 is_default = ?11,
                 connection_json = ?12,
                 revision = revision + 1,
                 updated_at = ?13
             WHERE id = ?1 AND revision = ?14;",
            params![
                id_str,
                config.name,
                config.icon,
                config.cwd,
                config.initial_command,
                i64::from(config.add_to_workspace),
                i64::from(config.continuous),
                config.environment.kind.as_db_str(),
                config.environment.agent_id,
                env_json,
                i64::from(effective_default),
                connection_json,
                now_ms,
                expected_revision,
            ],
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_update"))?;

    if updated != 1 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_CONFLICT),
        });
    }

    assert_single_default_tx(tx, &proj_str)?;
    crate::search::projection::sync_template_if_ready(tx, template_id)?;

    get_template_by_id_conn(tx, &id_str)?.ok_or_else(|| DbError::ConstraintViolation {
        constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
    })
}

fn delete_template_tx(
    tx: &Transaction<'_>,
    template_id: TemplateId,
    expected_revision: i64,
) -> DbResult<ProjectId> {
    let id_str = template_id.to_string();
    let Some(existing) = get_template_by_id_conn(tx, &id_str)? else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
        });
    };

    if existing.revision != expected_revision {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_CONFLICT),
        });
    }

    if existing.is_default {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED),
        });
    }

    let proj_str = existing.project_id.to_string();
    let deleted = tx
        .execute(
            "DELETE FROM tab_templates WHERE id = ?1 AND revision = ?2;",
            params![id_str, expected_revision],
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_delete"))?;

    if deleted != 1 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_CONFLICT),
        });
    }

    assert_single_default_tx(tx, &proj_str)?;
    crate::search::projection::delete_projection_if_ready(
        tx,
        crate::search::projection::ProjectionSource::Templates,
        &id_str,
    )?;
    Ok(existing.project_id)
}

fn set_default_tx(
    tx: &Transaction<'_>,
    template_id: TemplateId,
    now_ms: i64,
) -> DbResult<Vec<TabTemplate>> {
    let id_str = template_id.to_string();
    let Some(existing) = get_template_by_id_conn(tx, &id_str)? else {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
        });
    };

    let proj_str = existing.project_id.to_string();

    // Duas instruções UPDATE separadas conforme spec §5.1 (evitando violação transitória do índice parcial)
    tx.execute(
        "UPDATE tab_templates
         SET is_default = 0, updated_at = ?2
         WHERE project_id = ?1 AND is_default = 1;",
        params![proj_str, now_ms],
    )
    .map_err(|e| DbError::from_rusqlite(e, "templates_set_default_clear"))?;

    let updated = tx
        .execute(
            "UPDATE tab_templates
             SET is_default = 1, revision = revision + 1, updated_at = ?3
             WHERE id = ?1 AND project_id = ?2;",
            params![id_str, proj_str, now_ms],
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_set_default_mark"))?;

    if updated != 1 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_NOT_FOUND),
        });
    }

    assert_single_default_tx(tx, &proj_str)?;
    list_by_project_conn(tx, &proj_str)
}

fn assert_single_default_tx(tx: &Transaction<'_>, project_id: &str) -> DbResult<()> {
    let default_count: i64 = tx
        .query_row(
            "SELECT COUNT(*) FROM tab_templates WHERE project_id = ?1 AND is_default = 1;",
            [project_id],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_check_default_count"))?;

    if default_count != 1 {
        return Err(DbError::ConstraintViolation {
            constraint: String::from(CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED),
        });
    }
    Ok(())
}

fn project_exists_conn(conn: &Connection, project_id: &str) -> DbResult<bool> {
    let exists: i64 = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM projects WHERE id = ?1);",
            [project_id],
            |r| r.get(0),
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_project_exists"))?;
    Ok(exists != 0)
}

pub(crate) fn find_default_id_conn(
    conn: &Connection,
    project_id: &str,
) -> DbResult<Option<TemplateId>> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT id FROM tab_templates WHERE project_id = ?1 AND is_default = 1 LIMIT 1;",
            [project_id],
            |r| r.get(0),
        )
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "templates_find_default"))?;
    raw.map(|s| parse_template_id(&s)).transpose()
}

pub(crate) fn get_default_template_conn(
    conn: &Connection,
    project_id: &str,
) -> DbResult<Option<TabTemplate>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, project_id, name, icon, cwd, initial_command,
                    add_to_workspace, continuous, environment_kind, agent_id,
                    env_json, is_default, position, revision, created_at, updated_at,
                    connection_json
             FROM tab_templates
             WHERE project_id = ?1 AND is_default = 1
             LIMIT 1;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_get_default"))?;
    stmt.query_row([project_id], map_template_row)
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "templates_get_default"))
}

pub(crate) fn get_template_by_id_conn(
    conn: &Connection,
    template_id: &str,
) -> DbResult<Option<TabTemplate>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, project_id, name, icon, cwd, initial_command,
                    add_to_workspace, continuous, environment_kind, agent_id,
                    env_json, is_default, position, revision, created_at, updated_at,
                    connection_json
             FROM tab_templates
             WHERE id = ?1;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_get_by_id"))?;
    stmt.query_row([template_id], map_template_row)
        .optional()
        .map_err(|e| DbError::from_rusqlite(e, "templates_get_by_id"))
}

fn list_by_project_conn(conn: &Connection, project_id: &str) -> DbResult<Vec<TabTemplate>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT id, project_id, name, icon, cwd, initial_command,
                    add_to_workspace, continuous, environment_kind, agent_id,
                    env_json, is_default, position, revision, created_at, updated_at,
                    connection_json
             FROM tab_templates
             WHERE project_id = ?1
             ORDER BY is_default DESC, position ASC, name COLLATE NOCASE ASC, id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_list_by_project"))?;
    let rows = stmt
        .query_map([project_id], map_template_row)
        .map_err(|e| DbError::from_rusqlite(e, "templates_list_by_project"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "templates_list_by_project"))
}

fn list_workspace_templates_conn(conn: &Connection) -> DbResult<Vec<TabTemplate>> {
    let mut stmt = conn
        .prepare_cached(
            "SELECT t.id, t.project_id, t.name, t.icon, t.cwd, t.initial_command,
                    t.add_to_workspace, t.continuous, t.environment_kind, t.agent_id,
                    t.env_json, t.is_default, t.position, t.revision, t.created_at, t.updated_at,
                    t.connection_json
             FROM tab_templates t
             INNER JOIN projects p ON p.id = t.project_id
             WHERE t.add_to_workspace = 1
             ORDER BY p.sort_order ASC, p.id ASC, t.position ASC, t.id ASC;",
        )
        .map_err(|e| DbError::from_rusqlite(e, "templates_list_workspace"))?;
    let rows = stmt
        .query_map([], map_template_row)
        .map_err(|e| DbError::from_rusqlite(e, "templates_list_workspace"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| DbError::from_rusqlite(e, "templates_list_workspace"))
}

fn parse_template_id(raw: &str) -> DbResult<TemplateId> {
    TemplateId::from_str(raw).map_err(|_| DbError::ConstraintViolation {
        constraint: format!("TemplateId inválido no banco: '{raw}'"),
    })
}

fn map_template_row(row: &Row<'_>) -> rusqlite::Result<TabTemplate> {
    let id_raw: String = row.get(0)?;
    let id = TemplateId::from_str(&id_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::from("TemplateId inválido"),
        )
    })?;

    let proj_raw: String = row.get(1)?;
    let project_id = ProjectId::from_str(&proj_raw).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            1,
            rusqlite::types::Type::Text,
            Box::from("ProjectId inválido"),
        )
    })?;

    let name: String = row.get(2)?;
    let icon: String = row.get(3)?;
    let cwd: Option<String> = row.get(4)?;
    let initial_command: Option<String> = row.get(5)?;
    let add_to_workspace = row.get::<_, i64>(6)? != 0;
    let continuous = row.get::<_, i64>(7)? != 0;
    let env_kind_raw: String = row.get(8)?;
    let kind = EnvironmentKind::from_db_str(&env_kind_raw).unwrap_or(EnvironmentKind::Shell);
    let agent_id: Option<String> = row.get(9)?;
    let env_json: String = row.get(10)?;
    let env_vars: BTreeMap<String, EnvValue> = serde_json::from_str(&env_json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, Box::new(e))
    })?;
    let is_default = row.get::<_, i64>(11)? != 0;
    let position: i64 = row.get(12)?;
    let revision: i64 = row.get(13)?;
    let created_at: i64 = row.get(14)?;
    let updated_at: i64 = row.get(15)?;
    let connection_json: Option<String> = row.get(16).ok();
    let connection = match connection_json {
        Some(ref s) if !s.trim().is_empty() => serde_json::from_str(s).unwrap_or_default(),
        _ => mt_core::workspace::TemplateConnection::Local,
    };

    Ok(TabTemplate {
        id,
        project_id,
        config: TemplateConfig {
            name,
            icon,
            cwd,
            initial_command,
            add_to_workspace,
            continuous,
            environment: TemplateEnvironment { kind, agent_id },
            env_vars,
            connection,
        },
        is_default,
        position,
        revision,
        created_at,
        updated_at,
    })
}
