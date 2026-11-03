import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type PaneId,
    type TabId,
    type TabView_Serialize,
    type WorkspaceEvent,
    type WorkspaceRestoreReport_Serialize,
    type WorkspaceSnapshot_Serialize
} from '@/types/bindings';

/** Retorna o snapshot atômico de hidratação do workspace (`workspace_snapshot`). */
export function workspaceSnapshot(): Promise<WorkspaceSnapshot_Serialize> {
    return unwrap(commands.workspaceSnapshot(), 'workspace_snapshot');
}

/** Dispara de forma idempotente o comando inicial de uma aba/painel (`workspace_initial_command_run`). */
export function workspaceInitialCommandRun(
    tabId: TabId,
    paneId: PaneId,
    generation: number,
    requestId: string
): Promise<TabView_Serialize> {
    return unwrap(
        commands.workspaceInitialCommandRun(tabId, paneId, generation, requestId),
        'workspace_initial_command_run'
    );
}

/** Escuta o evento global `workspace://restored`. */
export function onWorkspaceRestored(
    cb: (ev: WorkspaceEvent<WorkspaceRestoreReport_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<WorkspaceRestoreReport_Serialize>>(
        'workspace://restored',
        (ev) => {
            cb(ev.payload);
        }
    );
}
