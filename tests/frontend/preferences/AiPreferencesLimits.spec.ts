import type { ComponentPublicInstance } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import AiPreferences from '@/components/preferences/AiPreferences.vue';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';

describe('AiPreferences.vue — Validações de limites numéricos (debounceMs e topK)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('rejeita debounce com valor decimal ou fora dos limites (0..10000)', async () => {
        const store = useAiStore();
        const saveSpy = vi.spyOn(store, 'saveSettings').mockResolvedValue(store.settings ?? DEFAULT_AI_SETTINGS);

        const wrapper = mount(AiPreferences, {
            global: {
                stubs: {
                    HarnessPicker: true
                }
            }
        });

        const debounceComp = wrapper.findComponent<ComponentPublicInstance>('[data-testid="input-diagnosis-debounce"]');

        // Testar com valor negativo
        await debounceComp.vm.$emit('update:modelValue', -10);
        await wrapper.get('[data-testid="btn-save-ai-settings"]').trigger('click');
        expect(saveSpy).not.toHaveBeenCalled();

        let alert = wrapper.find('[data-testid="ai-prefs-feedback"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('O atraso de debounce do diagnóstico deve estar entre 0 e 10000 ms.');

        // Testar com valor decimal
        await debounceComp.vm.$emit('update:modelValue', 150.5);
        await wrapper.get('[data-testid="btn-save-ai-settings"]').trigger('click');
        expect(saveSpy).not.toHaveBeenCalled();

        alert = wrapper.find('[data-testid="ai-prefs-feedback"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('O atraso de debounce do diagnóstico deve ser um número inteiro (decimais não permitidos).');

        // Testar com valor > 10000
        await debounceComp.vm.$emit('update:modelValue', 10001);
        await wrapper.get('[data-testid="btn-save-ai-settings"]').trigger('click');
        expect(saveSpy).not.toHaveBeenCalled();

        alert = wrapper.find('[data-testid="ai-prefs-feedback"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('O atraso de debounce do diagnóstico deve estar entre 0 e 10000 ms.');
    });

    it('rejeita topK com valor decimal ou fora dos limites (1..20)', async () => {
        const store = useAiStore();
        const saveSpy = vi.spyOn(store, 'saveSettings').mockResolvedValue(store.settings ?? DEFAULT_AI_SETTINGS);

        const wrapper = mount(AiPreferences, {
            global: {
                stubs: {
                    HarnessPicker: true
                }
            }
        });

        const topkComp = wrapper.findComponent<ComponentPublicInstance>('[data-testid="input-rag-topk"]');

        // Testar com zero (< 1)
        await topkComp.vm.$emit('update:modelValue', 0);
        await wrapper.get('[data-testid="btn-save-ai-settings"]').trigger('click');
        expect(saveSpy).not.toHaveBeenCalled();

        let alert = wrapper.find('[data-testid="ai-prefs-feedback"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('A quantidade de trechos RAG (topK) deve estar entre 1 e 20.');

        // Testar com decimal
        await topkComp.vm.$emit('update:modelValue', 4.2);
        await wrapper.get('[data-testid="btn-save-ai-settings"]').trigger('click');
        expect(saveSpy).not.toHaveBeenCalled();

        alert = wrapper.find('[data-testid="ai-prefs-feedback"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('A quantidade de trechos RAG (topK) deve ser um número inteiro (decimais não permitidos).');

        // Testar com valor > 20
        await topkComp.vm.$emit('update:modelValue', 21);
        await wrapper.get('[data-testid="btn-save-ai-settings"]').trigger('click');
        expect(saveSpy).not.toHaveBeenCalled();

        alert = wrapper.find('[data-testid="ai-prefs-feedback"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('A quantidade de trechos RAG (topK) deve estar entre 1 e 20.');
    });

    it('salva com sucesso quando debounceMs e topK são inteiros válidos dentro dos limites', async () => {
        const store = useAiStore();
        const saveSpy = vi.spyOn(store, 'saveSettings').mockResolvedValue(store.settings ?? DEFAULT_AI_SETTINGS);

        const wrapper = mount(AiPreferences, {
            global: {
                stubs: {
                    HarnessPicker: true
                }
            }
        });

        const debounceComp = wrapper.findComponent<ComponentPublicInstance>('[data-testid="input-diagnosis-debounce"]');
        const topkComp = wrapper.findComponent<ComponentPublicInstance>('[data-testid="input-rag-topk"]');

        await debounceComp.vm.$emit('update:modelValue', 250);
        await topkComp.vm.$emit('update:modelValue', 8);

        await wrapper.get('[data-testid="btn-save-ai-settings"]').trigger('click');
        expect(saveSpy).toHaveBeenCalledTimes(1);

        const alert = wrapper.find('[data-testid="ai-prefs-feedback"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('Preferências de IA salvas com sucesso.');
    });
});
