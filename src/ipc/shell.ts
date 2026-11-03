import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type CommandBlockDto_Serialize,
    type CommandBlockHit_Serialize,
    type CommandBlockId,
    type CommandBlockListQuery_Deserialize,
    type CommandBlockSearchQuery_Deserialize,
    type CommandFinished_Serialize,
    type CommandStarted_Serialize,
    type ErrorContext_Serialize,
    type IntegrationInfo_Serialize,
    type Page,
    type PtyId,
    type ShellCwdChangedEvent_Serialize,
    type ShellIntegrationModeEvent_Serialize
} from '@/types/bindings';

/** Retorna diagnósticos da integração de shell ativa para um PTY (`shell_integration_info`). */
export function shellIntegrationInfo(ptyId: PtyId): Promise<IntegrationInfo_Serialize> {
    return unwrap(commands.shellIntegrationInfo(ptyId), 'shell_integration_info');
}

/** Lista blocos de comando paginados com filtros por sessão, aba, PTY ou status (`command_block_list`). */
export function commandBlockList(
    query: CommandBlockListQuery_Deserialize = {}
): Promise<Page<CommandBlockDto_Serialize>> {
    return unwrap(commands.commandBlockList(query), 'command_block_list');
}

/** Obtém os detalhes completos de um bloco de comando pelo ID (`command_block_get`). */
export function commandBlockGet(blockId: CommandBlockId): Promise<CommandBlockDto_Serialize> {
    return unwrap(commands.commandBlockGet(blockId), 'command_block_get');
}

/** Constrói o contexto de erro sanitizado e truncado para inspeção no MaxCode (`command_block_error_context`). */
export function commandBlockErrorContext(blockId: CommandBlockId): Promise<ErrorContext_Serialize> {
    return unwrap(commands.commandBlockErrorContext(blockId), 'command_block_error_context');
}

/** Executa busca textual FTS5 sobre o histórico de blocos de comando (`command_block_search`). */
export function commandBlockSearch(
    query: CommandBlockSearchQuery_Deserialize
): Promise<Page<CommandBlockHit_Serialize>> {
    return unwrap(commands.commandBlockSearch(query), 'command_block_search');
}

/** Escuta o evento global `shell://command-started`. */
export function onCommandStarted(
    cb: (ev: CommandStarted_Serialize) => void
): Promise<UnlistenFn> {
    return listen<CommandStarted_Serialize>('shell://command-started', (event) => {
        cb(event.payload);
    });
}

/** Escuta o evento global `shell://command-finished`. */
export function onCommandFinished(
    cb: (ev: CommandFinished_Serialize) => void
): Promise<UnlistenFn> {
    return listen<CommandFinished_Serialize>('shell://command-finished', (event) => {
        cb(event.payload);
    });
}

/** Escuta o evento global `shell://cwd-changed`. */
export function onShellCwdChanged(
    cb: (ev: ShellCwdChangedEvent_Serialize) => void
): Promise<UnlistenFn> {
    return listen<ShellCwdChangedEvent_Serialize>('shell://cwd-changed', (event) => {
        cb(event.payload);
    });
}

/** Escuta o evento global `shell://integration-mode`. */
export function onShellIntegrationMode(
    cb: (ev: ShellIntegrationModeEvent_Serialize) => void
): Promise<UnlistenFn> {
    return listen<ShellIntegrationModeEvent_Serialize>('shell://integration-mode', (event) => {
        cb(event.payload);
    });
}
