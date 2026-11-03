import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type ContinuousPolicy,
    type FocusSnapshot_Deserialize,
    type PaneId,
    type SeenRequest_Deserialize,
    type TabId,
    type TabStateRemovedEvent,
    type TabStateSnapshot_Serialize
} from '@/types/bindings';

/** Lista os snapshots de estado de todas as abas registradas (`tab_state_list`). */
export function tabStateList(): Promise<TabStateSnapshot_Serialize[]> {
    return unwrap(commands.tabStateList(), 'tab_state_list');
}

/** Obtém o snapshot de estado atual de uma aba pelo seu `TabId` (`tab_state_get`). */
export function tabStateGet(tabId: TabId): Promise<TabStateSnapshot_Serialize> {
    return unwrap(commands.tabStateGet(tabId), 'tab_state_get');
}

/** Marca conclusões não vistas (`completed_unseen` / `error_unseen`) e alertas de atenção como vistos (`tab_mark_seen`). */
export function tabMarkSeen(request: SeenRequest_Deserialize): Promise<TabStateSnapshot_Serialize> {
    return unwrap(commands.tabMarkSeen(request), 'tab_mark_seen');
}

/** Atualiza o foco global da janela e da aba/painel ativo (`tab_set_focus`). */
export function tabSetFocus(focus: FocusSnapshot_Deserialize): Promise<TabStateSnapshot_Serialize[]> {
    return unwrap(commands.tabSetFocus(focus), 'tab_set_focus');
}

/** Define a política de execução contínua (`ContinuousPolicy`) para uma aba ou painel específico (`tab_set_continuous`). */
export function tabSetContinuous(
    tabId: TabId,
    paneId: PaneId | null,
    policy: ContinuousPolicy
): Promise<TabStateSnapshot_Serialize> {
    return unwrap(commands.tabSetContinuous(tabId, paneId, policy), 'tab_set_continuous');
}

/** Escuta o evento global `tab://state-changed`. */
export function onTabStateChanged(
    cb: (ev: TabStateSnapshot_Serialize) => void
): Promise<UnlistenFn> {
    return listen<TabStateSnapshot_Serialize>('tab://state-changed', (event) => {
        cb(event.payload);
    });
}

/** Escuta o evento global `tab://state-removed`. */
export function onTabStateRemoved(
    cb: (ev: TabStateRemovedEvent) => void
): Promise<UnlistenFn> {
    return listen<TabStateRemovedEvent>('tab://state-removed', (event) => {
        cb(event.payload);
    });
}
