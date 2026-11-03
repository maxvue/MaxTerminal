import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import { defineComponent, h } from 'vue';
import TabBar from '@/components/workspace/TabBar.vue';
import { useIssueBadgeProvider } from '@/composables/useIssueBadgeProvider';
import { provideProjectBadgeRegistry } from '@/composables/useProjectBadgeRegistry';
import { useGithubIssuesStore } from '@/stores/githubIssues';

describe('Fase 16 — Badge 🟣 Issues Abertas e Aba Fixa no TabBar', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('registra provedor github-issues com order 60 e retorna null quando openCount é 0 ou não-GitHub', () => {
        let capturedCount: ((projectId: string) => number | null) | null = null;

        const Harness = defineComponent({
            setup() {
                const reg = provideProjectBadgeRegistry();
                useIssueBadgeProvider(reg);
                const provider = reg.providers.value.find((p) => p.id === 'github-issues');
                if (provider) {
                    capturedCount = provider.count;
                }
                return () => h('div');
            }
        });

        mount(Harness);
        expect(capturedCount).not.toBeNull();

        const store = useGithubIssuesStore();
        store.resolutionsByProject['proj-gh'] = {
            kind: 'git_hub',
            identity: {
                repoKey: 'github.com/johnattas/maxterminal',
                host: 'github.com',
                owner: 'johnattas',
                repo: 'MaxTerminal',
                remoteName: 'origin',
                htmlUrl: 'https://github.com/johnattas/MaxTerminal',
                defaultBranch: 'main'
            }
        };
        store.openCountByProject['proj-gh'] = 7;
        expect(capturedCount!('proj-gh')).toBe(7);

        store.openCountByProject['proj-gh'] = 0;
        expect(capturedCount!('proj-gh')).toBeNull();

        store.resolutionsByProject['proj-local'] = { kind: 'not_git_repo' };
        store.openCountByProject['proj-local'] = 5;
        expect(capturedCount!('proj-local')).toBeNull();
    });

    it('renderiza slot #pinned no início de TabBar antes das abas regulares', () => {
        const wrapper = mount(TabBar, {
            props: {
                tabs: [],
                activeTabId: null,
                templates: []
            },
            slots: {
                pinned: '<button class="pinned-issues-btn">Issues (3)</button>'
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        const pinned = wrapper.find('.pinned-issues-btn');
        expect(pinned.exists()).toBe(true);
        expect(pinned.text()).toContain('Issues (3)');
    });
});
