import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { useWorkspaceSelection } from '@/composables/workspace/useWorkspaceSelection';
import { useGithubIssuesStore } from '@/stores/githubIssues';
import { useProjectsStore } from '@/stores/projects';
import { useSettingsStore } from '@/stores/settings';
import { useTabsStore } from '@/stores/tabs';
import { useWorkspaceSelectionStore } from '@/stores/workspaceSelection';
import type { Project_Serialize, TabView_Serialize } from '@/types/bindings';
import type { WorkspaceSelection } from '@/types/workspaceSelection';

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

vi.mock('@/ipc/github', () => ({
    githubIpc: {
        repoResolve: vi.fn().mockResolvedValue({ kind: 'git_hub', owner: 'org', name: 'repo' }),
        issueCounts: vi.fn().mockResolvedValue({ openCount: 3, totalCount: 10 }),
        issuesList: vi.fn().mockResolvedValue({ items: [], totalMatching: 0, openIssuesCount: 0 })
    }
}));

function makeProject(id: string, sortOrder: number): Project_Serialize {
    return {
        id,
        title: `Projeto ${id}`,
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

function makeTabView(id: string, projectId: string, position: number): TabView_Serialize {
    return {
        id,
        projectId,
        templateId: 'tpl-1',
        title: `Terminal ${id}`,
        icon: 'lucide:terminal',
        position,
        origin: 'manual',
        continuous: false,
        lifecycle: 'ready',
        revision: 1,
        layoutRevision: 1,
        layout: { kind: 'leaf', paneId: `pane-${id}` },
        panes: [],
        activePaneId: `pane-${id}`,
        primaryPaneId: `pane-${id}`,
        initialDispatch: 'none',
        tabSessionId: `sess-${id}`
    };
}

describe('useWorkspaceSelection', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        localStorage.clear();
        vi.clearAllMocks();
    });

    it('restaura projeto e aba terminal anteriormente visíveis', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        const settingsStore = useSettingsStore();
        const selectionStore = useWorkspaceSelectionStore();
        await settingsStore.hydrate();

        const p1 = makeProject('proj-1', 0);
        const p2 = makeProject('proj-2', 1);
        projectsStore.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1, p2],
            roots: [],
            discovered: []
        });

        const t1 = makeTabView('tab-1', 'proj-2', 0);
        const t2 = makeTabView('tab-2', 'proj-2', 1);
        tabsStore.hydrateFromSnapshot({
            bootId: 'boot-1',
            eventSeq: 1,
            restored: true,
            restoreReport: null,
            tabs: [t1, t2]
        });

        const savedSelection: WorkspaceSelection = {
            version: 1,
            mutationId: 'mut-saved',
            selectedProjectId: 'proj-2',
            activeViewByProject: {
                'proj-2': {
                    kind: 'terminal',
                    terminalTabId: 'tab-2'
                }
            }
        };
        selectionStore.data = savedSelection;

        const { restoreSelection, isRestored } = useWorkspaceSelection();
        await restoreSelection();

        expect(isRestored.value).toBe(true);
        expect(projectsStore.selectedId).toBe('proj-2');
        expect(projectsStore.activeTabByProject['proj-2']).toBe('tab-2');
    });

    it('restaura aba Issues quando era a visão ativa e repositório é GitHub', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        const githubIssuesStore = useGithubIssuesStore();
        const selectionStore = useWorkspaceSelectionStore();

        const p1 = makeProject('proj-gh', 0);
        projectsStore.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1],
            roots: [],
            discovered: []
        });

        const t1 = makeTabView('tab-gh-1', 'proj-gh', 0);
        tabsStore.hydrateFromSnapshot({
            bootId: 'boot-1',
            eventSeq: 1,
            restored: true,
            restoreReport: null,
            tabs: [t1]
        });

        selectionStore.data = {
            version: 1,
            mutationId: 'mut-issues',
            selectedProjectId: 'proj-gh',
            activeViewByProject: {
                'proj-gh': {
                    kind: 'issues',
                    terminalTabId: 'tab-gh-1'
                }
            }
        };

        const { restoreSelection } = useWorkspaceSelection();
        await restoreSelection();

        expect(projectsStore.selectedId).toBe('proj-gh');
        expect(githubIssuesStore.isPinnedTabActive('proj-gh')).toBe(true);
    });

    it('faz fallback para primeira aba caso a aba salva tenha sido excluída', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        const selectionStore = useWorkspaceSelectionStore();

        const p1 = makeProject('proj-1', 0);
        projectsStore.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1],
            roots: [],
            discovered: []
        });

        const t1 = makeTabView('tab-existente', 'proj-1', 0);
        tabsStore.hydrateFromSnapshot({
            bootId: 'boot-1',
            eventSeq: 1,
            restored: true,
            restoreReport: null,
            tabs: [t1]
        });

        selectionStore.data = {
            version: 1,
            mutationId: 'mut-missing-tab',
            selectedProjectId: 'proj-1',
            activeViewByProject: {
                'proj-1': {
                    kind: 'terminal',
                    terminalTabId: 'tab-removida'
                }
            }
        };

        const { restoreSelection } = useWorkspaceSelection();
        await restoreSelection();

        expect(projectsStore.selectedId).toBe('proj-1');
        expect(projectsStore.activeTabByProject['proj-1']).toBe('tab-existente');
    });

    it('mantém seleção nula quando o projeto salvo foi excluído do catálogo', async () => {
        const projectsStore = useProjectsStore();
        const selectionStore = useWorkspaceSelectionStore();

        const p1 = makeProject('proj-valido', 0);
        projectsStore.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1],
            roots: [],
            discovered: []
        });

        selectionStore.data = {
            version: 1,
            mutationId: 'mut-missing-proj',
            selectedProjectId: 'proj-excluido',
            activeViewByProject: {}
        };

        const { restoreSelection } = useWorkspaceSelection();
        await restoreSelection();

        expect(projectsStore.selectedId).toBeNull();
    });

    it('preserva a escolha do usuário se houver interação explícita antes da restauração', async () => {
        const projectsStore = useProjectsStore();
        const tabsStore = useTabsStore();
        const selectionStore = useWorkspaceSelectionStore();

        const p1 = makeProject('proj-1', 0);
        const p2 = makeProject('proj-2', 1);
        projectsStore.applySnapshot({
            epoch: 'epoch-1',
            revision: 1,
            projects: [p1, p2],
            roots: [],
            discovered: []
        });

        const t1 = makeTabView('tab-1', 'proj-1', 0);
        const t2 = makeTabView('tab-2', 'proj-2', 0);
        tabsStore.hydrateFromSnapshot({
            bootId: 'boot-1',
            eventSeq: 1,
            restored: true,
            restoreReport: null,
            tabs: [t1, t2]
        });

        selectionStore.data = {
            version: 1,
            mutationId: 'mut-hist',
            selectedProjectId: 'proj-2',
            activeViewByProject: {}
        };

        const { restoreSelection } = useWorkspaceSelection();

        // Usuário clica em proj-1 antes de restoreSelection finalizar
        projectsStore.select('proj-1');

        await restoreSelection();

        // Não deve ser sobrescrito pelo histórico proj-2
        expect(projectsStore.selectedId).toBe('proj-1');
    });
});
