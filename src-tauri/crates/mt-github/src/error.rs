//! Erros tipados do domínio GitHub Issues (`mt-github`).

use mt_core::{ErrorCode, MtError, error::codes};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Resultado padrão do crate `mt-github`.
pub type GithubResult<T> = Result<T, GithubError>;

/// Código estruturado de erro retornado pelos fluxos de GitHub Issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GithubErrorKind {
    /// Projeto ou diretório não possui repositório GitHub válido.
    NotGitHubRepo,
    /// Credenciais ausentes tanto no `gh` CLI quanto no token REST.
    AuthRequired,
    /// Token ou sessão `gh` inválida (`401 Unauthorized`).
    AuthInvalid,
    /// Sem permissão no repositório (`403 Forbidden` sem rate limit).
    PermissionDenied,
    /// Cota primária ou secundária da API atingida (`403`/`429`).
    RateLimited,
    /// Falha de rede ou timeout.
    NetworkUnavailable,
    /// Issue ou repositório não encontrado (`404 Not Found`).
    NotFound,
    /// Validação de entrada falhou (`422 Unprocessable Entity` ou regra local).
    ValidationFailed,
    /// Conflito de estado (ex.: ciclo em lote já em andamento).
    Conflict,
    /// Ponte com a extensão `MaxCode` indisponível e sem fallback aplicável.
    BridgeUnavailable,
    /// Falha ao resolver ou criar worktree Git.
    WorktreeFailed,
    /// Erro de persistência SQLite.
    Database,
    /// Erro interno inesperado.
    Internal,
}

impl GithubErrorKind {
    /// Converte o código específico para o `ErrorCode` canônico do `mt-core`.
    #[must_use]
    pub const fn to_core_code(self) -> ErrorCode {
        match self {
            Self::NotGitHubRepo | Self::ValidationFailed => codes::INVALID_ARGUMENT,
            Self::AuthRequired | Self::AuthInvalid | Self::PermissionDenied => {
                codes::PERMISSION_DENIED
            }
            Self::NotFound => codes::NOT_FOUND,
            Self::Conflict => codes::CONFLICT,
            Self::RateLimited | Self::NetworkUnavailable | Self::BridgeUnavailable => codes::IO,
            Self::WorktreeFailed => codes::GIT_COMMAND_FAILED,
            Self::Database | Self::Internal => codes::INTERNAL,
        }
    }
}

/// Erro do subsistema `mt-github` com mensagem em `pt-BR`.
#[derive(Debug, Clone, Error, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
#[error("{message}")]
pub struct GithubError {
    /// Classificação do erro.
    pub kind: GithubErrorKind,
    /// Mensagem amigável em `pt-BR` pronta para exibição na interface.
    pub message: String,
    /// Detalhe técnico sanitizado opcional.
    pub details: Option<String>,
    /// Timestamp epoch ms até quando aguardar em caso de rate limit.
    pub retry_after_ms: Option<i64>,
}

impl GithubError {
    /// Constrói um novo `GithubError`.
    #[must_use]
    pub fn new(kind: GithubErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            details: None,
            retry_after_ms: None,
        }
    }

    /// Anexa detalhe técnico sanitizado.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        let sanitized = mt_db::sanitizer::sanitize(&details.into()).into_owned();
        self.details = Some(sanitized);
        self
    }

    /// Define janela de espera de rate limit.
    #[must_use]
    pub const fn with_retry_after(mut self, retry_after_ms: Option<i64>) -> Self {
        self.retry_after_ms = retry_after_ms;
        self
    }

    /// Converte para `MtError` preservando detalhes estruturados.
    #[must_use]
    pub fn into_mt_error(self) -> MtError {
        let code = self.kind.to_core_code();
        let details = serde_json::json!({
            "githubKind": self.kind,
            "details": self.details,
            "retryAfterMs": self.retry_after_ms,
        });
        MtError::new(code, self.message).with_details(details)
    }
}

impl From<GithubError> for MtError {
    fn from(value: GithubError) -> Self {
        value.into_mt_error()
    }
}

impl From<mt_db::DbError> for GithubError {
    fn from(err: mt_db::DbError) -> Self {
        Self::new(
            GithubErrorKind::Database,
            "Falha ao acessar o banco de dados local de issues do GitHub.",
        )
        .with_details(err.to_string())
    }
}

impl From<rusqlite::Error> for GithubError {
    fn from(err: rusqlite::Error) -> Self {
        Self::new(
            GithubErrorKind::Database,
            "Erro em consulta SQLite de issues do GitHub.",
        )
        .with_details(err.to_string())
    }
}
