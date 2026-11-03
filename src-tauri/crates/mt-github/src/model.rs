//! Modelos de domínio, DTOs e tipos exportados via `tauri-specta` para a Fase 16.

use std::collections::BTreeMap;

use mt_agents::registry::AgentKind as CliAgentKind;
use mt_core::ids::{PaneId, ProjectId, TabId};
use serde::{Deserialize, Serialize};

/// Identidade canônica de um repositório GitHub (`github.com/{owner}/{repo}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubRepoIdentity {
    /// Chave canônica em lowercase (`"github.com/owner/repo"`).
    pub repo_key: String,
    /// Host canônico (`"github.com"`).
    pub host: String,
    /// Proprietário/organização com capitalização preservada.
    pub owner: String,
    /// Nome do repositório sem `.git`.
    pub repo: String,
    /// Nome do remote Git selecionado (`origin`, `upstream`, etc.).
    pub remote_name: String,
    /// URL da página web do repositório (`https://github.com/owner/repo`).
    pub html_url: String,
    /// Branch padrão conhecida (`main`, `master`, etc.).
    pub default_branch: Option<String>,
}

/// Resultado discriminado da resolução de repositório GitHub para um projeto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RepoResolution {
    /// O projeto aponta para um repositório GitHub.com válido.
    GitHub {
        /// Identidade do repositório resolvido.
        identity: GithubRepoIdentity,
    },
    /// O projeto possui remotes Git, mas nenhum aponta para `github.com`.
    NonGitHubRemote {
        /// Nome do primeiro remote inspecionado.
        remote_name: String,
        /// Host detectado no remote.
        host: String,
    },
    /// É um repositório Git, mas sem remotes configurados.
    NoRemotes,
    /// A pasta do projeto não é um repositório Git.
    NotGitRepo,
    /// Ocorreu um erro ao inspecionar o repositório.
    Error {
        /// Mensagem descritiva em `pt-BR`.
        message: String,
    },
}

/// Transporte utilizado na comunicação com o GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GithubTransportKind {
    /// `gh` CLI autenticado.
    GhCli,
    /// Cliente HTTP REST (`api.github.com`).
    Rest,
    /// Dados servidos exclusivamente do cache local SQLite.
    Cache,
}

/// Estado do `gh` CLI no sistema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GhCliStatus {
    /// Se o binário `gh` foi encontrado no `PATH`.
    pub installed: bool,
    /// Versão reportada por `gh --version`.
    pub version: Option<String>,
    /// Se `gh auth status --hostname github.com` está autenticado.
    pub authenticated: bool,
    /// Login da conta ativa no `gh` CLI.
    pub account_login: Option<String>,
    /// Escopos OAuth detectados.
    pub scopes: Vec<String>,
    /// Mensagem de erro ou orientação em `pt-BR` quando indisponível/não autenticado.
    pub message: Option<String>,
}

/// Estado do token REST (PAT) e variáveis de ambiente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RestTokenStatus {
    /// Se há token configurado ou presente no ambiente.
    pub configured: bool,
    /// Origem do token (`"env"`, `"keyring"`, `"encrypted_file"`, `"memory"`, `"none"`).
    pub source: String,
    /// Login associado ao token quando validado.
    pub account_login: Option<String>,
    /// Sufixo mascarado seguro (ex.: `"••••abcd"`), nunca o segredo completo.
    pub masked_hint: Option<String>,
}

/// Status consolidado de autenticação GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubAuthStatus {
    /// Status do `gh` CLI.
    pub gh_cli: GhCliStatus,
    /// Status do token REST.
    pub rest_token: RestTokenStatus,
    /// Transporte preferencial ativo (`gh_cli`, `rest` ou `cache`).
    pub preferred_transport: GithubTransportKind,
    /// Último snapshot de rate limit conhecido.
    pub rate_limit: Option<RateLimitSnapshot>,
}

/// Snapshot de cota da API do GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSnapshot {
    /// Limite total de requisições na janela.
    pub limit: u32,
    /// Requisições restantes na janela atual.
    pub remaining: u32,
    /// Requisições consumidas na janela atual.
    pub used: u32,
    /// Timestamp epoch ms do reset da janela.
    pub reset_at_ms: i64,
    /// Se está em pausa por `Retry-After` / secondary rate limit.
    pub retry_after_until_ms: Option<i64>,
    /// Se restam menos de 10% da cota.
    pub low_remaining: bool,
}

