import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import { MaxButton } from '@maxvue/max-components-ui';
import AiStatusIndicator from '@/components/ai/AiStatusIndicator.vue';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';
import { useWindowStore } from '@/stores/window';

describe('AiStatusIndicator.vue (Fase 12 — Unidade 05)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('exibe o estado de conectividade do MaxAiManager e abre a seção ai das Preferências ao clicar', async () => {
        const aiStore = useAiStore();
        const windowStore = useWindowStore();

        aiStore.settings = { ...DEFAULT_AI_SETTINGS, enabled: true };
        aiStore.health = {
            status: 'online',
            version: '1.4.0',
            latencyMs: 9,
            baseUrl: 'http://127.0.0.1:8787',
            checkedAt: '2026-11-01T12:00:00Z',
            configRevision: 1,
            capabilities: {
                catalog: { status: 'validated' },
                generation: { status: 'validated' },
                reasoning: { status: 'validated' },
                jsonMode: { status: 'validated' },
                sse: { status: 'validated' },
                ragSearch: { status: 'validated' },
                ragList: { status: 'validated' },
                health: { status: 'validated' }
            }
        };

        const wrapper = mount(AiStatusIndicator);
        expect(wrapper.classes()).toContain('is-success');
        expect(wrapper.text()).toContain('MaxAiManager Online');
        const title = wrapper.attributes('title') ?? wrapper.findComponent(MaxButton).props('title');
        expect(title).toContain('1.4.0');

        await wrapper.trigger('click');
        expect(wrapper.emitted('openSettings')).toBeTruthy();
        expect(windowStore.preferencesOpen).toBe(true);
        expect(windowStore.preferencesSection).toBe('ai');
    });
});
