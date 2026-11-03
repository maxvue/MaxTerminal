import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import ProjectIdentityForm from '@/components/projects/ProjectIdentityForm.vue';
import * as projectsIpc from '@/ipc/projects';
import type { Project_Serialize } from '@/types/bindings';

const MaxIconStub = {
    props: ['icon'],
    template: '<i class="max-icon-stub" :data-icon="icon"></i>'
};

const MaxInputTextStub = {
    props: ['modelValue', 'label', 'disabled'],
    emits: ['update:modelValue', 'input'],
    template: '<input class="max-input-text-stub" :value="modelValue" :disabled="disabled" :aria-label="label" @input="$emit(\'update:modelValue\', $event.target.value); $emit(\'input\', $event)" />'
};

vi.mock('@/ipc/projects', () => ({
    projectSnapshot: vi.fn(),
    projectUpdate: vi.fn()
}));

function makeProject(id: string, title = 'Meu Projeto'): Project_Serialize {
    return {
        id,
        title,
        icon: 'mdi:folder',
        color: '#14B8A6',
        path: `/home/user/GitHub/${id}`,
        rootId: null,
        stack: 'unknown',
        identitySource: 'auto',
        sortOrder: 0,
        missing: false,
        createdAt: 1000,
        updatedAt: 1000
    };
}

