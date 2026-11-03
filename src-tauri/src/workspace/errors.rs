//! Códigos de erro e construtores `MtError` para o subsistema de workspace (Fase 07).

use mt_core::error::{MtError, codes};
use mt_db::{
    CONSTRAINT_LAYOUT_CONFLICT, CONSTRAINT_LAYOUT_INVALID, CONSTRAINT_PANE_LIMIT_REACHED,
    CONSTRAINT_PANE_NOT_FOUND, CONSTRAINT_PERSISTENCE_PENDING, CONSTRAINT_PROJECT_NOT_FOUND,
    CONSTRAINT_SESSION_NOT_FOUND, CONSTRAINT_SESSION_QUERY_INVALID, CONSTRAINT_TAB_NOT_FOUND,
    CONSTRAINT_TAB_REORDER_MISMATCH, CONSTRAINT_TEMPLATE_CONFLICT,
    CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED, CONSTRAINT_TEMPLATE_INVALID,
    CONSTRAINT_TEMPLATE_NOT_FOUND, CONSTRAINT_TEMPLATE_SECRET_LITERAL,
    CONSTRAINT_WORKSPACE_OPERATION_INTERRUPTED, CONSTRAINT_WORKSPACE_REQUEST_CONFLICT, DbError,
};
use serde_json::json;

/// Mapeia erros da camada `mt-db` para os códigos de erro semânticos do workspace em `mt-core`.
#[must_use]
pub fn map_db_error(err: DbError) -> MtError {
    match err {
        DbError::ConstraintViolation { ref constraint } => match constraint.as_str() {
            CONSTRAINT_TEMPLATE_NOT_FOUND => MtError::new(
                codes::TEMPLATE_NOT_FOUND,
                "O template solicitado não foi encontrado.",
            ),
            CONSTRAINT_TEMPLATE_INVALID => MtError::new(
                codes::TEMPLATE_INVALID,
                "Os dados informados para o template são inválidos.",
            ),
            CONSTRAINT_TEMPLATE_CONFLICT => MtError::new(
                codes::TEMPLATE_CONFLICT,
                "Conflito de revisão ou nome ao atualizar o template.",
            ),
            CONSTRAINT_TEMPLATE_DEFAULT_REQUIRED => MtError::new(
                codes::TEMPLATE_DEFAULT_REQUIRED,
                "Todo projeto deve manter ao menos um template marcado como padrão.",
            ),
            CONSTRAINT_TEMPLATE_SECRET_LITERAL => MtError::new(
                codes::TEMPLATE_SECRET_LITERAL,
                "Segredo literal detectado no template; utilize variáveis herdadas do ambiente.",
            ),
            CONSTRAINT_PROJECT_NOT_FOUND => MtError::new(
                codes::PROJECT_NOT_FOUND,
                "O projeto informado não foi encontrado.",
            ),
            CONSTRAINT_TAB_NOT_FOUND => {
                MtError::new(codes::TAB_NOT_FOUND, "A aba solicitada não foi encontrada.")
            }
            CONSTRAINT_TAB_REORDER_MISMATCH => MtError::new(
                codes::TAB_REORDER_MISMATCH,
                "A lista de abas informada não corresponde exatamente às abas abertas do projeto.",
            ),
            CONSTRAINT_PANE_NOT_FOUND => MtError::new(
                codes::PANE_NOT_FOUND,
                "O painel solicitado não foi encontrado nesta aba.",
            ),
            CONSTRAINT_PANE_LIMIT_REACHED => MtError::new(
                codes::PANE_LIMIT_REACHED,
                "Limite máximo de painéis ou profundidade de divisão atingido nesta aba.",
            ),
            CONSTRAINT_LAYOUT_CONFLICT => MtError::new(
                codes::LAYOUT_CONFLICT,
                "A revisão de layout da aba mudou; sincronize antes de aplicar a divisão ou redimensionamento.",
            ),
            CONSTRAINT_LAYOUT_INVALID => MtError::new(
                codes::LAYOUT_INVALID,
                "A árvore de layout de painéis informada é inválida.",
            ),
            CONSTRAINT_SESSION_NOT_FOUND => MtError::new(
                codes::SESSION_NOT_FOUND,
                "A sessão de aba solicitada não foi encontrada.",
            ),
            CONSTRAINT_SESSION_QUERY_INVALID => MtError::new(
                codes::SESSION_QUERY_INVALID,
                "A consulta de histórico ou busca na sessão é inválida.",
            ),
            CONSTRAINT_PERSISTENCE_PENDING => MtError::new(
                codes::WORKSPACE_PERSISTENCE_PENDING,
                "Aguardando conclusão da gravação de saída antes de arquivar a sessão.",
            ),
            CONSTRAINT_WORKSPACE_REQUEST_CONFLICT => MtError::new(
                codes::WORKSPACE_REQUEST_CONFLICT,
                "O identificador de requisição já foi utilizado com argumentos diferentes.",
            ),
            CONSTRAINT_WORKSPACE_OPERATION_INTERRUPTED => MtError::new(
                codes::WORKSPACE_OPERATION_INTERRUPTED,
                "A operação anterior foi interrompida por encerramento abrupto.",
            ),
            _ => err.into(),
        },
        other => other.into(),
    }
}

/// Constrói o erro quando um template solicita ambiente de agente CLI (disponível apenas na Fase 13).
#[must_use]
pub fn template_agent_unsupported(agent_id: Option<&str>) -> MtError {
    MtError::new(
        codes::WORKSPACE_ENVIRONMENT_UNAVAILABLE,
        "Ambientes de agente CLI serão disponibilizados na Fase 13.",
    )
    .with_details(json!({ "agentId": agent_id }))
}

/// Constrói o erro de falha de abertura de aba ou PTY.
#[must_use]
pub fn tab_open_failed(reason: impl Into<String>) -> MtError {
    MtError::new(codes::IO, reason.into())
}

/// Constrói o erro quando o painel informado não existe na aba.
#[must_use]
pub fn pane_not_found() -> MtError {
    MtError::new(
        codes::PANE_NOT_FOUND,
        "O painel solicitado não foi encontrado nesta aba.",
    )
}

/// Constrói o erro ao tentar reiniciar um painel cujo PTY ainda está vivo.
#[must_use]
pub fn pane_not_exited() -> MtError {
    MtError::new(
        codes::PANE_NOT_EXITED,
        "Este painel ainda está em execução; encerre-o ou inicie uma nova sessão da aba.",
    )
}

/// Constrói o erro quando o comando inicial já foi despachado ou pertence a outra geração.
#[must_use]
pub fn initial_command_stale() -> MtError {
    MtError::new(
        codes::WORKSPACE_REQUEST_CONFLICT,
        "O comando inicial desta aba já foi despachado ou pertence a uma geração anterior do painel.",
    )
}
