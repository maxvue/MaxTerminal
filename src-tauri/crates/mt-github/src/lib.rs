//! Crate `mt-github` — Aba Fixa de Issues do GitHub (Padrão `MaxCode`) para o `MaxTerminal`.

#![cfg_attr(test, allow(clippy::unwrap_used))]
#![allow(
    missing_docs,
    clippy::pedantic,
    clippy::clone_on_copy,
    clippy::expect_used,
    clippy::large_enum_variant,
    clippy::match_like_matches_macro,
    clippy::needless_borrow,
    clippy::too_many_arguments,
    clippy::type_complexity,
    clippy::unnecessary_lazy_evaluations,
    clippy::unnecessary_literal_unwrap
)]

pub mod actions;
pub mod auth;
pub mod bulk;
pub mod cache;
pub mod client;
pub mod error;
pub mod events;
pub mod labels;
pub mod model;
pub mod plan;
pub mod ports;
pub mod rate_limit;
pub mod remote;
pub mod sync;
pub mod worktree;

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use mt_core::ids::ProjectId;

use crate::actions::IssueActionsCoordinator;
use crate::auth::TokenStore;
use crate::bulk::BugCycleOrchestrator;
use crate::cache::GithubCacheRepo;
use crate::client::GithubRemoteClient;
use crate::client::fallback::FallbackGithubClient;
use crate::error::{GithubError, GithubErrorKind, GithubResult};
use crate::events::{
    GithubAuthChangedPayload, GithubIssueUpdatedPayload, GithubIssuesChangedPayload,
};
use crate::labels::{
    canonical_labels_catalog, compute_next_issue_labels, find_canonical_label,
    normalize_label_name, to_repo_label,
};
use crate::model::{
    BugCyclePreview, BugCycleRunState, CanonicalLabelGroup, CanonicalLabelSpec, CreateIssueInput,
    CreateIssueWorktreeRequest, GithubAuthStatus, GithubRepoIdentity, GithubTransportKind,
    IssueCounts, IssueDetail, IssueFilter, IssueListPage, IssueStateFilter, IssueSummary,
    IssueWorktreeResolution, LabelEnsureReport, LabelUpdatePatch, OpenIssueTabReceipt,
    OpenIssueTabRequest, RepoLabel, RepoResolution, SendIssueToMaxCodeReceipt,
    SendIssueToMaxCodeRequest, StartBugCycleRequest,
};
use crate::ports::{
    GitRepoPort, GithubEventSink, MaxCodeBridgePort, ProjectLookupPort, WorkspaceTabPort,
};
use crate::remote::resolve_from_remotes;
use crate::sync::GithubSyncCoordinator;

/// Fachada principal de domínio da Fase 16 (`mt-github`), injetada no estado global do Tauri.
pub struct GithubService {
    cache: GithubCacheRepo,
    fallback_client: Arc<FallbackGithubClient>,
    sync: GithubSyncCoordinator,
    actions: IssueActionsCoordinator,
    bulk: BugCycleOrchestrator,
    projects: Arc<dyn ProjectLookupPort>,
    git: Arc<dyn GitRepoPort>,
    events: Arc<dyn GithubEventSink>,
}

impl GithubService {
    /// Constrói um novo `GithubService` com todas as portas e clientes configurados.
    #[must_use]
    pub fn new(
        cache: GithubCacheRepo,
        fallback_client: Arc<FallbackGithubClient>,
        projects: Arc<dyn ProjectLookupPort>,
        git: Arc<dyn GitRepoPort>,
        workspace: Arc<dyn WorkspaceTabPort>,
        bridge: Arc<dyn MaxCodeBridgePort>,
        events: Arc<dyn GithubEventSink>,
    ) -> Self {
        let remote_client: Arc<dyn GithubRemoteClient> =
            Arc::clone(&fallback_client) as Arc<dyn GithubRemoteClient>;
        let sync = GithubSyncCoordinator::new(cache.clone(), remote_client, Arc::clone(&events));
        let actions = IssueActionsCoordinator::new(
            cache.clone(),
            Arc::clone(&projects),
            Arc::clone(&git),
            workspace,
            Arc::clone(&bridge),
        );
        let bulk = BugCycleOrchestrator::new(
            cache.clone(),
            Arc::clone(&projects),
            bridge,
            Arc::clone(&events),
        );

        Self {
            cache,
            fallback_client,
            sync,
            actions,
            bulk,
            projects,
            git,
            events,
        }
    }

