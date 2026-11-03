//! Motor de sugestão NL→Shell (`@` e `?`), parser estrito de resposta JSON e eventos de streaming.

use crate::config::AiSelection;
use crate::context::{AiContext, strip_ansi_and_sanitize};
use crate::error::{ai_error, codes};
use crate::ids::AiRequestId;
use crate::risk::{RiskAssessment, RiskLevel};
use mt_core::ids::PaneId;
use mt_core::{MtError, MtResult};
use serde::{Deserialize, Serialize};

/// Versão do template de prompt de sugestão NL→Shell.
pub const PROMPT_VERSION_SUGGEST: u32 = 1;
/// Limite máximo de caracteres no prompt do usuário para sugestão.
pub const MAX_SUGGEST_PROMPT_CHARS: usize = 2000;
/// Limite máximo de caracteres na explicação em português do Brasil.
pub const MAX_EXPLANATION_CHARS: usize = 400;

/// Gatilho que originou a solicitação de sugestão de comando.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AiTrigger {
    /// Gatilho `@` (seleção em cascata de Harness → Modelo → Raciocínio).
    At,
    /// Gatilho `?` (utiliza a seleção padrão configurada).
    Question,
}

impl AiTrigger {
    /// Retorna a representação persistida no histórico SQLite.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::At => "at",
            Self::Question => "question",
        }
    }
}

/// Requisição para sugerir um comando shell a partir de linguagem natural (`pt-BR`).
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SuggestRequest {
    /// Identificador único alocado pelo chamador (UUID v7).
    pub request_id: AiRequestId,
    /// Painel de terminal onde o gatilho foi acionado.
    pub pane_id: PaneId,
    /// Gatilho utilizado (`At` ou `Question`).
    pub trigger: AiTrigger,
    /// Seleção explícita (obrigatória para `At`, opcional para `Question`).
    pub selection: Option<AiSelection>,
    /// Pedido do usuário em português do Brasil.
    pub prompt: String,
    /// Identificador da sugestão anterior quando acionado via "Regenerar".
    pub regenerate_from: Option<AiRequestId>,
}

/// Sugestão de comando finalizada, validada e classificada quanto ao risco.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandSuggestion {
    /// Identificador da requisição.
    pub request_id: AiRequestId,
    /// Comando shell em linha única pronto para revisão/inserção/execução.
    pub command: String,
    /// Explicação curta em português do Brasil (`<= 400` caracteres).
    pub explanation: String,
    /// Avaliação de risco combinada (`max(local, model_hint)`).
    pub risk: RiskAssessment,
    /// Seleção efetivamente usada na geração.
    pub selection: AiSelection,
    /// Snapshot sanitizado do contexto enviado ao gateway.
    pub context_summary: AiContext,
    /// Latência total da geração em milissegundos.
    pub latency_ms: u32,
}

/// Evento emitido no `Channel<AiStreamEvent>` durante o ciclo de vida de `ai_suggest_command`.
#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AiStreamEvent {
    /// Início confirmado com seleção resolvida e contexto sanitizado.
    Started {
        /// Identificador da requisição.
        request_id: AiRequestId,
        /// Seleção resolvida.
        selection: AiSelection,
        /// Contexto sanitizado enviado ao gateway.
        context: AiContext,
    },
    /// Pedaço incremental sanitizado recebido via SSE.
    Delta {
        /// Identificador da requisição.
        request_id: AiRequestId,
        /// Delta textual sanitizado.
        text: String,
    },
    /// Conclusão com sucesso e sugestão validada.
    Completed {
        /// Identificador da requisição.
        request_id: AiRequestId,
        /// Sugestão completa.
        suggestion: CommandSuggestion,
    },
    /// Falha durante a geração ou validação.
    Failed {
        /// Identificador da requisição.
        request_id: AiRequestId,
        /// Erro canônico.
        error: MtError,
    },
    /// Requisição cancelada pelo usuário ou pelo ciclo de vida do painel.
    Cancelled {
        /// Identificador da requisição.
        request_id: AiRequestId,
    },
}

