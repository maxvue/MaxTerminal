import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { call } from '@/ipc/core';
import {
    commands,
    type MemoryEntry_Serialize,
    type MemoryQuery_Deserialize,
    type MemoryUpdatedPayload,
    type ProjectId
} from '@/types/bindings';

export const memoryIpc = {
    /** Consulta sugestões de comandos recorrentes ordenadas por `pinned` e `score` (`memory_suggestions`). */
    suggestions: (query: MemoryQuery_Deserialize): Promise<MemoryEntry_Serialize[]> =>
        call('memory_suggestions', () => commands.memorySuggestions(query)),

    /** Lista as entradas da memória contextual de um projeto (`memory_list`). */
    list: (
        projectId: ProjectId,
        includeHidden = false,
        limit = 50
    ): Promise<MemoryEntry_Serialize[]> =>
        call('memory_list', () => commands.memoryList(projectId, includeHidden, limit)),

    /** Fixa ou desafixa um comando na memória contextual do projeto (`memory_pin`). */
    pin: (
        projectId: ProjectId,
        command: string,
        pinned: boolean
    ): Promise<MemoryEntry_Serialize> =>
        call('memory_pin', () => commands.memoryPin(projectId, command, pinned)),

    /** Oculta ou reexibe um comando nas sugestões automáticas do projeto (`memory_hide`). */
    hide: (
        projectId: ProjectId,
        command: string,
        hidden: boolean
    ): Promise<MemoryEntry_Serialize> =>
        call('memory_hide', () => commands.memoryHide(projectId, command, hidden)),

    /** Remove permanentemente um comando da memória contextual do projeto e registra tombstone (`memory_forget`). */
    forget: (projectId: ProjectId, command: string): Promise<boolean> =>
        call('memory_forget', () => commands.memoryForget(projectId, command)),

    /** Reconstrói deterministicamente a memória contextual a partir de `command_blocks` (`memory_rebuild`). */
    rebuild: (projectId: ProjectId | null = null): Promise<number> =>
        call('memory_rebuild', () => commands.memoryRebuild(projectId)),

    /** Escuta o evento global `memory://updated`. */
    onUpdated: (cb: (payload: MemoryUpdatedPayload) => void): Promise<UnlistenFn> =>
        listen<MemoryUpdatedPayload>('memory://updated', (ev) => cb(ev.payload))
};
