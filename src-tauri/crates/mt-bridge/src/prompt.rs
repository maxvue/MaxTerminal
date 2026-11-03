//! Formatação canônica e truncamento UTF-8 seguro do prompt de inspeção em Plan Mode para o `MaxCode`.

/// Orçamento máximo em bytes para o texto do comando no payload de inspeção (4 KiB).
pub const MAX_PROMPT_COMMAND_BYTES: usize = 4 * 1024;
/// Orçamento máximo em bytes para a saída de erro no payload de inspeção (48 KiB).
pub const MAX_PROMPT_ERROR_BYTES: usize = 48 * 1024;
/// Sufixo indicador de truncamento quando o comando ou erro excede o orçamento.
pub const TRUNCATION_SUFFIX: &str = "\n...[truncado]";

/// Resultado preparado e truncado em fronteira de caractere UTF-8 para envio ao `MaxCode`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedInspectPrompt {
    /// Comando truncado em fronteira UTF-8 segura.
    pub command: String,
    /// Saída de erro truncada em fronteira UTF-8 segura.
    pub error_output: String,
    /// Prompt canônico obrigatório: `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"`.
    pub formatted_prompt: String,
    /// Indica se houve truncamento de tamanho.
    pub truncated: bool,
}

/// Formata literalmente o prompt canônico exigido para o Plan Mode no `MaxCode`:
///
/// `Ao executar o comando "${comando}" tive o seguinte erro: "${erro}"`
#[must_use]
pub fn format_plan_mode_inspect_prompt(command: &str, error_output: &str) -> String {
    format!("Ao executar o comando \"{command}\" tive o seguinte erro: \"{error_output}\"")
}

/// Trunca `command` (até 4 KiB) e `error_output` (até 48 KiB) preservando fronteiras de codepoint UTF-8
/// e monta o prompt canônico de Plan Mode.
#[must_use]
pub fn prepare_inspect_payload(
    sanitized_command: &str,
    sanitized_error: &str,
) -> PreparedInspectPrompt {
    let (cmd, cmd_trunc) = truncate_utf8_with_suffix(sanitized_command, MAX_PROMPT_COMMAND_BYTES);
    let (err, err_trunc) = truncate_utf8_with_suffix(sanitized_error, MAX_PROMPT_ERROR_BYTES);
    let formatted_prompt = format_plan_mode_inspect_prompt(&cmd, &err);

    PreparedInspectPrompt {
        command: cmd,
        error_output: err,
        formatted_prompt,
        truncated: cmd_trunc || err_trunc,
    }
}

fn truncate_utf8_with_suffix(input: &str, max_bytes: usize) -> (String, bool) {
    if input.len() <= max_bytes {
        return (input.to_string(), false);
    }
    let suffix_len = TRUNCATION_SUFFIX.len();
    let budget = max_bytes.saturating_sub(suffix_len);
    let mut end = budget.min(input.len());
    while end > 0 && !input.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + suffix_len);
    out.push_str(&input[..end]);
    out.push_str(TRUNCATION_SUFFIX);
    (out, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_exact_canonical_plan_mode_prompt() {
        let prep = prepare_inspect_payload("php artisan migrate", "SQLSTATE[HY000] [2002]");
        assert_eq!(
            prep.formatted_prompt,
            "Ao executar o comando \"php artisan migrate\" tive o seguinte erro: \"SQLSTATE[HY000] [2002]\""
        );
        assert!(!prep.truncated);
    }

    #[test]
    fn truncates_multibyte_utf8_without_panicking() {
        let long_err = "áéíóú".repeat(12_000);
        let prep = prepare_inspect_payload("npm test", &long_err);
        assert!(prep.truncated);
        assert!(prep.error_output.len() <= MAX_PROMPT_ERROR_BYTES);
        assert!(prep.error_output.ends_with(TRUNCATION_SUFFIX));
    }
}
