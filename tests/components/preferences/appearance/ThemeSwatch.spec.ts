import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import ThemeSwatch from '@/components/preferences/appearance/ThemeSwatch.vue';
import type { ThemeSummary } from '@/types/theme';

const builtinSummary: ThemeSummary = {
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
    ]
};

const customSummary: ThemeSummary = {
    ...builtinSummary,
    id: 'oceano-custom',
    name: 'Oceano Custom',
    origin: 'custom'
};

describe('ThemeSwatch.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('desabilita editar e oculta exportar/excluir em tema nativo, mantendo duplicar acessível', async () => {
        const wrapper = mount(ThemeSwatch, {
            props: {
                summary: builtinSummary,
                active: true
            }
        });

        const editBtn = wrapper.find(
            'button[aria-label="Tema nativo Max Navy/Teal somente leitura — duplique para editar"]'
        );
        expect(editBtn.exists()).toBe(true);
        expect(editBtn.attributes('disabled')).toBeDefined();

        const dupBtn = wrapper.find('button[aria-label="Duplicar tema Max Navy/Teal"]');
        expect(dupBtn.exists()).toBe(true);

        expect(
            wrapper.find('button[aria-label="Exportar tema Max Navy/Teal"]').exists()
        ).toBe(false);
        expect(
            wrapper.find('button[aria-label="Excluir tema Max Navy/Teal"]').exists()
        ).toBe(false);

        await dupBtn.trigger('click');
        await dupBtn.trigger('keydown.enter');
        expect(wrapper.emitted('duplicate')?.[0]).toEqual(['max-navy-teal']);
        expect(wrapper.emitted('select')).toBeUndefined();
    });

    it('expõe as quatro ações em tema customizado ( inclusive ativo ) sem selecionar o cartão ao clicar ou pressionar Enter nas ações', async () => {
        const wrapper = mount(ThemeSwatch, {
            props: {
                summary: customSummary,
                active: true
            }
        });

        const editBtn = wrapper.find('button[aria-label="Editar tema Oceano Custom"]');
        const dupBtn = wrapper.find('button[aria-label="Duplicar tema Oceano Custom"]');
        const exportBtn = wrapper.find('button[aria-label="Exportar tema Oceano Custom"]');
        const deleteBtn = wrapper.find('button[aria-label="Excluir tema Oceano Custom"]');

        expect(editBtn.exists()).toBe(true);
        expect(dupBtn.exists()).toBe(true);
        expect(exportBtn.exists()).toBe(true);
        expect(deleteBtn.exists()).toBe(true);

        await editBtn.trigger('click');
        await editBtn.trigger('keydown.enter');
        await dupBtn.trigger('click');
        await exportBtn.trigger('click');
        await deleteBtn.trigger('click');
        await deleteBtn.trigger('keydown.space');

        expect(wrapper.emitted('edit')?.[0]).toEqual(['oceano-custom']);
        expect(wrapper.emitted('duplicate')?.[0]).toEqual(['oceano-custom']);
        expect(wrapper.emitted('export')?.[0]).toEqual(['oceano-custom']);
        expect(wrapper.emitted('delete')?.[0]).toEqual(['oceano-custom']);
        expect(wrapper.emitted('select')).toBeUndefined();

        await wrapper.find('.theme-swatch').trigger('keydown.enter');
        expect(wrapper.emitted('select')?.[0]).toEqual(['oceano-custom']);
    });
});
