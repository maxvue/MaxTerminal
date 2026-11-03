import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type AttentionReason,
    type AutostartStatus_Serialize,
    type FocusTabPayload_Serialize,
    type LinkCandidate_Deserialize,
    type LinkContext,
    type NotifyOutcome,
    type NotifyRequest_Deserialize,
    type OpenPreferencesPayload,
    type OsCountersTauriEvent,
    type OsEnvironmentChangedTauriEvent_Serialize,
    type OsEnvironment_Serialize,
    type OsEvent,
    type OsFocusTabTauriEvent_Serialize,
    type OsLinkContextChangedTauriEvent,
    type OsOpenPreferencesTauriEvent,
    type OsRuntimeSnapshot_Serialize,
    type OsSettingsChangedTauriEvent_Serialize,
    type OsSettingsPatch_Deserialize,
    type OsSettings_Serialize,
    type OsShortcutStatusTauriEvent_Serialize,
    type OsShutdownFailedTauriEvent_Serialize,
    type OsShutdownStartedTauriEvent,
    type OsWindowVisibilityTauriEvent,
    type QuitOutcome,
    type ResolvedLink_Serialize,
    type ShortcutStatus_Serialize,
    type ShutdownFailedPayload_Serialize,
    type ShutdownStartedPayload,
    type StatusCounters,
    type TabId,
    type ToggleDecisionDto,
    type ToggleSource,
    type WindowVisibilityPayload
} from '@/types/bindings';

/** Obtém as preferências persistidas de integração com o sistema operacional (`os_settings_get`). */
export function getSettings(): Promise<OsSettings_Serialize> {
    return unwrap(commands.osSettingsGet(), 'os_settings_get');
}

/** Atualiza parcialmente e persiste as preferências do sistema operacional (`os_settings_update`). */
export function updateSettings(patch: OsSettingsPatch_Deserialize): Promise<OsSettings_Serialize> {
    return unwrap(commands.osSettingsUpdate(patch), 'os_settings_update');
}

/** Retorna ou re-sonda as informações do ambiente gráfico e capacidades D-Bus (`os_environment_info`). */
export function environment(refresh = false): Promise<OsEnvironment_Serialize> {
    return unwrap(commands.osEnvironmentInfo(refresh), 'os_environment_info');
}

/** Retorna o status atual do registro de atalho global (`os_shortcut_status`). */
export function shortcutStatus(): Promise<ShortcutStatus_Serialize> {
    return unwrap(commands.osShortcutStatus(), 'os_shortcut_status');
}

/** Solicita configuração interativa do atalho global no portal XDG (`os_shortcut_configure_portal`). */
export function configurePortal(): Promise<ShortcutStatus_Serialize> {
    return unwrap(commands.osShortcutConfigurePortal(), 'os_shortcut_configure_portal');
}

/** Alterna a visibilidade da janela principal (`os_window_toggle`). */
export function toggleWindow(source: ToggleSource = 'ipc'): Promise<ToggleDecisionDto> {
    return unwrap(commands.osWindowToggle(source), 'os_window_toggle');
}

/** Exibe e foca a janela principal (`os_window_show`). */
export function showWindow(): Promise<null> {
    return unwrap(commands.osWindowShow(), 'os_window_show');
}

/** Oculta a janela principal na bandeja ou minimiza (`os_window_hide`). */
export function hideWindow(): Promise<null> {
    return unwrap(commands.osWindowHide(), 'os_window_hide');
}

/** Solicita o encerramento coordenado da aplicação (`os_quit_request`). */
export function requestQuit(): Promise<QuitOutcome> {
    return unwrap(commands.osQuitRequest(), 'os_quit_request');
}

/** Resolve um lote de até 64 candidatos de links do terminal (`os_link_resolve`). */
export function resolveLinks(
    context: LinkContext,
    candidates: LinkCandidate_Deserialize[]
): Promise<ResolvedLink_Serialize[]> {
    return unwrap(commands.osLinkResolve(context, candidates), 'os_link_resolve');
}

/** Re-resolve e abre um candidato de link no editor configurado (`os_link_open`). */
export function openLink(
    context: LinkContext,
    candidate: LinkCandidate_Deserialize
): Promise<null> {
    return unwrap(commands.osLinkOpen(context, candidate), 'os_link_open');
}

/** Despacha uma notificação nativa sanitizada (`os_notify`). */
export function notify(request: NotifyRequest_Deserialize): Promise<NotifyOutcome> {
    return unwrap(commands.osNotify(request), 'os_notify');
}

