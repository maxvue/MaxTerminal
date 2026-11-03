//! Catálogo de códigos de erro tipados e construtores de `MtError` para `mt-agents`.

use mt_core::{ErrorCode, MtError};
use serde_json::json;

/// Códigos de erro canônicos da Fase 13 (Agentes CLI, Telemetria, MaxSkills e Memória).
pub mod codes {
    /// Binário do agente não foi encontrado no `PATH`.
    pub const NOT_INSTALLED: &str = "agents.not_installed";
    /// Identificador de agente desconhecido.
    pub const UNKNOWN_AGENT: &str = "agents.unknown_agent";
    /// Configuração `agents.toml` ou `agents.settings` inválida.
    pub const SETTINGS_INVALID: &str = "agents.settings_invalid";
    /// Conflito de atalho de teclado entre agentes ou com atalhos reservados.
    pub const SHORTCUT_CONFLICT: &str = "agents.shortcut_conflict";
    /// Sintaxe de atalho de teclado inválida.
    pub const SHORTCUT_INVALID: &str = "agents.shortcut_invalid";
    /// Pasta do projeto não é um repositório Git.
    pub const NOT_A_REPO: &str = "agents.not_a_repo";
    /// Worktree informada pertence a outro repositório.
    pub const WORKTREE_FOREIGN: &str = "agents.worktree_foreign";
    /// Worktree possui alterações ou arquivos ignorados que impedem remoção automática.
    pub const WORKTREE_DIRTY: &str = "agents.worktree_dirty";
    /// Vínculo de aba ↔ worktree não encontrado.
    pub const LINK_NOT_FOUND: &str = "agents.link_not_found";
    /// Vínculo de worktree já foi resolvido anteriormente.
    pub const LINK_RESOLVED: &str = "agents.link_already_resolved";
    /// Servidor Unix domain socket do relay indisponível.
    pub const RELAY_UNAVAILABLE: &str = "agents.relay_unavailable";
    /// Aba solicitada não corresponde a uma execução de agente CLI.
    pub const TAB_NOT_AGENT: &str = "agents.tab_not_agent";
    /// Agente não suporta seleção de modelo por flag ou modelo é incompatível.
    pub const MODEL_UNSUPPORTED: &str = "agents.model_unsupported";
    /// Token de inspeção de worktree expirou ou o estado do repositório mudou.
    pub const INSPECTION_STALE: &str = "agents.inspection_stale";
    /// Worktree ou aba ainda possui processos/abas ativos em uso.
    pub const EXECUTION_IN_USE: &str = "agents.execution_in_use";
    /// Template dedicado de agente foi excluído pelo usuário.
    pub const TEMPLATE_DELETED: &str = "agents.template_deleted";

    /// Diretório-fonte do repositório `MaxSkills` não existe.
    pub const SKILLS_SOURCE_MISSING: &str = "skills.source_missing";
    /// Arquivo `index.json` do `MaxSkills` é inválido.
    pub const SKILLS_INDEX_INVALID: &str = "skills.index_invalid";
    /// O estado da fonte ou do destino mudou desde que o plano (`skills_plan`) foi gerado.
    pub const SKILLS_PLAN_STALE: &str = "skills.plan_stale";
    /// Alvo de sincronização inválido ou com symlink ancestral inseguro.
    pub const SKILLS_TARGET_INVALID: &str = "skills.target_invalid";
    /// Agente não possui destino de skills documentado sem `dest_override` explícito.
    pub const SKILLS_CAPABILITY_UNVERIFIED: &str = "skills.capability_unverified";
    /// Limite de profundidade, quantidade de arquivos ou tamanho de skill excedido.
    pub const SKILLS_LIMIT_EXCEEDED: &str = "skills.limit_exceeded";
    /// Falha de I/O ao ler, copiar ou criar symlink de skill.
    pub const SKILLS_IO: &str = "skills.io";

    /// Registro de comando não encontrado na memória contextual.
    pub const MEMORY_NOT_FOUND: &str = "memory.not_found";
    /// Limite de comandos fixados (`pinned`) por projeto atingido.
    pub const MEMORY_PIN_LIMIT: &str = "memory.pin_limit";
}

