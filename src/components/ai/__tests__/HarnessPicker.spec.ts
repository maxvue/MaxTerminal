import { mount } from '@vue/test-utils';
import { describe, expect, it } from 'vitest';
import HarnessPicker from '@/components/ai/HarnessPicker.vue';
import type { Harness_Serialize } from '@/types/bindings';

const sampleHarnesses: Harness_Serialize[] = [
    {
        id: 'claude-code',
        label: 'Claude Code',
        icon: 'simple-icons:anthropic',
        models: [
            {
                id: 'claude-sonnet-4-5',
                label: 'Claude Sonnet 4.5',
                defaultReasoning: 'medium',
                contextWindow: 200000,
                reasoningLevels: [
                    { id: 'low', label: 'Baixo', rank: 1 },
                    { id: 'high', label: 'Alto', rank: 2 }
                ]
            },
            {
                id: 'claude-haiku',
                label: 'Claude Haiku',
                defaultReasoning: null,
                contextWindow: 100000,
                reasoningLevels: []
            }
        ]
    }
];

describe('HarnessPicker.vue (Fase 12 — Unidade 03)', () => {
    it('navega pelos 3 estágios (Harness → Modelo → Raciocínio) e emite update:modelValue e select', async () => {
        const wrapper = mount(HarnessPicker, {
            props: {
                harnesses: sampleHarnesses,
                modelValue: null
            },
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        // Estágio 0: escolher Harness
        expect(wrapper.find('[data-testid="stage-harnesses"]').exists()).toBe(true);
        await wrapper.find('[data-testid="harness-opt-claude-code"]').trigger('click');

        // Estágio 1: escolher Modelo com níveis de raciocínio
        expect(wrapper.find('[data-testid="stage-models"]').exists()).toBe(true);
        await wrapper.find('[data-testid="model-opt-claude-sonnet-4-5"]').trigger('click');

        // Estágio 2: escolher Nível de Raciocínio
        expect(wrapper.find('[data-testid="stage-reasoning"]').exists()).toBe(true);
        await wrapper.find('[data-testid="reasoning-opt-high"]').trigger('click');

        expect(wrapper.emitted('select')?.[0]).toEqual([
            {
                harnessId: 'claude-code',
                modelId: 'claude-sonnet-4-5',
                reasoningId: 'high'
            }
        ]);

        // Ao escolher modelo sem níveis de raciocínio, emite imediatamente com reasoningId: null
        await wrapper.find('[data-testid="step-model"]').trigger('click');
        await wrapper.find('[data-testid="model-opt-claude-haiku"]').trigger('click');
        expect(wrapper.emitted('select')?.[1]).toEqual([
            {
                harnessId: 'claude-code',
                modelId: 'claude-haiku',
                reasoningId: null
            }
        ]);
    });
});
