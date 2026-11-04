import { createApp, ref } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import { beforeEach, afterEach, describe, expect, it, vi } from 'vitest';
import { useHorizonStore } from '@/stores/horizon';
import { useProjectsStore } from '@/stores/projects';
import { createHorizonAxiosInstance } from '@/ipc/horizonMaxPiniaAdapter';
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

// Harness isolado de LocalForage em memória conforme exigido em T1
vi.mock('localforage', () => {
    const memory = new Map<string, unknown>();
    const instance = {
        getItem: vi.fn(async (key: string) => memory.get(key) ?? null),
        setItem: vi.fn(async (key: string, val: unknown) => {
            memory.set(key, val);
            return val;
        }),
        removeItem: vi.fn(async (key: string) => {
            memory.delete(key);
        }),
        clear: vi.fn(async () => {
            memory.clear();
        }),
        keys: vi.fn(async () => Array.from(memory.keys())),
        config: vi.fn()
    };
    return {
        default: {
            createInstance: () => instance,
            config: () => {},
            getItem: instance.getItem,
            setItem: instance.setItem,
            removeItem: instance.removeItem,
            clear: instance.clear,
            keys: instance.keys
        }
    };
});

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
        const app = createApp({});
        const pinia = createPinia();
        pinia.use(
            createMaxPinia({
                axios: createHorizonAxiosInstance() as never
            }) as never
        );
        app.use(pinia);
        setActivePinia(pinia);
        vi.clearAllMocks();
        mockVisibility.value = 'visible';
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('inicializa com valores vazios quando não há projeto selecionado', () => {
        const store = useHorizonStore();
        expect(store.id).toBe('');
        expect(store.enabled).toBe(false);
        expect(store.data?.context).toBeUndefined();
        expect(store.isAvailable).toBe(false);
        expect(store.runtimeStatus).toBe('stopped');
        // store.status pertence exclusivamente ao MaxPinia (injetado via plugin em runtime)
        const storeWithPlugin = store as unknown as { status?: { reset?: () => void } };
        expect(storeWithPlugin.status).toBeDefined();
        expect(typeof storeWithPlugin.status?.reset).toBe('function');
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
        expect(store.runtimeStatus).toBe('stopped');
    });

    it('descarta snapshot recebido cujo projectId difira do projeto ativo no onServerData', () => {
        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = 'proj-1';
        const store = useHorizonStore();

        // Tenta injetar snapshot de outro projeto
        const foreignSnapshot: HorizonSnapshot_Serialize = {
            ...sampleSnapshot,
            context: {
                ...sampleSnapshot.context,
                projectId: 'proj-other'
            }
        };

        store.onServerData(foreignSnapshot);
        expect(store.data?.context?.projectId).not.toBe('proj-other');
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
            expect(store.canStart).toBe(true);
        });

        await store.start();

        expect(horizonIpc.startHorizon).toHaveBeenCalledWith(
            'proj-1',
            'rev-100',
            expect.any(String)
        );
        expect(store.runtimeStatus).toBe('running');
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
            expect(store.runtimeStatus).toBe('running');
            expect(store.canStop).toBe(true);
        });

        await store.stop();

        expect(horizonIpc.stopHorizon).toHaveBeenCalledWith('proj-1', 2, expect.any(String));
        expect(store.runtimeStatus).toBe('stopped');
    });

    it('executa previewClear() e confirmClear() com validação estrita de token e TTL', async () => {
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(sampleSnapshot);
        const expiresAtMs = Date.now() + 30000;
        vi.mocked(horizonIpc.previewClearPending).mockResolvedValue({
            token: 'uuid-token-xyz',
            contextRevision: 'rev-100',
            expiresAtMs,
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

        // Tentativa com token divergente rejeita
        await expect(store.confirmClear('wrong-token')).rejects.toThrow(
            'Prévia de confirmação de limpeza ausente ou token divergente.'
        );

        // Confirmação válida
        const result = await store.confirmClear(preview.token);
        expect(result.totalCleared).toBe(20);
        expect(horizonIpc.clearPending).toHaveBeenCalledWith(
            'proj-1',
            'uuid-token-xyz',
            expect.any(String)
        );
    });

    it('rejeita confirmClear quando a prévia estiver expirada', async () => {
        vi.mocked(horizonIpc.getHorizonSnapshot).mockResolvedValue(sampleSnapshot);
        // Token já expirado no passado
        vi.mocked(horizonIpc.previewClearPending).mockResolvedValue({
            token: 'expired-token',
            contextRevision: 'rev-100',
            expiresAtMs: Date.now() - 1000,
            scopeId: 'scope-200',
            queues: [{ connection: 'redis', queue: 'default' }],
            estimatedPending: 5
        });

        const projectsStore = useProjectsStore();
        projectsStore.projects = [sampleProject];
        projectsStore.selectedId = sampleProject.id;

        const store = useHorizonStore();
        await vi.waitFor(() => {
            expect(store.isAvailable).toBe(true);
        });

        const preview = await store.previewClear();
        await expect(store.confirmClear(preview.token)).rejects.toThrow(
            'O tempo para confirmação da limpeza expirou (máximo de 30 segundos).'
        );
        expect(horizonIpc.clearPending).not.toHaveBeenCalled();
    });
});