    /// Retorna referência ao cache SQLite.
    #[must_use]
    pub const fn cache(&self) -> &GithubCacheRepo {
        &self.cache
    }

    /// Retorna referência ao `TokenStore`.
    #[must_use]
    pub fn token_store(&self) -> &TokenStore {
        self.fallback_client.rest().token_store()
    }

    /// Retorna referência ao coordenador de ciclos em lote.
    #[must_use]
    pub const fn bulk(&self) -> &BugCycleOrchestrator {
        &self.bulk
    }

    /// Resolve se um projeto é um repositório GitHub.com e persiste o vínculo no SQLite.
    pub async fn resolve_project_repo(
        &self,
        project_id: ProjectId,
    ) -> GithubResult<RepoResolution> {
        let path = self.projects.project_root_path(project_id).await?;
        let remotes = self.git.list_remotes(&path).await?;
        let default_branch = self.git.detect_default_branch(&path).await.ok().flatten();
        let raw_url = remotes
            .as_ref()
            .and_then(|list| list.first().map(|r| r.url.clone()));

        let resolution = resolve_from_remotes(remotes.as_deref(), default_branch);
        self.cache
            .save_repo_resolution(project_id, raw_url, &resolution, current_epoch_ms())
            .await?;

        Ok(resolution)
    }

    /// Obtém a identidade GitHub de um projeto (resolvendo sob demanda caso ainda não esteja em cache).
    pub async fn require_github_repo(
        &self,
        project_id: ProjectId,
    ) -> GithubResult<GithubRepoIdentity> {
        if let Some(identity) = self.cache.get_project_repo_identity(project_id).await? {
            return Ok(identity);
        }
        match self.resolve_project_repo(project_id).await? {
            RepoResolution::GitHub { identity } => Ok(identity),
            _ => Err(GithubError::new(
                GithubErrorKind::NotGitHubRepo,
                "Este projeto não está vinculado a um repositório do GitHub.com.",
            )),
        }
    }

    /// Consulta o status consolidado de autenticação (`gh` CLI + Token REST + Rate Limit).
    pub async fn auth_status(&self, force_probe: bool) -> GithubResult<GithubAuthStatus> {
        Ok(self.fallback_client.auth_status(force_probe).await)
    }

    /// Configura um novo token PAT para o fallback REST e emite `github:auth-changed`.
    pub async fn set_auth_token(&self, token: &str) -> GithubResult<GithubAuthStatus> {
        self.token_store().set_token(token, None)?;
        // Tenta validar login de forma oportunista sem falhar caso esteja offline
        let _ = self
            .fallback_client
            .rest()
            .validate_authenticated_user()
            .await;
        let status = self.fallback_client.auth_status(false).await;
        self.events.emit_auth_changed(GithubAuthChangedPayload {
            status: status.clone(),
        });
        Ok(status)
    }

    /// Remove o token PAT configurado e emite `github:auth-changed`.
    pub async fn clear_auth_token(&self) -> GithubResult<GithubAuthStatus> {
        self.token_store().clear_token()?;
        let status = self.fallback_client.auth_status(false).await;
        self.events.emit_auth_changed(GithubAuthChangedPayload {
            status: status.clone(),
        });
        Ok(status)
    }

    /// Lista issues do projeto aplicando filtros sobre o cache SQLite (e sincroniza na primeira abertura se o cache estiver vazio).
    pub async fn list_issues(
        &self,
        project_id: ProjectId,
        filter: IssueFilter,
    ) -> GithubResult<IssueListPage> {
        let repo = self.require_github_repo(project_id).await?;
        let cached = self
            .cache
            .list_issues(&repo.repo_key, &filter, false)
            .await?;
        if cached.last_success_at_ms.is_none() && cached.items.is_empty() {
            // Primeira leitura: tenta sincronizar com o GitHub
            let _ = self
                .sync
                .refresh_repo(project_id, &repo, filter.state, false)
                .await?;
            return self.cache.list_issues(&repo.repo_key, &filter, false).await;
        }
        Ok(cached)
    }

