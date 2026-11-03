import { mockIPC } from '@tauri-apps/api/mocks';
import { mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import { nextTick } from 'vue';
import { beforeEach, describe, expect, it } from 'vitest';
import App from '@/app/App.vue';
import AppEmptyState from '@/components/shell/AppEmptyState.vue';
import AppShell from '@/components/shell/AppShell.vue';
import AppTitleBar from '@/components/shell/AppTitleBar.vue';
import type { AppInfo } from '@/types/bindings';

const mockAppInfo: AppInfo = {
    name: 'MaxTerminal',
    version: '0.1.0',
    buildProfile: 'debug',
    tauriVersion: '2.0.0',
    os: 'linux',
    arch: 'x86_64',
    paths: {
        configDir: '/tmp/config',
        dataDir: '/tmp/data',
        stateDir: '/tmp/state',
        cacheDir: '/tmp/cache',
        logDir: '/tmp/logs',
        dbPath: '/tmp/data/maxterminal.db'
    }
};

describe('Shell Visual - AppShell, AppTitleBar, AppEmptyState e App.vue', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
    });

    describe('AppShell', () => {
        it('renderiza os slots obrigatórios e omite sidebar e statusbar quando vazios', () => {
            const wrapper = mount(AppShell, {
                slots: {
                    titlebar: '<div class="test-title">Barra</div>',
                    workspace: '<div class="test-work">Workspace</div>'
                }
            });

            expect(wrapper.find('.test-title').exists()).toBe(true);
            expect(wrapper.find('.test-work').exists()).toBe(true);
            expect(wrapper.find('.app-shell__sidebar').exists()).toBe(false);
            expect(wrapper.find('.app-shell__statusbar').exists()).toBe(false);
        });

        it('renderiza sidebar e statusbar quando os respectivos slots são fornecidos', () => {
            const wrapper = mount(AppShell, {
                slots: {
                    titlebar: 'Barra',
                    sidebar: '<nav class="test-side">Sidebar</nav>',
                    workspace: 'Workspace',
                    statusbar: '<span class="test-status">Status</span>'
                }
            });

            expect(wrapper.find('.app-shell__sidebar').exists()).toBe(true);
            expect(wrapper.find('.test-side').exists()).toBe(true);
            expect(wrapper.find('.app-shell__statusbar').exists()).toBe(true);
            expect(wrapper.find('.test-status').exists()).toBe(true);
        });

        it('reflete o atributo data-loading de acordo com a prop loading', async () => {
            const wrapper = mount(AppShell, {
                props: { loading: true }
            });
            expect(wrapper.find('.app-shell').attributes('data-loading')).toBe('true');

            await wrapper.setProps({ loading: false });
            expect(wrapper.find('.app-shell').attributes('data-loading')).toBe('false');
        });
    });

    describe('AppTitleBar', () => {
        it('renderiza título, versão e slot de ações', () => {
            const wrapper = mount(AppTitleBar, {
                props: {
                    title: 'MaxTerminal',
                    version: 'v0.1.0'
                },
                slots: {
                    actions: '<button class="action-btn">Fechar</button>'
                },
                global: {
                    stubs: { MaxIcon: true }
                }
            });

            expect(wrapper.text()).toContain('MaxTerminal');
            expect(wrapper.text()).toContain('v0.1.0');
            expect(wrapper.find('.action-btn').exists()).toBe(true);
        });
    });

    describe('AppEmptyState', () => {
        it('renderiza título, descrição e slot de ações padrão', () => {
            const wrapper = mount(AppEmptyState, {
                props: {
                    title: 'Nenhum projeto selecionado',
                    description: 'Adicione ou selecione um projeto para começar.'
                },
                slots: {
                    default: '<button class="custom-action">Criar Projeto</button>'
                },
                global: {
                    stubs: { MaxIcon: true }
                }
            });

            expect(wrapper.text()).toContain('Nenhum projeto selecionado');
            expect(wrapper.text()).toContain('Adicione ou selecione um projeto para começar.');
            expect(wrapper.find('.custom-action').exists()).toBe(true);
        });
    });

    describe('App.vue integrado com ciclo de bootstrap e recuperação', () => {
        it('renderiza no estado pronto sem a titlebar redundante e com o workspace pronto', async () => {
            mockIPC((cmd) => {
                if (cmd === 'app_info') {
                    return mockAppInfo;
                }
            });

            const pinia = createPinia();
            const wrapper = mount(App, {
                global: {
                    plugins: [pinia],
                    stubs: { MaxIcon: true }
                }
            });

            // Aguarda o onMounted assíncrono e atualização do DOM
            await new Promise((resolve) => setTimeout(resolve, 30));
            await nextTick();

            expect(wrapper.find('.app-title-bar').exists()).toBe(false);
            expect(wrapper.text()).not.toContain('MaxTerminal v0.1.0');
            expect(wrapper.text()).toContain('Nenhum projeto selecionado');
            expect(wrapper.find('.app-retry-btn').exists()).toBe(false);
        });

        it('em estado de erro de boot exibe mensagem em pt-BR, code do erro e botão Tentar novamente que recupera a UI', async () => {
            let shouldFail = true;

            mockIPC((cmd) => {
                if (cmd === 'app_info') {
                    if (shouldFail) {
                        throw {
                            code: 'core.paths_unresolved',
                            message: 'Não foi possível localizar os diretórios de configuração do usuário.'
                        };
                    }
                    return mockAppInfo;
                }
                if (cmd === 'app_log_frontend') {
                    return null;
                }
            });

            const pinia = createPinia();
            const wrapper = mount(App, {
                global: {
                    plugins: [pinia],
                    stubs: { MaxIcon: true }
                }
            });

            // Aguarda o onMounted assíncrono com erro
            await new Promise((resolve) => setTimeout(resolve, 30));
            await nextTick();

            // Validações do estado de erro
            expect(wrapper.text()).toContain('Falha ao inicializar o MaxTerminal');
            expect(wrapper.text()).toContain('Não foi possível localizar os diretórios de configuração do usuário.');
            expect(wrapper.find('.app-error-code').text()).toBe('core.paths_unresolved');

            const retryButton = wrapper.find('.app-retry-btn');
            expect(retryButton.exists()).toBe(true);
            expect(retryButton.text()).toBe('Tentar novamente');

            // Simula resolução do erro e clica em Tentar novamente
            shouldFail = false;
            await retryButton.trigger('click');

            await new Promise((resolve) => setTimeout(resolve, 30));
            await nextTick();

            // Após retry bem-sucedido, transita para o estado pronto
            expect(wrapper.text()).toContain('Nenhum projeto selecionado');
            expect(wrapper.find('.app-title-bar').exists()).toBe(false);
            expect(wrapper.find('.app-retry-btn').exists()).toBe(false);
        });
    });
});
