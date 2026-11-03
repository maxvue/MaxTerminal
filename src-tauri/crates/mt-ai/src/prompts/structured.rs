//! Prompts de sistema, usuário e reparo único para Geração Assistida Estruturada (`StructuredGenerator`).

use crate::client::dialect::{ChatMessage, Role};
use serde_json::Value;

/// Monta as mensagens iniciais para geração estruturada guiada por JSON Schema.
#[must_use]
pub fn build_structured_messages(
    schema_id: &str,
    schema: &Value,
    custom_system: Option<&str>,
    user_prompt: &str,
) -> Vec<ChatMessage> {
    let schema_str = serde_json::to_string_pretty(schema).unwrap_or_else(|_| "{}".to_string());
    let base_sys = custom_system.unwrap_or(
        "Você é o assistente de geração estruturada do MaxTerminal (idioma: português do Brasil).",
    );
    let system_content = format!(
        "{base_sys}\n\nVocê DEVE responder EXCLUSIVAMENTE com um objeto JSON estritamente válido que obedeça ao JSON Schema '{schema_id}' abaixo, sem markdown e sem texto explicativo adicional:\n{schema_str}"
    );

    vec![
        ChatMessage {
            role: Role::System,
            content: system_content,
        },
        ChatMessage {
            role: Role::User,
            content: user_prompt.to_string(),
        },
    ]
}

/// Monta a sequência de mensagens para a tentativa única de reparo após falha de validação JSON Schema.
#[must_use]
pub fn build_repair_messages(
    initial_messages: &[ChatMessage],
    invalid_response: &str,
    validation_errors: &[String],
) -> Vec<ChatMessage> {
    let mut msgs = initial_messages.to_vec();
    msgs.push(ChatMessage {
        role: Role::Assistant,
        content: invalid_response.to_string(),
    });
    let joined_errors = validation_errors.join("\n- ");
    msgs.push(ChatMessage {
        role: Role::User,
        content: format!(
            "A resposta anterior violou o JSON Schema obrigatório pelos seguintes motivos:\n- {joined_errors}\n\nCorrija e retorne APENAS o JSON válido conforme o schema."
        ),
    });
    msgs
}