/// Grupo das 24 labels canônicas do workflow MaxCode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalLabelGroup {
    /// 15 labels de estado mutuamente exclusivas.
    State,
    /// 3 labels de encerramento mutuamente exclusivas.
    Closure,
    /// 6 labels de categoria acumuláveis.
    Category,
}

/// Especificação de uma das 24 labels canônicas do workflow MaxCode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalLabelSpec {
    /// Nome canônico exato (ex.: `"Waiting Plan"`).
    pub name: String,
    /// Grupo canônico (`state`, `closure`, `category`).
    pub group: CanonicalLabelGroup,
    /// Ordem canônica (`1..=24`).
    pub order: u8,
    /// Cor hexadecimal de 6 dígitos sem `#` (ex.: `"0E8A16"`).
    pub color_hex: String,
    /// Descrição em `pt-BR`.
    pub description: String,
}

/// Label presente em um repositório ou issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RepoLabel {
    /// Nome da label.
    pub name: String,
    /// Nome normalizado em lowercase.
    pub name_lower: String,
    /// Cor hexadecimal de 6 dígitos sem `#`.
    pub color_hex: String,
    /// Descrição da label.
    pub description: String,
    /// Grupo canônico quando pertence às 24 labels do MaxCode.
    pub canonical_group: Option<CanonicalLabelGroup>,
}

/// Resultado da verificação/criação das 24 labels canônicas no repositório.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelEnsureReport {
    /// Chave do repositório.
    pub repo_key: String,
    /// Labels que foram criadas nesta operação.
    pub created: Vec<String>,
    /// Labels canônicas que já existiam.
    pub already_present: Vec<String>,
    /// Labels canônicas que ainda estão ausentes (ex.: modo `dry_run` ou falha parcial).
    pub missing: Vec<String>,
    /// Catálogo atualizado de labels do repositório após a operação.
    pub repo_labels: Vec<RepoLabel>,
}

/// Estado da issue no GitHub.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueState {
    /// Issue aberta.
    Open,
    /// Issue fechada.
    Closed,
}

impl IssueState {
    /// Representação em string para persistência SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
        }
    }

    /// Converte de string persistida no SQLite.
    #[must_use]
    pub fn from_db_str(s: &str) -> Self {
        if s.eq_ignore_ascii_case("closed") {
            Self::Closed
        } else {
            Self::Open
        }
    }
}

/// Filtro de estado na listagem de issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueStateFilter {
    /// Apenas issues abertas (padrão).
    #[default]
    Open,
    /// Apenas issues fechadas.
    Closed,
    /// Todas as issues (abertas e fechadas).
    All,
}

/// Ordenação da listagem de issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum IssueSortOrder {
    /// Mais recentemente atualizadas primeiro (padrão).
    #[default]
    UpdatedDesc,
    /// Mais recentemente criadas primeiro.
    CreatedDesc,
    /// Mais antigas primeiro.
    CreatedAsc,
    /// Maior número de comentários primeiro.
    CommentsDesc,
}

/// Filtro estruturado para listagem de issues.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueFilter {
    /// Filtro de estado (`open`, `closed`, `all`).
    #[serde(default)]
    pub state: IssueStateFilter,
    /// Labels canônicas de estado selecionadas (`Waiting Plan`, `Planned`, etc.).
    #[serde(default)]
    pub state_labels: Vec<String>,
    /// Labels canônicas de encerramento selecionadas (`Duplicated`, etc.).
    #[serde(default)]
    pub closure_labels: Vec<String>,
    /// Labels canônicas de categoria selecionadas (`Regra de Negocio`, etc.).
    #[serde(default)]
    pub category_labels: Vec<String>,
    /// Labels customizadas selecionadas.
    #[serde(default)]
    pub custom_labels: Vec<String>,
    /// Busca textual livre por `#numero`, título ou corpo.
    #[serde(default)]
    pub search_text: Option<String>,
    /// Filtra apenas issues com ou sem `## PLANO DE IMPLEMENTAÇÃO`.
    #[serde(default)]
    pub has_plan: Option<bool>,
    /// Filtra por login do responsável (`assignee`).
    #[serde(default)]
    pub assignee: Option<String>,
    /// Filtra por login do autor.
    #[serde(default)]
    pub author: Option<String>,
    /// Ordenação desejada.
    #[serde(default)]
    pub sort: IssueSortOrder,
    /// Limite de itens por página (1..=200; padrão 50).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Deslocamento de paginação (`offset`).
    #[serde(default)]
    pub offset: Option<u32>,
}