    /// Força atualização imediata de issues (e catálogo de labels) a partir do GitHub.
    pub async fn refresh_issues(
        &self,
        project_id: ProjectId,
        state_filter: Option<IssueStateFilter>,
        force_ignore_etag: bool,
    ) -> GithubResult<IssueListPage> {
        let repo = self.require_github_repo(project_id).await?;
        let _ = self
            .sync
            .sync_repo_labels(project_id, &repo, force_ignore_etag)
            .await;
        self.sync
            .refresh_repo(
                project_id,
                &repo,
                state_filter.unwrap_or(IssueStateFilter::Open),
                force_ignore_etag,
            )
            .await
    }

    /// Carrega o detalhe completo de uma issue (descrição, comentários e `## PLANO DE IMPLEMENTAÇÃO`).
    pub async fn issue_detail(
        &self,
        project_id: ProjectId,
        issue_number: u64,
        force_refresh: bool,
    ) -> GithubResult<IssueDetail> {
        let repo = self.require_github_repo(project_id).await?;
        self.sync
            .sync_issue_detail(&repo, issue_number, force_refresh)
            .await
    }

    /// Obtém os contadores agregados de issues de um projeto (incluindo `open_count` para o badge 🟣).
    pub async fn issue_counts(&self, project_id: ProjectId) -> GithubResult<IssueCounts> {
        let repo_opt = match self.cache.get_project_repo_identity(project_id).await? {
            Some(id) => Some(id),
            None => match self.resolve_project_repo(project_id).await? {
                RepoResolution::GitHub { identity } => Some(identity),
                _ => None,
            },
        };
        self.cache
            .get_issue_counts(project_id, repo_opt.as_ref().map(|r| r.repo_key.as_str()))
            .await
    }

    /// Retorna o catálogo estático das 24 labels canônicas do workflow `MaxCode`.
    #[must_use]
    pub fn labels_catalog(&self) -> Vec<CanonicalLabelSpec> {
        canonical_labels_catalog().to_vec()
    }

    /// Lista as labels atualmente sincronizadas do repositório do projeto.
    pub async fn repo_labels(
        &self,
        project_id: ProjectId,
        force_refresh: bool,
    ) -> GithubResult<Vec<RepoLabel>> {
        let repo = self.require_github_repo(project_id).await?;
        self.sync
            .sync_repo_labels(project_id, &repo, force_refresh)
            .await
    }

    /// Inspeciona ou cria no repositório as 24 labels canônicas ausentes.
    pub async fn ensure_labels(
        &self,
        project_id: ProjectId,
        dry_run: bool,
    ) -> GithubResult<LabelEnsureReport> {
        let repo = self.require_github_repo(project_id).await?;
        self.sync
            .ensure_canonical_labels(project_id, &repo, dry_run)
            .await
    }

