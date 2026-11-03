import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { unwrap } from '@/ipc/core';
import {
    commands,
    type GitContext_Serialize,
    type GitStatusChanged_Serialize,
    type WorktreeCreateRequest_Deserialize,
    type WorktreeEntry_Serialize
} from '@/types/bindings';

/** Obtém o contexto Git para o caminho informado (`git_status_for_path`). */
export function gitStatusForPath(path: string): Promise<GitContext_Serialize> {
    return unwrap(commands.gitStatusForPath(path), 'git_status_for_path');
}

/** Força uma nova coleta imediata do contexto Git para o caminho informado (`git_status_refresh`). */
export function gitStatusRefresh(path: string): Promise<GitContext_Serialize> {
    return unwrap(commands.gitStatusRefresh(path), 'git_status_refresh');
}

/** Lista todas as worktrees associadas ao repositório (`git_worktree_list`). */
export function gitWorktreeList(projectRoot: string): Promise<WorktreeEntry_Serialize[]> {
    return unwrap(commands.gitWorktreeList(projectRoot), 'git_worktree_list');
}

/** Cria uma nova worktree gerenciada em `.worktrees/` ou `.max-code-worktrees/` (`git_worktree_create`). */
export function gitWorktreeCreate(
    req: WorktreeCreateRequest_Deserialize
): Promise<WorktreeEntry_Serialize> {
    return unwrap(commands.gitWorktreeCreate(req), 'git_worktree_create');
}

/** Remove uma worktree gerenciada (`git_worktree_remove`). */
export function gitWorktreeRemove(worktreePath: string, force: boolean): Promise<null> {
    return unwrap(commands.gitWorktreeRemove(worktreePath, force), 'git_worktree_remove');
}

/** Escuta o evento global `git://status-changed`. */
export function onGitStatusChanged(
    cb: (payload: GitStatusChanged_Serialize) => void
): Promise<UnlistenFn> {
    return listen<GitStatusChanged_Serialize>('git://status-changed', (ev) => {
        cb(ev.payload);
    });
}
