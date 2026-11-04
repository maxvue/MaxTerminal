import { useStorage } from '@vueuse/core';
import { defineStore } from 'pinia';
import { computed, ref } from 'vue';
import type { ProjectId, TabId } from '@/types/bindings';
import {
    createDefaultWorkspaceSelection,
    isWorkspaceSelection,
    WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
    WORKSPACE_SELECTION_ROUTE,
    type ProjectActiveView,
    type WorkspaceSelection,
    type WorkspaceSelectionViewKind
} from '@/types/workspaceSelection';

export interface SetSelectionParams {
    selectedProjectId?: ProjectId | null;
    kind?: WorkspaceSelectionViewKind;
    terminalTabId?: TabId | null;
}

export const useWorkspaceSelectionStore = defineStore('workspaceSelection', () => {
    // Configurações reativas de opt-in para o plugin MaxPinia
    const isCached = ref(true);
    const id = ref('global');
    const enabled = ref(true);
    const default_value = ref<WorkspaceSelection>(createDefaultWorkspaceSelection());
    const data = ref<WorkspaceSelection>(createDefaultWorkspaceSelection());

    const options = {
        get: {
            route: WORKSPACE_SELECTION_ROUTE
        },
        save: WORKSPACE_SELECTION_ROUTE
    };
    const save_return = true;

    // Snapshot pendente com escrita síncrona (flush: 'sync') e serialização explícita via VueUse/MaxUse
    const pendingStorage = useStorage<WorkspaceSelection | null>(
        WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
        null,
        undefined,
        {
            flush: 'sync',
            writeDefaults: false,
            serializer: {
                read: (v: string) => {
                    try {
                        return v ? JSON.parse(v) : null;
                    } catch {
                        return null;
                    }
                },
                write: (v: WorkspaceSelection | null) => (v ? JSON.stringify(v) : '')
            }
        }
    );

    const currentProjectId = computed<ProjectId | null>(() => data.value.selectedProjectId);

    function viewForProject(projectId: ProjectId): ProjectActiveView | null {
        return data.value.activeViewByProject[projectId] ?? null;
    }

    /**
     * Gera um identificador opaco exclusivo para cada mutação lógica do usuário.
     */
    function generateMutationId(): string {
        try {
            return `mut-${crypto.randomUUID()}`;
        } catch {
            return `mut-${Date.now()}-${Math.random().toString(36).slice(2, 9)}`;
        }
    }

    /**
     * Aplica uma alteração de seleção no workspace, gravando imediatamente no storage síncrono
     * e notificando o MaxPinia para persistência durável em segundo plano.
     */
    function setSelection(params: SetSelectionParams): WorkspaceSelection {
        const current = data.value;
        const targetProjectId =
            params.selectedProjectId !== undefined ? params.selectedProjectId : current.selectedProjectId;

        const nextViews = { ...current.activeViewByProject };

        if (targetProjectId) {
            const existingView = nextViews[targetProjectId] ?? {
                kind: 'terminal' as const,
                terminalTabId: null
            };

            const nextKind = params.kind !== undefined ? params.kind : existingView.kind;
            const nextTerminalTabId =
                params.terminalTabId !== undefined
                    ? params.terminalTabId
                    : existingView.terminalTabId;

            nextViews[targetProjectId] = {
                kind: nextKind,
                terminalTabId: nextTerminalTabId
            };
        }

        const nextSelection: WorkspaceSelection = {
            version: 1,
            mutationId: generateMutationId(),
            selectedProjectId: targetProjectId,
            activeViewByProject: nextViews
        };

        // 1. Escrita síncrona de proteção no storage local antes de qualquer microtask ou await
        try {
            if (typeof localStorage !== 'undefined') {
                localStorage.setItem(
                    WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
                    JSON.stringify(nextSelection)
                );
            }
            pendingStorage.value = nextSelection;
        } catch {
            // Se o storage local estiver inacessível, mantém o fluxo em memória
        }

        // 2. Atualiza os dados reativos da store (aciona o debounce de 300ms do MaxPinia)
        data.value = nextSelection;

        return nextSelection;
    }

    /**
     * Hook chamado pelo MaxPinia quando dados do servidor (SQLite) chegam.
     * Protege contra sobrescrita tardia se houver snapshot pendente local mais recente.
     */
    function onServerData(serverData: unknown): void {
        if (!isWorkspaceSelection(serverData)) {
            return;
        }

        const pending = pendingStorage.value;
        if (pending && isWorkspaceSelection(pending) && pending.mutationId !== serverData.mutationId) {
            // Existe uma mutação local mais nova em voo; não sobrescreve
            return;
        }

        data.value = serverData;
    }

    /**
     * Restaura o estado da store para testes unitários.
     */
    function resetForTests(): void {
        try {
            if (typeof localStorage !== 'undefined') {
                localStorage.removeItem(WORKSPACE_SELECTION_PENDING_STORAGE_KEY);
            }
            pendingStorage.value = null;
        } catch {
            // Silencioso em mock
        }
        data.value = createDefaultWorkspaceSelection();
    }

    return {
        // MaxPinia opt-in
        isCached,
        id,
        enabled,
        default_value,
        data,
        options,
        save_return,
        onServerData,
        // State & Getters
        currentProjectId,
        viewForProject,
        // Actions
        setSelection,
        resetForTests
    };
});
