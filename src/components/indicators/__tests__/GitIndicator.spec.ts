import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import GitIndicator from '@/components/indicators/GitIndicator.vue';
import type { GitContext_Serialize } from '@/types/bindings';

describe('GitIndicator.vue', () => {
    it('renderiza branch nomeada, ponto dirty, upstream e abre popover de detalhes', async () => {
        const ctx: GitContext_Serialize = {
            kind: 'repository',
            repoKey: '/repo/app',
            worktreeRoot: '/repo/app',
            gitDir: '/repo/app/.git',
            commonDir: '/repo/app/.git',
            branch: { state: 'named', name: 'feat/git-ui', oid: '11223344' },
            changes: {
                staged: 2,
                unstaged: 1,
                untracked: 3,
                conflicted: 0,
                dirty: true,
                summaryTruncated: false
            },
            upstream: { name: 'origin/feat/git-ui', ahead: 1, behind: 2 },
            worktree: {
                kind: 'main',
                name: 'app',
                path: '/repo/app',
                mainWorktreePath: '/repo/app',
                isLinked: false,
                locked: false,
                prunable: false
            },
            revision: 1,
            refreshedAtMs: 1000
        };

        const wrapper = mount(GitIndicator, {
            props: { context: ctx },
            global: {
                stubs: { MaxIcon: true }
            }
        });

        expect(wrapper.text()).toContain('feat/git-ui');
        expect(wrapper.text()).toContain('↑1 ↓2');
        expect(wrapper.find('.git-indicator__dirty-dot').exists()).toBe(true);

        await wrapper.find('.git-indicator__chip').trigger('click');
        expect(wrapper.find('.git-indicator__popover').exists()).toBe(true);

        await wrapper.find('.git-indicator__refresh-btn').trigger('click');
        expect(wrapper.emitted('refresh')?.[0]).toEqual(['/repo/app']);
    });

    it('oculta indicador quando o contexto é non_repository', () => {
        const wrapper = mount(GitIndicator, {
            props: {
                context: {
                    kind: 'non_repository',
                    path: '/tmp/no-git',
                    revision: 1,
                    refreshedAtMs: 1000
                }
            },
            global: {
                stubs: { MaxIcon: true }
            }
        });
        expect(wrapper.find('.git-indicator').exists()).toBe(false);
    });
});
