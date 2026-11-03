import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AgentWorktreeCloseDialog from '@/components/agents/AgentWorktreeCloseDialog.vue';
import { MtIpcError } from '@/ipc/core';
import { useAgentsStore } from '@/stores/agents';
import type { AgentCloseInspection_Serialize } from '@/types/bindings';

function makeInspection(
    overrides: Partial<AgentCloseInspection_Serialize> = {}
): AgentCloseInspection_Serialize {
    return {
        link: {
            id: 42,
            tabId: 'tab-42',
            projectId: 'proj-1',
            agent: 'claude-code',
            worktreePath: '/repo/.max-code-worktrees/wt-claude-1',
            branch: 'agent/claude-code/wt-1',
            baseRef: 'deadbeef',
            createdByApp: true,
            openedAtMs: 1_000,
            tabClosedAtMs: 2_000,
            resolution: null,
            resolvedAtMs: null
        },
        exists: true,
        dirty: true,
        hasIgnoredFiles: false,
        inUse: false,
        aheadOfBase: 2,
        branchMerged: false,
        changedFiles: ['M src/lib.rs', '?? novo.txt'],
        changedFilesTruncated: false,
        inspectionComplete: true,
        requiresConfirmation: true,
        allowed: ['keep'],
        inspectionToken: 'token-v1',
        ...overrides
    };
}

describe('AgentWorktreeCloseDialog.vue (Fase 13)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('exibe arquivos modificados, motivos de bloqueio em pt-BR e desabilita remoções não permitidas (CA-14, CA-15)', () => {
        const inspection = makeInspection();
        const wrapper = mount(AgentWorktreeCloseDialog, {
            props: {
                modelValue: true,
                inspection
            },
            global: {
                stubs: { MaxIcon: true }
            }
        });

        expect(wrapper.text()).toContain('/repo/.max-code-worktrees/wt-claude-1');
        expect(wrapper.text()).toContain('M src/lib.rs');
        expect(wrapper.text()).toContain('Existem alterações rastreadas ou arquivos não rastreados');

        const keepBtn = wrapper.find('.agent-close-dialog__btn--keep');
        const removeBtn = wrapper.find('.agent-close-dialog__btn--remove');
        const deleteBranchBtn = wrapper.find('.agent-close-dialog__btn--delete-branch');

        expect((keepBtn.element as HTMLButtonElement).disabled).toBe(false);
        expect((removeBtn.element as HTMLButtonElement).disabled).toBe(true);
        expect((deleteBranchBtn.element as HTMLButtonElement).disabled).toBe(true);
    });

    it('revalida a inspeção automaticamente quando resolveWorktree retorna agents.inspection_stale', async () => {
        const store = useAgentsStore();
        const initial = makeInspection({
            dirty: false,
            aheadOfBase: 0,
            branchMerged: true,
            allowed: ['keep', 'remove', 'removeAndDeleteBranch'],
            inspectionToken: 'token-old'
        });

        const refreshed = makeInspection({
            dirty: true,
            aheadOfBase: 1,
            branchMerged: false,
            allowed: ['keep'],
            inspectionToken: 'token-new'
        });

        vi.spyOn(store, 'resolveWorktree').mockRejectedValue(
            new MtIpcError(
                {
                    code: 'agents.inspection_stale',
                    message: 'A inspeção expirou.'
                },
                'agent_worktree_resolve'
            )
        );
        const inspectCloseSpy = vi.spyOn(store, 'inspectClose').mockResolvedValue(refreshed);

        const wrapper = mount(AgentWorktreeCloseDialog, {
            props: {
                modelValue: true,
                inspection: initial
            },
            global: {
                stubs: { MaxIcon: true }
            }
        });

        const removeBtn = wrapper.find('.agent-close-dialog__btn--remove');
        await removeBtn.trigger('click');
        await flushPromises();

        expect(inspectCloseSpy).toHaveBeenCalledWith('tab-42');
        expect(wrapper.text()).toContain('O estado da worktree mudou desde a inspeção');
        expect((wrapper.find('.agent-close-dialog__btn--remove').element as HTMLButtonElement).disabled).toBe(true);
    });
});
