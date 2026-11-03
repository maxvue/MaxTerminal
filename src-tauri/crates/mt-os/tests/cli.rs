//! Testes de `parse_cli_args` (`mt-os`).

use mt_os::{CliAction, parse_cli_args};

#[test]
fn cli_whitelist_and_priority_order() {
    assert_eq!(parse_cli_args(["maxterminal"]), CliAction::None);
    assert_eq!(
        parse_cli_args(["maxterminal", "--unknown", "/tmp/secret"]),
        CliAction::None
    );
    assert_eq!(
        parse_cli_args(["maxterminal", "--start-hidden"]),
        CliAction::StartHidden
    );
    assert_eq!(
        parse_cli_args(["maxterminal", "--hide", "--start-hidden"]),
        CliAction::Hide
    );
    // Combinação --start-hidden --toggle -> Toggle tem prioridade
    assert_eq!(
        parse_cli_args(["maxterminal", "--start-hidden", "--toggle"]),
        CliAction::Toggle
    );
    // --show tem prioridade máxima
    assert_eq!(
        parse_cli_args(["maxterminal", "--hide", "--toggle", "--show"]),
        CliAction::Show
    );
}
