//! Fachada central de orquestração (`CommandGroupService`) da Fase 15 — Grupos de Comandos (`>>`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use mt_core::{CommandGroupId, MtResult, ProjectId};
use tokio::sync::Mutex;

use crate::groups::compose::SequentialComposer;
use crate::groups::events::{
    GroupChangeOperation, GroupChangedEvent, GroupEventSink, GroupRunEvent, OutputAttachReceipt,
    OutputFrameSubscriber,
};
use crate::groups::model::{
    CommandGroup, CommandGroupCatalog, CommandGroupInput, DeleteGroupReceipt, ExecutionMode,
    GroupErrorCode, GroupRun, GroupRunId, GroupRunItemId, GroupRunItemSnapshot, GroupRunListPage,
    GroupRunRequest, GroupRunSnapshot, GroupScope, OutputSubscriptionId, ReorderGroupEntry,
    ShellDialect,
};
use crate::groups::multi_tab::MultiTabRunner;
use crate::groups::plan::MultiTabPlanner;
use crate::groups::ports::{
    GroupAiPort, GroupPtyPort, GroupShellPort, GroupTabPort, PaneExecutionLease,
    RecurringCommandsSource,
};
use crate::groups::repo::{AcceptRunOutcome, CommandGroupRepo, CreateRunRecord};
use crate::groups::resolve::{CwdResolver, compute_request_hash, detect_shell_dialect};
use crate::groups::runner::ParallelRunner;
use crate::groups::sequential::SequentialRunner;
use crate::groups::suggest::{
    GroupSuggestRequest, GroupSuggestResponse, GroupSuggester, RecurringCommandsResponse,
};

/// Janela padrão de retenção de execuções finalizadas (30 dias em milissegundos).
pub const DEFAULT_RUN_RETENTION_MS: i64 = 30 * 24 * 60 * 60 * 1000;

/// Fachada central de orquestração de Grupos de Comandos.
pub struct CommandGroupService {
    repo: Arc<dyn CommandGroupRepo>,
    pty_port: Arc<dyn GroupPtyPort>,
    shell_port: Arc<dyn GroupShellPort>,
    tab_port: Arc<dyn GroupTabPort>,
    events: Arc<dyn GroupEventSink>,
    suggester: GroupSuggester,
    sequential_runner: Arc<SequentialRunner>,
    parallel_runner: Arc<ParallelRunner>,
    multi_tab_runner: Arc<MultiTabRunner>,
    accept_mutex: Mutex<()>,
}

impl CommandGroupService {
    /// Cria uma nova instância compartilhada de `CommandGroupService`.
    #[must_use]
    pub fn new(
        repo: Arc<dyn CommandGroupRepo>,
        pty_port: Arc<dyn GroupPtyPort>,
        shell_port: Arc<dyn GroupShellPort>,
        tab_port: Arc<dyn GroupTabPort>,
        ai_port: Arc<dyn GroupAiPort>,
        recurring_source: Option<Arc<dyn RecurringCommandsSource>>,
        events: Arc<dyn GroupEventSink>,
    ) -> Arc<Self> {
        let parallel_runner = ParallelRunner::new(
            Arc::clone(&pty_port),
            Arc::clone(&repo),
            Arc::clone(&events),
        );
        let sequential_runner = SequentialRunner::with_sink_registry(
            Arc::clone(&pty_port),
            Arc::clone(&shell_port),
            Arc::clone(&repo),
            Arc::clone(&events),
            Arc::clone(&parallel_runner),
        );
        let multi_tab_runner = MultiTabRunner::with_sink_registry(
            Arc::clone(&tab_port),
            Arc::clone(&pty_port),
            Arc::clone(&shell_port),
            Arc::clone(&repo),
            Arc::clone(&events),
            Arc::clone(&parallel_runner),
        );
        let suggester = GroupSuggester::new(
            ai_port,
            Arc::clone(&tab_port),
            Arc::clone(&shell_port),
            recurring_source,
        );

        Arc::new(Self {
            repo,
            pty_port,
            shell_port,
            tab_port,
            events,
            suggester,
            sequential_runner,
            parallel_runner,
            multi_tab_runner,
            accept_mutex: Mutex::new(()),
        })
    }

