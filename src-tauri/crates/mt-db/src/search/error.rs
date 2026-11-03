//! Taxonomia de erros do domínio `search.*` e conversão para `mt_core::MtError`.

use mt_core::{ErrorCode, MtError};
use thiserror::Error;

use crate::error::DbError;

/// Códigos estáveis do domínio `search.*`.
pub mod codes {
    use mt_core::ErrorCode;

    /// Consulta vazia, com byte nulo ou acima do limite permitido.
    pub const INVALID_QUERY: ErrorCode = ErrorCode("search.invalid_query");
    /// Filtros incompatíveis ou intervalo de datas inválido (`from_ms > until_ms`).
    pub const INVALID_FILTERS: ErrorCode = ErrorCode("search.invalid_filters");
    /// Cursor de paginação malformado, com assinatura HMAC inválida ou pertencente a outra consulta.
    pub const INVALID_CURSOR: ErrorCode = ErrorCode("search.invalid_cursor");
    /// O snapshot associado ao cursor expirou ou foi invalidado.
    pub const CURSOR_EXPIRED: ErrorCode = ErrorCode("search.cursor_expired");
    /// A busca foi cancelada explicitamente pelo usuário/store.
    pub const CANCELLED: ErrorCode = ErrorCode("search.cancelled");
    /// O tempo limite da consulta FTS5 foi excedido.
    pub const TIMEOUT: ErrorCode = ErrorCode("search.timeout");
    /// Limite de buscas concorrentes esgotado.
    pub const BUSY: ErrorCode = ErrorCode("search.busy");
    /// Índice FTS5 indisponível no momento.
    pub const INDEX_UNAVAILABLE: ErrorCode = ErrorCode("search.index_unavailable");
    /// Fonte de dados indisponível.
    pub const SOURCE_UNAVAILABLE: ErrorCode = ErrorCode("search.source_unavailable");
    /// Alvo de navegação não existe mais no banco de dados.
    pub const TARGET_MISSING: ErrorCode = ErrorCode("search.target_missing");
    /// Erro interno do motor SQLite durante a busca.
    pub const INTERNAL: ErrorCode = ErrorCode("search.internal");
}

/// Erro tipado do motor de busca global (`mt-db::search`).
#[derive(Debug, Error)]
pub enum SearchError {
    /// Consulta inválida.
    #[error("A consulta informada é inválida: {reason}")]
    InvalidQuery {
        /// Motivo legível em `pt-BR`.
        reason: &'static str,
    },
    /// Filtros inválidos.
    #[error("Os filtros de busca informados são inválidos: {reason}")]
    InvalidFilters {
        /// Motivo legível em `pt-BR`.
        reason: &'static str,
    },
    /// Cursor adulterado ou incompatível com a consulta atual.
    #[error("O cursor de paginação é inválido ou foi adulterado.")]
    InvalidCursor,
    /// Snapshot associado ao cursor expirou.
    #[error("Os resultados desta busca expiraram. Atualize a busca para continuar.")]
    CursorExpired,
    /// Busca cancelada via `search_cancel` ou substituição de geração.
    #[error("A busca foi cancelada.")]
    Cancelled,
    /// Tempo limite de execução excedido.
    #[error("O tempo limite da busca foi excedido. Refine os filtros e tente novamente.")]
    Timeout,
    /// Semáforo de concorrência ou pool de leitura ocupado.
    #[error("O motor de busca está ocupado processando outras consultas. Tente novamente.")]
    Busy,
    /// Índice FTS5 indisponível.
    #[error("O índice de busca da fonte '{source_name}' está indisponível no momento.")]
    IndexUnavailable {
        /// Nome da fonte afetada.
        source_name: &'static str,
    },
    /// Fonte de dados indisponível.
    #[error("A fonte de dados '{source_name}' não está disponível.")]
    SourceUnavailable {
        /// Nome da fonte afetada.
        source_name: &'static str,
    },
    /// Item alvo já foi expurgado ou removido.
    #[error("Este resultado não está mais disponível.")]
    TargetMissing,
    /// Erro interno do SQLite.
    #[error("Erro interno ao executar a busca: {0}")]
    Internal(#[from] rusqlite::Error),
}

impl SearchError {
    /// Retorna o `ErrorCode` canônico correspondente.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidQuery { .. } => codes::INVALID_QUERY,
            Self::InvalidFilters { .. } => codes::INVALID_FILTERS,
            Self::InvalidCursor => codes::INVALID_CURSOR,
            Self::CursorExpired => codes::CURSOR_EXPIRED,
            Self::Cancelled => codes::CANCELLED,
            Self::Timeout => codes::TIMEOUT,
            Self::Busy => codes::BUSY,
            Self::IndexUnavailable { .. } => codes::INDEX_UNAVAILABLE,
            Self::SourceUnavailable { .. } => codes::SOURCE_UNAVAILABLE,
            Self::TargetMissing => codes::TARGET_MISSING,
            Self::Internal(_) => codes::INTERNAL,
        }
    }
}

impl From<DbError> for SearchError {
    fn from(err: DbError) -> Self {
        match err {
            DbError::Busy { .. } => Self::Busy,
            DbError::QueryInvalid { .. } => Self::InvalidQuery {
                reason: "consulta FTS5 inválida",
            },
            DbError::CursorInvalid { .. } => Self::InvalidCursor,
            DbError::Sqlite(sql_err) => {
                if let rusqlite::Error::SqliteFailure(ffi, _) = &sql_err {
                    if ffi.code == rusqlite::ErrorCode::OperationInterrupted {
                        return Self::Cancelled;
                    }
                    if matches!(
                        ffi.code,
                        rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
                    ) {
                        return Self::Busy;
                    }
                }
                Self::Internal(sql_err)
            }
            _ => Self::IndexUnavailable { source_name: "db" },
        }
    }
}

impl From<SearchError> for MtError {
    fn from(err: SearchError) -> Self {
        let code = err.code();
        let msg = crate::sanitizer::sanitize(&err.to_string()).into_owned();
        MtError::new(code, msg)
    }
}
