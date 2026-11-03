import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type BridgeError,
    type BridgeProjectSyncedEvent,
    type BridgeSettingsPatch_Deserialize,
    type BridgeSettings_Serialize,
    type BridgeStatusSnapshot_Serialize,
    type BridgeTerminalOpenedEvent,
    type BugCycleProgressPayload_Serialize,
    type BugsCycleAckPayload_Serialize,
    type BugsCycleStartPayload_Deserialize,
    type InspectDispatchResult_Serialize,
    type IssueActionDispatchOutcome_Serialize,
    type IssueCommandPayload_Deserialize,
    type IssueProtocolSupport,
    type OpenFilePayload_Deserialize
} from '@/types/bindings';

/** Obtém o snapshot atual de conexão e telemetria da ponte com o `MaxCode` (`bridge_status`). */
export function bridgeStatus(): Promise<BridgeStatusSnapshot_Serialize> {
    return unwrap(commands.bridgeStatus(), 'bridge_status');
}

/** Obtém as configurações persistidas da ponte (`bridge_get_settings`). */
export function bridgeGetSettings(): Promise<BridgeSettings_Serialize> {
    return unwrap(commands.bridgeGetSettings(), 'bridge_get_settings');
}

/** Atualiza parcialmente as configurações da ponte (`bridge_update_settings`). */
export function bridgeUpdateSettings(
    patch: BridgeSettingsPatch_Deserialize
): Promise<BridgeSettings_Serialize> {
    return unwrap(commands.bridgeUpdateSettings(patch), 'bridge_update_settings');
}

/** Dispara a inspeção de um bloco de comando com falha em Plan Mode no `MaxCode` (`bridge_inspect_command_error`). */
export function bridgeInspectCommandError(
    blockId: string
): Promise<InspectDispatchResult_Serialize> {
    return unwrap(commands.bridgeInspectCommandError(blockId), 'bridge_inspect_command_error');
}

/** Notifica o `MaxCode` sobre o projeto ativo selecionado no `MaxTerminal` (`bridge_notify_project_selected`). */
export function bridgeNotifyProjectSelected(
    projectPath: string,
    projectName: string | null = null,
    gitBranch: string | null = null
): Promise<boolean> {
    return unwrap(
        commands.bridgeNotifyProjectSelected(projectPath, projectName, gitBranch),
        'bridge_notify_project_selected'
    );
}

/** Solicita ao `MaxCode` a abertura de um arquivo em linha e coluna (`bridge_open_file_in_maxcode`). */
export function bridgeOpenFileInMaxcode(payload: OpenFilePayload_Deserialize): Promise<boolean> {
    return unwrap(commands.bridgeOpenFileInMaxcode(payload), 'bridge_open_file_in_maxcode');
}

/** Força releitura do arquivo de descoberta e reconexão imediata (`bridge_reconnect_now`). */
export function bridgeReconnectNow(): Promise<BridgeStatusSnapshot_Serialize> {
    return unwrap(commands.bridgeReconnectNow(), 'bridge_reconnect_now');
}

/** Retorna as capabilities do protocolo de Issues suportadas pela instância conectada do `MaxCode` (`bridge_issue_protocol_support`). */
export function bridgeIssueProtocolSupport(): Promise<IssueProtocolSupport> {
    return unwrap(commands.bridgeIssueProtocolSupport(), 'bridge_issue_protocol_support');
}

/** Envia uma ação unitária de Issue (`open`, `plan`, `fix`) ao `MaxCode` (`bridge_send_issue_command`). */
export function bridgeSendIssueCommand(
    payload: IssueCommandPayload_Deserialize
): Promise<IssueActionDispatchOutcome_Serialize> {
    return unwrap(commands.bridgeSendIssueCommand(payload), 'bridge_send_issue_command');
}

/** Calcula a prévia (`dryRun = true`) do ciclo automatizado de bugs (`bridge_bugs_cycle_preview`). */
export function bridgeBugsCyclePreview(
    payload: BugsCycleStartPayload_Deserialize
): Promise<BugsCycleAckPayload_Serialize> {
    return unwrap(commands.bridgeBugsCyclePreview(payload), 'bridge_bugs_cycle_preview');
}

/** Inicia (`dryRun = false`) o ciclo automatizado de bugs no `MaxCode` (`bridge_bugs_cycle_start`). */
export function bridgeBugsCycleStart(
    payload: BugsCycleStartPayload_Deserialize
): Promise<BugsCycleAckPayload_Serialize> {
    return unwrap(commands.bridgeBugsCycleStart(payload), 'bridge_bugs_cycle_start');
}

/** Escuta o evento `bridge://status-changed`. */
export function onBridgeStatusChanged(
    cb: (status: BridgeStatusSnapshot_Serialize) => void
): Promise<UnlistenFn> {
    return listen<BridgeStatusSnapshot_Serialize>('bridge://status-changed', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento `bridge://project-synced`. */
export function onBridgeProjectSynced(
    cb: (payload: BridgeProjectSyncedEvent) => void
): Promise<UnlistenFn> {
    return listen<BridgeProjectSyncedEvent>('bridge://project-synced', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento `bridge://terminal-opened`. */
export function onBridgeTerminalOpened(
    cb: (payload: BridgeTerminalOpenedEvent) => void
): Promise<UnlistenFn> {
    return listen<BridgeTerminalOpenedEvent>('bridge://terminal-opened', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento `bridge://bug-cycle-progress`. */
export function onBridgeBugCycleProgress(
    cb: (payload: BugCycleProgressPayload_Serialize) => void
): Promise<UnlistenFn> {
    return listen<BugCycleProgressPayload_Serialize>('bridge://bug-cycle-progress', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento `bridge://error`. */
export function onBridgeError(cb: (payload: BridgeError) => void): Promise<UnlistenFn> {
    return listen<BridgeError>('bridge://error', (ev) => {
        cb(ev.payload);
    });
}

export const bridgeIpc = {
    status: bridgeStatus,
    getSettings: bridgeGetSettings,
    updateSettings: bridgeUpdateSettings,
    inspectCommandError: bridgeInspectCommandError,
    inspectBlock: bridgeInspectCommandError,
    notifyProjectSelected: bridgeNotifyProjectSelected,
    openFileInMaxCode: bridgeOpenFileInMaxcode,
    reconnectNow: bridgeReconnectNow,
    issueProtocolSupport: bridgeIssueProtocolSupport,
    sendIssueCommand: bridgeSendIssueCommand,
    bugsCyclePreview: bridgeBugsCyclePreview,
    bugsCycleStart: bridgeBugsCycleStart,
    onStatusChanged: onBridgeStatusChanged,
    onProjectSynced: onBridgeProjectSynced,
    onTerminalOpened: onBridgeTerminalOpened,
    onBugCycleProgress: onBridgeBugCycleProgress,
    onError: onBridgeError
};