/// Origem onde o `## PLANO DE IMPLEMENTAÇÃO` foi encontrado.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum PlanSource {
    /// Extraído da descrição (`body`) da issue.
    Body,
    /// Extraído de um comentário da issue.
    Comment,
}

impl PlanSource {
    /// Representação em string para persistência SQLite.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Body => "body",
            Self::Comment => "comment",
        }
    }

    /// Converte de string do SQLite.
    #[must_use]
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "body" => Some(Self::Body),
            "comment" => Some(Self::Comment),
            _ => None,
        }
    }
}

/// Plano de implementação extraído de uma issue (`## PLANO DE IMPLEMENTAÇÃO`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExtractedPlan {
    /// Origem do plano (`body` ou `comment`).
    pub source: PlanSource,
    /// ID do comentário quando `source == Comment`.
    pub comment_id: Option<i64>,
    /// Login do autor do plano.
    pub author_login: String,
    /// Timestamp epoch ms da última atualização do plano.
    pub updated_at_ms: i64,
    /// Conteúdo Markdown sanitizado da seção `## PLANO DE IMPLEMENTAÇÃO`.
    pub markdown: String,
    /// `true` quando a lista de comentários ainda estava incompleta no momento da extração.
    pub provisional: bool,
}

/// Resumo de uma issue para exibição na lista e nos contadores.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueSummary {
    /// Chave canônica do repositório (`github.com/owner/repo`).
    pub repo_key: String,
    /// Número da issue (`#123`).
    pub number: u64,
    /// Título sanitizado.
    pub title: String,
    /// Estado (`open` ou `closed`).
    pub state: IssueState,
    /// Razão de fechamento quando aplicável.
    pub state_reason: Option<String>,
    /// Login do autor.
    pub author_login: String,
    /// Avatar do autor.
    pub author_avatar_url: Option<String>,
    /// Logins dos responsáveis (`assignees`).
    pub assignees: Vec<String>,
    /// Título do milestone se houver.
    pub milestone_title: Option<String>,
    /// Labels aplicadas à issue (ordenadas canonicamente).
    pub labels: Vec<RepoLabel>,
    /// Label canônica de estado ativa (no máximo 1).
    pub active_state_label: Option<String>,
    /// Label canônica de encerramento ativa (no máximo 1).
    pub active_closure_label: Option<String>,
    /// Labels canônicas de categoria ativas.
    pub category_labels: Vec<String>,
    /// Quantidade de comentários.
    pub comments_count: u32,
    /// Se a issue possui `## PLANO DE IMPLEMENTAÇÃO` detectado.
    pub has_plan: bool,
    /// URL HTML da issue no GitHub.
    pub html_url: String,
    /// Timestamp epoch ms de criação.
    pub created_at_ms: i64,
    /// Timestamp epoch ms da última atualização.
    pub updated_at_ms: i64,
    /// Timestamp epoch ms de fechamento.
    pub closed_at_ms: Option<i64>,
}

/// Comentário de uma issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueComment {
    /// ID único do comentário no GitHub.
    pub comment_id: i64,
    /// Número da issue.
    pub issue_number: u64,
    /// Login do autor.
    pub author_login: String,
    /// URL do avatar do autor.
    pub author_avatar_url: Option<String>,
    /// Corpo Markdown sanitizado.
    pub body_markdown: String,
    /// URL HTML do comentário.
    pub html_url: String,
    /// Timestamp epoch ms de criação.
    pub created_at_ms: i64,
    /// Timestamp epoch ms de atualização.
    pub updated_at_ms: i64,
    /// Se este comentário contém o plano vencedor da issue.
    pub contains_active_plan: bool,
}

