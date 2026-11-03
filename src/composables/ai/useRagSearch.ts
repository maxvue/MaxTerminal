import { computed, ref, toValue, type ComputedRef, type MaybeRefOrGetter, type Ref } from 'vue';
import {
    createAiRequestId,
    insertCommandIntoRichInput,
    resolvePaneProjectInfo
} from '@/composables/ai/adapters';
import { useIpcError } from '@/composables/useIpcError';
import { aiIpc } from '@/ipc/ai';
import { useAiStore } from '@/stores/ai';
import type {
    PaneId,
    RagResult_Serialize,
    RagSearchResponse_Serialize,
    RagSlug_Serialize
} from '@/types/bindings';

export interface UseRagSearchReturn {
    slugs: ComputedRef<RagSlug_Serialize[]>;
    selectedSlug: Ref<string>;
    loading: Ref<boolean>;
    error: Ref<string | null>;
    response: ComputedRef<RagSearchResponse_Serialize | null>;
    results: ComputedRef<RagResult_Serialize[]>;
    loadSlugs: () => Promise<RagSlug_Serialize[]>;
    search: (query: string, slugOverride?: string, topK?: number) => Promise<RagSearchResponse_Serialize>;
    copyResultContent: (result: RagResult_Serialize) => Promise<boolean>;
    insertResultIntoInput: (result: RagResult_Serialize) => boolean;
    clear: () => void;
}

/**
 * Composable de consulta RAG (`/v1/rags/{slug}/search`) para um painel de terminal.
 * Resolve automaticamente o slug padrão a partir da stack detectada do projeto ativo.
 */
export function useRagSearch(paneIdSource: MaybeRefOrGetter<PaneId>): UseRagSearchReturn {
    const aiStore = useAiStore();
    const ipcError = useIpcError();

    const paneId = computed<PaneId>(() => toValue(paneIdSource));
    const loading = ref<boolean>(false);
    const error = ref<string | null>(null);

    function resolveInitialSlug(): string {
        const modeSlug = aiStore.activeModeForPane(paneId.value)?.ragSlug;
        if (modeSlug) {
            return modeSlug;
        }
        const { stack } = resolvePaneProjectInfo(paneId.value);
        const stackMap = aiStore.settings?.rag.stackSlugs ?? {};
        if (stack) {
            const normalized = stack.toLowerCase();
            if (stackMap[normalized]) {
                return stackMap[normalized]!;
            }
            return normalized;
        }
        if (aiStore.settings?.rag.lastSlug) {
            return aiStore.settings.rag.lastSlug;
        }
        return aiStore.ragSlugs[0]?.slug ?? 'laravel';
    }

    const selectedSlug = ref<string>(resolveInitialSlug());

    const slugs = computed<RagSlug_Serialize[]>(() => aiStore.ragSlugs);

    const response = computed<RagSearchResponse_Serialize | null>(() =>
        aiStore.ragResultsForPane(paneId.value)
    );

    const results = computed<RagResult_Serialize[]>(() => response.value?.results ?? []);

    async function loadSlugs(): Promise<RagSlug_Serialize[]> {
        try {
            const list = await aiStore.loadRagSlugs();
            if (!selectedSlug.value && list.length > 0) {
                selectedSlug.value = list[0]!.slug;
            }
            return list;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            return [];
        }
    }

    async function search(
        query: string,
        slugOverride?: string,
        topK?: number
    ): Promise<RagSearchResponse_Serialize> {
        const trimmed = query.trim();
        if (!trimmed) {
            const msg = 'Informe um termo ou dúvida para pesquisar na documentação.';
            error.value = msg;
            throw new Error(msg);
        }

        const slug = (slugOverride ?? selectedSlug.value ?? resolveInitialSlug()).trim();
        if (!slug) {
            const msg = 'Selecione uma base de documentação (slug RAG).';
            error.value = msg;
            throw new Error(msg);
        }

        selectedSlug.value = slug;
        loading.value = true;
        error.value = null;

        try {
            const res = await aiIpc.ragSearch({
                requestId: createAiRequestId('ai-rag'),
                slug,
                query: trimmed,
                topK: topK ?? aiStore.settings?.rag.topK ?? 5,
                paneId: paneId.value
            });

            aiStore.setPaneRagResults(paneId.value, res);
            aiStore.closePanePrompt(paneId.value);
            return res;
        } catch (err: unknown) {
            error.value = ipcError.message(err);
            throw err;
        } finally {
            loading.value = false;
        }
    }

    async function copyResultContent(result: RagResult_Serialize): Promise<boolean> {
        if (typeof navigator !== 'undefined' && navigator.clipboard?.writeText) {
            try {
                await navigator.clipboard.writeText(result.content);
                return true;
            } catch {
                return false;
            }
        }
        return false;
    }

    function insertResultIntoInput(result: RagResult_Serialize): boolean {
        return insertCommandIntoRichInput(paneId.value, result.content);
    }

    function clear(): void {
        error.value = null;
        aiStore.setPaneRagResults(paneId.value, null);
    }

    return {
        slugs,
        selectedSlug,
        loading,
        error,
        response,
        results,
        loadSlugs,
        search,
        copyResultContent,
        insertResultIntoInput,
        clear
    };
}
