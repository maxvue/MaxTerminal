//! Traits (ports) desacopladas do runtime Tauri para o crate `mt-github`.

use std::path::PathBuf;

use async_trait::async_trait;
use mt_core::ids::{ProjectId, TabId};

use crate::error::GithubResult;
use crate::events::{
    GithubAuthChangedPayload, GithubBugCycleProgressEventPayload, GithubIssueUpdatedPayload,
    GithubIssuesChangedPayload, GithubLabelsSyncedPayload, GithubRateLimitPayload,
};
use crate::model::{
    BugCycleRunState, CreateIssueWorktreeRequest, IssueWorktreeResolution, MaxCodeIssueAction,
    OpenIssueTabReceipt, OpenIssueTabRequest, SendIssueToMaxCodeReceipt,
};

/// Porta de consulta de projetos cadastrados (Fase 06).
#[async_trait]
pub trait ProjectLookupPort: Send + Sync {
    /// Retorna o caminho absoluto da pasta raiz do projeto.
    async fn project_root_path(&self, project_id: ProjectId) -> GithubResult<PathBuf>;
}

/// Par `(nome_do_remote, url_bruta)` retornado pelo repositório Git.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRemoteEntry {
    /// Nome do remote (`origin`, `upstream`, etc.).
    pub name: String,
    /// URL configurada (`fetch` URL).
    pub url: String,
}

/// Porta de inspeção de remotes, branches e worktrees Git (Fase 08).
#[async_trait]
pub trait GitRepoPort: Send + Sync {
    /// Verifica se `project_path` pertence a um repositório Git e lista seus remotes.
    /// Retorna `Ok(None)` quando `project_path` não é um repositório Git.
    async fn list_remotes(
        &self,
        project_path: &std::path::Path,
    ) -> GithubResult<Option<Vec<GitRemoteEntry>>>;

    /// Retorna a branch padrão local/remota se detectável (`main`, `master`, etc.).
    async fn detect_default_branch(
        &self,
        project_path: &std::path::Path,
    ) -> GithubResult<Option<String>>;

    /// Resolve se já existe worktree ou branch local associada à issue.
    async fn resolve_issue_worktree(
        &self,
        project_path: &std::path::Path,
        issue_number: u64,
        issue_title: &str,
        default_branch: Option<&str>,
    ) -> GithubResult<IssueWorktreeResolution>;

    /// Cria ou reutiliza uma worktree dedicada para a issue e retorna `(worktree_path, branch_name)`.
    async fn ensure_issue_worktree(
        &self,
        project_path: &std::path::Path,
        req: &CreateIssueWorktreeRequest,
        issue_title: &str,
        default_branch: Option<&str>,
    ) -> GithubResult<(String, String)>;
}

/// Porta de abertura e foco de abas de Terminal e Agente CLI no Workspace (Fases 07 e 13).
#[async_trait]
pub trait WorkspaceTabPort: Send + Sync {
    /// Procura uma aba já aberta cujo diretório de trabalho coincida com `cwd`.
    async fn find_tab_by_cwd(
        &self,
        project_id: ProjectId,
        cwd: &str,
    ) -> GithubResult<Option<(TabId, mt_core::ids::PaneId)>>;

    /// Foca uma aba já existente no projeto.
    async fn focus_tab(
        &self,
        project_id: ProjectId,
        tab_id: TabId,
        pane_id: mt_core::ids::PaneId,
    ) -> GithubResult<()>;

    /// Abre uma nova aba de Terminal ou Agente CLI para a issue.
    async fn open_issue_tab(
        &self,
        req: &OpenIssueTabRequest,
        resolved_cwd: &str,
        resolved_branch: Option<&str>,
        issue_title: &str,
        issue_html_url: &str,
    ) -> GithubResult<OpenIssueTabReceipt>;
}

/// Porta de comunicação com a extensão `MaxCode` (Fase 11).
#[async_trait]
pub trait MaxCodeBridgePort: Send + Sync {
    /// Envia uma ação individual de issue (`Open`, `BugPlan`, `BugFix`) ao `MaxCode` (via WS/HTTP loopback com fallback URI).
    async fn send_issue_action(
        &self,
        project_id: ProjectId,
        project_path: &str,
        repo_key: &str,
        issue_number: u64,
        issue_title: &str,
        issue_url: &str,
        has_plan: bool,
        action: MaxCodeIssueAction,
    ) -> GithubResult<SendIssueToMaxCodeReceipt>;

    /// Dispara o início do ciclo automatizado de bugs (`bugs.cycle.start`) na extensão `MaxCode`.
    async fn start_bugs_cycle(
        &self,
        cycle: &BugCycleRunState,
        project_path: &str,
    ) -> GithubResult<()>;
}

/// Sink de eventos emitidos pelo `mt-github` para o frontend.
pub trait GithubEventSink: Send + Sync {
    /// Emitido quando a listagem ou contadores de issues mudam.
    fn emit_issues_changed(&self, payload: GithubIssuesChangedPayload);
    /// Emitido quando o detalhe ou labels de uma issue específica mudam.
    fn emit_issue_updated(&self, payload: GithubIssueUpdatedPayload);
    /// Emitido quando o catálogo de labels do repositório é sincronizado.
    fn emit_labels_synced(&self, payload: GithubLabelsSyncedPayload);
    /// Emitido quando o estado de autenticação GitHub muda.
    fn emit_auth_changed(&self, payload: GithubAuthChangedPayload);
    /// Emitido quando um ciclo em lote avança ou muda de estado.
    fn emit_bug_cycle_progress(&self, payload: GithubBugCycleProgressEventPayload);
    /// Emitido quando o snapshot de rate limit é atualizado ou entra em alerta.
    fn emit_rate_limit(&self, payload: GithubRateLimitPayload);
}

/// Implementação no-op de `GithubEventSink` para testes unitários.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopGithubEventSink;

impl GithubEventSink for NoopGithubEventSink {
    fn emit_issues_changed(&self, _payload: GithubIssuesChangedPayload) {}
    fn emit_issue_updated(&self, _payload: GithubIssueUpdatedPayload) {}
    fn emit_labels_synced(&self, _payload: GithubLabelsSyncedPayload) {}
    fn emit_auth_changed(&self, _payload: GithubAuthChangedPayload) {}
    fn emit_bug_cycle_progress(&self, _payload: GithubBugCycleProgressEventPayload) {}
    fn emit_rate_limit(&self, _payload: GithubRateLimitPayload) {}
}