/// Detalhe completo de uma issue (descrição, comentários, plano extraído e worktree associada).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueDetail {
    /// Resumo estruturado da issue.
    pub summary: IssueSummary,
    /// Descrição (`body`) Markdown sanitizada.
    pub body_markdown: String,
    /// Lista cronológica de comentários.
    pub comments: Vec<IssueComment>,
    /// Se todos os comentários foram carregados sem truncamento de paginação.
    pub comments_complete: bool,
    /// Plano de implementação extraído (`## PLANO DE IMPLEMENTAÇÃO`), se existir.
    pub plan: Option<ExtractedPlan>,
}

/// Página de listagem de issues com metadados de sincronização e stale-while-revalidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueListPage {
    /// Chave do repositório.
    pub repo_key: String,
    /// Itens da página atual.
    pub items: Vec<IssueSummary>,
    /// Total de itens que correspondem ao filtro no cache local.
    pub total_matching: u32,
    /// Total de issues abertas do repositório (para o badge 🟣).
    pub open_issues_count: u32,
    /// Se os dados vieram do cache enquanto uma atualização está pendente ou em modo offline.
    pub stale: bool,
    /// Transporte usado na última sincronização.
    pub transport_used: GithubTransportKind,
    /// Timestamp epoch ms da última sincronização bem-sucedida.
    pub last_success_at_ms: Option<i64>,
    /// Quantidade de labels canônicas ausentes no repositório.
    pub missing_canonical_labels_count: u32,
    /// Snapshot de rate limit atual.
    pub rate_limit: Option<RateLimitSnapshot>,
}

/// Contadores agregados de issues de um projeto/repositório.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct IssueCounts {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Chave do repositório (`None` quando o projeto não é GitHub).
    pub repo_key: Option<String>,
    /// Total de issues abertas (exibido no badge 🟣 quando `> 0`).
    pub open_count: u32,
    /// Total de issues fechadas em cache.
    pub closed_count: u32,
    /// Contagem por label canônica de estado (`Waiting Plan`, `Planned`, etc.).
    pub by_state_label: BTreeMap<String, u32>,
    /// Contagem por label canônica de categoria (`Regra de Negocio`, etc.).
    pub by_category_label: BTreeMap<String, u32>,
    /// Total de issues abertas que possuem `## PLANO DE IMPLEMENTAÇÃO`.
    pub with_plan_count: u32,
}

/// Entrada para criação de uma nova issue no GitHub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateIssueInput {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Título da issue (1..=256 caracteres).
    pub title: String,
    /// Descrição em Markdown (0..=65536 bytes).
    pub body: String,
    /// Label canônica de estado inicial (padrão sugerido na UI: `"Waiting Plan"`).
    pub state_label: Option<String>,
    /// Labels canônicas de categoria selecionadas.
    #[serde(default)]
    pub category_labels: Vec<String>,
    /// Labels customizadas adicionais.
    #[serde(default)]
    pub extra_labels: Vec<String>,
    /// Logins de assignees opcionais.
    #[serde(default)]
    pub assignees: Vec<String>,
    /// Chave de idempotência gerada pelo cliente para evitar duplicação em retry.
    pub client_mutation_id: Option<String>,
}

/// Operação de alteração da label exclusiva de estado ou encerramento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "op", content = "value", rename_all = "snake_case")]
pub enum ExclusiveLabelChange {
    /// Mantém a label atual inalterada.
    Keep,
    /// Remove qualquer label do grupo exclusivo.
    Clear,
    /// Define a label informada, removendo automaticamente as demais do mesmo grupo exclusivo.
    Set(String),
}

/// Patch atômico de atualização de labels de uma issue seguindo as regras MaxCode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LabelUpdatePatch {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Número da issue.
    pub issue_number: u64,
    /// Alteração no grupo exclusivo `state` (15 labels).
    pub state_change: ExclusiveLabelChange,
    /// Alteração no grupo exclusivo `closure` (3 labels).
    pub closure_change: ExclusiveLabelChange,
    /// Labels de categoria a adicionar.
    #[serde(default)]
    pub add_categories: Vec<String>,
    /// Labels de categoria a remover.
    #[serde(default)]
    pub remove_categories: Vec<String>,
    /// Labels customizadas a adicionar.
    #[serde(default)]
    pub add_custom: Vec<String>,
    /// Labels customizadas a remover.
    #[serde(default)]
    pub remove_custom: Vec<String>,
    /// Chave opcional de idempotência.
    pub client_mutation_id: Option<String>,
}

