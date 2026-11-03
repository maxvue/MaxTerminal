//! Testes do parser estrito de sugestões NL→Shell (`parse_suggestion` — `CA-17`).

#![allow(clippy::unwrap_used)]

use mt_ai::error::codes;
use mt_ai::risk::RiskLevel;
use mt_ai::suggest::parse_suggestion;

#[test]
fn accepts_valid_json_and_single_fenced_block() {
    let direct = r#"{"command":"find . -maxdepth 2 -type f","explanation":"Lista arquivos até 2 níveis.","risk":"safe"}"#;
    let parsed = parse_suggestion(direct).unwrap();
    assert_eq!(parsed.command, "find . -maxdepth 2 -type f");
    assert_eq!(parsed.explanation, "Lista arquivos até 2 níveis.");
    assert_eq!(parsed.risk, Some(RiskLevel::Safe));

    let fenced = "```json\n{\"command\":\"git status -sb\",\"explanation\":\"Exibe status curto do Git.\",\"risk\":\"safe\"}\n```";
    let parsed_fenced = parse_suggestion(fenced).unwrap();
    assert_eq!(parsed_fenced.command, "git status -sb");
}

#[test]
fn rejects_prose_empty_command_multiline_and_control_chars() {
    // Prosa fora do JSON
    let prose = "Aqui está seu comando:\n{\"command\":\"ls\",\"explanation\":\"ok\"}";
    assert_eq!(
        parse_suggestion(prose).unwrap_err().code,
        codes::AI_BAD_RESPONSE
    );

    // Comando vazio -> ai.empty_command
    let empty = r#"{"command":"   ","explanation":"vazio"}"#;
    assert_eq!(
        parse_suggestion(empty).unwrap_err().code,
        codes::AI_EMPTY_COMMAND
    );

    // Comando multilinhas -> ai.bad_response
    let multiline = "{\"command\":\"cd /tmp\\nls -la\",\"explanation\":\"duas linhas\"}";
    assert_eq!(
        parse_suggestion(multiline).unwrap_err().code,
        codes::AI_BAD_RESPONSE
    );

    // Caractere de controle ESC -> ai.bad_response
    let with_esc = "{\"command\":\"echo \\u001b[31mred\",\"explanation\":\"ansi\"}";
    assert_eq!(
        parse_suggestion(with_esc).unwrap_err().code,
        codes::AI_BAD_RESPONSE
    );
}