describe('ProjectIdentityForm', () => {
    let pinia: ReturnType<typeof createPinia>;

    beforeEach(() => {
        pinia = createPinia();
        setActivePinia(pinia);
        vi.clearAllMocks();
    });

    it('valida campos obrigatórios e formato antes de submeter', async () => {
        const project = makeProject('proj-1');
        const wrapper = mount(ProjectIdentityForm, {
            props: {
                project,
                active: true
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxInputText: MaxInputTextStub
                }
            }
        });

        // Título vazio
        const titleInput = wrapper.find('input[aria-label="Título"]');
        await titleInput.setValue('');
        await wrapper.find('form').trigger('submit');

        expect(wrapper.text()).toContain('O título do projeto deve ter entre 1 e 120 caracteres válidos.');
        expect(projectsIpc.projectUpdate).not.toHaveBeenCalled();

        // Título válido, mas ícone inválido
        await titleInput.setValue('Novo Título');
        const iconInput = wrapper.find('input[aria-label="Ícone (Iconify)"]');
        await iconInput.setValue('icone-invalido-sem-dois-pontos');
        await wrapper.find('form').trigger('submit');

        expect(wrapper.text()).toContain('O identificador de ícone informado é inválido.');
        expect(projectsIpc.projectUpdate).not.toHaveBeenCalled();

        // Ícone válido, mas cor inválida
        await iconInput.setValue('mdi:rocket');
        const colorInput = wrapper.find('input[aria-label="Cor (#RRGGBB)"]');
        await colorInput.setValue('vermelho');
        await wrapper.find('form').trigger('submit');

        expect(wrapper.text()).toContain('A cor do projeto deve estar no formato hexadecimal #RRGGBB.');
        expect(projectsIpc.projectUpdate).not.toHaveBeenCalled();
    });

    it('submete alterações válidas e emite evento saved', async () => {
        const project = makeProject('proj-1');
        const updatedProject: Project_Serialize = {
            ...project,
            title: 'Projeto Renomeado',
            icon: 'mdi:star',
            color: '#FF0055'
        };

        vi.mocked(projectsIpc.projectUpdate).mockResolvedValue(updatedProject);
        vi.mocked(projectsIpc.projectSnapshot).mockResolvedValue({
            epoch: 'ep1',
            revision: 2,
            projects: [updatedProject],
            roots: [],
            discovered: []
        });

        const wrapper = mount(ProjectIdentityForm, {
            props: {
                project,
                active: true
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxInputText: MaxInputTextStub
                }
            }
        });

        await wrapper.find('input[aria-label="Título"]').setValue('Projeto Renomeado');
        await wrapper.find('input[aria-label="Ícone (Iconify)"]').setValue('mdi:star');
        await wrapper.find('input[aria-label="Cor (#RRGGBB)"]').setValue('#FF0055');

        await wrapper.find('form').trigger('submit');
        await flushPromises();

        expect(projectsIpc.projectUpdate).toHaveBeenCalledWith('proj-1', {
            title: 'Projeto Renomeado',
            icon: 'mdi:star',
            color: '#FF0055',
            resetIdentity: null
        });

        expect(wrapper.emitted('saved')?.[0]).toEqual([updatedProject]);
        expect(wrapper.text()).toContain('Identidade salva com sucesso.');
    });

    it('restaura identidade automática com a ação correspondente', async () => {
        const project = makeProject('proj-1', 'Projeto Custom');
        const restoredProject: Project_Serialize = {
            ...project,
            icon: 'mdi:folder',
            color: '#14B8A6'
        };

        vi.mocked(projectsIpc.projectUpdate).mockResolvedValue(restoredProject);
        vi.mocked(projectsIpc.projectSnapshot).mockResolvedValue({
            epoch: 'ep1',
            revision: 2,
            projects: [restoredProject],
            roots: [],
            discovered: []
        });

        const wrapper = mount(ProjectIdentityForm, {
            props: {
                project,
                active: true
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxInputText: MaxInputTextStub
                }
            }
        });

        await wrapper.find('.project-identity-form__reset-btn').trigger('click');
        await flushPromises();

        expect(projectsIpc.projectUpdate).toHaveBeenCalledWith('proj-1', {
            title: null,
            resetIdentity: true
        });

        expect(wrapper.emitted('saved')?.[0]).toEqual([restoredProject]);
        expect(wrapper.text()).toContain('Identidade restaurada com sucesso.');
    });

    it('preserva rascunho de digitação durante resync do mesmo projeto', async () => {
        const project = makeProject('proj-1', 'Título Original');
        const wrapper = mount(ProjectIdentityForm, {
            props: {
                project,
                active: true
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxInputText: MaxInputTextStub
                }
            }
        });

        // Usuário digita algo
        await wrapper.find('input[aria-label="Título"]').setValue('Rascunho Em Andamento');

        // Resync externo envia novo objeto para a prop project com mesmo id
        await wrapper.setProps({
            project: { ...project, updatedAt: 2000 }
        });

        // O valor digitado não pode ser sobrescrito pelo resync
        const titleInput = wrapper.find('input[aria-label="Título"]');
        expect((titleInput.element as HTMLInputElement).value).toBe('Rascunho Em Andamento');
    });

    it('recarrega dados do projeto ao reabrir (active passa de false para true)', async () => {
        const project = makeProject('proj-1', 'Título Original');
        const wrapper = mount(ProjectIdentityForm, {
            props: {
                project,
                active: false
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxInputText: MaxInputTextStub
                }
            }
        });

        await wrapper.setProps({ active: true });
        const titleInput = wrapper.find('input[aria-label="Título"]');
        expect((titleInput.element as HTMLInputElement).value).toBe('Título Original');

        // Altera e fecha
        await titleInput.setValue('Edição Não Salva');
        await wrapper.setProps({ active: false });

        // Reabre
        await wrapper.setProps({ active: true });
        expect((wrapper.find('input[aria-label="Título"]').element as HTMLInputElement).value).toBe('Título Original');
    });

    it('botão cancelar descarta alterações locais e emite cancel', async () => {
        const project = makeProject('proj-1', 'Título Original');
        const wrapper = mount(ProjectIdentityForm, {
            props: {
                project,
                active: true
            },
            global: {
                plugins: [pinia],
                stubs: {
                    MaxIcon: MaxIconStub,
                    MaxInputText: MaxInputTextStub
                }
            }
        });

        await wrapper.find('input[aria-label="Título"]').setValue('Alteração Descartável');
        await wrapper.find('.project-identity-form__btn--cancel').trigger('click');

        expect(wrapper.emitted('cancel')).toBeTruthy();
        expect((wrapper.find('input[aria-label="Título"]').element as HTMLInputElement).value).toBe('Título Original');
    });
});
