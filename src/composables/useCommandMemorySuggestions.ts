import {
    computed,
    getCurrentScope,
    onScopeDispose,
    ref,
    toValue,
    watch,
    type ComputedRef,
    type MaybeRefOrGetter,
    type Ref
} from 'vue';
import {
    insertCommandIntoRichInput,
    resolvePaneProjectInfo
} from '@/composables/ai/adapters';
import { memoryIpc } from '@/ipc/memory';
import { useShellHistoryStore } from '@/stores/shellHistory';
import { useSkillsStore } from '@/stores/skills';
import { useTabsStore } from '@/stores/tabs';
import type { MemoryEntry_Serialize, PaneId, ProjectId } from '@/types/bindings';

const DEBOUNCE_MS = 150;
const SUGGESTIONS_LIMIT = 20;

export interface UseCommandMemorySuggestionsOptions {
    projectId?: MaybeRefOrGetter<ProjectId | null>;
    cwd?: MaybeRefOrGetter<string | null>;
    draftRef?: Ref<string>;
    debounceMs?: number;
}

export interface UseCommandMemorySuggestionsReturn {
    queryText: Ref<string>;
    suggestions: ComputedRef<MemoryEntry_Serialize[]>;
    loading: Readonly<Ref<boolean>>;
    requestSeq: Readonly<Ref<number>>;
    queryNow: (prefix?: string) => Promise<MemoryEntry_Serialize[]>;
    applySuggestion: (entry: MemoryEntry_Serialize) => boolean;
    cancelPending: () => void;
}

/**
 * Composable de consulta de sugestões da memória contextual de comandos (`spec.md` §6.3, `CA-31`).
 * Aplica debounce de 150 ms, número de sequência monotônico para descartar respostas antigas
 * e escreve `entry.command` apenas no draft do input rico (sem executar no PTY nem alterar `.history`).
 */
export function useCommandMemorySuggestions(
    paneId: MaybeRefOrGetter<PaneId>,
    options?: UseCommandMemorySuggestionsOptions
): UseCommandMemorySuggestionsReturn {
    const skillsStore = useSkillsStore();
    const tabsStore = useTabsStore();
    const historyStore = useShellHistoryStore();

    const queryText = options?.draftRef ?? ref<string>('');
    const loading = ref<boolean>(false);
    const requestSeq = ref<number>(0);

    let debounceTimer: ReturnType<typeof setTimeout> | null = null;
    let latestSeq = 0;

    function resolveContext(): { projectId: ProjectId | null; cwd: string | null } {
        const currentPaneId = toValue(paneId);
        let projectId = options?.projectId !== undefined ? toValue(options.projectId) : null;
        let cwd = options?.cwd !== undefined ? toValue(options.cwd) : null;

        if (!projectId) {
            projectId = resolvePaneProjectInfo(currentPaneId).projectId;
        }

        if (!cwd) {
            for (const tab of Object.values(tabsStore.byId)) {
                const pane = tab.panes.find((p) => p.id === currentPaneId);
                if (pane) {
                    cwd = (pane.ptyId ? historyStore.cwdOf(pane.ptyId) : null) ?? pane.cwd ?? null;
                    break;
                }
            }
        }

        return { projectId, cwd };
    }

    const suggestions = computed<MemoryEntry_Serialize[]>(() => {
        const currentPaneId = toValue(paneId);
        return skillsStore.suggestionsByPane[currentPaneId] ?? [];
    });

    function cancelPending(): void {
        if (debounceTimer !== null) {
            clearTimeout(debounceTimer);
            debounceTimer = null;
        }
        latestSeq += 1;
        requestSeq.value = latestSeq;
        loading.value = false;
    }

    async function queryNow(overridePrefix?: string): Promise<MemoryEntry_Serialize[]> {
        if (debounceTimer !== null) {
            clearTimeout(debounceTimer);
            debounceTimer = null;
        }

        const currentPaneId = toValue(paneId);
        const { projectId, cwd } = resolveContext();
        if (!projectId) {
            return [];
        }

        latestSeq += 1;
        const seq = latestSeq;
        requestSeq.value = seq;
        loading.value = true;

        const rawPrefix = overridePrefix !== undefined ? overridePrefix : queryText.value;
        const trimmedPrefix = rawPrefix.trim();

        try {
            const entries = await memoryIpc.suggestions({
                projectId,
                prefix: trimmedPrefix.length > 0 ? trimmedPrefix : null,
                limit: SUGGESTIONS_LIMIT,
                includeHidden: false,
                cwd
            });

            if (seq !== latestSeq || toValue(paneId) !== currentPaneId) {
                return [];
            }

            const sliced = entries.slice(0, SUGGESTIONS_LIMIT);
            skillsStore.suggestionsByPane = {
                ...skillsStore.suggestionsByPane,
                [currentPaneId]: sliced
            };
            return sliced;
        } catch {
            return [];
        } finally {
            if (seq === latestSeq) {
                loading.value = false;
            }
        }
    }

    function scheduleQuery(): void {
        if (debounceTimer !== null) {
            clearTimeout(debounceTimer);
        }
        const waitMs = options?.debounceMs ?? DEBOUNCE_MS;
        debounceTimer = setTimeout(() => {
            debounceTimer = null;
            void queryNow();
        }, waitMs);
    }

    const stopWatchQuery = watch(queryText, () => {
        scheduleQuery();
    });

    const stopWatchPane = watch(
        () => toValue(paneId),
        () => {
            cancelPending();
        }
    );

    /**
     * Insere o comando selecionado exclusivamente no draft do `RichInput` (sem executar no PTY
     * e sem modificar o histórico `.history` do painel — `spec.md` §6.3).
     */
    function applySuggestion(entry: MemoryEntry_Serialize): boolean {
        const currentPaneId = toValue(paneId);
        if (options?.draftRef) {
            options.draftRef.value = entry.command;
        }
        return insertCommandIntoRichInput(currentPaneId, entry.command);
    }

    if (getCurrentScope()) {
        onScopeDispose(() => {
            stopWatchQuery();
            stopWatchPane();
            cancelPending();
        });
    }

    return {
        queryText,
        suggestions,
        loading,
        requestSeq,
        queryNow,
        applySuggestion,
        cancelPending
    };
}
