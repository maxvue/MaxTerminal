import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import TemplateEditorModal from '@/components/workspace/TemplateEditorModal.vue';

vi.mock('@/ipc/templates', () => ({ templateTestConnection: vi.fn() }));

import { templateTestConnection } from '@/ipc/templates';

describe('TemplateEditorModal.vue (R2 - Conexão Remota SSH)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.mocked(templateTestConnection).mockReset();
    });

    it('emite payload com conexão remota SSH quando selecionado', async () => {
        const wrapper = mount(TemplateEditorModal, {
            props: {
                open: true,
                projectId: 'project-1',
                template: null,
                saving: false
            }
        });

        // Preenche campos básicos
        const nameInput = wrapper.find('input[placeholder="Ex.: Servidor Dev"]');
        await nameInput.setValue('Servidor Produção');

        // Alterna para origem remota
        const vm = wrapper.vm as unknown as {
            origin: string;
            sshHost: string;
            sshUser: string;
            sshPassword: string;
            sshUseKey: boolean;
        };
        vm.origin = 'remote';
        await wrapper.vm.$nextTick();

        vm.sshHost = '10.0.0.5';
        vm.sshUser = 'admin';
        vm.sshPassword = 'secret-password';
        vm.sshUseKey = false;
        await wrapper.vm.$nextTick();

        await wrapper.find('form').trigger('submit');
        await wrapper.vm.$nextTick();

        const emitted = wrapper.emitted('save');
        expect(emitted).toBeTruthy();
        const firstCall = emitted?.[0];
        expect(firstCall).toBeDefined();
        const payload = firstCall![0] as {
            name: string;
            connection: {
                kind: string;
                host?: string;
                user?: string;
                auth?: { kind: string; operation?: { action: string; password?: string } };
            };
        };

        expect(payload.name).toBe('Servidor Produção');
        expect(payload.connection.kind).toBe('ssh');
        expect(payload.connection.host).toBe('10.0.0.5');
        expect(payload.connection.user).toBe('admin');
        expect(payload.connection.auth?.kind).toBe('password');
        expect(payload.connection.auth?.operation).toEqual({
            action: 'set',
            password: 'secret-password'
        });
    });
});

interface RemoteForm {
    origin: 'local' | 'remote';
    sshHost: string;
    sshUser: string;
    sshUseKey: boolean;
    sshIdentityFile: string;
    sshPassword: string;
    cwd: string;
}

async function mountRemoteForm() {
    const wrapper = mount(TemplateEditorModal, {
        props: { open: true, projectId: 'project-1', template: null }
    });
    const form = wrapper.vm as unknown as RemoteForm;
    form.origin = 'remote';
    form.sshHost = '192.168.3.31';
    form.sshUser = 'engeapp';
    form.sshUseKey = true;
    form.cwd = '~/htdocs/engeapp.com.br/current';
    await wrapper.vm.$nextTick();
    return { wrapper, form };
}

function testButton(wrapper: ReturnType<typeof mount<typeof TemplateEditorModal>>) {
    return wrapper.findAll('button').find((button) => button.text().includes('Testar Conexão') || button.text().includes('Testando conexão'))!;
}

describe('Teste da conexão SSH do template', () => {
    beforeEach(() => {
        vi.mocked(templateTestConnection).mockReset();
    });

    it('exibe o botão apenas para conexão remota', async () => {
        const { wrapper, form } = await mountRemoteForm();
        expect(testButton(wrapper)).toBeDefined();
        form.origin = 'local';
        await wrapper.vm.$nextTick();
        expect(testButton(wrapper)).toBeUndefined();
        wrapper.unmount();
    });

    it('testa a configuração sem salvar e preserva o til para o servidor', async () => {
        vi.mocked(templateTestConnection).mockResolvedValue('/home/engeapp/htdocs/engeapp.com.br/current');
        const { wrapper } = await mountRemoteForm();
        await testButton(wrapper).trigger('click');
        await flushPromises();
        expect(templateTestConnection).toHaveBeenCalledWith('project-1', null, {
            kind: 'ssh', host: '192.168.3.31', user: 'engeapp', auth: { kind: 'key', identityFile: null }
        }, '~/htdocs/engeapp.com.br/current');
        expect(wrapper.find('[role="status"]').text()).toContain('/home/engeapp/htdocs/engeapp.com.br/current');
        expect(wrapper.emitted('save')).toBeUndefined();
        wrapper.unmount();
    });

    it('mostra erro de pasta remota e permite tentar novamente', async () => {
        vi.mocked(templateTestConnection).mockRejectedValue(new Error('A pasta remota não existe.'));
        const { wrapper } = await mountRemoteForm();
        await testButton(wrapper).trigger('click');
        await flushPromises();
        expect(wrapper.find('[role="alert"]').text()).toBe('A pasta remota não existe.');
        expect(testButton(wrapper).attributes('disabled')).toBeUndefined();
        wrapper.unmount();
    });

    it('valida usuário antes de chamar SSH', async () => {
        const { wrapper, form } = await mountRemoteForm();
        form.sshUser = '';
        await testButton(wrapper).trigger('click');
        expect(templateTestConnection).not.toHaveBeenCalled();
        expect(wrapper.find('[role="alert"]').text()).toContain('usuário');
        wrapper.unmount();
    });

    it('desabilita testes simultâneos e ignora resultado de configuração alterada', async () => {
        let resolveTest!: (cwd: string) => void;
        vi.mocked(templateTestConnection).mockImplementation(() => new Promise((resolve) => { resolveTest = resolve; }));
        const { wrapper, form } = await mountRemoteForm();
        await testButton(wrapper).trigger('click');
        expect(testButton(wrapper).attributes('disabled')).toBeDefined();
        form.cwd = '~/outra-pasta';
        await wrapper.vm.$nextTick();
        resolveTest('/home/engeapp/htdocs/engeapp.com.br/current');
        await flushPromises();
        expect(wrapper.find('[role="status"]').exists()).toBe(false);
        expect(testButton(wrapper).attributes('disabled')).toBeUndefined();
        wrapper.unmount();
    });
});
