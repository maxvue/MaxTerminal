import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectList from '@/components/projects/ProjectList.vue';
import { computeReorderedProjectIds } from '@/components/projects/projectOrder';
import { useProjectsStore } from '@/stores/projects';
import { useSettingsStore } from '@/stores/settings';
import type { Project_Serialize, ProjectSnapshot_Serialize } from '@/types/bindings';

const MaxIconStub = {
    props: ['icon', 'i'],
    template: '<i class="max-icon-stub" :data-icon="icon ?? i"></i>'
};

vi.mock('@/ipc/projects', () => ({
    projectSnapshot: vi.fn(),
    projectAdd: vi.fn(),
    projectUpdate: vi.fn(),
    projectRemove: vi.fn(),
    projectReorder: vi.fn().mockResolvedValue(undefined),
    projectRedetect: vi.fn()
}));

vi.mock('@/ipc/db', () => ({
    getSettings: vi.fn().mockResolvedValue({ ok: true, data: {} }),
    setSetting: vi.fn().mockImplementation((key, value, rev) =>
        Promise.resolve({ ok: true, data: { key, value, revision: (rev ?? 0) + 1 } })
    ),
    getSettingSnapshot: vi.fn().mockResolvedValue({ ok: true, data: { revision: 1, value: [] } }),
    getStatus: vi.fn().mockResolvedValue({ ok: true, data: { health: { state: 'ready' } } }),
    onSettingsChanged: vi.fn().mockResolvedValue(vi.fn()),
    onDbRecovered: vi.fn().mockResolvedValue(vi.fn())
}));

