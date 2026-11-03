//! Parser de argumentos de linha de comando (`CliAction` e `parse_cli_args`) para single-instance.

use serde::{Deserialize, Serialize};

/// Ação reconhecida na whitelist estrita de argumentos CLI do `MaxTerminal`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type, Default)]
#[serde(rename_all = "snake_case")]
pub enum CliAction {
    /// Alterna a visibilidade da janela (`--toggle`).
    Toggle,
    /// Exibe e foca a janela (`--show`).
    Show,
    /// Oculta a janela (`--hide`).
    Hide,
    /// Inicia oculto na bandeja no cold start (`--start-hidden`).
    StartHidden,
    /// Nenhuma flag de controle de janela informada.
    #[default]
    None,
}

/// Analisa `args` aplicando a whitelist estrita e prioridade explícita:
/// `--show > --toggle > --hide > --start-hidden`.
///
/// Qualquer outro argumento é ignorado sem ecoar o valor bruto nos logs.
pub fn parse_cli_args<I: IntoIterator<Item = S>, S: AsRef<str>>(args: I) -> CliAction {
    let mut has_show = false;
    let mut has_toggle = false;
    let mut has_hide = false;
    let mut has_start_hidden = false;

    for (idx, raw) in args.into_iter().enumerate() {
        let token = raw.as_ref().trim();
        match token {
            "--show" => has_show = true,
            "--toggle" => has_toggle = true,
            "--hide" => has_hide = true,
            "--start-hidden" => has_start_hidden = true,
            _ => {
                if idx > 0 && !token.starts_with("maxterminal://") {
                    tracing::debug!(
                        target: "mt_os",
                        arg_index = idx,
                        "Argumento CLI desconhecido ignorado pela whitelist"
                    );
                }
            }
        }
    }

    if has_show {
        CliAction::Show
    } else if has_toggle {
        CliAction::Toggle
    } else if has_hide {
        CliAction::Hide
    } else if has_start_hidden {
        CliAction::StartHidden
    } else {
        CliAction::None
    }
}
