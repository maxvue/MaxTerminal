import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { beforeEach, describe, expect, it } from 'vitest';
import TemplateEditorModal from '@/components/workspace/TemplateEditorModal.vue';

describe('TemplateEditorModal.vue (R2 - Conexão Remota SSH)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    it('emite payload com conexão remota SSH quando selecionado', async () => {
        const wrapper = mount(TemplateEditorModal, {
            props: {
                open: true,
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