    /// Lista os grupos locais do projeto (quando informado) e os grupos globais.
    pub async fn list(&self, project_id: Option<ProjectId>) -> MtResult<CommandGroupCatalog> {
        self.repo.list(project_id.as_ref())
    }

    /// Obtém os detalhes de um grupo pelo seu identificador.
    pub async fn get(&self, group_id: CommandGroupId) -> MtResult<CommandGroup> {
        self.repo.get(&group_id)
    }

    /// Cria um novo grupo (validando `cwd` contra a raiz do projeto quando `Local`) e emite `command-group://changed`.
    pub async fn create(&self, input: CommandGroupInput) -> MtResult<CommandGroup> {
        if input.scope == GroupScope::Local {
            if let Some(ref pid) = input.project_id {
                if let Ok(root) = self.tab_port.resolve_project_root(pid).await {
                    let _ = crate::groups::validate::validate_group_input(&input, Some(&root))?;
                }
            }
        }
        let created = self.repo.create(&input)?;
        self.events.emit_group_changed(GroupChangedEvent {
            group_id: Some(created.id.clone()),
            scope: created.scope,
            project_id: created.project_id.clone(),
            revision: created.revision,
            operation: GroupChangeOperation::Created,
        });
        Ok(created)
    }

    /// Atualiza um grupo existente verificando `expected_revision` e emite `command-group://changed`.
    pub async fn update(
        &self,
        group_id: CommandGroupId,
        expected_revision: u64,
        input: CommandGroupInput,
    ) -> MtResult<CommandGroup> {
        if input.scope == GroupScope::Local {
            if let Some(ref pid) = input.project_id {
                if let Ok(root) = self.tab_port.resolve_project_root(pid).await {
                    let _ = crate::groups::validate::validate_group_input(&input, Some(&root))?;
                }
            }
        }
        let updated = self.repo.update(&group_id, expected_revision, &input)?;
        self.events.emit_group_changed(GroupChangedEvent {
            group_id: Some(updated.id.clone()),
            scope: updated.scope,
            project_id: updated.project_id.clone(),
            revision: updated.revision,
            operation: GroupChangeOperation::Updated,
        });
        Ok(updated)
    }

    /// Exclui um grupo verificando `expected_revision` e emite `command-group://changed`.
    pub async fn delete(
        &self,
        group_id: CommandGroupId,
        expected_revision: u64,
    ) -> MtResult<DeleteGroupReceipt> {
        let existing = self.repo.get(&group_id)?;
        let receipt = self.repo.delete(&group_id, expected_revision)?;
        self.events.emit_group_changed(GroupChangedEvent {
            group_id: Some(receipt.id.clone()),
            scope: existing.scope,
            project_id: existing.project_id,
            revision: receipt.revision,
            operation: GroupChangeOperation::Deleted,
        });
        Ok(receipt)
    }

    /// Reordena atomicamente todos os grupos do escopo/projeto e emite `command-group://changed`.
    pub async fn reorder(
        &self,
        scope: GroupScope,
        project_id: Option<ProjectId>,
        entries: Vec<ReorderGroupEntry>,
    ) -> MtResult<CommandGroupCatalog> {
        let catalog = self.repo.reorder(scope, project_id.as_ref(), &entries)?;
        let max_revision = match scope {
            GroupScope::Local => catalog
                .local_groups
                .iter()
                .map(|g| g.revision)
                .max()
                .unwrap_or(1),
            GroupScope::Global => catalog
                .global_groups
                .iter()
                .map(|g| g.revision)
                .max()
                .unwrap_or(1),
        };
        self.events.emit_group_changed(GroupChangedEvent {
            group_id: None,
            scope,
            project_id,
            revision: max_revision,
            operation: GroupChangeOperation::Reordered,
        });
        Ok(catalog)
    }

