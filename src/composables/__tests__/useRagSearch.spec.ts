import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import * as adapters from '@/composables/ai/adapters';
import { useRagSearch } from '@/composables/ai/useRagSearch';
import { aiIpc } from '@/ipc/ai';
import { DEFAULT_AI_SETTINGS, useAiStore } from '@/stores/ai';
import type { RagSearchResponse_Serialize } from '@/types/bindings';

describe('useRagSearch (Fase 12 — Unidade 04)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('resolve slug inicial da stack do projeto, executa busca RAG, copia e insere resultado no RichInput', async () => {
        vi.spyOn(adapters, 'resolvePaneProjectInfo').mockReturnValue({
            projectId: 'proj-1',
            stack: 'vue'
        });

        const aiStore = useAiStore();
        aiStore.settings = { ...DEFAULT_AI_SETTINGS };

        const rag = useRagSearch('pane-1');
        expect(rag.selectedSlug.value).toBe('vue');

        const fakeResponse: RagSearchResponse_Serialize = {
            slug: 'vue',
            query: 'defineProps',
            latencyMs: 42,
            results: [
                {
                    id: 'doc-1',
                    title: 'Tipagem de defineProps no Vue 3',
                    content: 'defineProps<PropsInterface>()',
                    source: 'vue-docs/typescript.md',
                    url: 'https://vuejs.org/guide/typescript/composition-api.html',
                    score: 0.94
                }
            ]
        };

        vi.spyOn(aiIpc, 'ragSearch').mockResolvedValueOnce(fakeResponse);

        const res = await rag.search('defineProps');
        expect(res.results).toHaveLength(1);
        expect(rag.results.value[0]?.title).toContain('defineProps');

        const insertSpy = vi.spyOn(adapters, 'insertCommandIntoRichInput').mockReturnValue(true);
        expect(rag.insertResultIntoInput(rag.results.value[0]!)).toBe(true);
        expect(insertSpy).toHaveBeenCalledWith('pane-1', 'defineProps<PropsInterface>()');

        rag.clear();
        expect(rag.results.value).toHaveLength(0);
    });

    it('rejeita consulta vazia e carrega lista de slugs do gateway', async () => {
        const rag = useRagSearch('pane-1');
        await expect(rag.search('   ')).rejects.toThrow(
            'Informe um termo ou dúvida para pesquisar na documentação.'
        );

        vi.spyOn(aiIpc, 'ragListSlugs').mockResolvedValueOnce([
            { slug: 'tauri', title: 'Tauri v2', documents: 80 }
        ]);
        const list = await rag.loadSlugs();
        expect(list).toHaveLength(1);
        expect(rag.slugs.value[0]?.slug).toBe('tauri');
    });
});
