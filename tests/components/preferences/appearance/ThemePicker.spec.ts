import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import ThemePicker from '@/components/preferences/appearance/ThemePicker.vue';
import type { ThemeSummary } from '@/types/theme';

const summaries: ThemeSummary[] = [
    {
        id: 'max-navy-teal',
        name: 'Max Navy/Teal',
        origin: 'builtin',
        polarity: 'dark',
        background: '#0f1c2e',
        foreground: '#e2e8f0',
        cursor: '#2ea4bc',
        accentSwatches: [
            '#0d1b2a',
            '#ef4444',
            '#10b981',
            '#f59e0b',
            '#0ea5e9',
            '#a78bfa',
            '#2ea4bc',
            '#d5dde6'
        ],
        author: 'Max',
        sourceFormat: 'tabby-scheme-yaml'
    },
    {
        id: 'max-light',
        name: 'Max Light',
        origin: 'builtin',
        polarity: 'light',
        background: '#f8fafc',
        foreground: '#0f172a',
        cursor: '#00768e',
        accentSwatches: [
            '#1e293b',
            '#dc2626',
            '#059669',
            '#d97706',
            '#0284c7',
            '#7c3aed',
            '#00768e',
            '#e2e8f0'
        ],
        author: 'Max',
        sourceFormat: 'tabby-scheme-yaml'
    },
    {
        id: 'oceano-custom',
        name: 'Oceano Custom',
        origin: 'custom',
        polarity: 'dark',
        background: '#0b1420',
        foreground: '#e2e8f0',
        cursor: '#14b8a6',
        accentSwatches: [
            '#0b1420',
            '#f43f5e',
            '#10b981',
            '#f59e0b',
            '#3b82f6',
            '#a855f7',
            '#06b6d4',
            '#e2e8f0'
        ],
        author: 'Equipe Max',
        sourceFormat: null
    }
];

describe('ThemePicker.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('filtra temas por polaridade e busca textual e emite select, edit, duplicate e delete', async () => {
        const wrapper = mount(ThemePicker, {
            props: {
                themes: summaries,
                activeThemeId: 'oceano-custom'
            }
        });

        expect(wrapper.findAll('.theme-swatch')).toHaveLength(3);

        const filterBtns = wrapper.findAll('[role="radio"]');
        await filterBtns[2]!.trigger('click');
        expect(wrapper.findAll('.theme-swatch')).toHaveLength(1);

        await filterBtns[0]!.trigger('click');
        const input = wrapper.find('.theme-picker__search input');
        await input.setValue('Navy');
        expect(wrapper.findAll('.theme-swatch')).toHaveLength(1);

        // No tema builtin (Max Navy/Teal), o botão de editar está desabilitado, mas o de duplicar está habilitado
        const builtinEditBtn = wrapper.find('[aria-label*="somente leitura"]');
        expect(builtinEditBtn.exists()).toBe(true);
        expect(builtinEditBtn.attributes('disabled')).toBeDefined();

        const builtinDupBtn = wrapper.find('[aria-label="Duplicar tema Max Navy/Teal"]');
        await builtinDupBtn.trigger('click');
        expect(wrapper.emitted('duplicate')?.[0]).toEqual(['max-navy-teal']);

        // Filtra pelo tema customizado ativo: permite editar, duplicar e excluir mesmo estando ativo
        await input.setValue('Oceano');
        expect(wrapper.findAll('.theme-swatch')).toHaveLength(1);

        const customEditBtn = wrapper.find('[aria-label="Editar tema Oceano Custom"]');
        await customEditBtn.trigger('click');
        expect(wrapper.emitted('edit')?.[0]).toEqual(['oceano-custom']);

        const customDeleteBtn = wrapper.find('[aria-label="Excluir tema Oceano Custom"]');
        expect(customDeleteBtn.exists()).toBe(true);
        await customDeleteBtn.trigger('click');
        expect(wrapper.emitted('delete')?.[0]).toEqual(['oceano-custom']);

        await wrapper.find('.theme-swatch').trigger('click');
        expect(wrapper.emitted('select')?.[0]).toEqual(['oceano-custom']);
    });

    it('exibe MaxEmptyDiv com label quando nenhum tema corresponde ao filtro', async () => {
        const wrapper = mount(ThemePicker, {
            props: {
                themes: summaries,
                activeThemeId: 'oceano-custom'
            }
        });

        const input = wrapper.find('.theme-picker__search input');
        await input.setValue('TemaInexistente123');
        expect(wrapper.findAll('.theme-swatch')).toHaveLength(0);

        const emptyDiv = wrapper.findComponent({ name: 'MaxEmptyDiv' });
        expect(emptyDiv.exists()).toBe(true);
        expect(emptyDiv.props('label')).toBe('Nenhum esquema de cores corresponde ao filtro informado.');
        expect(emptyDiv.text()).toContain('Nenhum esquema de cores corresponde ao filtro informado.');
    });
});
