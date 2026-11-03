import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import ArtisanParamsBuilder from '../ArtisanParamsBuilder.vue';
import type { ArtisanParam } from '@/types/bindings';

describe('ArtisanParamsBuilder', () => {
    it('adiciona, move e remove parametros emitindo update:modelValue', async () => {
        const initial: ArtisanParam[] = [
            {
                kind: 'argument',
                name: 'tenant',
                description: 'Tenant ID',
                optional: false,
                isArray: false,
                default: null,
                shortcut: null,
                acceptsValue: false
            }
        ];

        const wrapper = mount(ArtisanParamsBuilder, {
            props: {
                modelValue: initial,
                issues: []
            }
        });

        const addOptionBtn = wrapper.find('[data-testid="add-option-btn"]');
        await addOptionBtn.trigger('click');

        const emittedAfterAdd = wrapper.emitted('update:modelValue');
        expect(emittedAfterAdd).toBeTruthy();
        const listWithOption = emittedAfterAdd![0]![0] as ArtisanParam[];
        expect(listWithOption).toHaveLength(2);
        expect(listWithOption[1]?.kind).toBe('option');

        await wrapper.setProps({ modelValue: listWithOption });
        expect(wrapper.find('[data-testid="artisan-param-row-0"]').exists()).toBe(true);
        expect(wrapper.find('[data-testid="artisan-param-row-1"]').exists()).toBe(true);
    });
});
