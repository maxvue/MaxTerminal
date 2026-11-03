import { describe, expect, it } from 'vitest';
import { mount } from '@vue/test-utils';
import ArtisanSchedulePicker from '../ArtisanSchedulePicker.vue';

describe('ArtisanSchedulePicker', () => {
    it('habilita agendamento e alterna entre presets e cron customizado', async () => {
        const wrapper = mount(ArtisanSchedulePicker, {
            props: {
                modelValue: null,
                params: [],
                issues: []
            }
        });

        const checkbox = wrapper.find('[data-testid="schedule-enable-checkbox"]');
        await checkbox.setValue(true);

        const emittedEnable = wrapper.emitted('update:modelValue');
        expect(emittedEnable).toBeTruthy();
        expect(emittedEnable![0]![0]).toEqual({
            frequency: { kind: 'dailyAt', time: '08:00' },
            arguments: [],
            options: [],
            withoutOverlapping: false,
            onOneServer: false,
            runInBackground: false
        });

        await wrapper.setProps({
            modelValue: {
                frequency: { kind: 'cron', expression: '0 3 * * *' },
                arguments: [],
                options: [],
                withoutOverlapping: true,
                onOneServer: false,
                runInBackground: false
            }
        });

        const cronInput = wrapper.find('[data-testid="schedule-cron-input"]');
        expect(cronInput.exists()).toBe(true);
    });
});