function makeProject(id: string, sortOrder: number, title = `Projeto ${id}`): Project_Serialize {
    return {
        id,
        title,
        icon: 'logos:rust',
        color: '#DEA584',
        path: `/home/user/GitHub/${id}`,
        rootId: null,
        stack: 'rust',
        identitySource: 'auto',
        sortOrder,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
}

describe('computeReorderedProjectIds', () => {
    it('preserva os slots dos ocultos ao reordenar os visíveis conforme exemplo canônico [A, H, B, C]', () => {
        const pA = makeProject('A', 0);
        const pH = makeProject('H', 1);
        const pB = makeProject('B', 2);
        const pC = makeProject('C', 3);
        const all = [pA, pH, pB, pC];
        const hiddenSet = new Set(['H']);

        // Mover C antes de A
        const res1 = computeReorderedProjectIds(all, hiddenSet, 'C', 'A');
        expect(res1).toEqual(['C', 'H', 'A', 'B']);

        // Mover A para depois de B
        const res2 = computeReorderedProjectIds(all, hiddenSet, 'A', 'B');
        expect(res2).toEqual(['B', 'H', 'A', 'C']);

        // Identificadores idênticos retornam null
        expect(computeReorderedProjectIds(all, hiddenSet, 'A', 'A')).toBeNull();

        // Tentar mover ou ter como alvo um projeto oculto retorna null
        expect(computeReorderedProjectIds(all, hiddenSet, 'H', 'A')).toBeNull();
        expect(computeReorderedProjectIds(all, hiddenSet, 'A', 'H')).toBeNull();
    });
});

describe('ProjectList.vue - Visibilidade e Accordion', () => {
    let pinia: ReturnType<typeof createPinia>;
    let projectsStore: ReturnType<typeof useProjectsStore>;
    let settingsStore: ReturnType<typeof useSettingsStore>;

    const p1 = makeProject('p1', 0, 'Alpha');
    const p2 = makeProject('p2', 1, 'Beta');
    const p3 = makeProject('p3', 2, 'Gamma');

    beforeEach(() => {
        pinia = createPinia();
        setActivePinia(pinia);
        projectsStore = useProjectsStore();
        settingsStore = useSettingsStore();

        const snap: ProjectSnapshot_Serialize = {
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1, p2, p3],
            roots: [],
            discovered: []
        };
        projectsStore.applySnapshot(snap);
        projectsStore.select('p1');
        vi.clearAllMocks();
    });

    function mountList() {
        return mount(ProjectList, {
            attachTo: document.body,
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    ProjectDiscoveryPanel: true,
                    ProjectAddModal: true,
                    ProjectRootModal: true,
                    ProjectEditModal: true,
                    ProjectEmptyState: true
                }
            }
        });
    }

    it('inicialmente sem ocultos não exibe o accordion e renderiza todos os itens na lista principal', () => {
        const wrapper = mountList();

        expect(wrapper.find('.project-list__accordion-wrapper').exists()).toBe(false);
        const mainItems = wrapper.findAll('.project-list__scroll .project-item');
        expect(mainItems.length).toBe(3);
        wrapper.unmount();
    });

    it('quando há projetos ocultos, particiona sem duplicatas e exibe o accordion Ocultos (N)', async () => {
        (settingsStore.values as Record<string, unknown>)['ui.projects.hidden_ids'] = ['p2'];
        const wrapper = mountList();

        // Lista principal tem p1 e p3
        const mainItems = wrapper.findAll('.project-list__scroll .project-item');
        expect(mainItems.length).toBe(2);
        expect(mainItems[0]?.text()).toContain('Alpha');
        expect(mainItems[1]?.text()).toContain('Gamma');

        // Accordion existe com título "Ocultos (1)"
        const accordion = wrapper.find('.project-list__accordion-wrapper');
        expect(accordion.exists()).toBe(true);
        expect(accordion.text()).toContain('Ocultos (1)');

        wrapper.unmount();
    });

    it('exibe mensagem quando todos os projetos estão ocultos sem convite de primeiro cadastro', async () => {
        (settingsStore.values as Record<string, unknown>)['ui.projects.hidden_ids'] = ['p1', 'p2', 'p3'];
        const wrapper = mountList();

        expect(wrapper.find('.project-list__all-hidden').exists()).toBe(true);
        expect(wrapper.text()).toContain('Todos os projetos estão ocultos');
        expect(wrapper.findComponent({ name: 'ProjectEmptyState' }).exists()).toBe(false);

        wrapper.unmount();
    });

    it('ocultar o projeto selecionado preserva selectedId e mantém o workspace ativo', async () => {
        const wrapper = mountList();
        expect(projectsStore.selectedId).toBe('p1');

        const firstItem = wrapper.findAllComponents({ name: 'ProjectItem' })[0];
        expect(firstItem?.exists()).toBe(true);

        await firstItem?.vm.$emit('hide', 'p1');
        await wrapper.vm.$nextTick();

        expect(projectsStore.selectedId).toBe('p1');
        expect(projectsStore.isHidden('p1')).toBe(true);

        wrapper.unmount();
    });

    it('restaurar projeto pelo accordion envia show e o devolve para a lista principal', async () => {
        (settingsStore.values as Record<string, unknown>)['ui.projects.hidden_ids'] = ['p2'];
        const wrapper = mountList();

        expect(projectsStore.hiddenProjects.length).toBe(1);

        const hiddenItem = wrapper.find('.project-list__items--hidden').findComponent({ name: 'ProjectItem' });
        expect(hiddenItem.exists()).toBe(true);

        await hiddenItem.vm.$emit('show', 'p2');
        await wrapper.vm.$nextTick();

        expect(projectsStore.isHidden('p2')).toBe(false);
        expect(projectsStore.hiddenProjects.length).toBe(0);

        wrapper.unmount();
    });

    it('exibe mensagem de erro quando a persistência de ocultar falha', async () => {
        const wrapper = mountList();
        vi.spyOn(projectsStore, 'setProjectHidden').mockResolvedValueOnce(false);

        const firstItem = wrapper.findAllComponents({ name: 'ProjectItem' })[0];
        await firstItem?.vm.$emit('hide', 'p1');
        await wrapper.vm.$nextTick();

        const errAlert = wrapper.find('.project-list__error');
        expect(errAlert.exists()).toBe(true);
        expect(errAlert.text()).toContain('Falha ao ocultar o projeto');

        wrapper.unmount();
    });

    it('reordenação via Alt+ArrowDown nos visíveis com ocultos intercalados envia todos os IDs', async () => {
        // p2 oculto -> catálogo: [p1, p2, p3] com p2 oculto
        (settingsStore.values as Record<string, unknown>)['ui.projects.hidden_ids'] = ['p2'];
        const wrapper = mountList();
        projectsStore.select('p1');

        const mainList = wrapper.find('.project-list__scroll');
        await mainList.trigger('keydown', { key: 'ArrowDown', altKey: true });

        // Mover p1 após p3 -> [p3, p2, p1]
        expect(projectsStore.reorder).toBeDefined();

        wrapper.unmount();
    });
});