/// Ação de 1 clique para envio de uma issue ao `MaxCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum MaxCodeIssueAction {
    /// Abrir a issue no MaxCode (`issue.open`).
    Open,
    /// Analisar a issue em modo `BUG PLAN` (`issue.plan`).
    BugPlan,
    /// Corrigir a issue em modo `BUG FIX` (`issue.fix`).
    BugFix,
}

/// Requisição para enviar uma issue ao `MaxCode` com 1 clique.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SendIssueToMaxCodeRequest {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Número da issue.
    pub issue_number: u64,
    /// Ação desejada (`open`, `bug_plan`, `bug_fix`).
    pub action: MaxCodeIssueAction,
}

/// Recibo do envio de uma issue para o `MaxCode`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SendIssueToMaxCodeReceipt {
    /// Número da issue.
    pub issue_number: u64,
    /// Ação disparada.
    pub action: MaxCodeIssueAction,
    /// Canal utilizado (`"loopback_ws"`, `"loopback_http"` ou `"uri_scheme"`).
    pub delivery_channel: String,
    /// ID da requisição gerada.
    pub request_id: String,
    /// Mensagem de confirmação em `pt-BR`.
    pub message: String,
}

/// Resolução de worktree/branch associada a uma issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum IssueWorktreeResolution {
    /// Já existe uma worktree dedicada para esta issue.
    ExistingWorktree {
        /// Caminho absoluto da worktree existente.
        path: String,
        /// Branch associada.
        branch: String,
    },
    /// Existe uma branch local para esta issue, mas sem worktree criada.
    ExistingBranch {
        /// Nome da branch local existente.
        branch: String,
        /// Caminho sugerido para criar a worktree.
        suggested_worktree_path: String,
    },
    /// Não há worktree nem branch para a issue; sugere criação.
    NeedsCreation {
        /// Nome canônico sugerido para a nova branch (`fix/issue-{number}-{slug}`).
        suggested_branch: String,
        /// Caminho canônico sugerido para a nova worktree.
        suggested_worktree_path: String,
        /// Branch base sugerida (`main`, `master`, etc.).
        base_branch: String,
    },
}

/// Requisição para criar explicitamente uma worktree dedicada para uma issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CreateIssueWorktreeRequest {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Número da issue.
    pub issue_number: u64,
    /// Nome da branch (se omitido, usa a branch sugerida `fix/issue-{n}-{slug}`).
    pub branch_name: Option<String>,
    /// Branch base (se omitido, usa `default_branch` ou `HEAD`).
    pub base_ref: Option<String>,
}

/// Modo de abertura de aba no `MaxTerminal` a partir de uma issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IssueTabTargetKind {
    /// Aba de Terminal padrão posicionada na worktree/branch da issue.
    Terminal,
    /// Aba de Agente CLI posicionada na worktree/branch da issue.
    Agent {
        /// Agente CLI selecionado (`claude_code`, `codex`, `antigravity`, `opencode`).
        agent: CliAgentKind,
        /// Se deve injetar prompt contextual com resumo da issue.
        inject_issue_context: bool,
    },
}

/// Estratégia de diretório/worktree ao abrir aba para uma issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "strategy", rename_all = "snake_case")]
pub enum IssueTabDirectoryStrategy {
    /// Reutiliza worktree existente da issue ou cria uma nova automaticamente.
    UseOrCreateIssueWorktree {
        /// Branch personalizada opcional ao criar worktree.
        branch_name: Option<String>,
        /// Ref base opcional.
        base_ref: Option<String>,
    },
    /// Abre diretamente na pasta raiz do projeto sem criar worktree.
    UseProjectRoot,
}

