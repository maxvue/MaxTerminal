import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectAddModal from '@/components/projects/ProjectAddModal.vue';
import { MtIpcError } from '@/ipc/core';
import * as projectsIpc from '@/ipc/projects';
import type { Project_Serialize, ProjectIdentity } from '@/types/bindings';

vi.mock('@/ipc/projects', () => ({
    projectSnapshot: vi.fn().mockResolvedValue({
        epoch: 'e1',
        revision: 1,
        projects: [],
        roots: [],
        discovered: []
    }),
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

describe('ProjectAddModal.vue (Fase 06)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.useFakeTimers();
        vi.clearAllMocks();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('aplica debounce de 250ms no preview, descarta respostas fora de ordem, trata cancelamento do seletor e preserva edição manual', async () => {
        let resolveFirstPreview!: (id: ProjectIdentity) => void;
        let resolveSecondPreview!: (id: ProjectIdentity) => void;

        vi.mocked(projectsIpc.projectPreviewIdentity)
            .mockImplementationOnce(
                () =>
                    new Promise((resolve) => {
                        resolveFirstPreview = resolve;
                    })
            )
            .mockImplementationOnce(
                () =>
                    new Promise((resolve) => {
                        resolveSecondPreview = resolve;
                    })
            );

        const wrapper = mount(ProjectAddModal, {
            props: { visible: true },
            global: {
                stubs: { MaxIcon: true, teleport: true }
            }
        });

        // 1. Cancelar o seletor de pasta (retorna null) não dispara preview nem altera path
        vi.mocked(projectsIpc.pickFolder).mockResolvedValueOnce(null);
        await wrapper.find('[data-testid="pick-folder-btn"]').trigger('click');
        await flushPromises();
        expect(projectsIpc.projectPreviewIdentity).not.toHaveBeenCalled();

        // 2. Digita primeiro caminho -> aguarda 250ms -> dispara 1º preview
        const pathInput = wrapper.find('#project-add-path');
        await pathInput.setValue('/home/user/proj-antigo');
        await vi.advanceTimersByTimeAsync(250);
        expect(projectsIpc.projectPreviewIdentity).toHaveBeenCalledTimes(1);
        expect(projectsIpc.projectPreviewIdentity).toHaveBeenLastCalledWith('/home/user/proj-antigo');

        // 3. Digita segundo caminho antes do 1º retornar -> aguarda 250ms -> dispara 2º preview
        await pathInput.setValue('/home/user/proj-novo');
        await vi.advanceTimersByTimeAsync(250);
        expect(projectsIpc.projectPreviewIdentity).toHaveBeenCalledTimes(2);
        expect(projectsIpc.projectPreviewIdentity).toHaveBeenLastCalledWith('/home/user/proj-novo');

        // 4. Resolve o 2º primeiro e depois o 1º (fora de ordem): o 1º deve ser descartado!
        resolveSecondPreview({
            icon: 'logos:tauri',
            color: '#24C8DB',
            stack: 'tauri',
            detected: ['tauri']
        });
        await flushPromises();

        expect((wrapper.find('#project-add-icon').element as HTMLInputElement).value).toBe('logos:tauri');
        expect((wrapper.find('#project-add-color').element as HTMLInputElement).value).toBe('#24C8DB');
        expect((wrapper.find('#project-add-title').element as HTMLInputElement).value).toBe('proj-novo');

        resolveFirstPreview({
            icon: 'logos:python',
            color: '#3776AB',
            stack: 'python',
            detected: ['python']
        });
        await flushPromises();

        // Continua com a identidade do 2º (logos:tauri)
        expect((wrapper.find('#project-add-icon').element as HTMLInputElement).value).toBe('logos:tauri');

        // 5. Usuário customiza manualmente o ícone -> novo preview não deve sobrescrever o ícone manual
        const iconField = wrapper.find('#project-add-icon');
        await iconField.setValue('mdi:rocket-launch-outline');

        vi.mocked(projectsIpc.projectPreviewIdentity).mockResolvedValueOnce({
            icon: 'logos:laravel',
            color: '#FF2D20',
            stack: 'laravel',
            detected: ['laravel']
        });
        await pathInput.setValue('/home/user/proj-laravel');
        await vi.advanceTimersByTimeAsync(250);
        await flushPromises();

        // Ícone manual preservado; cor (não editada manualmente) atualizada para a stack detectada
        expect((wrapper.find('#project-add-icon').element as HTMLInputElement).value).toBe(
            'mdi:rocket-launch-outline'
        );
        expect((wrapper.find('#project-add-color').element as HTMLInputElement).value).toBe('#FF2D20');
    });

    it('exibe erro project.duplicate_path inline e impede submissão duplicada por double-click', async () => {
        const wrapper = mount(ProjectAddModal, {
            props: { visible: true },
            global: {
                stubs: { MaxIcon: true, teleport: true }
            }
        });

        await wrapper.find('#project-add-path').setValue('/home/user/duplicado');
        await wrapper.find('#project-add-title').setValue('Duplicado');

        // 1. Simula erro project.duplicate_path
        vi.mocked(projectsIpc.projectAdd).mockRejectedValueOnce(
            new MtIpcError(
                {
                    code: 'project.duplicate_path',
                    message: 'Esta pasta já está cadastrada como projeto no MaxTerminal.'
                },
                'project_add'
            )
        );

        await wrapper.find('form').trigger('submit.prevent');
        await flushPromises();

        const alert = wrapper.find('[role="alert"]');
        expect(alert.exists()).toBe(true);
        expect(alert.text()).toContain('já está cadastrada');

        // 2. Proteção contra double-click enquanto a chamada está em andamento
        let resolveAdd!: (p: Project_Serialize) => void;
        vi.mocked(projectsIpc.projectAdd).mockImplementationOnce(
            () =>
                new Promise((resolve) => {
                    resolveAdd = resolve;
                })
        );

        void wrapper.find('form').trigger('submit.prevent');
        void wrapper.find('form').trigger('submit.prevent');
        expect(projectsIpc.projectAdd).toHaveBeenCalledTimes(2); // 1 da falha anterior + 1 agora

        resolveAdd({
            id: 'proj-new',
            title: 'Duplicado',
            icon: 'mdi:folder-code-outline',
            color: '#14B8A6',
            path: '/home/user/duplicado',
            rootId: null,
            stack: 'unknown',
            identitySource: 'auto',
            sortOrder: 0,
            missing: false,
            createdAt: 1000,
            updatedAt: 1000
        });
        await flushPromises();

        expect(wrapper.emitted('created')).toHaveLength(1);
        expect(wrapper.emitted('update:visible')?.[0]).toEqual([false]);
    });
});
