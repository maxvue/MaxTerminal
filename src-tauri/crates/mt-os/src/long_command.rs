//! Regra pura de notificação ao concluir comandos longos (`should_notify` e formatação de mensagem).

use mt_core::ids::{CommandBlockId, PaneId, TabId};
use mt_core::shell::BlockEndReason;
use mt_db::sanitizer::sanitize;

use crate::notify::NotificationKind;
use crate::settings::NotificationSettings;

/// Resumo de término de comando projetado a partir de `CommandFinished` (Fase 05).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandFinishedInfo {
    /// Identificador do bloco de comando.
    pub block_id: CommandBlockId,
    /// Identificador da aba.
    pub tab_id: TabId,
    /// Identificador do painel, se conhecido.
    pub pane_id: Option<PaneId>,
    /// Texto do comando executado.
    pub command: String,
    /// Código de saída do comando (`None` quando encerrado por sinal sem código ou desconhecido).
    pub exit_code: Option<i32>,
    /// Duração total da execução em milissegundos.
    pub duration_ms: u64,
    /// Razão estruturada do término do bloco.
    pub end_reason: BlockEndReason,
    /// `true` se a aba está marcada como execução contínua (daemon/servidor).
    pub is_continuous: bool,
}

/// Avalia de forma pura se o término de um comando deve disparar notificação nativa (CA-16):
/// - Retorna `None` se notificações ou alerta de comando longo estiverem desabilitados;
/// - Retorna `None` para abas contínuas;
/// - Retorna `None` se `duration_ms < threshold_secs * 1000`;
/// - Retorna `None` se `only_when_unfocused && window_focused && tab_visible`;
/// - Retorna `None` em `MissingEnd`, `ModeSwitched`, `RecoveredFromCrash`, `InterruptedBySignal`
///   ou quando `exit_code == Some(130)` (`Ctrl+C`) ou `exit_code.is_none()`;
/// - Retorna `Some(CommandFinished)` para `Some(0)` e `Some(CommandFailed)` para `Some(code != 0)`.
#[must_use]
pub fn should_notify(
    info: &CommandFinishedInfo,
    s: &NotificationSettings,
    window_focused: bool,
    _tab_visible: bool,
) -> Option<NotificationKind> {
    if !s.enabled || !s.long_command_enabled || info.is_continuous {
        return None;
    }

    let threshold_ms = u64::from(s.long_command_threshold_secs).saturating_mul(1000);
    if info.duration_ms < threshold_ms {
        return None;
    }

    if s.only_when_unfocused && window_focused {
        return None;
    }

    if !matches!(
        info.end_reason,
        BlockEndReason::ExitZero | BlockEndReason::ExitNonZero
    ) {
        return None;
    }

    let code = info.exit_code?;
    // 130 corresponde a SIGINT (Ctrl+C interativo pelo usuário)
    if code == 130 {
        return None;
    }

    if code == 0 {
        Some(NotificationKind::CommandFinished)
    } else {
        Some(NotificationKind::CommandFailed)
    }
}

/// Formata título e corpo em `pt-BR` para a notificação de comando longo.
#[must_use]
pub fn format_long_command_notification(info: &CommandFinishedInfo) -> (String, String) {
    let code = info.exit_code.unwrap_or(0);
    let title = if code == 0 {
        String::from("Comando concluído")
    } else {
        String::from("Comando falhou")
    };

    let clean_cmd = sanitize(&info.command);
    let cmd_single_line = clean_cmd.split_whitespace().collect::<Vec<_>>().join(" ");
    let cmd_display = if cmd_single_line.is_empty() {
        String::from("comando")
    } else {
        cmd_single_line
    };

    let duration_label = format_duration_ms(info.duration_ms);
    let body = format!("{cmd_display} — {duration_label} (exit {code})");
    (title, body)
}

fn format_duration_ms(duration_ms: u64) -> String {
    let total_secs = duration_ms / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    if mins > 0 {
        format!("{mins}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_info(
        duration_ms: u64,
        exit_code: Option<i32>,
        end_reason: BlockEndReason,
    ) -> CommandFinishedInfo {
        CommandFinishedInfo {
            block_id: CommandBlockId::new(),
            tab_id: TabId::new(),
            pane_id: None,
            command: String::from("cargo test --workspace"),
            exit_code,
            duration_ms,
            end_reason,
            is_continuous: false,
        }
    }

    #[test]
    fn evaluates_threshold_focus_and_exit_codes() {
        let settings = NotificationSettings::default(); // threshold = 15s, only_when_unfocused = true

        // Abaixo do limiar (14.9s) -> None
        let short = sample_info(14_999, Some(0), BlockEndReason::ExitZero);
        assert_eq!(should_notify(&short, &settings, false, false), None);

        // No limiar (15s), janela desfocada, exit 0 -> Some(CommandFinished)
        let ok_cmd = sample_info(15_000, Some(0), BlockEndReason::ExitZero);
        assert_eq!(
            should_notify(&ok_cmd, &settings, false, false),
            Some(NotificationKind::CommandFinished)
        );

        // Janela focada com only_when_unfocused = true -> None
        assert_eq!(should_notify(&ok_cmd, &settings, true, true), None);

        // Exit != 0 -> Some(CommandFailed)
        let fail_cmd = sample_info(20_000, Some(1), BlockEndReason::ExitNonZero);
        assert_eq!(
            should_notify(&fail_cmd, &settings, false, false),
            Some(NotificationKind::CommandFailed)
        );

        // Ctrl+C (exit 130) -> None
        let sigint = sample_info(25_000, Some(130), BlockEndReason::ExitNonZero);
        assert_eq!(should_notify(&sigint, &settings, false, false), None);
    }
}
