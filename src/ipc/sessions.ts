import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type CursorPage,
    type ScrollbackChunk,
    type SessionDetail_Serialize,
    type SessionId,
    type SessionSearchHit,
    type SessionSummary_Serialize,
    type TabId,
    type TabSessionId,
    type TabView_Serialize,
    type WorkspaceEvent
} from '@/types/bindings';

/** Arquiva a sessão atual da aba e inicia uma nova sessão limpa (`session_new`). */
export function sessionNew(tabId: TabId, requestId: string): Promise<TabView_Serialize> {
    return unwrap(commands.sessionNew(tabId, requestId), 'session_new');
}

/** Lista paginadamente as sessões de uma aba (`session_list`). */
export function sessionList(
    tabId: TabId,
    cursor: string | null,
    limit: number
): Promise<CursorPage<SessionSummary_Serialize>> {
    return unwrap(commands.sessionList(tabId, cursor, limit), 'session_list');
}

/** Obtém o detalhe completo de uma sessão de aba (`session_get`). */
export function sessionGet(tabSessionId: TabSessionId): Promise<SessionDetail_Serialize> {
    return unwrap(commands.sessionGet(tabSessionId), 'session_get');
}

/** Lista paginadamente os chunks de scrollback de um painel na sessão (`session_scrollback_get`). */
export function sessionScrollbackGet(
    tabSessionId: TabSessionId,
    sessionId: SessionId,
    cursor: string | null,
    limit: number
): Promise<CursorPage<ScrollbackChunk>> {
    return unwrap(
        commands.sessionScrollbackGet(tabSessionId, sessionId, cursor, limit),
        'session_scrollback_get'
    );
}

/** Realiza busca textual FTS5 em comandos e scrollback das sessões de uma aba (`session_search`). */
export function sessionSearch(
    tabId: TabId,
    query: string,
    limit: number
): Promise<SessionSearchHit[]> {
    return unwrap(commands.sessionSearch(tabId, query, limit), 'session_search');
}

/** Escuta o evento global `session://started`. */
export function onSessionStarted(
    cb: (ev: WorkspaceEvent<SessionSummary_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<SessionSummary_Serialize>>('session://started', (ev) => {
        cb(ev.payload);
    });
}

/** Escuta o evento global `session://archived`. */
export function onSessionArchived(
    cb: (ev: WorkspaceEvent<SessionSummary_Serialize>) => void
): Promise<UnlistenFn> {
    return listen<WorkspaceEvent<SessionSummary_Serialize>>('session://archived', (ev) => {
        cb(ev.payload);
    });
}
