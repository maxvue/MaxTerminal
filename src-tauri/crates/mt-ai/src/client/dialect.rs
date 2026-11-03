//! Dialeto de tradução HTTP/SSE entre o `MaxTerminal` e o gateway `MaxAiManager`.

use crate::config::AiSelection;
use crate::error::{ai_error, codes};
use mt_core::MtResult;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Estado de disponibilidade de uma capacidade individual do gateway.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CapabilityState {
    /// Capacidade validada e operacional.
    Validated,
    /// Capacidade indisponível pelo motivo descrito.
    Unavailable {
        /// Explicação em português do Brasil.
        reason: String,
    },
}

/// Mapa de capacidades validadas do gateway `MaxAiManager`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct GatewayCapabilities {
    /// Descoberta de harnesses e modelos (`/v1/harnesses`).
    pub catalog: CapabilityState,
    /// Geração de respostas (`/v1/chat/completions`).
    pub generation: CapabilityState,
    /// Parâmetro `reasoning_effort` suportado.
    pub reasoning: CapabilityState,
    /// Modo de resposta estruturada `response_format: { type: "json_object" }`.
    pub json_mode: CapabilityState,
    /// Streaming de tokens via Server-Sent Events (`text/event-stream`).
    pub sse: CapabilityState,
    /// Busca de documentação RAG (`/v1/rags/{slug}/search`).
    pub rag_search: CapabilityState,
    /// Listagem de bases RAG (`/v1/rags`).
    pub rag_list: CapabilityState,
    /// Endpoint de saúde (`/health`).
    pub health: CapabilityState,
}

impl GatewayCapabilities {
    /// Constrói um conjunto de capacidades todas marcadas como `Unavailable` pelo motivo informado.
    #[must_use]
    pub fn unconfigured(reason: &str) -> Self {
        let u = || CapabilityState::Unavailable {
            reason: reason.to_string(),
        };
        Self {
            catalog: u(),
            generation: u(),
            reasoning: u(),
            json_mode: u(),
            sse: u(),
            rag_search: u(),
            rag_list: u(),
            health: u(),
        }
    }

    /// Constrói um conjunto com todas as capacidades validadas.
    #[must_use]
    pub fn all_validated() -> Self {
        Self {
            catalog: CapabilityState::Validated,
            generation: CapabilityState::Validated,
            reasoning: CapabilityState::Validated,
            json_mode: CapabilityState::Validated,
            sse: CapabilityState::Validated,
            rag_search: CapabilityState::Validated,
            rag_list: CapabilityState::Validated,
            health: CapabilityState::Validated,
        }
    }
}

/// Papel da mensagem na conversa enviada ao gateway.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    /// Instrução de sistema.
    System,
    /// Entrada do usuário.
    User,
    /// Resposta prévia do assistente.
    Assistant,
}

/// Mensagem individual de chat enviada ao gateway.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    /// Papel da mensagem.
    pub role: Role,
    /// Conteúdo textual sanitizado.
    pub content: String,
}

/// Requisição de geração enviada ao gateway via `GatewayDialect`.
#[derive(Clone, Debug)]
pub struct ChatRequest {
    /// Seleção de harness/modelo/raciocínio.
    pub selection: AiSelection,
    /// Sequência de mensagens da conversa.
    pub messages: Vec<ChatMessage>,
    /// Solicita resposta em modo JSON.
    pub json_mode: bool,
    /// Solicita streaming SSE quando suportado.
    pub stream: bool,
    /// Limite opcional de tokens gerados.
    pub max_tokens: Option<u32>,
}

/// Métricas de consumo de tokens retornadas pelo gateway.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    /// Tokens de entrada (prompt).
    pub prompt_tokens: Option<u32>,
    /// Tokens gerados na conclusão.
    pub completion_tokens: Option<u32>,
    /// Total de tokens consumidos.
    pub total_tokens: Option<u32>,
}

/// Resposta consolidada de uma chamada `chat` ao gateway.
#[derive(Clone, Debug)]
pub struct ChatResponse {
    /// Conteúdo textual final sanitizado.
    pub content: String,
    /// Identificador do modelo efetivamente utilizado.
    pub model: Option<String>,
    /// Estatísticas de uso de tokens.
    pub usage: Option<Usage>,
}

