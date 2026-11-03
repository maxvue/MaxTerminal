import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as gitIpc from '@/ipc/git';
import { useGitStore } from '@/stores/git';
import { useTabsStore } from '@/stores/tabs';
import type { GitContext_Serialize, WorktreeEntry_Serialize } from '@/types/bindings';

vi.mock('@/ipc/git', () => ({
    gitStatusForPath: vi.fn(),
    gitStatusRefresh: vi.fn(),
    gitWorktreeList: vi.fn(),
    gitWorktreeCreate: vi.fn(),
    gitWorktreeRemove: vi.fn(),
    onGitStatusChanged: vi.fn(async () => () => {})
}));

function makeRepoContext(overrides: Partial<Extract<GitContext_Serialize, { kind: 'repository' }>> = {}): GitContext_Serialize {
    return {
        kind: 'repository',
        repoKey: '/repo/max',
        worktreeRoot: '/repo/max',
        gitDir: '/repo/max/.git',
        commonDir: '/repo/max/.git',
        branch: { state: 'named', name: 'main', oid: '12345678' },
        changes: {
            staged: 1,
            unstaged: 2,
            untracked: 0,
            conflicted: 0,
            dirty: true,
            summaryTruncated: false
        },
        upstream: { name: 'origin/main', ahead: 2, behind: 1 },
        worktree: {
            kind: 'main',
            name: 'max',
            path: '/repo/max',
            mainWorktreePath: '/repo/max',
            isLinked: false,
            locked: false,
            prunable: false
        },
        revision: 1,
        refreshedAtMs: 1000,
        ...overrides
    };
}

describe('useGitStore (Fase 08)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('aplica contexto Git com revisão monotônica e mapeia por painel e aba', async () => {
        const gitStore = useGitStore();
        const tabsStore = useTabsStore();

        tabsStore.byId['tab-1'] = {
            id: 'tab-1',
            projectId: 'proj-1',
            templateId: null,
            title: 'Terminal',
            icon: 'lucide:terminal',
            position: 0,
            origin: 'manual',
            continuous: false,
            lifecycle: 'ready',
            revision: 1,
            layoutRevision: 1,
            layout: { kind: 'leaf', paneId: 'pane-1' },
            panes: [
                {
                    id: 'pane-1',
                    ptyId: 'pty-1',
                    generation: 1,
                    cwd: '/repo/max',
                    state: 'live',
                    exit: null,
                    error: null
                }
            ],
            activePaneId: 'pane-1',
            primaryPaneId: 'pane-1',
            initialDispatch: 'none',
            tabSessionId: 'tsess-1'
        };

        const ctxV2 = makeRepoContext({ revision: 2 });
        vi.mocked(gitIpc.gitStatusForPath).mockResolvedValueOnce(ctxV2);

        const loaded = await gitStore.ensurePaneStatus('pane-1', '/repo/max');
        expect(loaded?.revision).toBe(2);
        expect(gitStore.contextForPane('pane-1')?.revision).toBe(2);
        expect(gitStore.contextForTab('tab-1')?.revision).toBe(2);

        // Evento com revisão antiga (1 < 2) é ignorado
        gitStore.applyStatusChangedEvent({
            context: makeRepoContext({ revision: 1 }),
            affectedPaneIds: ['pane-1'],
            affectedTabIds: ['tab-1'],
            reason: 'fs_event'
        });
        expect(gitStore.contextForPane('pane-1')?.revision).toBe(2);

        // Refresh manual atualiza revisão
        vi.mocked(gitIpc.gitStatusRefresh).mockResolvedValueOnce(makeRepoContext({ revision: 3 }));
        const refreshed = await gitStore.refreshForPath('/repo/max', 'pane-1');
        expect(refreshed.revision).toBe(3);

        // Worktrees CRUD
        const wtEntry: WorktreeEntry_Serialize = {
            path: '/repo/max/.worktrees/wt-1',
            headOid: '12345678',
            branch: 'wt/wt-1',
            detached: false,
            bare: false,
            locked: null,
            prunable: null,
            kind: 'max_terminal',
            name: 'wt-1',
            isMain: false
        };
        vi.mocked(gitIpc.gitWorktreeCreate).mockResolvedValueOnce(wtEntry);
        vi.mocked(gitIpc.gitWorktreeList).mockResolvedValue([wtEntry]);
        vi.mocked(gitIpc.gitWorktreeRemove).mockResolvedValueOnce(null);

        const created = await gitStore.createWorktree({
            projectRoot: '/repo/max',
            name: 'wt-1',
            branch: null,
            baseRef: null,
            location: 'max_terminal'
        });
        expect(created.name).toBe('wt-1');
        expect(gitStore.worktreesByRoot['/repo/max']).toHaveLength(1);

        await gitStore.removeWorktree('/repo/max', '/repo/max/.worktrees/wt-1', false);
        expect(gitIpc.gitWorktreeRemove).toHaveBeenCalledWith('/repo/max/.worktrees/wt-1', false);

        await gitStore.startListening();
        gitStore.removePane('pane-1');
        expect(gitStore.contextForPane('pane-1')).toBeNull();
        gitStore.resetForTests();
    });
});
