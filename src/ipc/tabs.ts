import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type LayoutSizePatch,
    type PaneCloseOutcome_Serialize,
    type PaneExitedPayload_Serialize,
    type PaneId,
    type PanePtyReplacedPayload_Serialize,
    type ProjectId,
    type SplitDirection,
    type TabClosedPayload,
    type TabId,
    type TabOpenRequest_Deserialize,
    type TabReorderedPayload,
    type TabView_Serialize,
    type WorkspaceEvent
} from '@/types/bindings';

/** Lista as abas abertas de um projeto ordenadas por `position` (`tab_list`). */
export function tabList(projectId: ProjectId): Promise<TabView_Serialize[]> {
    return unwrap(commands.tabList(projectId), 'tab_list');
}

/** Abre uma nova aba no projeto a partir do template informado ou do template padrão (`tab_open`). */
export function tabOpen(request: TabOpenRequest_Deserialize): Promise<TabView_Serialize> {
    return unwrap(commands.tabOpen(request), 'tab_open');
}

/** Fecha uma aba aberta, encerrando seus PTYs e arquivando sua sessão (`tab_close`). */
export function tabClose(tabId: TabId, requestId: string): Promise<null> {
    return unwrap(commands.tabClose(tabId, requestId), 'tab_close');
}

/** Reordena as abas abertas de um projeto (`tab_reorder`). */
export function tabReorder(projectId: ProjectId, orderedTabIds: TabId[]): Promise<TabId[]> {
    return unwrap(commands.tabReorder(projectId, orderedTabIds), 'tab_reorder');
}

/** Atualiza o foco de uma aba e de seu painel ativo (`tab_focus`). */
export function tabFocus(tabId: TabId, paneId: PaneId | null): Promise<TabView_Serialize> {
    return unwrap(commands.tabFocus(tabId, paneId), 'tab_focus');
}

/** Divide um painel existente horizontal ou verticalmente (`pane_split`). */
export function paneSplit(
    paneId: PaneId,
    direction: SplitDirection,
    expectedLayoutRevision: number,
    requestId: string
): Promise<TabView_Serialize> {
    return unwrap(
        commands.paneSplit(paneId, direction, expectedLayoutRevision, requestId),
        'pane_split'
    );
}

/** Fecha um painel individual ou a aba caso seja o último painel (`pane_close`). */
export function paneClose(paneId: PaneId, requestId: string): Promise<PaneCloseOutcome_Serialize> {
    return unwrap(commands.paneClose(paneId, requestId), 'pane_close');
}

/** Reinicia o PTY de um painel que já encerrou (`pane_restart`). */
export function paneRestart(
    paneId: PaneId,
    cwdFallbackToProject: boolean,
    requestId: string
): Promise<TabView_Serialize> {
    return unwrap(
        commands.paneRestart(paneId, cwdFallbackToProject, requestId),
        'pane_restart'
    );
}

/** Atualiza as proporções dos splitters na árvore de layout de uma aba (`pane_layout_update`). */
export function paneLayoutUpdate(
    tabId: TabId,
    expectedLayoutRevision: number,
    patches: LayoutSizePatch[]
): Promise<TabView_Serialize> {
    return unwrap(
        commands.paneLayoutUpdate(tabId, expectedLayoutRevision, patches),
        'pane_layout_update'
    );
}

/** Escuta o evento global `tab://opened`. */
export function onTabOpened(
    cb: (ev: WorkspaceEvent<TabView_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<TabView_Serialize>>('tab://opened', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `tab://closed`. */
export function onTabClosed(
    cb: (ev: WorkspaceEvent<TabClosedPayload>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<TabClosedPayload>>('tab://closed', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `tab://reordered`. */
export function onTabReordered(
    cb: (ev: WorkspaceEvent<TabReorderedPayload>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<TabReorderedPayload>>('tab://reordered', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `tab://layout-changed`. */
export function onTabLayoutChanged(
    cb: (ev: WorkspaceEvent<TabView_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<TabView_Serialize>>('tab://layout-changed', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `pane://pty-replaced`. */
export function onPanePtyReplaced(
    cb: (ev: WorkspaceEvent<PanePtyReplacedPayload_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<PanePtyReplacedPayload_Serialize>>('pane://pty-replaced', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `pane://exited`. */
export function onPaneExited(
    cb: (ev: WorkspaceEvent<PaneExitedPayload_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<PaneExitedPayload_Serialize>>('pane://exited', (ev) => {
        cb(ev.payload);
    });
}