/// Ponto único de tradução entre o `MaxTerminal` e o protocolo HTTP do gateway `MaxAiManager` (`R-01`).
pub trait GatewayDialect: Send + Sync {
    /// Identificador estável do dialeto.
    fn id(&self) -> &'static str;
    /// Capacidades conhecidas do dialeto.
    fn capabilities(&self) -> GatewayCapabilities;
    /// Caminho relativo do health check (`/health`).
    fn health_path(&self) -> &str;
    /// Caminho relativo do catálogo de harnesses (`/v1/harnesses`).
    fn catalog_path(&self) -> &str;
    /// Caminho relativo de chat completions (`/v1/chat/completions`).
    fn chat_path(&self) -> &str;
    /// Caminho relativo de listagem de bases RAG (`/v1/rags`).
    fn rag_list_path(&self) -> &str;
    /// Headers adicionais específicos para uma seleção.
    fn extra_headers(&self, selection: Option<&AiSelection>) -> Vec<(String, String)>;
    /// Constrói o corpo JSON para `/v1/chat/completions`.
    fn build_chat_body(&self, req: &ChatRequest) -> Value;
    /// Extrai a `ChatResponse` de um JSON não-streaming.
    fn parse_chat_response(&self, v: Value) -> MtResult<ChatResponse>;
    /// Extrai o delta de texto de uma linha `data:` SSE.
    fn parse_sse_delta(&self, data: &str) -> MtResult<Option<String>>;
    /// Indica se o payload `data:` representa o marcador terminal `[DONE]`.
    fn is_sse_done(&self, data: &str) -> bool;
}

/// Dialeto compatível `OpenAI` (`/v1/chat/completions`, `/v1/harnesses`, `/v1/rags/{slug}/search`).
pub struct OpenAiCompatDialect {
    capabilities: GatewayCapabilities,
}

impl OpenAiCompatDialect {
    /// Cria uma instância do dialeto com as capacidades informadas.
    #[must_use]
    pub fn new(capabilities: GatewayCapabilities) -> Self {
        Self { capabilities }
    }

    /// Cria uma instância do dialeto com todas as capacidades validadas.
    #[must_use]
    pub fn for_tests() -> Self {
        Self::new(GatewayCapabilities::all_validated())
    }
}

impl Default for OpenAiCompatDialect {
    fn default() -> Self {
        Self::new(GatewayCapabilities::all_validated())
    }
}

impl GatewayDialect for OpenAiCompatDialect {
    fn id(&self) -> &'static str {
        "openai-compat-v1"
    }

    fn capabilities(&self) -> GatewayCapabilities {
        self.capabilities.clone()
    }

    fn health_path(&self) -> &'static str {
        "/health"
    }

    fn catalog_path(&self) -> &'static str {
        "/v1/harnesses"
    }

    fn chat_path(&self) -> &'static str {
        "/v1/chat/completions"
    }

    fn rag_list_path(&self) -> &'static str {
        "/v1/rags"
    }

    fn extra_headers(&self, selection: Option<&AiSelection>) -> Vec<(String, String)> {
        match selection {
            Some(sel) => vec![("X-Harness".to_string(), sel.harness_id.clone())],
            None => Vec::new(),
        }
    }

    fn build_chat_body(&self, req: &ChatRequest) -> Value {
        let messages: Vec<Value> = req
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                };
                json!({
                    "role": role,
                    "content": mt_db::sanitizer::sanitize(&m.content).into_owned(),
                })
            })
            .collect();

        let mut body = json!({
            "model": format!("{}/{}", req.selection.harness_id, req.selection.model_id),
            "stream": req.stream && matches!(self.capabilities.sse, CapabilityState::Validated),
            "messages": messages,
        });

        if let Some(reasoning) = &req.selection.reasoning_id
            && matches!(self.capabilities.reasoning, CapabilityState::Validated)
        {
            body["reasoning_effort"] = Value::String(reasoning.clone());
        }
        if req.json_mode && matches!(self.capabilities.json_mode, CapabilityState::Validated) {
            body["response_format"] = json!({ "type": "json_object" });
        }
        if let Some(max_tokens) = req.max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }
        body
    }

    fn parse_chat_response(&self, v: Value) -> MtResult<ChatResponse> {
        let content = v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("message"))
            .and_then(|msg| msg.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                ai_error(
                    codes::AI_BAD_RESPONSE,
                    "A resposta do MaxAiManager não contém 'choices[0].message.content'.",
                )
            })?;

        let model = v
            .get("model")
            .and_then(|m| m.as_str())
            .map(ToOwned::to_owned);
        let usage = v
            .get("usage")
            .and_then(|u| serde_json::from_value(u.clone()).ok());

        Ok(ChatResponse {
            content: mt_db::sanitizer::sanitize(content).into_owned(),
            model,
            usage,
        })
    }

    fn parse_sse_delta(&self, data: &str) -> MtResult<Option<String>> {
        let trimmed = data.trim();
        if trimmed.is_empty() || self.is_sse_done(trimmed) {
            return Ok(None);
        }
        let v: Value = serde_json::from_str(trimmed).map_err(|_| {
            ai_error(
                codes::AI_BAD_RESPONSE,
                "Frame SSE malformado retornado pelo MaxAiManager.",
            )
        })?;
        let delta = v
            .get("choices")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
            .map(ToOwned::to_owned);
        Ok(delta)
    }

    fn is_sse_done(&self, data: &str) -> bool {
        data.trim() == "[DONE]"
    }
}