    /// Aceita e dispara a execução de um grupo de comandos com idempotência estrita por `request_id`
    /// e preflight completo antes de criar abas ou PTYs.
    #[allow(clippy::too_many_lines)]
    pub async fn run(self: &Arc<Self>, request: GroupRunRequest) -> MtResult<GroupRun> {
        if request.request_id.0.trim().is_empty() {
            return Err(GroupErrorCode::Validation.into_error_with_fields(
                "O identificador request_id é obrigatório.",
                vec!["requestId".to_string()],
            ));
        }

        let _accept_guard = self.accept_mutex.lock().await;
        let request_hash = compute_request_hash(&request);

        // 1. Consulta idempotente por request_id ANTES de validar existência ou revisão atual do grupo
        if let Some((stored_hash, existing_run)) =
            self.repo.find_run_by_request_id(&request.request_id)?
        {
            if stored_hash == request_hash {
                return Ok(existing_run);
            }
            return Err(GroupErrorCode::RequestConflict.into_error(
                "O identificador request_id já foi utilizado com parâmetros divergentes.",
            ));
        }

        // 2. Carrega o grupo e valida revisão e escopo
        let group = self.repo.get(&request.group_id)?;
        if group.revision != request.expected_revision {
            return Err(GroupErrorCode::RevisionConflict
                .into_error(format!(
                    "O grupo '{}' foi atualizado (revisão esperada {}, atual {}). Recarregue antes de executar.",
                    group.name, request.expected_revision, group.revision
                ))
                .with_details(serde_json::json!({
                    "expectedRevision": request.expected_revision,
                    "currentRevision": group.revision,
                })));
        }

        if group.scope == GroupScope::Local
            && group.project_id.as_ref() != Some(&request.project_id)
        {
            return Err(GroupErrorCode::ScopeMismatch.into_error(
                "Este grupo local pertence a outro projeto e não pode ser executado no projeto atual.",
            ));
        }

        // 3. Resolve a raiz do projeto no disco
        let raw_root = self
            .tab_port
            .resolve_project_root(&request.project_id)
            .await?;
        let canonical_root = raw_root.canonicalize().map_err(|_| {
            GroupErrorCode::CwdNotFound.into_error_with_fields(
                "A pasta raiz do projeto não existe no disco.",
                vec!["projectId".to_string()],
            )
        })?;

        let effective_mode = request.mode_override.unwrap_or(group.mode);
        let mut acquired_lease: Option<Box<dyn PaneExecutionLease>> = None;
        let mut origin_tuple = None;
        let mut default_shell_path = PathBuf::from("/bin/bash");
        let mut default_dialect = ShellDialect::Bash;
        let mut origin_generation: Option<u64> = None;

        // 4. Preflight específico por modo de execução
        let mut item_snapshots: Vec<(crate::groups::model::GroupRunItemId, GroupRunItemSnapshot)> =
            Vec::with_capacity(group.items.len());

        match effective_mode {
            ExecutionMode::Sequential => {
                let Some(ref pane_id) = request.origin_pane_id else {
                    return Err(GroupErrorCode::Validation.into_error_with_fields(
                        "O modo Sequencial exige um painel de terminal ativo (originPaneId).",
                        vec!["originPaneId".to_string()],
                    ));
                };

                let lease = self.shell_port.acquire_pane_lease(pane_id).await?;
                let p_state = lease.state().clone();
                let dialect = match detect_shell_dialect(&p_state.shell_path) {
                    Ok(d) => d,
                    Err(err) => {
                        lease.release().await;
                        return Err(err);
                    }
                };

                let env_chdir_supported = self.shell_port.supports_env_chdir();
                let group_has_cwd = group.cwd.is_some();

                for (idx, item) in group.items.iter().enumerate() {
                    let field_path = format!("items[{idx}].cwd");
                    let resolved_cwd = match CwdResolver::resolve_effective_cwd(
                        &canonical_root,
                        group.cwd.as_deref(),
                        item.cwd.as_deref(),
                        &field_path,
                    ) {
                        Ok(c) => c,
                        Err(err) => {
                            lease.release().await;
                            return Err(err);
                        }
                    };

                    let merged_env =
                        match CwdResolver::merge_env(&BTreeMap::new(), &group.env, &item.env) {
                            Ok(e) => e,
                            Err(err) => {
                                lease.release().await;
                                return Err(err);
                            }
                        };

                    let cwd_override = if item.cwd.is_some() || group_has_cwd {
                        Some(resolved_cwd.as_path())
                    } else {
                        None
                    };

                    if let Err(err) = SequentialComposer::compose_submission(
                        dialect,
                        &p_state.shell_path,
                        &item.command,
                        cwd_override,
                        &merged_env,
                        env_chdir_supported,
                    ) {
                        lease.release().await;
                        return Err(err);
                    }

                    item_snapshots.push((
                        crate::groups::model::GroupRunItemId::new(),
                        GroupRunItemSnapshot {
                            source_item_id: item.id.clone(),
                            position: item.position,
                            title: item.title.clone(),
                            command: item.command.clone(),
                            defined_cwd: item.cwd.clone().or_else(|| group.cwd.clone()),
                            resolved_cwd: resolved_cwd.to_string_lossy().into_owned(),
                            env: merged_env,
                            continuous: item.continuous,
                            template_id: None,
                            shell_path: p_state.shell_path.to_string_lossy().into_owned(),
                            shell_dialect: dialect,
                        },
                    ));
                }

                default_shell_path = p_state.shell_path.clone();
                default_dialect = dialect;
                origin_generation = Some(p_state.generation);
                origin_tuple = Some((
                    p_state.tab_id.clone(),
                    p_state.pane_id.clone(),
                    p_state.pty_id.clone(),
                    p_state.session_id.clone(),
                ));
                acquired_lease = Some(lease);
            }
            ExecutionMode::Parallel => {
                if let Some(ref pane_id) = request.origin_pane_id {
                    if let Ok(state) = self.shell_port.inspect_pane(pane_id).await {
                        default_dialect = detect_shell_dialect(&state.shell_path)?;
                        default_shell_path = state.shell_path;
                        origin_generation = Some(state.generation);
                        origin_tuple =
                            Some((state.tab_id, state.pane_id, state.pty_id, state.session_id));
                    }
                } else if let Ok(templates) = self
                    .tab_port
                    .list_project_templates(&request.project_id)
                    .await
                {
                    if let Some(tpl) = templates
                        .iter()
                        .find(|t| t.is_default && t.is_shell_environment)
                        .or_else(|| templates.iter().find(|t| t.is_shell_environment))
                    {
                        default_dialect = detect_shell_dialect(&tpl.shell_path)?;
                        default_shell_path = tpl.shell_path.clone();
                    }
                }

                for (idx, item) in group.items.iter().enumerate() {
                    let field_path = format!("items[{idx}].cwd");
                    let resolved_cwd = CwdResolver::resolve_effective_cwd(
                        &canonical_root,
                        group.cwd.as_deref(),
                        item.cwd.as_deref(),
                        &field_path,
                    )?;
                    let merged_env =
                        CwdResolver::merge_env(&BTreeMap::new(), &group.env, &item.env)?;

                    item_snapshots.push((
                        crate::groups::model::GroupRunItemId::new(),
                        GroupRunItemSnapshot {
                            source_item_id: item.id.clone(),
                            position: item.position,
                            title: item.title.clone(),
                            command: item.command.clone(),
                            defined_cwd: item.cwd.clone().or_else(|| group.cwd.clone()),
                            resolved_cwd: resolved_cwd.to_string_lossy().into_owned(),
                            env: merged_env,
                            continuous: item.continuous,
                            template_id: None,
                            shell_path: default_shell_path.to_string_lossy().into_owned(),
                            shell_dialect: default_dialect,
                        },
                    ));
                }
            }
            ExecutionMode::MultiTabs => {
                if let Some(ref pane_id) = request.origin_pane_id {
                    if let Ok(state) = self.shell_port.inspect_pane(pane_id).await {
                        origin_generation = Some(state.generation);
                        origin_tuple =
                            Some((state.tab_id, state.pane_id, state.pty_id, state.session_id));
                    }
                }

                let available_templates = self
                    .tab_port
                    .list_project_templates(&request.project_id)
                    .await?;
                let resolved_templates = MultiTabPlanner::resolve_templates_for_items(
                    &group.items,
                    &available_templates,
                    &request.template_by_item,
                )?;

                for (idx, item) in group.items.iter().enumerate() {
                    let tpl = resolved_templates.get(&item.id).ok_or_else(|| {
                        GroupErrorCode::TemplateIncompatible
                            .into_error("Template não encontrado para o item.")
                    })?;
                    let tpl_dialect = detect_shell_dialect(&tpl.shell_path)?;
                    if idx == 0 {
                        default_shell_path = tpl.shell_path.clone();
                        default_dialect = tpl_dialect;
                    }

                    let field_path = format!("items[{idx}].cwd");
                    let resolved_cwd = CwdResolver::resolve_effective_cwd(
                        &canonical_root,
                        group.cwd.as_deref(),
                        item.cwd.as_deref(),
                        &field_path,
                    )?;
                    let merged_env = CwdResolver::merge_env(&tpl.base_env, &group.env, &item.env)?;

                    item_snapshots.push((
                        crate::groups::model::GroupRunItemId::new(),
                        GroupRunItemSnapshot {
                            source_item_id: item.id.clone(),
                            position: item.position,
                            title: item.title.clone(),
                            command: item.command.clone(),
                            defined_cwd: item.cwd.clone().or_else(|| group.cwd.clone()),
                            resolved_cwd: resolved_cwd.to_string_lossy().into_owned(),
                            env: merged_env,
                            continuous: item.continuous,
                            template_id: Some(tpl.id.clone()),
                            shell_path: tpl.shell_path.to_string_lossy().into_owned(),
                            shell_dialect: tpl_dialect,
                        },
                    ));
                }
            }
        }

        let (origin_tab_id, origin_pane_id, origin_pty_id, origin_session_id) = match origin_tuple {
            Some((t, p, pty, s)) => (Some(t), Some(p), Some(pty), Some(s)),
            None => (None, request.origin_pane_id.clone(), None, None),
        };

        let run_snapshot = GroupRunSnapshot {
            group_id: group.id.clone(),
            group_revision: group.revision,
            scope: group.scope,
            name: group.name.clone(),
            description: group.description.clone(),
            labels: group.labels.clone(),
            defined_mode: group.mode,
            effective_mode,
            separator: group.separator,
            cwd: group.cwd.clone(),
            env: group.env.clone(),
            project_root: canonical_root.to_string_lossy().into_owned(),
            shell_path: default_shell_path.to_string_lossy().into_owned(),
            shell_dialect: default_dialect,
            origin_tab_id,
            origin_pane_id,
            origin_pty_id,
            origin_session_id: origin_session_id.clone(),
            origin_generation,
        };

        let now_ms = mt_db::time::now_unix_ms();
        let record = CreateRunRecord {
            run_id: GroupRunId::new(),
            request_id: request.request_id,
            request_hash,
            group_id: group.id,
            project_id: request.project_id,
            origin_session_id,
            snapshot: run_snapshot,
            mode: effective_mode,
            items: item_snapshots,
            created_at: now_ms,
        };

        let outcome = match self.repo.accept_run(record) {
            Ok(out) => out,
            Err(err) => {
                if let Some(lease) = acquired_lease {
                    lease.release().await;
                }
                return Err(err);
            }
        };

        let created_run = match outcome {
            AcceptRunOutcome::Existing(existing) => {
                if let Some(lease) = acquired_lease {
                    lease.release().await;
                }
                return Ok(existing);
            }
            AcceptRunOutcome::Created(run) => run,
        };

        self.events
            .emit_run_started(GroupRunEvent::from_run(&created_run, None));

        match effective_mode {
            ExecutionMode::Sequential => {
                if let Some(lease) = acquired_lease {
                    self.sequential_runner
                        .launch_with_lease(created_run.clone(), lease)
                        .await?;
                }
            }
            ExecutionMode::Parallel => {
                self.parallel_runner.launch(created_run.clone()).await?;
            }
            ExecutionMode::MultiTabs => {
                self.multi_tab_runner.launch(created_run.clone()).await?;
            }
        }

        Ok(created_run)
    }