/** Define ou remove o alerta de intervenção pendente de uma aba (`os_attention_set`). */
export function setAttention(
    tabId: TabId,
    reason: AttentionReason | null
): Promise<StatusCounters> {
    return unwrap(commands.osAttentionSet(tabId, reason), 'os_attention_set');
}

/** Retorna o status real do arquivo `.desktop` de inicialização automática (`os_get_autostart`). */
export function getAutostart(): Promise<AutostartStatus_Serialize> {
    return unwrap(commands.osGetAutostart(), 'os_get_autostart');
}

/** Habilita ou desabilita a inicialização automática (`os_set_autostart`). */
export function setAutostart(enabled: boolean): Promise<AutostartStatus_Serialize> {
    return unwrap(commands.osSetAutostart(enabled), 'os_set_autostart');
}

/** Retorna o snapshot completo de runtime do subsistema `os` (`os_runtime_snapshot`). */
export function runtimeSnapshot(): Promise<OsRuntimeSnapshot_Serialize> {
    return unwrap(commands.osRuntimeSnapshot(), 'os_runtime_snapshot');
}

/** Handshake invocado pela UI após montar o layout inicial (`os_ui_ready`). */
export function uiReady(): Promise<OsRuntimeSnapshot_Serialize> {
    return unwrap(commands.osUiReady(), 'os_ui_ready');
}

/** Escuta o evento `os://focus-tab`. */
export function onFocusTab(
    cb: (ev: OsEvent<FocusTabPayload_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<OsFocusTabTauriEvent_Serialize>('os://focus-tab', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://window-visibility`. */
export function onWindowVisibility(
    cb: (ev: OsEvent<WindowVisibilityPayload>) => void
): Promise<UnlistenFn> {
    return listen<OsWindowVisibilityTauriEvent>('os://window-visibility', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://settings-changed`. */
export function onSettingsChanged(
    cb: (ev: OsEvent<OsSettings_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<OsSettingsChangedTauriEvent_Serialize>('os://settings-changed', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://shortcut-status`. */
export function onShortcutStatus(
    cb: (ev: OsEvent<ShortcutStatus_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<OsShortcutStatusTauriEvent_Serialize>('os://shortcut-status', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://counters`. */
export function onCounters(cb: (ev: OsEvent<StatusCounters>) => void): Promise<UnlistenFn> {
    return listen<OsCountersTauriEvent>('os://counters', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://open-preferences`. */
export function onOpenPreferences(
    cb: (ev: OsEvent<OpenPreferencesPayload>) => void
): Promise<UnlistenFn> {
    return listen<OsOpenPreferencesTauriEvent>('os://open-preferences', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://shutdown-started`. */
export function onShutdownStarted(
    cb: (ev: OsEvent<ShutdownStartedPayload>) => void
): Promise<UnlistenFn> {
    return listen<OsShutdownStartedTauriEvent>('os://shutdown-started', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://shutdown-failed`. */
export function onShutdownFailed(
    cb: (ev: OsEvent<ShutdownFailedPayload_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<OsShutdownFailedTauriEvent_Serialize>('os://shutdown-failed', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://environment-changed`. */
export function onEnvironmentChanged(
    cb: (ev: OsEvent<OsEnvironment_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<OsEnvironmentChangedTauriEvent_Serialize>('os://environment-changed', (e) => {
        cb(e.payload);
    });
}

/** Escuta o evento `os://link-context-changed`. */
export function onLinkContextChanged(
    cb: (ev: OsEvent<LinkContext>) => void
): Promise<UnlistenFn> {
    return listen<OsLinkContextChangedTauriEvent>('os://link-context-changed', (e) => {
        cb(e.payload);
    });
}

export const osIpc = {
    getSettings,
    updateSettings,
    environment,
    shortcutStatus,
    configurePortal,
    toggleWindow,
    showWindow,
    hideWindow,
    requestQuit,
    resolveLinks,
    openLink,
    notify,
    setAttention,
    getAutostart,
    setAutostart,
    runtimeSnapshot,
    uiReady,
    onFocusTab,
    onWindowVisibility,
    onSettingsChanged,
    onShortcutStatus,
    onCounters,
    onOpenPreferences,
    onShutdownStarted,
    onShutdownFailed,
    onEnvironmentChanged,
    onLinkContextChanged
};
