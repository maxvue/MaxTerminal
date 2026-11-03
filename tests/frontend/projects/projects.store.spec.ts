import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { MtIpcError } from '@/ipc/core';
import * as projectsIpc from '@/ipc/projects';
import { useProjectsStore, ZERO_PROJECT_COUNTERS } from '@/stores/projects';
import { useTabStatusStore, type TabBadgeBucket } from '@/stores/tabStatus';
import type {
    Project_Serialize,
    ProjectSnapshot_Serialize,
    TabStateSnapshot_Serialize,
    TabStatus
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
    projectRefreshMissing: vi.fn(),
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

function makeProject(id: string, sortOrder: number, title = `Projeto ${id}`): Project_Serialize {
    return {
        id,
        title,
        icon: 'logos:rust',
        color: '#DEA584',
        path: `/home/user/GitHub/${id}`,
        rootId: null,
        stack: 'rust',
        identitySource: 'auto',
        sortOrder,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
}

function makeTabSnapshot(
    tabId: string,
    status: TabStatus,
    paneCount = 1,
    explicitBucket?: TabBadgeBucket
): TabStateSnapshot_Serialize & { badgeBucket?: TabBadgeBucket } {
    const panes = Array.from({ length: paneCount }, (_, idx) => ({
        paneId: `pane-${idx + 1}`,
        ptyId: `pty-${tabId}-${idx + 1}`,
        sessionId: `sess-${tabId}-${idx + 1}`,
        status,
        integrationMode: 'osc133' as const,
        continuousPolicy: 'auto' as const,
        cwd: '/home/user',
        activeExecution: null,
        lastCompletion: null,
        updatedAt: 1000
    }));
    const snap: TabStateSnapshot_Serialize & { badgeBucket?: TabBadgeBucket } = {
        tabId,
        status,
        panes,
        lastCompletion: null,
        attention: null,
        focused: false,
        revision: 1,
        updatedAt: 1000
    };
    if (explicitBucket) {
        snap.badgeBucket = explicitBucket;
    }
    return snap;
}

describe('useProjectsStore (Fase 06)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.clearAllMocks();
    });

    it('aplica snapshots de forma idempotente, descartando revisões antigas na mesma época e aceitando nova época', () => {
        const store = useProjectsStore();
        const p1 = makeProject('p1', 0);
        const p2 = makeProject('p2', 1);

        const snapRev2: ProjectSnapshot_Serialize = {
            epoch: 'epoch-1',
            revision: 2,
            projects: [p2, p1],
            roots: [],
            discovered: []
        };
        store.applySnapshot(snapRev2);
        expect(store.epoch).toBe('epoch-1');
        expect(store.revision).toBe(2);
        expect(store.projects.map((p) => p.id)).toEqual(['p1', 'p2']);

        // Snapshot atrasado com revisão menor na mesma época é descartado
        const snapStale: ProjectSnapshot_Serialize = {
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1],
            roots: [],
            discovered: []
        };
        store.applySnapshot(snapStale);
        expect(store.revision).toBe(2);
        expect(store.projects.map((p) => p.id)).toEqual(['p1', 'p2']);

        // Nova época aceita revisão 1
        const snapNewEpoch: ProjectSnapshot_Serialize = {
            epoch: 'epoch-2',
            revision: 1,
            projects: [p1],
            roots: [],
            discovered: []
        };
        store.applySnapshot(snapNewEpoch);
        expect(store.epoch).toBe('epoch-2');
        expect(store.revision).toBe(1);
        expect(store.projects.map((p) => p.id)).toEqual(['p1']);
    });

    it('reorder rejeitado mantém a ordem do backend recarregando o snapshot', async () => {
        const store = useProjectsStore();
        const p1 = makeProject('p1', 0);
        const p2 = makeProject('p2', 1);

        store.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1, p2],
            roots: [],
            discovered: []
        });

        vi.mocked(projectsIpc.projectReorder).mockRejectedValueOnce(
            new MtIpcError(
                {
                    code: 'project.reorder_mismatch',
                    message: 'A lista de projetos mudou desde a última leitura.'
                },
                'project_reorder'
            )
        );
        vi.mocked(projectsIpc.projectSnapshot).mockResolvedValueOnce({
            epoch: 'epoch-1',
            revision: 2,
            projects: [p1, p2],
            roots: [],
            discovered: []
        });

        await expect(store.reorder(['p2', 'p1'])).rejects.toThrow();
        expect(projectsIpc.projectSnapshot).toHaveBeenCalledTimes(1);
        expect(store.projects.map((p) => p.id)).toEqual(['p1', 'p2']);
        expect(store.error).toContain('mudou');
    });

    it('removeProject move a seleção para o vizinho imediato', async () => {
        const store = useProjectsStore();
        const p1 = makeProject('p1', 0);
        const p2 = makeProject('p2', 1);
        const p3 = makeProject('p3', 2);

        store.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1, p2, p3],
            roots: [],
            discovered: []
        });
        store.select('p2');
        expect(store.selectedId).toBe('p2');

        vi.mocked(projectsIpc.projectRemove).mockResolvedValueOnce(null);
        vi.mocked(projectsIpc.projectSnapshot).mockResolvedValueOnce({
            epoch: 'epoch-1',
            revision: 2,
            projects: [
                { ...p1, sortOrder: 0 },
                { ...p3, sortOrder: 1 }
            ],
            roots: [],
            discovered: []
        });

        await store.removeProject('p2');
        expect(store.selectedId).toBe('p3');
    });

    it('countersByProject agrega todos os buckets, ignora abas sem dono/snapshot, conta aba com 3 painéis uma única vez e iguala countsFor', () => {
        const projectsStore = useProjectsStore();
        const tabStatusStore = useTabStatusStore();

        const p1 = makeProject('p1', 0);
        const p2 = makeProject('p2', 1);
        projectsStore.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1, p2],
            roots: [],
            discovered: []
        });

        // Abas vinculadas a p1:
        // t1: running com 3 painéis -> deve contar 1 vez
        // t2: continuous
        // t3: completed_unseen -> finished
        // t4: error_unseen -> error
        // t5: idle -> idle
        // t6: vinculada a p1 mas sem snapshot em tabStatusStore -> ignorada
        projectsStore.attachTab('t1', 'p1');
        projectsStore.attachTab('t2', 'p1');
        projectsStore.attachTab('t3', 'p1');
        projectsStore.attachTab('t4', 'p1');
        projectsStore.attachTab('t5', 'p1');
        projectsStore.attachTab('t6', 'p1');

        // Aba sem dono no projectsStore -> ignorada
        tabStatusStore.applySnapshot(makeTabSnapshot('t-orphan', 'running'));

        tabStatusStore.applySnapshot(makeTabSnapshot('t1', 'running', 3));
        tabStatusStore.applySnapshot(makeTabSnapshot('t2', 'continuous', 1));
        tabStatusStore.applySnapshot(makeTabSnapshot('t3', 'completed_unseen', 2));
        tabStatusStore.applySnapshot(makeTabSnapshot('t4', 'error_unseen', 1));
        tabStatusStore.applySnapshot(makeTabSnapshot('t5', 'idle', 1));

        const c1 = projectsStore.countersFor('p1');
        expect(c1).toEqual({
            running: 1,
            continuous: 1,
            finished: 1,
            error: 1,
            idle: 1
        });

        // Invariante: soma dos 5 campos = número de TabIds distintos vinculados com snapshot conhecido (5)
        const sum = c1.running + c1.continuous + c1.finished + c1.error + c1.idle;
        expect(sum).toBe(5);

        // Comparação com o oráculo countsFor da Fase 05
        const oracle = tabStatusStore.countsFor(['t1', 't2', 't3', 't4', 't5', 't6']);
        expect(c1.running).toBe(oracle.running);
        expect(c1.continuous).toBe(oracle.continuous);
        expect(c1.finished).toBe(oracle.completed_unseen);
        expect(c1.error).toBe(oracle.error_unseen);
        expect(c1.idle).toBe(oracle.idle);

        // Projeto sem abas retorna ZERO_PROJECT_COUNTERS congelado
        expect(projectsStore.countersFor('p2')).toBe(ZERO_PROJECT_COUNTERS);

        // Estabilidade referencial: atualizar aba de outro projeto ou órfã mantém a mesma referência de c1
        tabStatusStore.applySnapshot({
            ...makeTabSnapshot('t-orphan', 'error_unseen'),
            revision: 2
        });
        expect(projectsStore.countersFor('p1')).toBe(c1);

        // Bucket fornecido explicitamente pela Fase 05 prevalece sobre status bruto
        tabStatusStore.tabs['t5'] = makeTabSnapshot('t5', 'idle', 1, 'error');
        expect(projectsStore.countersFor('p1').error).toBe(2);
        expect(projectsStore.countersFor('p1').idle).toBe(0);

        // Destacar aba remove imediatamente da contagem
        projectsStore.detachTab('t1');
        expect(projectsStore.countersFor('p1').running).toBe(0);
    });
});
