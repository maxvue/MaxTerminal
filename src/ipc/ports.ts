import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type PaneId,
    type PollMode,
    type PortClosed,
    type PortKillResult,
    type PortOpened_Serialize,
    type PortSnapshot_Serialize,
    type PortUpdated_Serialize,
    type TabId
} from '@/types/bindings';

/** Lista as portas TCP ativas confirmadas para os painéis de uma aba (`ports_list_for_tab`). */
export function portsListForTab(tabId: TabId): Promise<PortSnapshot_Serialize> {
    return unwrap(commands.portsListForTab(tabId), 'ports_list_for_tab');
}

/** Abre `http://127.0.0.1:<port>/` no navegador padrão (`ports_open_browser`). */
export function portsOpenBrowser(paneId: PaneId, port: number): Promise<null> {
    return unwrap(commands.portsOpenBrowser(paneId, port), 'ports_open_browser');
}

/** Encerra cirurgicamente o processo proprietário da porta (`ports_kill`). */
export function portsKill(
    paneId: PaneId,
    port: number,
    expectedPid: number | null = null
): Promise<PortKillResult> {
    return unwrap(commands.portsKill(paneId, port, expectedPid), 'ports_kill');
}

/** Define o modo adaptativo de polling de portas (`ports_set_poll_mode`). */
export function portsSetPollMode(mode: PollMode): Promise<null> {
    return unwrap(commands.portsSetPollMode(mode), 'ports_set_poll_mode');
}

/** Escuta o evento global `ports://opened`. */
export function onPortOpened(cb: (payload: PortOpened_Serialize) => void): Promise<UnlistenFn> {
    return listen<PortOpened_Serialize>('ports://opened', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `ports://updated`. */
export function onPortUpdated(cb: (payload: PortUpdated_Serialize) => void): Promise<UnlistenFn> {
    return listen<PortUpdated_Serialize>('ports://updated', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `ports://closed`. */
export function onPortClosed(cb: (payload: PortClosed) => void): Promise<UnlistenFn> {
    return listen<PortClosed>('ports://closed', (ev) => {
        cb(ev.payload);
    });
}
