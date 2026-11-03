import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import AiPreferences from '@/components/preferences/AiPreferences.vue';
import { aiIpc } from '@/ipc/ai';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';

describe('AiPreferences.vue (Fase 12 — Unidade 05)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('renderiza controles de configuração do MaxAiManager, salva alterações e executa ações de manutenção', async () => {
        const aiStore = useAiStore();
        aiStore.settings = { ...DEFAULT_AI_SETTINGS };

        const saveSpy = vi.spyOn(aiIpc, 'settingsUpdate').mockResolvedValue({
            ...DEFAULT_AI_SETTINGS,
            baseUrl: 'http://127.0.0.1:8899',
            revision: 2
        });
        const clearCacheSpy = vi.spyOn(aiIpc, 'cacheClear').mockResolvedValue(3);
        const clearHistSpy = vi.spyOn(aiIpc, 'historyClear').mockResolvedValue(5);

        const wrapper = mount(AiPreferences, {
            global: {
                stubs: {
                    MaxIcon: true
                }
            }
        });

        expect(wrapper.text()).toContain('Integração com MaxAiManager');

        await wrapper.find('[data-testid="input-ai-base-url"] input').setValue('http://127.0.0.1:8899');
        await wrapper.find('[data-testid="input-rag-topk"] input').setValue('8');
        await wrapper.find('[data-testid="btn-save-ai-settings"]').trigger('click');
        await flushPromises();

        expect(saveSpy).toHaveBeenCalledTimes(1);
        expect(wrapper.find('[data-testid="ai-prefs-feedback"]').text()).toContain(
            'Preferências de IA salvas com sucesso.'
        );

        await wrapper.find('[data-testid="btn-clear-cache"]').trigger('click');
        await flushPromises();
        expect(clearCacheSpy).toHaveBeenCalledWith('all');

        await wrapper.find('[data-testid="btn-clear-history"]').trigger('click');
        await flushPromises();
        expect(clearHistSpy).toHaveBeenCalledWith(null);
    });
});
