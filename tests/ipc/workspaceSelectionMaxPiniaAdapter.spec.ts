import type { InternalAxiosRequestConfig } from 'axios';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { workspaceSelectionMaxPiniaAdapter } from '@/ipc/workspaceSelectionMaxPiniaAdapter';
import { useSettingsStore } from '@/stores/settings';
import {
    WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
    WORKSPACE_SELECTION_ROUTE,
    type WorkspaceSelection
} from '@/types/workspaceSelection';

vi.mock('@/ipc/db', () => ({
    getSettings: vi.fn().mockResolvedValue({ ok: true, data: {} }),
    setSetting: vi.fn().mockImplementation((key, value, rev) =>
        Promise.resolve({ ok: true, data: { key, value, revision: (rev ?? 0) + 1 } })
    ),
    getSettingSnapshot: vi.fn().mockResolvedValue({ ok: true, data: { revision: 1, value: null } }),
    getStatus: vi.fn().mockResolvedValue({ ok: true, data: { health: { state: 'ready' } } }),
    onSettingsChanged: vi.fn().mockResolvedValue(vi.fn()),
    onDbRecovered: vi.fn().mockResolvedValue(vi.fn())
}));

function makeConfig(overrides: Partial<InternalAxiosRequestConfig> = {}): InternalAxiosRequestConfig {
    return {
        url: WORKSPACE_SELECTION_ROUTE,
        method: 'get',
        headers: {} as never,
        ...overrides
    };
}

describe('workspaceSelectionMaxPiniaAdapter', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        localStorage.clear();
        vi.clearAllMocks();
    });

    describe('GET', () => {
        it('retorna snapshot pendente do storage com prioridade máxima sobre o SQLite', async () => {
            const settingsStore = useSettingsStore();
            await settingsStore.hydrate();
            await settingsStore.set('ui.workspace.selection', {
                version: 1,
                mutationId: 'mut-sqlite',
                selectedProjectId: 'proj-sqlite',
                activeViewByProject: {}
            });

            const pendingSnapshot: WorkspaceSelection = {
                version: 1,
                mutationId: 'mut-pending-crash-recovery',
                selectedProjectId: 'proj-pending',
                activeViewByProject: {}
            };
            localStorage.setItem(
                WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
                JSON.stringify(pendingSnapshot)
            );

            const res = await workspaceSelectionMaxPiniaAdapter(makeConfig({ method: 'get' }));
            expect(res.status).toBe(200);
            expect(res.data).toEqual(pendingSnapshot);
        });

        it('retorna valor persistido no SQLite quando não há snapshot pendente', async () => {
            const settingsStore = useSettingsStore();
            await settingsStore.hydrate();
            const persisted: WorkspaceSelection = {
                version: 1,
                mutationId: 'mut-123',
                selectedProjectId: 'proj-salvo',
                activeViewByProject: {
                    'proj-salvo': { kind: 'terminal', terminalTabId: 'tab-9' }
                }
            };
            await settingsStore.set('ui.workspace.selection', persisted);

            const res = await workspaceSelectionMaxPiniaAdapter(makeConfig({ method: 'get' }));
            expect(res.status).toBe(200);
            expect(res.data).toEqual(persisted);
        });

        it('migra seleção legada projects.selected_id quando não há seleção nova gravada', async () => {
            const settingsStore = useSettingsStore();
            await settingsStore.hydrate();
            await settingsStore.set('projects.selected_id', 'proj-legado-42');

            const res = await workspaceSelectionMaxPiniaAdapter(makeConfig({ method: 'get' }));
            expect(res.status).toBe(200);
            expect(res.data.selectedProjectId).toBe('proj-legado-42');
            expect(res.data.mutationId).toBe('legacy-migration');
        });

        it('retorna default inicial quando o banco está vazio', async () => {
            const settingsStore = useSettingsStore();
            await settingsStore.hydrate();

            const res = await workspaceSelectionMaxPiniaAdapter(makeConfig({ method: 'get' }));
            expect(res.status).toBe(200);
            expect(res.data.selectedProjectId).toBeNull();
            expect(res.data.activeViewByProject).toEqual({});
        });

        it('rejeita com CanceledError se a requisição for abortada', async () => {
            const controller = new AbortController();
            controller.abort();

            await expect(
                workspaceSelectionMaxPiniaAdapter(
                    makeConfig({ method: 'get', signal: controller.signal })
                )
            ).rejects.toThrow();
        });
    });

    describe('POST', () => {
        it('valida e persiste seleção com sucesso, limpando o pendente correspondente', async () => {
            const settingsStore = useSettingsStore();
            await settingsStore.hydrate();

            const payload: WorkspaceSelection = {
                version: 1,
                mutationId: 'mut-confirmada',
                selectedProjectId: 'proj-final',
                activeViewByProject: {}
            };

            // Simula o storage pendente com o mesmo mutationId
            localStorage.setItem(
                WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
                JSON.stringify(payload)
            );

            const res = await workspaceSelectionMaxPiniaAdapter(
                makeConfig({ method: 'post', data: payload })
            );

            expect(res.status).toBe(200);
            expect(res.data).toEqual(payload);
            expect(settingsStore.get('ui.workspace.selection')).toEqual(payload);

            // Storage pendente deve ter sido limpo
            expect(localStorage.getItem(WORKSPACE_SELECTION_PENDING_STORAGE_KEY)).toBeNull();
        });

        it('não remove o snapshot pendente do storage se houver uma mutação mais recente registrada', async () => {
            const settingsStore = useSettingsStore();
            await settingsStore.hydrate();

            const oldPayload: WorkspaceSelection = {
                version: 1,
                mutationId: 'mut-antiga',
                selectedProjectId: 'proj-antigo',
                activeViewByProject: {}
            };

            const newerPending: WorkspaceSelection = {
                version: 1,
                mutationId: 'mut-mais-nova',
                selectedProjectId: 'proj-mais-novo',
                activeViewByProject: {}
            };

            // Storage já possui uma mutação mais recente do usuário
            localStorage.setItem(
                WORKSPACE_SELECTION_PENDING_STORAGE_KEY,
                JSON.stringify(newerPending)
            );

            await workspaceSelectionMaxPiniaAdapter(
                makeConfig({ method: 'post', data: oldPayload })
            );

            // O storage pendente ainda deve conter a mutação mais nova
            const remaining = JSON.parse(
                localStorage.getItem(WORKSPACE_SELECTION_PENDING_STORAGE_KEY)!
            );
            expect(remaining.mutationId).toBe('mut-mais-nova');
        });

        it('rejeita POST com payload com contrato inválido', async () => {
            await expect(
                workspaceSelectionMaxPiniaAdapter(
                    makeConfig({ method: 'post', data: { invalid: true } })
                )
            ).rejects.toThrow('Contrato de WorkspaceSelection inválido');
        });
    });
});
