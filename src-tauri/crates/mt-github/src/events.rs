//! Payloads dos 6 eventos Tauri do domínio GitHub Issues (`mt-github`).

use mt_core::ids::ProjectId;
use serde::{Deserialize, Serialize};

use crate::model::{
    BugCycleRunState, GithubAuthStatus, GithubTransportKind, IssueSummary, RateLimitSnapshot,
};

/// Payload do evento `github:issues-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubIssuesChangedPayload {
    /// ID do projeto afetado.
    pub project_id: ProjectId,
    /// Chave canônica do repositório (`github.com/owner/repo`).
    pub repo_key: String,
    /// Contagem atualizada de issues abertas (para o badge 🟣).
    pub open_issues_count: u32,
    /// Transporte que produziu a atualização.
    pub transport_used: GithubTransportKind,
    /// Timestamp epoch ms da sincronização.
    pub synced_at_ms: i64,
}

/// Payload do evento `github:issue-updated`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubIssueUpdatedPayload {
    /// ID do projeto afetado.
    pub project_id: ProjectId,
    /// Chave canônica do repositório.
    pub repo_key: String,
    /// Resumo atualizado da issue.
    pub issue: IssueSummary,
}

/// Payload do evento `github:labels-synced`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubLabelsSyncedPayload {
    /// ID do projeto afetado.
    pub project_id: ProjectId,
    /// Chave canônica do repositório.
    pub repo_key: String,
    /// Quantidade de labels canônicas ainda ausentes no repositório (`0` quando todas as 24 existem).
    pub missing_canonical_count: u32,
    /// Total de labels no repositório.
    pub total_labels: u32,
}

/// Payload do evento `github:auth-changed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubAuthChangedPayload {
    /// Novo status consolidado de autenticação.
    pub status: GithubAuthStatus,
}

/// Payload do evento `github:bug-cycle-progress`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubBugCycleProgressEventPayload {
    /// Estado atualizado do ciclo automatizado de bugs.
    pub cycle: BugCycleRunState,
}

/// Payload do evento `github:rate-limit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GithubRateLimitPayload {
    /// Chave do repositório quando aplicável.
    pub repo_key: Option<String>,
    /// Snapshot atualizado de cota da API do GitHub.
    pub rate_limit: RateLimitSnapshot,
}
