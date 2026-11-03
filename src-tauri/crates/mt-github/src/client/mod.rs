//! Clientes de transporte GitHub (`gh` CLI, REST HTTP com ETag e orquestrador de fallback).

pub mod fallback;
pub mod gh_cli;
pub mod rest;

use async_trait::async_trait;

use crate::error::GithubResult;
use crate::model::{
    GhCliStatus, GithubRepoIdentity, GithubTransportKind, IssueComment, IssueStateFilter,
    RateLimitSnapshot, RepoLabel,
};

/// Issue bruta retornada por um transporte (`gh` CLI ou REST API) antes da indexação no SQLite.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteIssueRecord {
    /// Número da issue (`#123`).
    pub number: u64,
    /// Node ID GraphQL opcional.
    pub node_id: Option<String>,
    /// Título bruto (será sanitizado na persistência).
    pub title: String,
    /// Descrição Markdown bruta (será sanitizada na persistência).
    pub body: String,
    /// Estado (`"open"` ou `"closed"`).
    pub state: String,
    /// Motivo do estado (`"completed"`, `"not_planned"`, etc.).
    pub state_reason: Option<String>,
    /// Login do autor.
    pub author_login: String,
    /// Avatar do autor.
    pub author_avatar_url: Option<String>,
    /// Logins dos assignees.
    pub assignees: Vec<String>,
    /// Título do milestone.
    pub milestone_title: Option<String>,
    /// Nomes/objetos de labels associados.
    pub labels: Vec<RepoLabel>,
    /// Total de comentários reportados pela API.
    pub comments_count: u32,
    /// URL HTML da issue.
    pub html_url: String,
    /// Timestamp epoch ms de criação.
    pub created_at_ms: i64,
    /// Timestamp epoch ms de última atualização.
    pub updated_at_ms: i64,
    /// Timestamp epoch ms de fechamento.
    pub closed_at_ms: Option<i64>,
}

/// Resultado de uma busca de listagem remota com suporte a `304 Not Modified`.
#[derive(Debug, Clone)]
pub enum RemoteFetchOutcome<T> {
    /// Dados novos recebidos (`200 OK`).
    Fresh {
        /// Dados desserializados.
        data: T,
        /// Novo `ETag` se fornecido.
        etag: Option<String>,
        /// Transporte efetivamente usado.
        transport: GithubTransportKind,
        /// Snapshot de rate limit atualizado.
        rate_limit: Option<RateLimitSnapshot>,
    },
    /// Servidor respondeu `304 Not Modified`; o cache local está atualizado.
    NotModified {
        /// Transporte consultado.
        transport: GithubTransportKind,
        /// Snapshot de rate limit atualizado.
        rate_limit: Option<RateLimitSnapshot>,
    },
}

/// Detalhe remoto completo de uma issue + comentários.
#[derive(Debug, Clone)]
pub struct RemoteIssueDetailBundle {
    /// Dados da issue.
    pub issue: RemoteIssueRecord,
    /// Comentários carregados em ordem cronológica.
    pub comments: Vec<IssueComment>,
    /// `true` se todas as páginas de comentários foram carregadas.
    pub comments_complete: bool,
    /// Transporte que atendeu à chamada.
    pub transport: GithubTransportKind,
}

/// Trait de transporte abstrato para operações remotas no GitHub (implementada por `GhCliClient`, `RestClient` e `FallbackGithubClient`).
#[async_trait]
pub trait GithubRemoteClient: Send + Sync {
    /// Inspeciona o status do `gh` CLI.
    async fn check_gh_status(&self) -> GhCliStatus;

    /// Lista issues de um repositório (`open`, `closed`, `all`).
    async fn fetch_issues(
        &self,
        repo: &GithubRepoIdentity,
        state: IssueStateFilter,
        per_page: u32,
        max_pages: u32,
        cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RemoteIssueRecord>>>;

    /// Busca o detalhe completo de uma issue e todos os seus comentários.
    async fn fetch_issue_detail(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
    ) -> GithubResult<RemoteIssueDetailBundle>;

    /// Lista todas as labels cadastradas no repositório.
    async fn fetch_repo_labels(
        &self,
        repo: &GithubRepoIdentity,
        cached_etag: Option<&str>,
    ) -> GithubResult<RemoteFetchOutcome<Vec<RepoLabel>>>;

    /// Cria uma nova label no repositório (`POST /repos/{owner}/{repo}/labels`).
    async fn create_repo_label(
        &self,
        repo: &GithubRepoIdentity,
        name: &str,
        color_hex: &str,
        description: &str,
    ) -> GithubResult<RepoLabel>;

    /// Cria uma nova issue no repositório (`POST /repos/{owner}/{repo}/issues`).
    async fn create_issue(
        &self,
        repo: &GithubRepoIdentity,
        title: &str,
        body: &str,
        labels: &[String],
        assignees: &[String],
    ) -> GithubResult<RemoteIssueRecord>;

    /// Substitui atomicamente o conjunto de labels de uma issue (`PUT /repos/{owner}/{repo}/issues/{number}/labels`).
    async fn replace_issue_labels(
        &self,
        repo: &GithubRepoIdentity,
        issue_number: u64,
        labels: &[String],
    ) -> GithubResult<Vec<RepoLabel>>;
}

/// Converte string ISO-8601 do GitHub (`"2026-03-15T12:34:56Z"`) para epoch ms de forma determinística.
#[must_use]
pub fn parse_github_timestamp_ms(iso: &str) -> i64 {
    let s = iso.trim();
    if s.len() < 19 {
        return 0;
    }
    let bytes = s.as_bytes();
    let parse_slice = |start: usize, end: usize| -> Option<i64> {
        std::str::from_utf8(bytes.get(start..end)?)
            .ok()?
            .parse::<i64>()
            .ok()
    };
    let Some(year) = parse_slice(0, 4) else {
        return 0;
    };
    let Some(month) = parse_slice(5, 7) else {
        return 0;
    };
    let Some(day) = parse_slice(8, 10) else {
        return 0;
    };
    let Some(hour) = parse_slice(11, 13) else {
        return 0;
    };
    let Some(minute) = parse_slice(14, 16) else {
        return 0;
    };
    let Some(second) = parse_slice(17, 19) else {
        return 0;
    };

    let days = days_from_civil(year, month, day);
    let total_secs = days * 86_400 + hour * 3_600 + minute * 60 + second;
    total_secs * 1_000
}

/// Algoritmo de Howard Hinnant para converter `(year, month, day)` UTC em dias desde `1970-01-01`.
const fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}
