import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useProjectsStore } from '@/stores/projects';
import { useSettingsStore } from '@/stores/settings';
import type { Project_Serialize, ProjectSnapshot_Serialize, SettingsChanged } from '@/types/bindings';

let onSettingsChangedCallback: ((ev: SettingsChanged) => void) | null = null;

vi.mock('@/ipc/db', () => ({
    getStatus: vi.fn().mockResolvedValue({
        ok: true,
        data: {
            database: { path: '/tmp/test.db', sizeBytes: 1024, mode: 'wal' },
            health: { state: 'ready' },
            writerLag: null,
            pool: { activeConnections: 1, idleConnections: 1, maxConnections: 5 }
        }
    }),
    getSettings: vi.fn().mockResolvedValue({
        ok: true,
        data: {
            'ui.projects.hidden_ids': ['proj-2']
        }
    }),
    setSetting: vi.fn().mockImplementation((key, value, rev) =>
        Promise.resolve({
            ok: true,
            data: { key, value, revision: (rev ?? 0) + 1 }
        })
    ),
    getSettingSnapshot: vi.fn().mockResolvedValue({
        ok: true,
        data: {
            key: 'ui.projects.hidden_ids',
            value: ['proj-2'],
            revision: 1
        }
    }),
    onSettingsChanged: vi.fn().mockImplementation((cb) => {
        onSettingsChangedCallback = cb;
        return Promise.resolve(vi.fn());
    }),
    onDbRecovered: vi.fn().mockResolvedValue(vi.fn())
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

describe('projectVisibility.persistence (T4)', () => {
    let pinia: ReturnType<typeof createPinia>;
    let projectsStore: ReturnType<typeof useProjectsStore>;
    let settingsStore: ReturnType<typeof useSettingsStore>;

    const p1 = makeProject('proj-1', 0, 'Projeto 1');
    const p2 = makeProject('proj-2', 1, 'Projeto 2');
    const p3 = makeProject('proj-3', 2, 'Projeto 3');

    beforeEach(() => {
        pinia = createPinia();
        setActivePinia(pinia);
        projectsStore = useProjectsStore();
        settingsStore = useSettingsStore();

        const snap: ProjectSnapshot_Serialize = {
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1, p2, p3],
            roots: [],
            discovered: []
        };
        projectsStore.applySnapshot(snap);
    });

    it('hidratação inicial recompõe visibilidade a partir das configurações SQLite persistidas', async () => {
        await settingsStore.hydrate();

        expect(settingsStore.get('ui.projects.hidden_ids')).toEqual(['proj-2']);
        expect(projectsStore.hiddenProjectIds).toEqual(['proj-2']);
        expect(projectsStore.visibleProjects.map((p) => p.id)).toEqual(['proj-1', 'proj-3']);
        expect(projectsStore.hiddenProjects.map((p) => p.id)).toEqual(['proj-2']);
    });

    it('evento externo de alteração de configuração atualiza grupos de projetos reativamente', async () => {
        await settingsStore.hydrate();
        expect(onSettingsChangedCallback).not.toBeNull();

        // Dispara evento externo de alteração (ex: outra janela ou processo salvou alteração)
        onSettingsChangedCallback?.({
            key: 'ui.projects.hidden_ids',
            value: ['proj-1', 'proj-3'],
            revision: 5,
            origin: 'ui'
        });

        expect(projectsStore.hiddenProjectIds).toEqual(['proj-1', 'proj-3']);
        expect(projectsStore.visibleProjects.map((p) => p.id)).toEqual(['proj-2']);
        expect(projectsStore.hiddenProjects.map((p) => p.id)).toEqual(['proj-1', 'proj-3']);
    });

    it('escrita com sucesso persiste e atualiza revisão otimista', async () => {
        await settingsStore.hydrate();

        const ok = await projectsStore.setProjectHidden('proj-3', true);
        expect(ok).toBe(true);

        // Deve conter proj-2 e proj-3
        expect(projectsStore.hiddenProjectIds).toContain('proj-2');
        expect(projectsStore.hiddenProjectIds).toContain('proj-3');
        expect(projectsStore.visibleProjects.map((p) => p.id)).toEqual(['proj-1']);
    });
});