    /// Cancela uma execução inteira (`item_id == None`) ou um item específico (`item_id == Some`).
    pub async fn cancel_run(
        self: &Arc<Self>,
        run_id: GroupRunId,
        item_id: Option<GroupRunItemId>,
    ) -> MtResult<GroupRun> {
        let existing = self.repo.get_run(&run_id)?;
        if existing.status.is_terminal() {
            return Ok(existing);
        }
        match existing.mode {
            ExecutionMode::Sequential => {
                self.sequential_runner
                    .cancel(&run_id, item_id.as_ref())
                    .await
            }
            ExecutionMode::Parallel => self.parallel_runner.cancel(&run_id, item_id.as_ref()).await,
            ExecutionMode::MultiTabs => {
                self.multi_tab_runner
                    .cancel(&run_id, item_id.as_ref())
                    .await
            }
        }
    }

    /// Lista as execuções de um projeto com paginação keyset.
    pub async fn list_runs(
        &self,
        project_id: ProjectId,
        cursor: Option<String>,
        limit: u32,
        active_only: Option<bool>,
    ) -> MtResult<GroupRunListPage> {
        self.repo.list_runs(
            &project_id,
            cursor.as_deref(),
            limit,
            active_only.unwrap_or(false),
        )
    }

    /// Obtém os detalhes completos de uma execução (`GroupRun`).
    pub async fn get_run(&self, run_id: GroupRunId) -> MtResult<GroupRun> {
        self.repo.get_run(&run_id)
    }

