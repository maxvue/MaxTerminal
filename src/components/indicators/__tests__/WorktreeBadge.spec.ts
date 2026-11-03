import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import WorktreeBadge from '@/components/indicators/WorktreeBadge.vue';

describe('WorktreeBadge.vue', () => {
    it('exibe badge para linked worktrees (.worktrees e .max-code-worktrees) e oculta na main', () => {
        const mainWrapper = mount(WorktreeBadge, {
            props: {
                worktree: {
                    kind: 'main',
                    name: 'repo',
                    path: '/repo',
                    mainWorktreePath: '/repo',
                    isLinked: false,
                    locked: false,
                    prunable: false
                }
            },
            global: { stubs: { MaxIcon: true } }
        });
        expect(mainWrapper.find('.worktree-badge').exists()).toBe(false);

        const maxCodeWrapper = mount(WorktreeBadge, {
            props: {
                worktree: {
                    kind: 'max_code',
                    name: 'wt-8f33ab43',
                    path: '/repo/.max-code-worktrees/wt-8f33ab43',
                    mainWorktreePath: '/repo',
                    isLinked: true,
                    locked: true,
                    prunable: false
                }
            },
            global: { stubs: { MaxIcon: true } }
        });
        expect(maxCodeWrapper.text()).toContain('WT: wt-8f33ab43');
        expect(maxCodeWrapper.find('.worktree-badge--max-code').exists()).toBe(true);
    });
});
