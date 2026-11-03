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
    }
];

describe('ThemePicker.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('filtra temas por polaridade e busca textual e emite select', async () => {
        const wrapper = mount(ThemePicker, {
            props: {
                themes: summaries,
                activeThemeId: 'max-navy-teal'
            }
        });

        expect(wrapper.findAll('.theme-swatch')).toHaveLength(2);

        const filterBtns = wrapper.findAll('.theme-picker__filter-btn');
        await filterBtns[2]!.trigger('click');
        expect(wrapper.findAll('.theme-swatch')).toHaveLength(1);

        await filterBtns[0]!.trigger('click');
        const input = wrapper.find('.theme-picker__search');
        await input.setValue('Navy');
        expect(wrapper.findAll('.theme-swatch')).toHaveLength(1);

        await wrapper.find('.theme-swatch').trigger('click');
        expect(wrapper.emitted('select')?.[0]).toEqual(['max-navy-teal']);
    });
});