    /// Anexa um assinante ao stream de saída (`Replay + Live`) de um item de execução.
    pub async fn attach_item_output(
        &self,
        run_id: GroupRunId,
        item_id: GroupRunItemId,
        after_seq: Option<u64>,
        subscriber: Arc<dyn OutputFrameSubscriber>,
    ) -> MtResult<OutputAttachReceipt> {
        self.parallel_runner
            .attach_output(&run_id, &item_id, after_seq, subscriber)
    }

    /// Remove uma assinatura ativa de saída sem cancelar o item.
    pub async fn detach_item_output(&self, subscription_id: OutputSubscriptionId) -> MtResult<()> {
        self.parallel_runner.detach_output(&subscription_id);
        Ok(())
    }

    /// Solicita uma sugestão assistida de grupo via `MaxAiManager` (Fase 12).
    pub async fn suggest(&self, request: GroupSuggestRequest) -> MtResult<GroupSuggestResponse> {
        self.suggester.suggest(request).await
    }

    /// Lista os comandos recorrentes do projeto via memória contextual opcional (Fase 13).
    pub async fn recurring_commands(
        &self,
        project_id: ProjectId,
        limit: u32,
    ) -> MtResult<RecurringCommandsResponse> {
        self.suggester.list_recurring(&project_id, limit).await
    }

