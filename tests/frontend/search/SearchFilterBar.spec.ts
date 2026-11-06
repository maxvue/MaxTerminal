import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import { MaxInputSelect } from '@maxvue/max-components-ui';
import SearchFilterBar from '@/components/search/SearchFilterBar.vue';
import { createDefaultSearchFilters } from '@/stores/search';
import type { Project_Serialize, SearchSort } from '@/types/bindings';

describe('SearchFilterBar', () => {
    let pinia: ReturnType<typeof createPinia>;

    beforeEach(() => {
        pinia = createPinia();
        setActivePinia(pinia);
    });

    const mockProjects: Project_Serialize[] = [
        {
            id: 'proj-1',
            rootId: null,
            title: 'MaxTerminal',
            icon: 'logos:vue',
            color: '#14B8A6',
            path: '/home/user/MaxTerminal',
            stack: 'vue',
            identitySource: 'manual',
            sortOrder: 0,
            missing: false,
            createdAt: 1000,
            updatedAt: 1000
        },
        {
            id: 'proj-2',
            rootId: null,
            title: 'Backend API',
            icon: 'logos:laravel',
            color: '#FF2D20',
            path: '/home/user/backend',
            stack: 'laravel',
            identitySource: 'manual',
            sortOrder: 1,
            missing: false,
            createdAt: 1000,
            updatedAt: 1000
        }
    ];

    const defaultSort: SearchSort = { kind: 'recent' };

    it('renderiza os 4 seletores MaxInputSelect para filtros e ordenação', () => {
        const wrapper = mount(SearchFilterBar, {
            global: { plugins: [pinia] },
            props: {
                filters: createDefaultSearchFilters(),
                sort: defaultSort,
                projects: mockProjects
            }
        });

        const selects = wrapper.findAllComponents(MaxInputSelect);
        expect(selects.length).toBe(4);

        // Seletor de Escopo de projeto
        const scopeSelect = selects[0]!;
        expect(scopeSelect.find('.max-select').attributes('aria-label')).toBe('Filtrar por projeto');
        const scopeOptions = scopeSelect.props('options') as { label: string; value: string }[];
        expect(scopeOptions).toHaveLength(3);
        expect(scopeOptions[0]?.label).toBe('Todos os projetos');
        expect(scopeOptions[1]?.label).toBe('MaxTerminal');

        // Seletor de Saída
        const exitSelect = selects[1]!;
        expect(exitSelect.find('.max-select').attributes('aria-label')).toBe('Filtrar por código de saída');

        // Seletor de Período
        const periodSelect = selects[2]!;
        expect(periodSelect.find('.max-select').attributes('aria-label')).toBe('Filtrar por período');

        // Seletor de Ordenação
        const sortSelect = selects[3]!;
        expect(sortSelect.find('.max-select').attributes('aria-label')).toBe('Ordenar resultados');
    });

    it('emite update:filters com novo projectId ao selecionar projeto', async () => {
        const wrapper = mount(SearchFilterBar, {
            global: { plugins: [pinia] },
            props: {
                filters: createDefaultSearchFilters(),
                sort: defaultSort,
                projects: mockProjects
            }
        });

        const selects = wrapper.findAllComponents(MaxInputSelect);
        const scopeSelect = selects[0]!;
        await scopeSelect.vm.$emit('update:modelValue', 'proj-2');

        const emitted = wrapper.emitted('update:filters');
        expect(emitted).toBeDefined();
        expect(emitted?.[0]?.[0]).toEqual({ projectId: 'proj-2' });
    });

    it('emite update:filters com null ao selecionar todos os projetos', async () => {
        const wrapper = mount(SearchFilterBar, {
            global: { plugins: [pinia] },
            props: {
                filters: {
                    ...createDefaultSearchFilters(),
                    projectId: 'proj-1'
                },
                sort: defaultSort,
                projects: mockProjects
            }
        });

        const selects = wrapper.findAllComponents(MaxInputSelect);
        const scopeSelect = selects[0]!;
        await scopeSelect.vm.$emit('update:modelValue', '');

        const emitted = wrapper.emitted('update:filters');
        expect(emitted).toBeDefined();
        expect(emitted?.[0]?.[0]).toEqual({ projectId: null });
    });

    it('emite update:filters com tipo de código de saída ao alterar seletor de saída', async () => {
        const wrapper = mount(SearchFilterBar, {
            global: { plugins: [pinia] },
            props: {
                filters: createDefaultSearchFilters(),
                sort: defaultSort,
                projects: mockProjects
            }
        });

        const selects = wrapper.findAllComponents(MaxInputSelect);
        const exitSelect = selects[1]!;
        await exitSelect.vm.$emit('update:modelValue', 'error');

        const emitted = wrapper.emitted('update:filters');
        expect(emitted).toBeDefined();
        expect(emitted?.[0]?.[0]).toEqual({ exit: { kind: 'error' } });
    });

    it('emite update:sort com novo critério de ordenação', async () => {
        const wrapper = mount(SearchFilterBar, {
            global: { plugins: [pinia] },
            props: {
                filters: createDefaultSearchFilters(),
                sort: defaultSort,
                projects: mockProjects
            }
        });

        const selects = wrapper.findAllComponents(MaxInputSelect);
        const sortSelect = selects[3]!;
        await sortSelect.vm.$emit('update:modelValue', 'relevance');

        const emitted = wrapper.emitted('update:sort');
        expect(emitted).toBeDefined();
        expect(emitted?.[0]?.[0]).toEqual({ kind: 'relevance' });
    });
});
