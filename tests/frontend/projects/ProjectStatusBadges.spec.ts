import { mount } from '@vue/test-utils';
import { nextTick, ref } from 'vue';
import { describe, expect, it } from 'vitest';
import ProjectStatusBadges from '@/components/projects/ProjectStatusBadges.vue';
import {
    createProjectBadgeRegistry,
    PROJECT_BADGE_REGISTRY_KEY
} from '@/composables/useProjectBadgeRegistry';
import { ZERO_PROJECT_COUNTERS, type ProjectStatusCounters } from '@/stores/projects';

describe('ProjectStatusBadges.vue (Fase 06)', () => {
    it('oculta contadores zero, respeita a ordem canônica, aplica tokens CSS e gera aria-label em pt-BR', () => {
        const wrapperZero = mount(ProjectStatusBadges, {
            props: {
                projectId: 'proj-1',
                counters: ZERO_PROJECT_COUNTERS
            },
            global: {
                stubs: { MaxIcon: true }
            }
        });
        expect(wrapperZero.find('.project-status-badges').exists()).toBe(false);

        const counters: ProjectStatusCounters = {
            running: 2,
            continuous: 1,
            finished: 0,
            error: 3,
            idle: 1
        };

        const wrapper = mount(ProjectStatusBadges, {
            props: {
                projectId: 'proj-1',
                counters
            },
            global: {
                stubs: { MaxIcon: true }
            }
        });

        const pills = wrapper.findAll('.project-status-badges__pill');
        expect(pills).toHaveLength(4);
        expect(pills.map((p) => p.attributes('data-badge-key'))).toEqual([
            'running',
            'continuous',
            'error',
            'idle'
        ]);

        expect(pills[0]!.attributes('style')).toContain('--max-success-500');
        expect(pills[0]!.attributes('aria-label')).toBe('2 abas executando');

        expect(pills[1]!.attributes('style')).toContain('--max-info-500');
        expect(pills[1]!.attributes('aria-label')).toBe('1 aba em execução contínua');

        expect(pills[2]!.attributes('style')).toContain('--max-danger-500');
        expect(pills[2]!.attributes('aria-label')).toBe('3 abas com erro');

        expect(pills[3]!.attributes('style')).toContain('--background-500');
        expect(pills[3]!.attributes('aria-label')).toBe('1 aba parada');
    });

    it('renderiza provider registrado (simulando Fase 16 github-issues) após os nativos, reage reativamente, respeita disposer, isola instâncias e rejeita duplicatas', async () => {
        const registry1 = createProjectBadgeRegistry();
        const registry2 = createProjectBadgeRegistry();

        const issuesCount = ref<number | null>(4);

        const disposeIssues = registry1.register({
            id: 'github-issues',
            order: 60,
            icon: 'mdi:record-circle-outline',
            colorVar: '--max-primary-500',
            count: (projectId) => (projectId === 'proj-1' ? issuesCount.value : null),
            tooltip: (n) => `${n} Issues abertas`
        });

        // Rejeita id duplicado na mesma instância
        expect(() => {
            registry1.register({
                id: 'github-issues',
                order: 65,
                icon: 'mdi:bug',
                colorVar: '--max-primary-500',
                count: () => 1,
                tooltip: () => 'Duplicado'
            });
        }).toThrowError(/duplicado/);

        const counters: ProjectStatusCounters = {
            running: 1,
            continuous: 0,
            finished: 0,
            error: 0,
            idle: 0
        };

        const wrapper1 = mount(ProjectStatusBadges, {
            props: {
                projectId: 'proj-1',
                counters
            },
            global: {
                provide: {
                    [PROJECT_BADGE_REGISTRY_KEY as symbol]: registry1
                },
                stubs: { MaxIcon: true }
            }
        });

        // Outra instância da aplicação (registry2) está isolada e não exibe github-issues
        const wrapper2 = mount(ProjectStatusBadges, {
            props: {
                projectId: 'proj-1',
                counters
            },
            global: {
                provide: {
                    [PROJECT_BADGE_REGISTRY_KEY as symbol]: registry2
                },
                stubs: { MaxIcon: true }
            }
        });

        expect(wrapper1.findAll('.project-status-badges__pill').map((p) => p.attributes('data-badge-key'))).toEqual([
            'running',
            'github-issues'
        ]);
        expect(wrapper2.findAll('.project-status-badges__pill').map((p) => p.attributes('data-badge-key'))).toEqual([
            'running'
        ]);

        const issuePill = wrapper1.find('[data-badge-key="github-issues"]');
        expect(issuePill.text()).toBe('4');
        expect(issuePill.attributes('aria-label')).toBe('4 Issues abertas');

        // Alteração reativa da contagem de Issues sem mudança nas props do projeto
        issuesCount.value = 9;
        await nextTick();
        expect(wrapper1.find('[data-badge-key="github-issues"]').text()).toBe('9');
        expect(counters.running).toBe(1);

        // Disposer remove o provider de forma idempotente
        disposeIssues();
        disposeIssues();
        await nextTick();
        expect(wrapper1.find('[data-badge-key="github-issues"]').exists()).toBe(false);
    });
});