    /// Executado no startup do aplicativo (após a limpeza de órfãos da Fase 02):
    /// marca runs e itens não terminais como `interrupted` e aplica retenção de 30 dias em runs terminais.
    pub async fn recover_interrupted(&self) -> MtResult<Vec<GroupRun>> {
        let now_ms = mt_db::time::now_unix_ms();
        let recovered = self.repo.recover_interrupted_runs(now_ms)?;
        let cutoff_ms = now_ms.saturating_sub(DEFAULT_RUN_RETENTION_MS);
        let _ = self.repo.prune_finished_runs_older_than(cutoff_ms);
        Ok(recovered)
    }

    /// Executado antes de remover um projeto (Fase 06): cancela e aguarda todas as execuções ativas daquele projeto.
    pub async fn on_project_deleting(self: &Arc<Self>, project_id: &ProjectId) -> MtResult<()> {
        let page = self.repo.list_runs(project_id, None, 100, true)?;
        for summary in page.items {
            let _ = self.cancel_run(summary.id, None).await;
        }
        Ok(())
    }

    /// Executado no encerramento explícito do aplicativo: cancela todas as filas e PTYs de grupos e aguarda supervisão.
    pub async fn shutdown(self: &Arc<Self>) {
        self.sequential_runner.shutdown_all().await;
        self.parallel_runner.shutdown_all().await;
        self.multi_tab_runner.shutdown_all().await;
        let _ = &self.pty_port;
    }
}
