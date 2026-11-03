import type { UnlistenFn } from '@tauri-apps/api/event';
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useProjectEvents } from '@/composables/useProjectEvents';
import * as projectsIpc from '@/ipc/projects';
import { useProjectsStore } from '@/stores/projects';
import type {
    Project_Serialize,
    ProjectChangedPayload_Serialize,
    ProjectEvent,
    ProjectSnapshot_Serialize
} from '@/types/bindings';

vi.mock('@/ipc/projects', () => ({
    projectSnapshot: vi.fn(),
    projectList: vi.fn(),
    projectAdd: vi.fn(),
    projectUpdate: vi.fn(),
    projectRemove: vi.fn(),
    projectReorder: vi.fn(),
    projectRedetect: vi.fn(),
    projectPreviewIdentity: vi.fn(),
    projectRefreshMissing: vi.fn().mockResolvedValue([]),
    projectRootList: vi.fn(),
    projectRootAdd: vi.fn(),
    projectRootUpdate: vi.fn(),
    projectRootRemove: vi.fn(),
    projectRootRescan: vi.fn(),
    projectDiscoveryList: vi.fn(),
    projectDiscoveryAccept: vi.fn(),
    projectDiscoveryDismiss: vi.fn(),
    onProjectDiscovered: vi.fn(),
    onProjectRemoved: vi.fn(),
    onProjectChanged: vi.fn(),
    onProjectRootChanged: vi.fn(),
    pickFolder: vi.fn()
}));

function makeProject(id: string, sortOrder: number): Project_Serialize {
    return {
        id,
        title: `Projeto ${id}`,
        icon: 'logos:rust',
        color: '#DEA584',
        path: `/home/user/${id}`,
        rootId: null,
        stack: 'rust',
        identitySource: 'auto',
        sortOrder,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
}

describe('useProjectEvents (Fase 06)', () => {
    let changedCb: ((ev: ProjectEvent<ProjectChangedPayload_Serialize>) => void) | null = null;
    let unlistenSpies: ReturnType<typeof vi.fn>[];

    beforeEach(() => {
        setActivePinia(createPinia());
        vi.useFakeTimers();
        vi.clearAllMocks();
        changedCb = null;
        unlistenSpies = [];

        const makeListenerMock = (
            capture?: (cb: (ev: ProjectEvent<ProjectChangedPayload_Serialize>) => void) => void
        ) => {
            return vi.fn().mockImplementation(async (cb) => {
                capture?.(cb);
                const spy = vi.fn();
                unlistenSpies.push(spy);
                return spy as unknown as UnlistenFn;
            });
        };

        vi.mocked(projectsIpc.onProjectDiscovered).mockImplementation(makeListenerMock());
        vi.mocked(projectsIpc.onProjectRemoved).mockImplementation(makeListenerMock());
        vi.mocked(projectsIpc.onProjectChanged).mockImplementation(
            makeListenerMock((cb) => {
                changedCb = cb;
            })
        );
        vi.mocked(projectsIpc.onProjectRootChanged).mockImplementation(makeListenerMock());
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('coalesceia eventos durante snapshot, trata gaps/mudança de época e executa polling de recuperação', async () => {
        const p1 = makeProject('p1', 0);
        const p2 = makeProject('p2', 1);

        let resolveFirstSnap!: (s: ProjectSnapshot_Serialize) => void;
        vi.mocked(projectsIpc.projectSnapshot)
            .mockImplementationOnce(
                () =>
                    new Promise((resolve) => {
                        resolveFirstSnap = resolve;
                    })
            )
            .mockResolvedValueOnce({
                epoch: 'epoch-1',
                revision: 5,
                projects: [p1, p2],
                roots: [],
                discovered: []
            })
            .mockResolvedValueOnce({
                epoch: 'epoch-2',
                revision: 1,
                projects: [p2],
                roots: [],
                discovered: []
            });

        const store = useProjectsStore();
        const events = useProjectEvents({ coalesceMs: 50, pollIntervalMs: 30_000 });
        const startPromise = events.start();

        // Aguarda registro dos 4 listeners antes do término do primeiro snapshot
        await Promise.resolve();
        await Promise.resolve();
        expect(changedCb).toBeTypeOf('function');

        // Dispara evento com revisão 5 enquanto o primeiro snapshot (rev 2) ainda está em voo
        changedCb?.({
            epoch: 'epoch-1',
            revision: 5,
            payload: { kind: 'upserted', projects: [p2] }
        });
        await vi.advanceTimersByTimeAsync(50);

        // Conclui o primeiro snapshot (rev 2 < targetRevision 5) -> deve repetir automaticamente até alcançar rev 5
        resolveFirstSnap({
            epoch: 'epoch-1',
            revision: 2,
            projects: [p1],
            roots: [],
            discovered: []
        });

        await startPromise;
        expect(store.revision).toBe(5);
        expect(store.projects.map((p) => p.id)).toEqual(['p1', 'p2']);

        // Polling de recuperação após 30 s traz nova época (epoch-2, rev 1)
        await vi.advanceTimersByTimeAsync(30_000);
        expect(store.epoch).toBe('epoch-2');
        expect(store.revision).toBe(1);
        expect(store.projects.map((p) => p.id)).toEqual(['p2']);

        events.stop();
        expect(unlistenSpies).toHaveLength(4);
        for (const spy of unlistenSpies) {
            expect(spy).toHaveBeenCalledTimes(1);
        }
    });

    it('libera listeners já criados em caso de falha parcial no registro', async () => {
        const unlisten1 = vi.fn();
        const unlisten2 = vi.fn();

        vi.mocked(projectsIpc.onProjectDiscovered).mockResolvedValueOnce(unlisten1);
        vi.mocked(projectsIpc.onProjectRemoved).mockResolvedValueOnce(unlisten2);
        vi.mocked(projectsIpc.onProjectChanged).mockRejectedValueOnce(new Error('Falha ACL'));
        vi.mocked(projectsIpc.onProjectRootChanged).mockResolvedValueOnce(vi.fn());

        const events = useProjectEvents();
        await expect(events.start()).rejects.toThrow('Falha ACL');

        expect(unlisten1).toHaveBeenCalledTimes(1);
        expect(unlisten2).toHaveBeenCalledTimes(1);
    });

    it('executa unlisten imediatamente se o componente desmontar durante o registro assíncrono', async () => {
        let resolveSlowListener!: (fn: UnlistenFn) => void;
        const lateUnlisten = vi.fn();

        vi.mocked(projectsIpc.onProjectDiscovered).mockImplementationOnce(
            () =>
                new Promise((resolve) => {
                    resolveSlowListener = resolve;
                })
        );

        const events = useProjectEvents();
        const p = events.start();

        // Desmonta antes de resolver o listener lento
        events.stop();

        resolveSlowListener(lateUnlisten);
        await p;

        expect(lateUnlisten).toHaveBeenCalledTimes(1);
    });
});