/// Constrói erro indicando que o agente não está instalado no `PATH`.
#[must_use]
pub fn not_installed(agent_display: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::NOT_INSTALLED),
        format!("Agente {agent_display} não encontrado no PATH"),
    )
}

/// Constrói erro para identificador de agente desconhecido.
#[must_use]
pub fn unknown_agent(raw: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::UNKNOWN_AGENT),
        format!("Agente desconhecido: {raw}"),
    )
    .with_details(json!({ "agentId": raw }))
}

/// Constrói erro para configuração de agentes inválida.
#[must_use]
pub fn settings_invalid(
    message: impl Into<String>,
    line: Option<usize>,
    column: Option<usize>,
) -> MtError {
    let msg = message.into();
    MtError::new(
        ErrorCode(codes::SETTINGS_INVALID),
        format!("Configuração de agentes inválida: {msg}"),
    )
    .with_details(json!({ "line": line, "column": column, "message": msg }))
}

/// Constrói erro para sintaxe de atalho de teclado inválida.
#[must_use]
pub fn shortcut_invalid(shortcut: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::SHORTCUT_INVALID),
        format!("Atalho de teclado inválido: {shortcut}"),
    )
    .with_details(json!({ "shortcut": shortcut }))
}

/// Constrói erro para colisão de atalho de teclado.
#[must_use]
pub fn shortcut_conflict(shortcut: &str, conflicts_with: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::SHORTCUT_CONFLICT),
        format!("O atalho '{shortcut}' conflita com '{conflicts_with}'"),
    )
    .with_details(json!({ "shortcut": shortcut, "conflictsWith": conflicts_with }))
}

/// Constrói erro quando o diretório do projeto não é um repositório Git.
#[must_use]
pub fn not_a_repo(path: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::NOT_A_REPO),
        format!("O diretório '{path}' não é um repositório Git válido."),
    )
    .with_details(json!({ "path": path }))
}

/// Constrói erro quando a worktree pertence a outro repositório Git.
#[must_use]
pub fn worktree_foreign(path: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::WORKTREE_FOREIGN),
        format!("A worktree '{path}' não pertence a este repositório."),
    )
    .with_details(json!({ "path": path }))
}

/// Constrói erro quando a worktree possui alterações pendentes ou arquivos ignorados.
#[must_use]
pub fn worktree_dirty(reason: impl Into<String>) -> MtError {
    MtError::new(ErrorCode(codes::WORKTREE_DIRTY), reason.into())
}

/// Constrói erro quando o vínculo aba ↔ worktree não é encontrado.
#[must_use]
pub fn link_not_found(id_desc: impl Into<String>) -> MtError {
    let desc = id_desc.into();
    MtError::new(
        ErrorCode(codes::LINK_NOT_FOUND),
        format!("Vínculo de worktree do agente não encontrado ({desc})."),
    )
}

/// Constrói erro quando o vínculo de worktree já foi resolvido.
#[must_use]
pub fn link_resolved(link_id: i64) -> MtError {
    MtError::new(
        ErrorCode(codes::LINK_RESOLVED),
        format!("O vínculo de worktree #{link_id} já foi resolvido."),
    )
    .with_details(json!({ "linkId": link_id }))
}

/// Constrói erro quando o servidor de relay Unix socket está indisponível.
#[must_use]
pub fn relay_unavailable(reason: impl Into<String>) -> MtError {
    let msg = reason.into();
    MtError::new(
        ErrorCode(codes::RELAY_UNAVAILABLE),
        format!("Canal de relay do agente indisponível: {msg}"),
    )
}

/// Constrói erro quando o modelo solicitado não é suportado pelo agente.
#[must_use]
pub fn model_unsupported(agent: &str, model: &str, reason: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::MODEL_UNSUPPORTED),
        format!("Modelo '{model}' não suportado para {agent}: {reason}"),
    )
    .with_details(json!({ "agent": agent, "model": model, "reason": reason }))
}