/// Estrutura bruta desserializada do JSON retornado pelo modelo.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RawModelSuggestion {
    /// Comando shell sugerido.
    pub command: String,
    /// Explicação curta em pt-BR.
    #[serde(default)]
    pub explanation: String,
    /// Dica opcional de risco sugerida pelo modelo.
    #[serde(default)]
    pub risk: Option<RiskLevel>,
}

/// Faz o parse estrito da sugestão retornada pelo modelo:
/// - Aceita JSON direto ou um único bloco cercado ```` ```json ... ``` ````.
/// - Rejeita prosa fora do JSON, múltiplos blocos ou JSON malformado (`ai.bad_response`).
/// - Rejeita `command` vazio após trim (`ai.empty_command`).
/// - Rejeita `command` contendo `\n`, `\r`, `\0`, `\x1b` (ESC) ou outros caracteres de controle (`ai.bad_response`).
/// - Nunca remove `$ ` nem junta múltiplas linhas com `&&` (`CA-17`).
pub fn parse_suggestion(raw: &str) -> MtResult<RawModelSuggestion> {
    let json_slice = extract_strict_json_object(raw)?;
    let parsed: RawModelSuggestion = serde_json::from_str(json_slice).map_err(|_| {
        ai_error(
            codes::AI_BAD_RESPONSE,
            "O modelo retornou uma resposta fora do formato JSON esperado.",
        )
    })?;

    if parsed.command.chars().any(|c| {
        c == '\n' || c == '\r' || c == '\0' || c == '\x1b' || (c.is_control() && c != '\t')
    }) {
        return Err(ai_error(
            codes::AI_BAD_RESPONSE,
            "O comando retornado contém quebras de linha ou caracteres de controle proibidos.",
        ));
    }

    let command_trimmed = parsed.command.trim();
    if command_trimmed.is_empty() {
        return Err(ai_error(
            codes::AI_EMPTY_COMMAND,
            "Não foi possível gerar um comando válido para este pedido. Reformule a solicitação.",
        ));
    }

    let sanitized_cmd = mt_db::sanitizer::sanitize(command_trimmed).into_owned();
    let clean_explanation = truncate_chars(
        &strip_ansi_and_sanitize(parsed.explanation.trim()),
        MAX_EXPLANATION_CHARS,
    );

    Ok(RawModelSuggestion {
        command: sanitized_cmd,
        explanation: clean_explanation,
        risk: parsed.risk,
    })
}

/// Extrai um único objeto JSON de `raw` (direto `{...}` ou cercado por um único bloco ```` ``` ```` sem prosa externa).
pub fn extract_strict_json_object(raw: &str) -> MtResult<&str> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ai_error(
            codes::AI_BAD_RESPONSE,
            "O MaxAiManager retornou uma resposta vazia.",
        ));
    }

    if let Some(after_open) = trimmed.strip_prefix("```") {
        let Some(newline_pos) = after_open.find('\n') else {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "Bloco markdown JSON incompleto na resposta da IA.",
            ));
        };
        let lang_tag = after_open[..newline_pos].trim();
        if !lang_tag.is_empty() && !lang_tag.eq_ignore_ascii_case("json") {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "Bloco de código retornado pela IA não é JSON.",
            ));
        }
        let body_and_close = &after_open[newline_pos + 1..];
        let Some(without_close) = body_and_close.strip_suffix("```") else {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "Prosa externa ou fechamento inválido fora do bloco JSON.",
            ));
        };
        if without_close.contains("```") {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "Múltiplos blocos de código detectados na resposta da IA.",
            ));
        }
        let inner = without_close.trim();
        if !inner.starts_with('{') || !inner.ends_with('}') {
            return Err(ai_error(
                codes::AI_BAD_RESPONSE,
                "O conteúdo do bloco JSON não é um objeto JSON.",
            ));
        }
        return Ok(inner);
    }

    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed);
    }

    Err(ai_error(
        codes::AI_BAD_RESPONSE,
        "A resposta da IA contém prosa fora do objeto JSON esperado.",
    ))
}

pub(crate) fn truncate_chars(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let mut out: String = input.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}
