//! Serviço de sincronização `MaxSkills` (`SkillsService`): targets CRUD, `plan` dry-run, `apply` atômico com backup e `remove`.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;

use mt_core::{AppPaths, MtError, MtResult, ProjectId};
use parking_lot::RwLock;
use rusqlite::{OptionalExtension, params};
use uuid::Uuid;

use crate::detect::current_unix_ms;
use crate::error;
use crate::ports::{ProjectRecord, WorkspacePort};
use crate::registry::AgentKind;
use crate::settings::SkillsSettings;
use crate::skills::catalog::{SKILL_MANIFEST_FILENAME, compute_skill_tree_hash, scan_catalog};
use crate::skills::conflict::{
    ManagedEntryRecord, classify_target_actions, compute_plan_fingerprint, select_skills,
};
use crate::skills::targets::{canonical_or_lexical_path, expand_tilde, resolve_target_dest_dir};
use crate::skills::{
    ConflictResolution, SkillCatalog, SkillCopyManifest, SkillEntry, SkillSelection,
    SkillSyncTarget, SyncAction, SyncMode, SyncPlan, SyncResult, SyncScope,
};

/// Tempo de vida máximo de um `SyncPlan` em memória (5 minutos).
pub const PLAN_TTL_MS: i64 = 5 * 60 * 1000;

struct CachedPlan {
    plan: SyncPlan,
    target: SkillSyncTarget,
    dest_dir: PathBuf,
    skills_by_name: BTreeMap<String, SkillEntry>,
    managed_by_name: BTreeMap<String, ManagedEntryRecord>,
    created_at_ms: i64,
}

/// Serviço de descoberta, planejamento e sincronização de skills (`MaxSkills`).
pub struct SkillsService {
    db: mt_db::Db,
    workspace: Arc<dyn WorkspacePort>,
    paths: AppPaths,
    home_dir: PathBuf,
    settings: Arc<RwLock<SkillsSettings>>,
    plans: RwLock<HashMap<String, CachedPlan>>,
}