    /// Cria uma nova issue no repositório GitHub do projeto com idempotência por `client_mutation_id`.
    pub async fn create_issue(&self, input: CreateIssueInput) -> GithubResult<IssueSummary> {
        let title = input.title.trim();
        if title.is_empty() || title.chars().count() > 256 {
            return Err(GithubError::new(
                GithubErrorKind::ValidationFailed,
                "O título da issue deve ter entre 1 e 256 caracteres.",
            ));
        }
        if input.body.len() > 65_536 {
            return Err(GithubError::new(
                GithubErrorKind::ValidationFailed,
                "A descrição da issue excede o limite máximo de 64 KiB.",
            ));
        }

        let repo = self.require_github_repo(input.project_id).await?;

        if let Some(ref mut_id) = input.client_mutation_id {
            if !mut_id.trim().is_empty() {
                if let Some(cached_json) = self.cache.get_mutation_response(mut_id).await? {
                    if let Ok(summary) = serde_json::from_str::<IssueSummary>(&cached_json) {
                        return Ok(summary);
                    }
                }
            }
        }

        let mut labels_to_apply: Vec<String> = Vec::new();
        if let Some(ref st) = input.state_label {
            let trimmed = st.trim();
            if !trimmed.is_empty() {
                let spec = find_canonical_label(trimmed).ok_or_else(|| {
                    GithubError::new(
                        GithubErrorKind::ValidationFailed,
                        format!("Label de estado inválida: '{trimmed}'."),
                    )
                })?;
                if spec.group != CanonicalLabelGroup::State {
                    return Err(GithubError::new(
                        GithubErrorKind::ValidationFailed,
                        format!("A label '{}' não pertence ao grupo de estado.", spec.name),
                    ));
                }
                labels_to_apply.push(spec.name.clone());
            }
        }

        for cat in &input.category_labels {
            let spec = find_canonical_label(cat).ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::ValidationFailed,
                    format!("Label de categoria inválida: '{cat}'."),
                )
            })?;
            if spec.group != CanonicalLabelGroup::Category {
                return Err(GithubError::new(
                    GithubErrorKind::ValidationFailed,
                    format!(
                        "A label '{}' não pertence ao grupo de categoria.",
                        spec.name
                    ),
                ));
            }
            if !labels_to_apply.contains(&spec.name) {
                labels_to_apply.push(spec.name.clone());
            }
        }

        for extra in &input.extra_labels {
            let norm = normalize_label_name(extra);
            if !norm.is_empty()
                && !labels_to_apply
                    .iter()
                    .any(|l| l.eq_ignore_ascii_case(&norm))
            {
                labels_to_apply.push(norm);
            }
        }

        let created_remote = self
            .fallback_client
            .create_issue(
                &repo,
                title,
                &input.body,
                &labels_to_apply,
                &input.assignees,
            )
            .await?;

        let now_ms = current_epoch_ms();
        let open_count = self
            .cache
            .upsert_issues_batch(
                &repo,
                std::slice::from_ref(&created_remote),
                IssueStateFilter::All,
                now_ms,
            )
            .await?;

        let detail = self
            .cache
            .get_issue_detail(&repo.repo_key, created_remote.number, true)
            .await?
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::Internal,
                    "Falha ao recuperar a issue recém-criada do cache local.",
                )
            })?;

        if let Some(ref mut_id) = input.client_mutation_id {
            if !mut_id.trim().is_empty() {
                if let Ok(json_str) = serde_json::to_string(&detail.summary) {
                    let _ = self
                        .cache
                        .record_mutation_response(
                            mut_id,
                            &repo.repo_key,
                            "create_issue",
                            &json_str,
                            now_ms,
                        )
                        .await;
                }
            }
        }

        self.events.emit_issues_changed(GithubIssuesChangedPayload {
            project_id: input.project_id,
            repo_key: repo.repo_key.clone(),
            open_issues_count: open_count,
            transport_used: GithubTransportKind::Rest,
            synced_at_ms: now_ms,
        });
        self.events.emit_issue_updated(GithubIssueUpdatedPayload {
            project_id: input.project_id,
            repo_key: repo.repo_key,
            issue: detail.summary.clone(),
        });

        Ok(detail.summary)
    }

    /// Atualiza atomicamente as labels de uma issue garantindo a exclusividade de `state` (15) e `closure` (3).
    pub async fn update_issue_labels(&self, patch: LabelUpdatePatch) -> GithubResult<IssueSummary> {
        let repo = self.require_github_repo(patch.project_id).await?;

        if let Some(ref mut_id) = patch.client_mutation_id {
            if !mut_id.trim().is_empty() {
                if let Some(cached_json) = self.cache.get_mutation_response(mut_id).await? {
                    if let Ok(summary) = serde_json::from_str::<IssueSummary>(&cached_json) {
                        return Ok(summary);
                    }
                }
            }
        }

        let current_detail = self
            .cache
            .get_issue_detail(&repo.repo_key, patch.issue_number, false)
            .await?
            .ok_or_else(|| {
                GithubError::new(
                    GithubErrorKind::NotFound,
                    format!(
                        "Issue #{} não encontrada no cache local.",
                        patch.issue_number
                    ),
                )
            })?;

        let current_names: Vec<String> = current_detail
            .summary
            .labels
            .iter()
            .map(|l| l.name.clone())
            .collect();

        let next_names = compute_next_issue_labels(&current_names, &patch)?;

        let remote_labels = self
            .fallback_client
            .replace_issue_labels(&repo, patch.issue_number, &next_names)
            .await
            .unwrap_or_else(|_| {
                next_names
                    .iter()
                    .map(|n| to_repo_label(n, None, None))
                    .collect()
            });

        let now_ms = current_epoch_ms();
        let updated_summary = self
            .cache
            .update_issue_labels_in_cache(
                &repo.repo_key,
                patch.issue_number,
                &remote_labels,
                now_ms,
            )
            .await?;

        if let Some(ref mut_id) = patch.client_mutation_id {
            if !mut_id.trim().is_empty() {
                if let Ok(json_str) = serde_json::to_string(&updated_summary) {
                    let _ = self
                        .cache
                        .record_mutation_response(
                            mut_id,
                            &repo.repo_key,
                            "update_labels",
                            &json_str,
                            now_ms,
                        )
                        .await;
                }
            }
        }

        self.events.emit_issue_updated(GithubIssueUpdatedPayload {
            project_id: patch.project_id,
            repo_key: repo.repo_key,
            issue: updated_summary.clone(),
        });

        Ok(updated_summary)
    }

    /// Envia uma issue ao `MaxCode` com 1 clique (`Open`, `BugPlan` ou `BugFix`).
    pub async fn send_issue_to_maxcode(
        &self,
        req: SendIssueToMaxCodeRequest,
    ) -> GithubResult<SendIssueToMaxCodeReceipt> {
        let repo = self.require_github_repo(req.project_id).await?;
        self.actions.send_to_maxcode(&repo, &req).await
    }

    /// Resolve se já existe worktree ou branch local associada à issue.
    pub async fn resolve_issue_worktree(
        &self,
        project_id: ProjectId,
        issue_number: u64,
    ) -> GithubResult<IssueWorktreeResolution> {
        let repo = self.require_github_repo(project_id).await?;
        self.actions
            .resolve_worktree(&repo, project_id, issue_number)
            .await
    }

    /// Cria explicitamente uma worktree dedicada para a issue.
    pub async fn create_issue_worktree(
        &self,
        req: CreateIssueWorktreeRequest,
    ) -> GithubResult<IssueWorktreeResolution> {
        let repo = self.require_github_repo(req.project_id).await?;
        self.actions.create_worktree(&repo, &req).await
    }

    /// Abre uma aba de Terminal ou Agente CLI posicionada na worktree/branch da issue.
    pub async fn open_issue_tab(
        &self,
        req: OpenIssueTabRequest,
    ) -> GithubResult<OpenIssueTabReceipt> {
        let repo = self.require_github_repo(req.project_id).await?;
        self.actions.open_issue_tab(&repo, &req).await
    }

    /// Gera a prévia determinística para o modal de confirmação de **"Corrigir todos os Bugs"**.
    pub async fn bug_cycle_preview(
        &self,
        project_id: ProjectId,
        include_replan_and_rejected: bool,
    ) -> GithubResult<BugCyclePreview> {
        let repo = self.require_github_repo(project_id).await?;
        self.bulk
            .preview(project_id, &repo, include_replan_and_rejected)
            .await
    }

    /// Inicia o ciclo em lote **"Corrigir todos os Bugs"**.
    pub async fn bug_cycle_start(
        &self,
        req: StartBugCycleRequest,
    ) -> GithubResult<BugCycleRunState> {
        let repo = self.require_github_repo(req.project_id).await?;
        self.bulk.start(&repo, &req).await
    }

    /// Consulta o estado do ciclo em lote de um projeto.
    pub async fn bug_cycle_status(
        &self,
        project_id: ProjectId,
    ) -> GithubResult<Option<BugCycleRunState>> {
        let repo = self.require_github_repo(project_id).await?;
        self.bulk.status(&repo.repo_key).await
    }
}

fn current_epoch_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}
