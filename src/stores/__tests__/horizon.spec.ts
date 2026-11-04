import { ref } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useHorizonStore } from '@/stores/horizon';
import { useProjectsStore } from '@/stores/projects';
import * as horizonIpc from '@/ipc/horizon';
import type { HorizonSnapshot_Serialize, Project_Serialize } from '@/types/bindings';

vi.mock('@/ipc/horizon', () => ({
    getHorizonSnapshot: vi.fn(),
    startHorizon: vi.fn(),
    stopHorizon: vi.fn(),
    previewClearPending: vi.fn(),
    clearPending: vi.fn()
}));

const mockVisibility = ref('visible');

vi.mock('@maxvue/max-use', () => ({
    useDocumentVisibility: () => mockVisibility
}));

describe('useHorizonStore', () => {
    const sampleProject: Project_Serialize = {
        id: 'proj-1',
        title: 'Meu Laravel App',
        icon: 'lucide:code',
        color: '#ff2d20',
        path: '/var/www/laravel',
        rootId: null,
        sortOrder: 0,
        createdAt: 1700000000000,
        updatedAt: 1700000000000,
        stack: 'laravel',
        identitySource: 'auto',
        missing: false
    };

    const sampleSnapshot: HorizonSnapshot_Serialize = {
        context: {
            projectId: 'proj-1',
            root: '/var/www/laravel',
            contextRevision: 'rev-100',
            scopeId: 'scope-200'
        },
        availability: 'available',
        horizonVersion: '5.48.3',
        environment: 'local',
        queues: [{ connection: 'redis', queue: 'default' }],
        status: 'stopped',
        pending: 20,
        running: 0,
        failed: 1,
        sampledAtMs: 1700000000000,
        revision: 1,
        diagnostic: null,
        canStart: true,
        canStop: false,
        canClearPending: true,
        session: null
    };

    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
        mockVisibility.value = 'visible';
    });

    it('inicializa com valores vazios quando não há projeto selecionado', () => {
        const store = useHorizonStore();
        expect(store.id).toBe('');
        expect(store.enabled).toBe(false);
        expect(store.data).toBeNull();
        expect(store.isAvailable).toBe(false);
    });

    it('carrega snapshot quando um projeto é selecionado no projectsStore', async () => {
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(sampleSnapshot);

        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = sampleProject.id;

        const store = useHorizonStore();
        await vi.waitFor(() => {
            expect(store.id).toBe('proj-1');
            expect(store.data).toEqual(sampleSnapshot);
        });

        expect(store.isAvailable).toBe(true);
        expect(store.canStart).toBe(true);
        expect(store.pendingCount).toBe(20);
        expect(store.failedCount).toBe(1);
    });

    it('executa a ação start() chamando o IPC e atualizando o snapshot', async () => {
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(sampleSnapshot);
        const runningSnap: HorizonSnapshot_Serialize = {
            ...sampleSnapshot,
            status: 'running',
            canStart: false,
            canStop: true,
            session: { ptyId: 'pty-1', paneId: 'pane-1', generation: 1 }
        };
        vi.mocked(horizonIpc.startHorizon).mockResolvedValue(runningSnap);

        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = sampleProject.id;

        const store = useHorizonStore();
        await vi.waitFor(() => {
            expect(store.isAvailable).toBe(true);
        });

        await store.start();

        expect(horizonIpc.startHorizon).toHaveBeenCalledWith(
            'proj-1',
            'rev-100',
            expect.any(String)
        );
        expect(store.status).toBe('running');
        expect(store.isRunning).toBe(true);
        expect(store.session).toEqual(runningSnap.session);
    });

    it('executa a ação stop() chamando o IPC correspondente', async () => {
        const runningSnap: HorizonSnapshot_Serialize = {
            ...sampleSnapshot,
            status: 'running',
            canStart: false,
            canStop: true,
            session: { ptyId: 'pty-1', paneId: 'pane-1', generation: 2 }
        };
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(runningSnap);
        vi.mocked(horizonIpc.stopHorizon).mockResolvedValue({
            ...sampleSnapshot,
            status: 'stopped',
            canStart: true,
            canStop: false,
            session: null
        });

        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = sampleProject.id;

        const store = useHorizonStore();
        await vi.waitFor(() => {
            expect(store.status).toBe('running');
        });

        await store.stop();

        expect(horizonIpc.stopHorizon).toHaveBeenCalledWith('proj-1', 2, expect.any(String));
        expect(store.status).toBe('stopped');
    });

    it('executa previewClear() e confirmClear() com reconciliação', async () => {
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(sampleSnapshot);
        vi.mocked(horizonIpc.previewClearPending).mockResolvedValue({
            token: 'uuid-token-xyz',
            contextRevision: 'rev-100',
            expiresAtMs: 1700000030000,
            scopeId: 'scope-200',
            queues: [{ connection: 'redis', queue: 'default' }],
            estimatedPending: 20
        });
        vi.mocked(horizonIpc.clearPending).mockResolvedValue({
            clearedPerQueue: [{ connection: 'redis', queue: 'default', cleared: 20 }],
            totalCleared: 20,
            isPartial: false,
            warning: null
        });

        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = sampleProject.id;

        const store = useHorizonStore();
        await vi.waitFor(() => {
            expect(store.isAvailable).toBe(true);
        });

        const preview = await store.previewClear();
        expect(preview.token).toBe('uuid-token-xyz');

        const result = await store.confirmClear(preview.token);
        expect(result.totalCleared).toBe(20);
        expect(horizonIpc.clearPending).toHaveBeenCalledWith(
            'proj-1',
            'uuid-token-xyz',
            expect.any(String)
        );
        expect(horizonIpc.getHorizonSnapshot).toHaveBeenCalledTimes(2);
    });
});