/// Constrói erro quando a inspeção de fechamento da worktree está desatualizada.
#[must_use]
pub fn inspection_stale(link_id: i64) -> MtError {
    MtError::new(
        ErrorCode(codes::INSPECTION_STALE),
        "O estado da worktree mudou desde a última inspeção. Revise os dados atualizados antes de prosseguir.",
    )
    .with_details(json!({ "linkId": link_id }))
}

/// Constrói erro quando a worktree ainda está em uso por processos ou abas.
#[must_use]
pub fn execution_in_use(reason: impl Into<String>) -> MtError {
    MtError::new(ErrorCode(codes::EXECUTION_IN_USE), reason.into())
}

/// Constrói erro quando a aba não corresponde a um agente CLI.
#[must_use]
pub fn tab_not_agent(tab_id: impl Into<String>) -> MtError {
    let tid = tab_id.into();
    MtError::new(
        ErrorCode(codes::TAB_NOT_AGENT),
        format!("A aba '{tid}' não está associada a um agente CLI."),
    )
}

/// Constrói erro quando o diretório fonte de `MaxSkills` não existe.
#[must_use]
pub fn skills_source_missing(path: &std::path::Path) -> MtError {
    let p = path.to_string_lossy().to_string();
    MtError::new(
        ErrorCode(codes::SKILLS_SOURCE_MISSING),
        format!("Diretório fonte do MaxSkills não encontrado: {p}"),
    )
    .with_details(json!({ "sourceDir": p }))
}

/// Constrói erro quando o índice ou catálogo do `MaxSkills` é inválido.
#[must_use]
pub fn skills_index_invalid(reason: impl Into<String>) -> MtError {
    let msg = reason.into();
    MtError::new(
        ErrorCode(codes::SKILLS_INDEX_INVALID),
        format!("Catálogo de skills inválido: {msg}"),
    )
}

/// Constrói erro quando o plano de sincronização expirou ou o estado do disco mudou.
#[must_use]
pub fn skills_plan_stale() -> MtError {
    MtError::new(
        ErrorCode(codes::SKILLS_PLAN_STALE),
        "O estado das skills no disco mudou desde que o plano foi gerado. Gere um novo plano antes de sincronizar.",
    )
}

/// Constrói erro quando o alvo de sincronização é inválido.
#[must_use]
pub fn skills_target_invalid(reason: impl Into<String>) -> MtError {
    MtError::new(ErrorCode(codes::SKILLS_TARGET_INVALID), reason.into())
}

/// Constrói erro quando o agente exige `dest_override` explícito.
#[must_use]
pub fn skills_capability_unverified(reason: impl Into<String>) -> MtError {
    MtError::new(
        ErrorCode(codes::SKILLS_CAPABILITY_UNVERIFIED),
        reason.into(),
    )
}

/// Constrói erro quando uma skill excede limites de profundidade, arquivos ou bytes.
#[must_use]
pub fn skills_limit_exceeded(reason: impl Into<String>) -> MtError {
    MtError::new(ErrorCode(codes::SKILLS_LIMIT_EXCEEDED), reason.into())
}

/// Constrói erro de I/O na manipulação de skills.
#[must_use]
pub fn skills_io(reason: impl Into<String>) -> MtError {
    MtError::new(ErrorCode(codes::SKILLS_IO), reason.into())
}

/// Constrói erro quando um comando não é encontrado na memória contextual.
#[must_use]
pub fn memory_not_found(command: &str) -> MtError {
    MtError::new(
        ErrorCode(codes::MEMORY_NOT_FOUND),
        format!("Comando não encontrado na memória contextual: {command}"),
    )
}

/// Constrói erro quando o limite de comandos fixados por projeto é atingido.
#[must_use]
pub fn memory_pin_limit(max: u32) -> MtError {
    MtError::new(
        ErrorCode(codes::MEMORY_PIN_LIMIT),
        format!("Limite máximo de {max} comandos fixados atingido para este projeto."),
    )
    .with_details(json!({ "maxPinned": max }))
}
