import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectList from '@/components/projects/ProjectList.vue';
import ProjectWorkspace from '@/components/projects/ProjectWorkspace.vue';
import { useProjectsStore } from '@/stores/projects';
import { useWindowStore } from '@/stores/window';
import type { Project_Serialize, ProjectSnapshot_Serialize } from '@/types/bindings';

const MaxIconStub = {
    props: ['icon'],
    template: '<i class="max-icon-stub" :data-icon="icon"></i>'
};

const MaxIconButtonStub = {
    props: ['icon', 'title', 'ariaLabel'],
    emits: ['click'],
    template: '<button type="button" class="max-icon-button-stub" :title="title" :aria-label="ariaLabel" @click="$emit(\'click\', $event)"></button>'
};

let onOpenPreferencesCallback: ((ev: { revision: number; payload: { section?: string } }) => void) | null = null;

vi.mock('@/ipc/os', () => ({
    uiReady: vi.fn().mockResolvedValue({
        revision: 1,
        window: { visible: true, focused: true, minimized: false },
        environment: { displayServer: 'x11', trayAvailable: true },
        shortcutStatus: null,
        counters: { running: 0, continuous: 0, finishedUnseen: 0, errorUnseen: 0, idle: 0, awaitingInput: 0 },
        shutdownPhase: 'idle',
        linkContexts: [],
        pendingFocus: null
    }),
    onWindowVisibility: vi.fn().mockResolvedValue(vi.fn()),
    onShortcutStatus: vi.fn().mockResolvedValue(vi.fn()),
    onCounters: vi.fn().mockResolvedValue(vi.fn()),
    onOpenPreferences: vi.fn().mockImplementation((cb) => {
        onOpenPreferencesCallback = cb;
        return Promise.resolve(vi.fn());
    }),
    onShutdownStarted: vi.fn().mockResolvedValue(vi.fn()),
    onShutdownFailed: vi.fn().mockResolvedValue(vi.fn()),
    onEnvironmentChanged: vi.fn().mockResolvedValue(vi.fn()),
    onLinkContextChanged: vi.fn().mockResolvedValue(vi.fn()),
    toggleWindow: vi.fn(),
    showWindow: vi.fn(),
    hideWindow: vi.fn(),
    requestQuit: vi.fn(),
    environment: vi.fn()
}));

vi.mock('@/ipc/projects', () => ({
    projectSnapshot: vi.fn(),
    projectAdd: vi.fn(),
    projectUpdate: vi.fn(),
    projectRemove: vi.fn(),
    projectReorder: vi.fn(),
    projectRedetect: vi.fn()
}));

vi.mock('@/ipc/db', () => ({
    getSettings: vi.fn().mockResolvedValue({}),
    setSetting: vi.fn().mockResolvedValue({ ok: true, data: { revision: 1 } }),
    getSettingSnapshot: vi.fn().mockResolvedValue({ revision: 1, values: {} })
}));