/// Requisição para abrir uma aba no `MaxTerminal` a partir de uma issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenIssueTabRequest {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Número da issue.
    pub issue_number: u64,
    /// Tipo de aba (`terminal` ou `agent`).
    pub target: IssueTabTargetKind,
    /// Estratégia de diretório/worktree.
    pub directory: IssueTabDirectoryStrategy,
}

/// Recibo da abertura de aba para uma issue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenIssueTabReceipt {
    /// ID da aba aberta ou reutilizada.
    pub tab_id: TabId,
    /// ID do painel principal da aba.
    pub pane_id: PaneId,
    /// Diretório de trabalho efetivo da aba.
    pub cwd: String,
    /// Branch associada quando conhecida.
    pub branch: Option<String>,
    /// Se uma aba já existente na mesma worktree foi focada em vez de duplicada.
    pub reused_existing_tab: bool,
}

/// Item elegível ou ignorado na prévia do ciclo automatizado "Corrigir todos os Bugs".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCycleCandidateItem {
    /// Número da issue.
    pub issue_number: u64,
    /// Título da issue.
    pub title: String,
    /// Label de estado atual (`Waiting Plan`, `Planned`, `Waiting Execute`, etc.).
    pub active_state_label: Option<String>,
    /// Se possui `## PLANO DE IMPLEMENTAÇÃO`.
    pub has_plan: bool,
    /// Próxima etapa prevista no ciclo (`deduplicate`, `plan`, `execute`, `check`).
    pub planned_stage: String,
}

/// Item excluído da fila do ciclo automatizado com justificativa.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCycleSkippedItem {
    /// Número da issue.
    pub issue_number: u64,
    /// Título da issue.
    pub title: String,
    /// Motivo pelo qual foi ignorada em `pt-BR`.
    pub reason: String,
}

/// Prévia determinística antes de iniciar o ciclo em lote "Corrigir todos os Bugs".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCyclePreview {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Chave do repositório.
    pub repo_key: String,
    /// Issues elegíveis ordenadas na fila de processamento.
    pub candidates: Vec<BugCycleCandidateItem>,
    /// Issues abertas que foram ignoradas (ex.: `Waiting More Info`, `Fix Merged`, `Duplicated`).
    pub skipped: Vec<BugCycleSkippedItem>,
    /// Ciclo já em andamento se houver.
    pub active_cycle: Option<BugCycleRunState>,
}

/// Requisição para iniciar ou controlar o ciclo em lote "Corrigir todos os Bugs".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct StartBugCycleRequest {
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Lista explícita de números de issues aprovados no modal de confirmação (se vazia, usa todos os candidatos elegíveis).
    #[serde(default)]
    pub issue_numbers: Vec<u64>,
    /// Incluir issues em `Waiting Re-plan` e `Checked Rejected`.
    #[serde(default = "default_true")]
    pub include_replan_and_rejected: bool,
}

const fn default_true() -> bool {
    true
}

/// Estado persistido e em tempo real de uma execução de ciclo de bugs em lote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BugCycleRunState {
    /// ID único do ciclo (`UUID`).
    pub cycle_id: String,
    /// ID do projeto.
    pub project_id: ProjectId,
    /// Chave do repositório.
    pub repo_key: String,
    /// Status geral (`queued`, `running`, `paused`, `completed`, `cancelled`, `failed`).
    pub status: String,
    /// Total de issues na fila do ciclo.
    pub total_issues: u32,
    /// Quantidade de issues concluídas.
    pub completed_issues: u32,
    /// Quantidade de issues com falha.
    pub failed_issues: u32,
    /// Issue atualmente em processamento.
    pub current_issue: Option<u64>,
    /// Etapa atual (`deduplicate`, `plan`, `execute`, `check`).
    pub current_stage: Option<String>,
    /// Lista ordenada de números de issues do ciclo.
    pub issue_numbers: Vec<u64>,
    /// Itens ignorados registrados no ciclo.
    pub skipped: Vec<BugCycleSkippedItem>,
    /// Mensagem de erro se falhou.
    pub error_message: Option<String>,
    /// Timestamp epoch ms de início.
    pub started_at_ms: i64,
    /// Timestamp epoch ms da última atualização.
    pub updated_at_ms: i64,
    /// Timestamp epoch ms de conclusão.
    pub finished_at_ms: Option<i64>,
}
