//! Comandos IPC de integração com o sistema operacional (`os_*` — Fase 09).

use std::sync::Arc;

use mt_core::error::MtResult;
use mt_core::ids::TabId;
use mt_os::{
    AttentionReason, AutostartStatus, LinkCandidate, LinkContext, NotifyOutcome, NotifyRequest,
    OsEnvironment, OsIntegration, OsRuntimeSnapshot, OsSettings, OsSettingsPatch, QuitOutcome,
    QuitSource, ResolvedLink, ShortcutStatus, StatusCounters, ToggleDecisionDto, ToggleSource,
    probe,
};
use tauri::State;

/// Retorna as preferências atuais de integração com o sistema operacional (`os_settings_get`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_settings_get(os: State<'_, Arc<OsIntegration>>) -> MtResult<OsSettings> {
    let settings = os.settings.read().clone();
    Ok(settings)
}

/// Atualiza parcialmente e persiste as preferências do sistema operacional (`os_settings_update`).
#[tauri::command]
#[specta::specta]
pub async fn os_settings_update(
    os: State<'_, Arc<OsIntegration>>,
    patch: OsSettingsPatch,
) -> MtResult<OsSettings> {
    os.update_settings(patch).await
}

/// Retorna ou re-sonda as informações do ambiente gráfico e capacidades D-Bus (`os_environment_info`).
#[tauri::command]
#[specta::specta]
pub async fn os_environment_info(
    os: State<'_, Arc<OsIntegration>>,
    refresh: bool,
) -> MtResult<OsEnvironment> {
    if refresh {
        let tray_prev = os.env.read().tray_available;
        let mut probed = probe(&|k| std::env::var(k).ok()).await;
        probed.tray_available = probed.tray_available || tray_prev;
        *os.env.write() = probed.clone();
        Ok(probed)
    } else {
        let current = os.env.read().clone();
        Ok(current)
    }
}

/// Retorna o status atual do registro do atalho global (`os_shortcut_status`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_shortcut_status(os: State<'_, Arc<OsIntegration>>) -> MtResult<ShortcutStatus> {
    Ok(os.shortcuts.status())
}

/// Solicita configuração do atalho no portal XDG (`os_shortcut_configure_portal`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_shortcut_configure_portal(os: State<'_, Arc<OsIntegration>>) -> MtResult<ShortcutStatus> {
    let env_snap = os.env.read().clone();
    os.shortcuts.configure_portal(&env_snap)
}

/// Alterna a visibilidade da janela principal (`os_window_toggle`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_window_toggle(
    os: State<'_, Arc<OsIntegration>>,
    source: ToggleSource,
) -> MtResult<ToggleDecisionDto> {
    let decision = os.quake.toggle(source)?;
    Ok(decision.into())
}

/// Exibe e foca a janela principal (`os_window_show`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_window_show(os: State<'_, Arc<OsIntegration>>) -> MtResult<()> {
    os.quake.show_and_focus()
}

/// Oculta a janela principal na bandeja ou minimiza (`os_window_hide`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_window_hide(os: State<'_, Arc<OsIntegration>>) -> MtResult<()> {
    os.quake.hide()
}

/// Solicita o encerramento coordenado da aplicação (`os_quit_request`).
#[tauri::command]
#[specta::specta]
pub async fn os_quit_request(os: State<'_, Arc<OsIntegration>>) -> MtResult<QuitOutcome> {
    os.shutdown.request_quit(QuitSource::Ipc).await
}

/// Resolve um lote de até 64 candidatos de links do terminal (`os_link_resolve`).
#[tauri::command]
#[specta::specta]
pub async fn os_link_resolve(
    os: State<'_, Arc<OsIntegration>>,
    context: LinkContext,
    candidates: Vec<LinkCandidate>,
) -> MtResult<Vec<ResolvedLink>> {
    os.links.resolve(&context, candidates).await
}

/// Re-resolve um candidato de link no backend e abre o arquivo correspondente no editor configurado (`os_link_open`).
#[tauri::command]
#[specta::specta]
pub async fn os_link_open(
    os: State<'_, Arc<OsIntegration>>,
    context: LinkContext,
    candidate: LinkCandidate,
) -> MtResult<()> {
    let resolved = os.links.resolve_single(&context, candidate).await?;
    os.editor.open(&resolved).await
}

/// Despacha uma notificação nativa sanitizada (`os_notify`).
#[tauri::command]
#[specta::specta]
pub async fn os_notify(
    os: State<'_, Arc<OsIntegration>>,
    request: NotifyRequest,
) -> MtResult<NotifyOutcome> {
    os.notifier.notify_explicit(&request).await
}

/// Define ou remove o alerta de intervenção pendente de uma aba (`os_attention_set`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_attention_set(
    os: State<'_, Arc<OsIntegration>>,
    tab_id: TabId,
    reason: Option<AttentionReason>,
) -> MtResult<StatusCounters> {
    os.set_attention(tab_id, reason)
}

/// Retorna o status real do arquivo `.desktop` de inicialização automática (`os_get_autostart`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_get_autostart(os: State<'_, Arc<OsIntegration>>) -> MtResult<AutostartStatus> {
    os.autostart_sync.status()
}

/// Habilita ou desabilita a inicialização automática sincronizando `os_settings` e o arquivo `.desktop` (`os_set_autostart`).
#[tauri::command]
#[specta::specta]
pub async fn os_set_autostart(
    os: State<'_, Arc<OsIntegration>>,
    enabled: bool,
) -> MtResult<AutostartStatus> {
    os.set_autostart(enabled).await
}

/// Retorna o snapshot completo de runtime do subsistema `os` (`os_runtime_snapshot`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_runtime_snapshot(os: State<'_, Arc<OsIntegration>>) -> MtResult<OsRuntimeSnapshot> {
    Ok(os.runtime_snapshot())
}

/// Handshake invocado pela UI após montar o layout inicial (`os_ui_ready`).
#[tauri::command]
#[specta::specta]
#[allow(clippy::needless_pass_by_value)]
pub fn os_ui_ready(os: State<'_, Arc<OsIntegration>>) -> MtResult<OsRuntimeSnapshot> {
    os.ui_ready()
}