function makeProject(id: string, title = `Projeto ${id}`): Project_Serialize {
    return {
        id,
        title,
        icon: 'logos:rust',
        color: '#DEA584',
        path: `/home/user/GitHub/${id}`,
        rootId: null,
        stack: 'rust',
        identitySource: 'auto',
        sortOrder: 0,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
}

describe('ProjectSettingsNavigation', () => {
    let pinia: ReturnType<typeof createPinia>;
    let projectsStore: ReturnType<typeof useProjectsStore>;
    let windowStore: ReturnType<typeof useWindowStore>;

    const projA = makeProject('proj-a', 'Projeto A');
    const projB = makeProject('proj-b', 'Projeto B');

    beforeEach(() => {
        pinia = createPinia();
        setActivePinia(pinia);
        projectsStore = useProjectsStore();
        windowStore = useWindowStore();

        const snap: ProjectSnapshot_Serialize = {
            epoch: 'epoch-1',
            revision: 1,
            projects: [projA, projB],
            roots: [],
            discovered: []
        };
        projectsStore.applySnapshot(snap);
    });

    it('abre configurações do projeto pelo id, seleciona o alvo e fecha preferências gerais', () => {
        windowStore.openPreferences('system');
        expect(windowStore.preferencesOpen).toBe(true);

        windowStore.openProjectSettings('proj-b');
        expect(windowStore.projectSettingsProjectId).toBe('proj-b');
        expect(projectsStore.selectedId).toBe('proj-b');
        expect(windowStore.preferencesOpen).toBe(false);
    });

    it('ignora abertura de configurações para id inexistente', () => {
        projectsStore.select('proj-a');
        windowStore.openProjectSettings('proj-inexistente');

        expect(windowStore.projectSettingsProjectId).toBeNull();
        expect(projectsStore.selectedId).toBe('proj-a');
    });

    it('garante exclusão mútua: abrir preferências gerais fecha configurações do projeto', () => {
        windowStore.openProjectSettings('proj-a');
        expect(windowStore.projectSettingsProjectId).toBe('proj-a');

        windowStore.openPreferences('appearance');
        expect(windowStore.projectSettingsProjectId).toBeNull();
        expect(windowStore.preferencesOpen).toBe(true);
        expect(windowStore.preferencesSection).toBe('appearance');
    });

    it('limpa projectSettingsProjectId no $reset()', () => {
        windowStore.openProjectSettings('proj-a');
        expect(windowStore.projectSettingsProjectId).toBe('proj-a');

        windowStore.$reset();
        expect(windowStore.projectSettingsProjectId).toBeNull();
        expect(windowStore.preferencesOpen).toBe(false);
    });

    it('evento nativo onOpenPreferences fecha configurações do projeto e abre preferências gerais', async () => {
        await windowStore.init();
        expect(onOpenPreferencesCallback).not.toBeNull();

        windowStore.openProjectSettings('proj-a');
        expect(windowStore.projectSettingsProjectId).toBe('proj-a');

        onOpenPreferencesCallback?.({
            revision: 10,
            payload: { section: 'ai' }
        });

        expect(windowStore.projectSettingsProjectId).toBeNull();
        expect(windowStore.preferencesOpen).toBe(true);
        expect(windowStore.preferencesSection).toBe('ai');
    });

    it('ProjectWorkspace oculta botão quando não há projeto selecionado e exibe quando selecionado', async () => {
        const wrapperEmpty = mount(ProjectWorkspace, {
            props: {
                project: null
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxIconButton: MaxIconButtonStub,
                    TabWorkspaceView: true,
                    AppEmptyState: true
                }
            }
        });

        expect(wrapperEmpty.find('.project-workspace__settings-btn').exists()).toBe(false);

        const wrapperActive = mount(ProjectWorkspace, {
            props: {
                project: projA
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxIconButton: MaxIconButtonStub,
                    TabWorkspaceView: true,
                    AppEmptyState: true
                }
            }
        });

        const btn = wrapperActive.find('.project-workspace__settings-btn');
        expect(btn.exists()).toBe(true);
        await btn.trigger('click');

        expect(windowStore.projectSettingsProjectId).toBe('proj-a');
    });

    it('ProjectList aciona configurações do projeto via emit configure e preferências gerais via rodapé', async () => {
        const wrapper = mount(ProjectList, {
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    ProjectDiscoveryPanel: true,
                    ProjectAddModal: true,
                    ProjectRootModal: true,
                    ProjectEditModal: true,
                    ProjectEmptyState: true
                }
            }
        });

        // Rodapé existe e abre preferências gerais
        const footerBtn = wrapper.find('.project-list__settings-btn');
        expect(footerBtn.exists()).toBe(true);
        await footerBtn.trigger('click');
        expect(windowStore.preferencesOpen).toBe(true);
        expect(windowStore.projectSettingsProjectId).toBeNull();

        // Aciona configure em um item
        const itemComponents = wrapper.findAllComponents({ name: 'ProjectItem' });
        expect(itemComponents.length).toBe(2);

        itemComponents[1]?.vm.$emit('configure', 'proj-b');
        expect(windowStore.projectSettingsProjectId).toBe('proj-b');
        expect(projectsStore.selectedId).toBe('proj-b');
        expect(windowStore.preferencesOpen).toBe(false);
    });
});
