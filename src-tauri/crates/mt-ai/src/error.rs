//! Códigos de erro e construtores tipados do subsistema de Inteligência Artificial (`mt-ai`).

use mt_core::{ErrorCode, MtError};
use serde_json::json;

/// Códigos canônicos `ai.<motivo>` (D-07). Nomes Rust com prefixo `AI_`; valor IPC em minúsculas.
pub mod codes {
    use mt_core::ErrorCode;

    /// Gateway do `MaxAiManager` inacessível na URL configurada.
    pub const AI_GATEWAY_UNAVAILABLE: ErrorCode = ErrorCode("ai.gateway_unavailable");
    /// Tempo limite excedido na operação com o `MaxAiManager`.
    pub const AI_TIMEOUT: ErrorCode = ErrorCode("ai.timeout");
    /// Requisição cancelada pelo usuário ou ciclo de vida do painel.
    pub const AI_CANCELLED: ErrorCode = ErrorCode("ai.cancelled");
    /// Status HTTP inesperado retornado pelo gateway.
    pub const AI_HTTP_STATUS: ErrorCode = ErrorCode("ai.http_status");
    /// Resposta malformada ou fora do contrato esperado.
    pub const AI_BAD_RESPONSE: ErrorCode = ErrorCode("ai.bad_response");
    /// Violação de JSON Schema ou de segurança de schema (`$ref` externo).
    pub const AI_SCHEMA_VALIDATION: ErrorCode = ErrorCode("ai.schema_validation");
    /// URL ou parâmetro de configuração inválido.
    pub const AI_URL_INVALID: ErrorCode = ErrorCode("ai.url_invalid");
    /// Host fora de loopback sem permissão `allow_remote = true`.
    pub const AI_URL_NOT_LOOPBACK: ErrorCode = ErrorCode("ai.url_not_loopback");
    /// Slug de consulta RAG inválido.
    pub const AI_INVALID_SLUG: ErrorCode = ErrorCode("ai.invalid_slug");
    /// Base de conhecimento RAG não encontrada (`404`).
    pub const AI_RAG_NOT_FOUND: ErrorCode = ErrorCode("ai.rag_not_found");
    /// Nenhuma seleção padrão configurada para o gatilho `?`.
    pub const AI_NO_DEFAULT_SELECTION: ErrorCode = ErrorCode("ai.no_default_selection");
    /// Harness, modelo ou nível de raciocínio ausente no catálogo.
    pub const AI_UNKNOWN_SELECTION: ErrorCode = ErrorCode("ai.unknown_selection");
    /// Limite de taxa atingido no gateway ou no cliente.
    pub const AI_RATE_LIMITED: ErrorCode = ErrorCode("ai.rate_limited");
    /// Comando sugerido ou prompt vazio.
    pub const AI_EMPTY_COMMAND: ErrorCode = ErrorCode("ai.empty_command");
    /// Integração com IA desativada nas preferências.
    pub const AI_DISABLED: ErrorCode = ErrorCode("ai.disabled");
    /// URL base do gateway não configurada.
    pub const AI_NOT_CONFIGURED: ErrorCode = ErrorCode("ai.not_configured");
    /// Capacidade solicitada indisponível no gateway.
    pub const AI_CAPABILITY_UNAVAILABLE: ErrorCode = ErrorCode("ai.capability_unavailable");
    /// Autenticação exigida pelo gateway (`401`/`403`).
    pub const AI_AUTH_REQUIRED: ErrorCode = ErrorCode("ai.auth_required");
    /// Conflito de `AiRequestId` já registrado ou concluído.
    pub const AI_REQUEST_CONFLICT: ErrorCode = ErrorCode("ai.request_conflict");
    /// Contexto do painel/sessão/PTY tornou-se obsoleto.
    pub const AI_CONTEXT_STALE: ErrorCode = ErrorCode("ai.context_stale");
    /// Conflito de revisão otimista ao salvar preferências de IA.
    pub const AI_SETTINGS_CONFLICT: ErrorCode = ErrorCode("ai.settings_conflict");
}

/// Constrói um `MtError` com código do domínio `ai.*`.
#[must_use]
pub fn ai_error(code: ErrorCode, message: impl Into<std::borrow::Cow<'static, str>>) -> MtError {
    MtError::new(code, message)
}

/// Constrói um `MtError` com código do domínio `ai.*` e metadados JSON adicionais.
#[must_use]
pub fn ai_error_with_details(
    code: ErrorCode,
    message: impl Into<std::borrow::Cow<'static, str>>,
    details: serde_json::Value,
) -> MtError {
    MtError::new(code, message).with_details(details)
}

/// Constrói o erro padronizado de gateway inacessível.
#[must_use]
pub fn gateway_unavailable(base_url: &str) -> MtError {
    ai_error_with_details(
        codes::AI_GATEWAY_UNAVAILABLE,
        format!(
            "MaxAiManager não está acessível em {base_url}. Verifique se ele está em execução."
        ),
        json!({ "baseUrl": base_url }),
    )
}

/// Constrói o erro padronizado de timeout em segundos.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn timeout_error(timeout_ms: u32) -> MtError {
    let secs = (f64::from(timeout_ms) / 1000.0).ceil() as u32;
    ai_error_with_details(
        codes::AI_TIMEOUT,
        format!("O MaxAiManager não respondeu em {secs} s."),
        json!({ "timeoutMs": timeout_ms }),
    )
}
