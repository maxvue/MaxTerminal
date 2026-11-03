import { effectScope, nextTick } from 'vue';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import type { BridgeProjectSyncedEvent, Project_Serialize } from '@/types/bindings';

let syncedCallback: ((ev: BridgeProjectSyncedEvent) => void) | null = null;
const mockNotifyProjectSelected = vi.fn().mockResolvedValue(true);

vi.mock('@/ipc/bridge', () => ({
    bridgeIpc: {
        status: vi.fn(),
        getSettings: vi.fn(),
        updateSettings: vi.fn(),
        notifyProjectSelected: (...args: unknown[]) => mockNotifyProjectSelected(...args),
        issueProtocolSupport: vi.fn().mockResolvedValue({
            issuesProtocolV1: false,
            bugsCycleV1: false
        })
    },
    onBridgeProjectSynced: vi.fn((cb: (ev: BridgeProjectSyncedEvent) => void) => {
        syncedCallback = cb;
        return Promise.resolve(() => {
            syncedCallback = null;
        });
    }),
    onBridgeStatusChanged: vi.fn(() => Promise.resolve(() => {})),
    onBridgeTerminalOpened: vi.fn(() => Promise.resolve(() => {})),
    onBridgeBugCycleProgress: vi.fn(() => Promise.resolve(() => {})),
    onBridgeError: vi.fn(() => Promise.resolve(() => {}))
}));

import { useActiveProjectSync } from '@/composables/useActiveProjectSync';
import { useMaxCodeBridgeStore } from '@/stores/maxCodeBridge';
import { useProjectsStore } from '@/stores/projects';

function makeProject(id: string, path: string, title: string): Project_Serialize {
    return {
        id,
        title,
        path,
        icon: 'logos:rust',
        color: '#22c55e',
        identitySource: 'auto',
        stack: 'rust',
        sortOrder: 0,
        rootId: null,
        missing: false,
        createdAt: 1700000000000,
        updatedAt: 1700000000000
    };
}

describe('useActiveProjectSync', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        syncedCallback = null;
    });

    it('envia notifyProjectSelected ao selecionar manualmente um projeto e suprime eco em seleção remota', async () => {
        const projectsStore = useProjectsStore();
        const bridgeStore = useMaxCodeBridgeStore();

        const p1 = makeProject('p-1', '/home/user/p1', 'Projeto 1');
        const p2 = makeProject('p-2', '/home/user/p2', 'Projeto 2');
        projectsStore.projects = [p1, p2];
        bridgeStore.settings.enabled = true;
        bridgeStore.settings.autoSyncActiveProject = true;

        const scope = effectScope();
        scope.run(() => {
            useActiveProjectSync();
        });
        await nextTick();

        // Seleção manual local -> notifica MaxCode
        projectsStore.select('p-1');
        await nextTick();
        expect(mockNotifyProjectSelected).toHaveBeenCalledTimes(1);
        expect(mockNotifyProjectSelected).toHaveBeenCalledWith('/home/user/p1', 'Projeto 1', null);

        mockNotifyProjectSelected.mockClear();

        // Seleção remota vinda do MaxCode -> seleciona p-2 sem ecoar notifyProjectSelected
        syncedCallback?.({
            projectId: 'p-2',
            projectPath: '/home/user/p2',
            created: false
        });
        await nextTick();

        expect(projectsStore.selectedId).toBe('p-2');
        expect(mockNotifyProjectSelected).not.toHaveBeenCalled();

        // Desativar autoSyncActiveProject interrompe envios e recebimentos imediatamente (CA-27)
        bridgeStore.settings.autoSyncActiveProject = false;
        projectsStore.select('p-1');
        await nextTick();
        expect(mockNotifyProjectSelected).not.toHaveBeenCalled();

        syncedCallback?.({
            projectId: 'p-2',
            projectPath: '/home/user/p2',
            created: false
        });
        await nextTick();
        expect(projectsStore.selectedId).toBe('p-1');

        scope.stop();
    });
});
