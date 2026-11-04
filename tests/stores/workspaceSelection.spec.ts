import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import { useWorkspaceSelectionStore } from '@/stores/workspaceSelection';
import {
    WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
    type WorkspaceSelection
} from '@/types/workspaceSelection';

describe('useWorkspaceSelectionStore', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        localStorage.clear();
    });

    it('inicializa com valores padrões vazios', () => {
        const store = useWorkspaceSelectionStore();
        expect(store.currentProjectId).toBeNull();
        expect(store.data.selectedProjectId).toBeNull();
        expect(store.data.activeViewByProject).toEqual({});
        expect(store.data.version).toBe(1);
    });

    it('setSelection atualiza o projeto selecionado e grava imediatamente no storage síncrono', () => {
        const store = useWorkspaceSelectionStore();

        const result = store.setSelection({ selectedProjectId: 'proj-1' });

        expect(result.selectedProjectId).toBe('proj-1');
        expect(store.currentProjectId).toBe('proj-1');
        expect(store.data.selectedProjectId).toBe('proj-1');

        // Validação da captura síncrona imediata no localStorage
        const rawPending = localStorage.getItem(WORKSPACE_SELECTION_PENDING_STORAGE_KEY);
        expect(rawPending).not.toBeNull();
        const pending = JSON.parse(rawPending!) as WorkspaceSelection;
        expect(pending.selectedProjectId).toBe('proj-1');
        expect(pending.mutationId).toBe(result.mutationId);
    });

    it('setSelection atualiza a aba terminal e a visão de issues do projeto preservando dados de retorno', () => {
        const store = useWorkspaceSelectionStore();

        // 1. Seleciona projeto e aba terminal
        store.setSelection({ selectedProjectId: 'proj-1', kind: 'terminal', terminalTabId: 'tab-100' });
        const view1 = store.viewForProject('proj-1');
        expect(view1).toEqual({ kind: 'terminal', terminalTabId: 'tab-100' });

        // 2. Muda para aba issues preservando o terminalTabId de retorno
        store.setSelection({ kind: 'issues' });
        const view2 = store.viewForProject('proj-1');
        expect(view2).toEqual({ kind: 'issues', terminalTabId: 'tab-100' });

        // 3. Volta para terminal
        store.setSelection({ kind: 'terminal' });
        const view3 = store.viewForProject('proj-1');
        expect(view3).toEqual({ kind: 'terminal', terminalTabId: 'tab-100' });
    });

    it('onServerData descarta resposta atrasada quando há mutação local pendente mais recente', () => {
        const store = useWorkspaceSelectionStore();

        // Usuário faz uma mutação local recente
        const recent = store.setSelection({ selectedProjectId: 'proj-novo' });

        // Chega do servidor um dado atrasado com mutationId diferente
        const staleServerData: WorkspaceSelection = {
            version: 1,
            mutationId: 'mut-antiga',
            selectedProjectId: 'proj-antigo',
            activeViewByProject: {}
        };

        store.onServerData(staleServerData);

        // Deve manter a seleção recente
        expect(store.currentProjectId).toBe('proj-novo');
        expect(store.data.mutationId).toBe(recent.mutationId);
    });

    it('onServerData aceita dados do servidor quando não há mutação conflitante', () => {
        const store = useWorkspaceSelectionStore();
        localStorage.clear();

        const serverData: WorkspaceSelection = {
            version: 1,
            mutationId: 'mut-sync-1',
            selectedProjectId: 'proj-restaurado',
            activeViewByProject: {
                'proj-restaurado': {
                    kind: 'terminal',
                    terminalTabId: 'tab-55'
                }
            }
        };

        store.onServerData(serverData);

        expect(store.currentProjectId).toBe('proj-restaurado');
        expect(store.viewForProject('proj-restaurado')?.terminalTabId).toBe('tab-55');
    });
});