impl SkillsService {
    /// Cria uma nova instância de `SkillsService`.
    #[must_use]
    pub fn new(
        db: mt_db::Db,
        workspace: Arc<dyn WorkspacePort>,
        paths: AppPaths,
        home_dir: PathBuf,
        settings: Arc<RwLock<SkillsSettings>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            db,
            workspace,
            paths,
            home_dir,
            settings,
            plans: RwLock::new(HashMap::new()),
        })
    }

    /// Retorna o caminho resolvido do repositório fonte (`~/GitHub/MaxSkills` por padrão).
    #[must_use]
    pub fn resolved_source_dir(&self) -> PathBuf {
        let raw = self.settings.read().source_dir.clone();
        expand_tilde(raw.trim(), &self.home_dir)
    }

    /// Atualiza as configurações de skills em memória e invalida planos em cache.
    pub fn update_settings(&self, new_settings: SkillsSettings) {
        *self.settings.write() = new_settings;
        self.invalidate_plans();
    }

    /// Invalida todos os planos dry-run armazenados em memória (ex.: após evento do watcher).
    pub fn invalidate_plans(&self) {
        self.plans.write().clear();
    }

    /// Escaneia o catálogo de skills na fonte configurada, calculando relevância para o projeto opcional.
    pub async fn scan(&self, project_id: Option<ProjectId>) -> MtResult<SkillCatalog> {
        let proj = if let Some(pid) = project_id {
            Some(self.workspace.project(pid).await?)
        } else {
            None
        };
        let source_dir = self.resolved_source_dir();
        tokio::task::spawn_blocking(move || scan_catalog(&source_dir, proj.as_ref()))
            .await
            .map_err(|e| error::skills_io(format!("Join error no scan de skills: {e}")))?
    }

    /// Lista os alvos de sincronização cadastrados (filtrando opcionalmente por projeto; inclui globais quando `project_id` é `None`).
    pub async fn list_targets(
        &self,
        project_id: Option<ProjectId>,
    ) -> MtResult<Vec<SkillSyncTarget>> {
        let pid_str = project_id.map(|p| p.to_string());
        let rows = self
            .db
            .read(move |conn| {
                let mut out = Vec::new();
                let sql = if pid_str.is_some() {
                    "SELECT id, agent_id, scope, project_id, mode, auto_sync, enabled, selection_json, dest_canonical, project_root, dest_override
                     FROM skill_sync_targets
                     WHERE project_id = ?1 OR scope = 'global'
                     ORDER BY id ASC;"
                } else {
                    "SELECT id, agent_id, scope, project_id, mode, auto_sync, enabled, selection_json, dest_canonical, project_root, dest_override
                     FROM skill_sync_targets
                     ORDER BY id ASC;"
                };
                let mut stmt = conn
                    .prepare(sql)
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_targets_list"))?;
                let mapper = |r: &rusqlite::Row<'_>| -> rusqlite::Result<SkillSyncTarget> {
                    let id: i64 = r.get(0)?;
                    let agent_str: String = r.get(1)?;
                    let scope_str: String = r.get(2)?;
                    let proj_opt: Option<String> = r.get(3)?;
                    let mode_str: String = r.get(4)?;
                    let auto_sync: i64 = r.get(5)?;
                    let enabled: i64 = r.get(6)?;
                    let sel_json: String = r.get(7)?;
                    let dest_canon: String = r.get(8)?;
                    let project_root: Option<String> = r.get(9)?;
                    let dest_override: Option<String> = r.get(10)?;

                    let agent = AgentKind::from_ref_str(&agent_str).unwrap_or(AgentKind::Claude);
                    let scope = SyncScope::from_db_str(&scope_str).unwrap_or(SyncScope::Project);
                    let mode = SyncMode::from_db_str(&mode_str).unwrap_or(SyncMode::Symlink);
                    let project_id = proj_opt.and_then(|s| ProjectId::from_str(&s).ok());
                    let selection: SkillSelection =
                        serde_json::from_str(&sel_json).unwrap_or(SkillSelection::Recommended);

                    Ok(SkillSyncTarget {
                        id: Some(id),
                        agent,
                        scope,
                        project_id,
                        mode,
                        auto_sync: auto_sync != 0,
                        enabled: enabled != 0,
                        selection,
                        project_root,
                        dest_override,
                        dest_dir: Some(dest_canon),
                    })
                };

                if let Some(ref pid) = pid_str {
                    let iter = stmt
                        .query_map(params![pid], mapper)
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_targets_list"))?;
                    for item in iter {
                        out.push(item.map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "skills_targets_list_row")
                        })?);
                    }
                } else {
                    let iter = stmt
                        .query_map([], mapper)
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_targets_list"))?;
                    for item in iter {
                        out.push(item.map_err(|e| {
                            mt_db::DbError::from_rusqlite(e, "skills_targets_list_row")
                        })?);
                    }
                }
                Ok(out)
            })
            .await
            .map_err(MtError::from)?;

        Ok(rows)
    }

    /// Cria ou atualiza um alvo de sincronização, validando escopo, symlink ancestral e unicidade de `dest_canonical`.
    pub async fn upsert_target(&self, mut target: SkillSyncTarget) -> MtResult<SkillSyncTarget> {
        let project: Option<ProjectRecord> = if let Some(pid) = target.project_id {
            Some(self.workspace.project(pid).await?)
        } else {
            None
        };

        let (dest_dir, dest_canonical) = resolve_target_dest_dir(
            &target,
            project.as_ref(),
            &self.home_dir,
            &self
                .paths
                .config_dir
                .parent()
                .unwrap_or(&self.paths.config_dir),
        )?;

        let sel_json = serde_json::to_string(&target.selection)
            .map_err(|e| error::skills_target_invalid(format!("Seleção inválida: {e}")))?;
        let now_ms = current_unix_ms();
        let target_clone = target.clone();
        let dest_canon_clone = dest_canonical.clone();

        let id_res: Result<i64, String> = self
            .db
            .write(move |conn| {
                // Verifica se outro alvo já usa o mesmo dest_canonical
                let existing_id: Option<i64> = conn
                    .query_row(
                        "SELECT id FROM skill_sync_targets WHERE dest_canonical = ?1;",
                        params![&dest_canon_clone],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_target_check_dup"))?;

                if let Some(eid) = existing_id
                    && target_clone.id != Some(eid)
                {
                    return Ok(Err(format!(
                        "Já existe um alvo de sincronização configurado para o destino {dest_canon_clone}"
                    )));
                }

                let pid_str = target_clone.project_id.map(|p| p.to_string());
                if let Some(id) = target_clone.id {
                    conn.execute(
                        "UPDATE skill_sync_targets
                         SET agent_id = ?1, scope = ?2, project_id = ?3, mode = ?4,
                             auto_sync = ?5, enabled = ?6, selection_json = ?7,
                             dest_canonical = ?8, project_root = ?9, dest_override = ?10
                         WHERE id = ?11;",
                        params![
                            target_clone.agent.as_ref_id(),
                            target_clone.scope.as_db_str(),
                            pid_str,
                            target_clone.mode.as_db_str(),
                            i64::from(target_clone.auto_sync),
                            i64::from(target_clone.enabled),
                            sel_json,
                            dest_canon_clone,
                            target_clone.project_root,
                            target_clone.dest_override,
                            id,
                        ],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_target_update"))?;
                    Ok(Ok(id))
                } else {
                    conn.execute(
                        "INSERT INTO skill_sync_targets
                            (agent_id, scope, project_id, mode, auto_sync, enabled, selection_json, dest_canonical, project_root, dest_override, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11);",
                        params![
                            target_clone.agent.as_ref_id(),
                            target_clone.scope.as_db_str(),
                            pid_str,
                            target_clone.mode.as_db_str(),
                            i64::from(target_clone.auto_sync),
                            i64::from(target_clone.enabled),
                            sel_json,
                            dest_canon_clone,
                            target_clone.project_root,
                            target_clone.dest_override,
                            now_ms,
                        ],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_target_insert"))?;
                    Ok(Ok(conn.last_insert_rowid()))
                }
            })
            .await
            .map_err(MtError::from)?;

        let id = id_res.map_err(error::skills_target_invalid)?;

        self.invalidate_plans();
        target.id = Some(id);
        target.dest_dir = Some(dest_dir.to_string_lossy().to_string());
        Ok(target)
    }

    /// Remove um alvo de sincronização. Se `remove_managed == true`, remove do disco apenas as entradas comprovadamente gerenciadas e não modificadas.
    pub async fn remove_target(&self, target_id: i64, remove_managed: bool) -> MtResult<u32> {
        let managed = self.load_managed_entries(target_id).await?;
        let mut removed_count = 0u32;

        if remove_managed {
            for entry in managed.values() {
                let p = PathBuf::from(&entry.dest_path);
                let Ok(meta) = std::fs::symlink_metadata(&p) else {
                    continue;
                };
                if meta.file_type().is_symlink() {
                    if std::fs::remove_file(&p).is_ok() {
                        removed_count += 1;
                    }
                } else if meta.file_type().is_dir()
                    && let Ok(actual_hash) = compute_skill_tree_hash(&p)
                    && actual_hash == entry.synced_hash
                {
                    let _ = self.backup_item(target_id, &entry.skill_name, &p);
                    if std::fs::remove_dir_all(&p).is_ok() {
                        removed_count += 1;
                    }
                }
            }
        }

        self.db
            .write(move |conn| {
                conn.execute(
                    "DELETE FROM skill_sync_targets WHERE id = ?1;",
                    params![target_id],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_target_remove"))?;
                Ok(())
            })
            .await
            .map_err(MtError::from)?;

        self.invalidate_plans();
        Ok(removed_count)
    }

    /// Gera um plano dry-run (`SyncPlan`) sem escrever nada no disco (`CA-27`).
    pub async fn plan(&self, target_id: i64) -> MtResult<SyncPlan> {
        let target = self.get_target(target_id).await?;
        let project = if let Some(pid) = target.project_id {
            Some(self.workspace.project(pid).await?)
        } else {
            None
        };

        let (dest_dir, _) = resolve_target_dest_dir(
            &target,
            project.as_ref(),
            &self.home_dir,
            self.paths
                .config_dir
                .parent()
                .unwrap_or(&self.paths.config_dir),
        )?;

        let catalog = self.scan(target.project_id).await?;
        let managed = self.load_managed_entries(target_id).await?;

        let selected = select_skills(&catalog.skills, &target.selection);
        let actions = classify_target_actions(&dest_dir, target.mode, &selected, &managed);
        let fingerprint = compute_plan_fingerprint(&dest_dir, target.mode, &selected, &actions);

        let mut warnings = Vec::new();
        if selected.is_empty() {
            warnings.push("Nenhuma skill selecionada para este alvo.".to_string());
        }

        let plan_id = Uuid::now_v7().to_string();
        let plan = SyncPlan {
            plan_id: plan_id.clone(),
            target_id,
            dest_dir: dest_dir.to_string_lossy().to_string(),
            actions,
            warnings,
            fingerprint,
        };

        let skills_by_name = catalog
            .skills
            .into_iter()
            .map(|s| (s.name.clone(), s))
            .collect();

        self.plans.write().insert(
            plan_id,
            CachedPlan {
                plan: plan.clone(),
                target,
                dest_dir,
                skills_by_name,
                managed_by_name: managed,
                created_at_ms: current_unix_ms(),
            },
        );

        Ok(plan)
    }

    /// Aplica um plano previamente calculado por `plan`, validando o `fingerprint` (`skills.plan_stale` se o disco mudou).
    #[allow(clippy::too_many_lines)]
    pub async fn apply(
        &self,
        plan_id: &str,
        resolutions: BTreeMap<String, ConflictResolution>,
    ) -> MtResult<SyncResult> {
        let cached = self
            .plans
            .write()
            .remove(plan_id)
            .ok_or_else(error::skills_plan_stale)?;

        let now_ms = current_unix_ms();
        if now_ms.saturating_sub(cached.created_at_ms) > PLAN_TTL_MS {
            return Err(error::skills_plan_stale());
        }

        // Revalida o fingerprint do destino e da fonte antes de escrever (CA-35)
        let all_skills: Vec<SkillEntry> = cached.skills_by_name.values().cloned().collect();
        let selected_refs = select_skills(&all_skills, &cached.target.selection);
        for sk in &selected_refs {
            let current_hash = compute_skill_tree_hash(Path::new(&sk.source_dir))?;
            if current_hash != sk.hash {
                return Err(error::skills_plan_stale());
            }
        }
        let current_actions = classify_target_actions(
            &cached.dest_dir,
            cached.target.mode,
            &selected_refs,
            &cached.managed_by_name,
        );
        let current_fp = compute_plan_fingerprint(
            &cached.dest_dir,
            cached.target.mode,
            &selected_refs,
            &current_actions,
        );
        if current_fp != cached.plan.fingerprint {
            return Err(error::skills_plan_stale());
        }

        std::fs::create_dir_all(&cached.dest_dir).map_err(|e| {
            error::skills_io(format!(
                "Falha ao criar diretório de destino {}: {e}",
                cached.dest_dir.display()
            ))
        })?;

        let target_id = cached.plan.target_id;
        let mode = cached.target.mode;
        let mut applied = 0u32;
        let mut skipped = 0u32;
        let mut backups = Vec::new();
        let mut errors = Vec::new();

        for action in &cached.plan.actions {
            match action {
                SyncAction::Unchanged { .. } => {
                    skipped += 1;
                }
                SyncAction::Create { skill, .. } | SyncAction::Update { skill, .. } => {
                    let Some(skill_entry) = cached.skills_by_name.get(skill) else {
                        errors.push(format!("Skill '{skill}' ausente no catálogo"));
                        continue;
                    };
                    let dest_path = cached.dest_dir.join(skill);
                    match self.install_skill_atomic(
                        target_id,
                        skill_entry,
                        &cached.dest_dir,
                        &dest_path,
                        mode,
                        now_ms,
                    ) {
                        Ok(synced_hash) => {
                            self.record_managed_entry(
                                target_id,
                                skill_entry,
                                &dest_path,
                                mode,
                                &synced_hash,
                                now_ms,
                            )
                            .await?;
                            applied += 1;
                        }
                        Err(e) => errors.push(e.message.to_string()),
                    }
                }
                SyncAction::Prune { skill, .. } => {
                    let dest_path = cached.dest_dir.join(skill);
                    if let Ok(meta) = std::fs::symlink_metadata(&dest_path) {
                        if meta.file_type().is_dir() {
                            match self.backup_item(target_id, skill, &dest_path) {
                                Ok(bpath) => backups.push(bpath),
                                Err(e) => {
                                    errors.push(e.message.to_string());
                                    continue;
                                }
                            }
                            if let Err(e) = std::fs::remove_dir_all(&dest_path) {
                                errors
                                    .push(format!("Falha ao remover {}: {e}", dest_path.display()));
                                continue;
                            }
                        } else if let Err(e) = std::fs::remove_file(&dest_path) {
                            errors.push(format!(
                                "Falha ao remover symlink {}: {e}",
                                dest_path.display()
                            ));
                            continue;
                        }
                    }
                    self.delete_managed_entry(target_id, skill).await?;
                    applied += 1;
                }
                SyncAction::Conflict { skill, .. } => {
                    let resolution = resolutions
                        .get(skill)
                        .copied()
                        .unwrap_or(ConflictResolution::Keep);
                    match resolution {
                        ConflictResolution::Keep => {
                            skipped += 1;
                        }
                        ConflictResolution::Overwrite => {
                            let dest_path = cached.dest_dir.join(skill);
                            match self.backup_item(target_id, skill, &dest_path) {
                                Ok(bpath) => backups.push(bpath),
                                Err(e) => {
                                    errors.push(e.message.to_string());
                                    continue;
                                }
                            }
                            if let Some(skill_entry) = cached.skills_by_name.get(skill) {
                                match self.install_skill_atomic(
                                    target_id,
                                    skill_entry,
                                    &cached.dest_dir,
                                    &dest_path,
                                    mode,
                                    now_ms,
                                ) {
                                    Ok(synced_hash) => {
                                        self.record_managed_entry(
                                            target_id,
                                            skill_entry,
                                            &dest_path,
                                            mode,
                                            &synced_hash,
                                            now_ms,
                                        )
                                        .await?;
                                        applied += 1;
                                    }
                                    Err(e) => errors.push(e.message.to_string()),
                                }
                            } else {
                                // Conflito em item de Prune com Overwrite -> remove após backup
                                let _ = remove_existing_path(&dest_path);
                                self.delete_managed_entry(target_id, skill).await?;
                                applied += 1;
                            }
                        }
                        ConflictResolution::Adopt => {
                            let Some(skill_entry) = cached.skills_by_name.get(skill) else {
                                errors.push(format!(
                                    "Não é possível adotar '{skill}' pois não faz parte da seleção ativa"
                                ));
                                continue;
                            };
                            let dest_path = cached.dest_dir.join(skill);
                            match self.verify_adoptable(skill_entry, &cached.dest_dir, &dest_path) {
                                Ok((adopted_mode, synced_hash)) => {
                                    self.record_managed_entry(
                                        target_id,
                                        skill_entry,
                                        &dest_path,
                                        adopted_mode,
                                        &synced_hash,
                                        now_ms,
                                    )
                                    .await?;
                                    applied += 1;
                                }
                                Err(e) => errors.push(e.message.to_string()),
                            }
                        }
                    }
                }
            }
        }

        // Atualiza last_synced_at se ao menos uma ação foi aplicada sem erros fatais
        if errors.is_empty() {
            self.db
                .write(move |conn| {
                    conn.execute(
                        "UPDATE skill_sync_targets SET last_synced_at = ?1 WHERE id = ?2;",
                        params![now_ms, target_id],
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_target_touch_sync"))?;
                    Ok(())
                })
                .await
                .map_err(MtError::from)?;
        }

        Ok(SyncResult {
            target_id,
            applied,
            skipped,
            backups,
            errors,
        })
    }

    /// Executa auto-sync para todos os alvos com `enabled = true` e `auto_sync = true`, aplicando apenas ações sem conflito (`CA-30`).
    pub async fn run_auto_sync_all(&self) -> Vec<SyncResult> {
        let Ok(targets) = self.list_targets(None).await else {
            return Vec::new();
        };
        let mut results = Vec::new();
        for t in targets {
            if !t.enabled || !t.auto_sync {
                continue;
            }
            let Some(tid) = t.id else {
                continue;
            };
            let Ok(plan) = self.plan(tid).await else {
                continue;
            };
            let has_work = plan.actions.iter().any(|a| {
                matches!(
                    a,
                    SyncAction::Create { .. }
                        | SyncAction::Update { .. }
                        | SyncAction::Prune { .. }
                )
            });
            if !has_work {
                continue;
            }
            if let Ok(res) = self.apply(&plan.plan_id, BTreeMap::new()).await {
                results.push(res);
            }
        }
        results
    }

    fn install_skill_atomic(
        &self,
        _target_id: i64,
        skill: &SkillEntry,
        dest_dir: &Path,
        final_dest: &Path,
        mode: SyncMode,
        now_ms: i64,
    ) -> MtResult<String> {
        let op_id = Uuid::now_v7().to_string();
        let staging_name = format!(".staging-{}-{op_id}", skill.name);
        let staging_path = dest_dir.join(staging_name);
        let _ = remove_existing_path(&staging_path);

        let source_path = Path::new(&skill.source_dir);
        match mode {
            SyncMode::Symlink => {
                // Cria symlink temporário apontando para o caminho absoluto da fonte (NUNCA escreve .maxterminal-skill.json)
                std::os::unix::fs::symlink(source_path, &staging_path).map_err(|e| {
                    error::skills_io(format!(
                        "Falha ao criar symlink staging {}: {e}",
                        staging_path.display()
                    ))
                })?;
                if final_dest
                    .symlink_metadata()
                    .map(|m| m.file_type().is_dir())
                    .unwrap_or(false)
                {
                    std::fs::remove_dir_all(final_dest).map_err(|e| {
                        error::skills_io(format!(
                            "Falha ao remover diretório anterior {}: {e}",
                            final_dest.display()
                        ))
                    })?;
                }
                std::fs::rename(&staging_path, final_dest).map_err(|e| {
                    let _ = remove_existing_path(&staging_path);
                    error::skills_io(format!(
                        "Falha ao renomear symlink para {}: {e}",
                        final_dest.display()
                    ))
                })?;
                Ok(skill.hash.clone())
            }
            SyncMode::Copy => {
                copy_dir_recursive(source_path, &staging_path)?;
                let verified_hash = compute_skill_tree_hash(&staging_path)?;
                if verified_hash != skill.hash {
                    let _ = remove_existing_path(&staging_path);
                    return Err(error::skills_io(format!(
                        "Hash da cópia em staging diverge da fonte para '{}'",
                        skill.name
                    )));
                }
                let manifest = SkillCopyManifest {
                    version: 1,
                    skill_name: skill.name.clone(),
                    source_path: skill.source_dir.clone(),
                    source_hash: skill.hash.clone(),
                    synced_hash: verified_hash.clone(),
                    operation_id: op_id,
                    synced_at_ms: now_ms,
                };
                let manifest_bytes = serde_json::to_vec_pretty(&manifest)
                    .map_err(|e| error::skills_io(format!("Erro ao serializar manifesto: {e}")))?;
                std::fs::write(staging_path.join(SKILL_MANIFEST_FILENAME), manifest_bytes)
                    .map_err(|e| error::skills_io(format!("Erro ao gravar manifesto: {e}")))?;

                let _ = remove_existing_path(final_dest);
                std::fs::rename(&staging_path, final_dest).map_err(|e| {
                    let _ = remove_existing_path(&staging_path);
                    error::skills_io(format!(
                        "Falha ao renomear cópia staging para {}: {e}",
                        final_dest.display()
                    ))
                })?;
                Ok(verified_hash)
            }
        }
    }

    fn verify_adoptable(
        &self,
        skill: &SkillEntry,
        dest_dir: &Path,
        dest_path: &Path,
    ) -> MtResult<(SyncMode, String)> {
        let meta = std::fs::symlink_metadata(dest_path).map_err(|e| {
            error::skills_io(format!(
                "Destino inexistente para adoção {}: {e}",
                dest_path.display()
            ))
        })?;
        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(dest_path)
                .map_err(|e| error::skills_io(format!("Erro ao ler symlink: {e}")))?;
            let resolved = if target.is_absolute() {
                canonical_or_lexical_path(&target)
            } else {
                canonical_or_lexical_path(&dest_dir.join(&target))
            };
            let canonical_source = canonical_or_lexical_path(Path::new(&skill.source_dir));
            if resolved != canonical_source {
                return Err(error::skills_io(format!(
                    "Não é possível adotar '{}': symlink aponta para destino divergente da fonte",
                    skill.name
                )));
            }
            Ok((SyncMode::Symlink, skill.hash.clone()))
        } else if meta.file_type().is_dir() {
            let actual_hash = compute_skill_tree_hash(dest_path)?;
            if actual_hash != skill.hash {
                return Err(error::skills_io(format!(
                    "Não é possível adotar '{}': conteúdo local difere da fonte MaxSkills",
                    skill.name
                )));
            }
            Ok((SyncMode::Copy, actual_hash))
        } else {
            Err(error::skills_io(format!(
                "Tipo de arquivo inválido para adoção em {}",
                dest_path.display()
            )))
        }
    }

    fn backup_item(&self, target_id: i64, skill_name: &str, path: &Path) -> MtResult<String> {
        let backup_root = self.paths.state_dir.join("skills-backup");
        std::fs::create_dir_all(&backup_root).map_err(|e| {
            error::skills_io(format!(
                "Falha ao criar diretório de backup {}: {e}",
                backup_root.display()
            ))
        })?;
        let ts = current_unix_ms();
        let backup_dest = backup_root.join(format!("{ts}-{target_id}-{skill_name}"));
        let meta = std::fs::symlink_metadata(path).map_err(|e| {
            error::skills_io(format!(
                "Falha ao inspecionar item para backup {}: {e}",
                path.display()
            ))
        })?;

        if meta.file_type().is_symlink() {
            let target = std::fs::read_link(path)
                .map_err(|e| error::skills_io(format!("Falha ao ler symlink para backup: {e}")))?;
            std::os::unix::fs::symlink(&target, &backup_dest)
                .map_err(|e| error::skills_io(format!("Falha ao salvar symlink de backup: {e}")))?;
        } else if meta.file_type().is_dir() {
            copy_dir_recursive(path, &backup_dest)?;
        } else {
            std::fs::copy(path, &backup_dest).map_err(|e| {
                error::skills_io(format!("Falha ao copiar arquivo para backup: {e}"))
            })?;
        }

        Ok(backup_dest.to_string_lossy().to_string())
    }

    async fn get_target(&self, target_id: i64) -> MtResult<SkillSyncTarget> {
        let all = self.list_targets(None).await?;
        all.into_iter()
            .find(|t| t.id == Some(target_id))
            .ok_or_else(|| {
                error::skills_target_invalid(format!("Alvo de skills #{target_id} não encontrado"))
            })
    }

    async fn load_managed_entries(
        &self,
        target_id: i64,
    ) -> MtResult<BTreeMap<String, ManagedEntryRecord>> {
        self.db
            .read(move |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT skill_name, dest_path, mode, source_hash, source_path, synced_hash
                         FROM skill_sync_entries
                         WHERE target_id = ?1;",
                    )
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_entries_load"))?;
                let iter = stmt
                    .query_map(params![target_id], |r| {
                        let mode_str: String = r.get(2)?;
                        Ok(ManagedEntryRecord {
                            skill_name: r.get(0)?,
                            dest_path: r.get(1)?,
                            mode: SyncMode::from_db_str(&mode_str).unwrap_or(SyncMode::Symlink),
                            source_hash: r.get(3)?,
                            source_path: r.get(4)?,
                            synced_hash: r.get(5)?,
                        })
                    })
                    .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_entries_load"))?;
                let mut map = BTreeMap::new();
                for item in iter {
                    let rec = item
                        .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_entries_load_row"))?;
                    map.insert(rec.skill_name.clone(), rec);
                }
                Ok(map)
            })
            .await
            .map_err(MtError::from)
    }

    async fn record_managed_entry(
        &self,
        target_id: i64,
        skill: &SkillEntry,
        dest_path: &Path,
        mode: SyncMode,
        synced_hash: &str,
        now_ms: i64,
    ) -> MtResult<()> {
        let skill_name = skill.name.clone();
        let dest_str = dest_path.to_string_lossy().to_string();
        let mode_str = mode.as_db_str().to_string();
        let source_hash = skill.hash.clone();
        let source_path = skill.source_dir.clone();
        let synced_hash = synced_hash.to_string();

        self.db
            .write(move |conn| {
                conn.execute(
                    "INSERT INTO skill_sync_entries
                        (target_id, skill_name, dest_path, mode, source_hash, source_path, synced_hash, synced_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(target_id, skill_name) DO UPDATE SET
                        dest_path = excluded.dest_path,
                        mode = excluded.mode,
                        source_hash = excluded.source_hash,
                        source_path = excluded.source_path,
                        synced_hash = excluded.synced_hash,
                        synced_at = excluded.synced_at;",
                    params![
                        target_id,
                        skill_name,
                        dest_str,
                        mode_str,
                        source_hash,
                        source_path,
                        synced_hash,
                        now_ms,
                    ],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_entry_upsert"))?;
                Ok(())
            })
            .await
            .map_err(MtError::from)
    }

    async fn delete_managed_entry(&self, target_id: i64, skill_name: &str) -> MtResult<()> {
        let name = skill_name.to_string();
        self.db
            .write(move |conn| {
                conn.execute(
                    "DELETE FROM skill_sync_entries WHERE target_id = ?1 AND skill_name = ?2;",
                    params![target_id, name],
                )
                .map_err(|e| mt_db::DbError::from_rusqlite(e, "skills_entry_delete"))?;
                Ok(())
            })
            .await
            .map_err(MtError::from)
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> MtResult<()> {
    std::fs::create_dir_all(dst).map_err(|e| {
        error::skills_io(format!("Falha ao criar diretório {}: {e}", dst.display()))
    })?;
    let rd = std::fs::read_dir(src)
        .map_err(|e| error::skills_io(format!("Falha ao ler diretório {}: {e}", src.display())))?;
    for entry_res in rd {
        let entry = entry_res.map_err(|e| error::skills_io(format!("Erro ao ler entrada: {e}")))?;
        let fname = entry.file_name();
        let fname_str = fname.to_string_lossy();
        if fname_str == SKILL_MANIFEST_FILENAME || fname_str == ".DS_Store" {
            continue;
        }
        let spath = entry.path();
        let dpath = dst.join(&fname);
        let meta = std::fs::symlink_metadata(&spath).map_err(|e| {
            error::skills_io(format!("Erro ao ler metadados de {}: {e}", spath.display()))
        })?;
        let ftype = meta.file_type();
        if ftype.is_dir() {
            copy_dir_recursive(&spath, &dpath)?;
        } else if ftype.is_file() {
            std::fs::copy(&spath, &dpath).map_err(|e| {
                error::skills_io(format!(
                    "Falha ao copiar {} para {}: {e}",
                    spath.display(),
                    dpath.display()
                ))
            })?;
            std::fs::set_permissions(&dpath, meta.permissions()).map_err(|e| {
                error::skills_io(format!(
                    "Falha ao preservar permissões em {}: {e}",
                    dpath.display()
                ))
            })?;
        } else if ftype.is_symlink() {
            let link_target = std::fs::read_link(&spath).map_err(|e| {
                error::skills_io(format!("Falha ao ler symlink {}: {e}", spath.display()))
            })?;
            std::os::unix::fs::symlink(&link_target, &dpath).map_err(|e| {
                error::skills_io(format!("Falha ao copiar symlink {}: {e}", dpath.display()))
            })?;
        }
    }
    Ok(())
}

fn remove_existing_path(path: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(path)?;
    if meta.file_type().is_dir() {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    }
}
