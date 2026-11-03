import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import RagResults from '@/components/ai/RagResults.vue';
import * as adapters from '@/composables/ai/adapters';
import { useAiStore } from '@/stores/ai';

describe('RagResults.vue (Fase 12 — Unidade 04)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('renderiza resultados da busca RAG, permite inserir trecho no input e fechar painel', async () => {
        const aiStore = useAiStore();
        aiStore.ragSlugs = [
            { slug: 'laravel', title: 'Laravel 12', documents: 120 },
            { slug: 'vue', title: 'Vue 3.6', documents: 95 }
        ];
        aiStore.setPaneRagResults('pane-1', {
            slug: 'laravel',
            query: 'artisan queue:work',
            latencyMs: 38,
            results: [
                {
                    id: 'chunk-1',
                    title: 'Filas no Laravel',
                    content: 'php artisan queue:work --tries=3',
                    score: 0.91,
                    source: 'docs/queues.md',
                    url: null
                }
            ]
        });

        const wrapper = mount(RagResults, {
            props: { paneId: 'pane-1' },
            global: { stubs: { MaxIcon: true } }
        });

        expect(wrapper.find('[data-testid="rag-results"]').exists()).toBe(true);
        expect(wrapper.text()).toContain('Filas no Laravel');
        expect(wrapper.text()).toContain('91%');

        const insertSpy = vi.spyOn(adapters, 'insertCommandIntoRichInput').mockReturnValue(true);
        await wrapper.find('[data-testid="btn-insert-rag-0"]').trigger('click');
        expect(insertSpy).toHaveBeenCalledWith('pane-1', 'php artisan queue:work --tries=3');

        await wrapper.find('[data-testid="btn-close-rag"]').trigger('click');
        expect(aiStore.ragResultsForPane('pane-1')).toBeNull();
    });
});
