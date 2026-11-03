/* eslint-disable vue/one-component-per-file */
import { createApp, defineComponent, h, onMounted, ref } from 'vue';
import { getActivePinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { installGlobalErrorHandlers } from '@/app/logger';
import { registerAppPlugins } from '@/app/plugins';
import { useAppStore } from '@/stores/app';
import type { FrontendLogEvent } from '@/types/bindings';

describe('Bootstrap da aplicação - fluxo de inicialização e montagem', () => {
    let container: HTMLDivElement | null = null;
    let uninstallLogger: (() => void) | null = null;

    beforeEach(() => {
        setActivePinia(undefined);
    });

    afterEach(() => {
        if (uninstallLogger) {
            uninstallLogger();
            uninstallLogger = null;
        }
        if (container && container.parentNode) {
            container.parentNode.removeChild(container);
            container = null;
        }
        setActivePinia(undefined);
        vi.restoreAllMocks();
    });

    it('atravessa a sequência de bootstrap (handlers -> plugins -> mount) sem erros de Pinia renderizando o conteúdo inicial', async () => {
        const reportedEvents: FrontendLogEvent[] = [];
        const mockLogger = {
            report: vi.fn((event: FrontendLogEvent) => {
                reportedEvents.push(event);
            })
        };

        // Componente raiz que consome stores no setup exatamente como App.vue
        const RootComponentFixture = defineComponent({
            name: 'RootBootstrapFixture',
            setup() {
                const appStore = useAppStore();
                const mounted = ref(false);

                onMounted(() => {
                    mounted.value = true;
                });

                return { appStore, mounted };
            },
            render() {
                return h('div', { id: 'app-root', class: 'maxterminal-root' }, [
                    h('header', { class: 'titlebar' }, 'MaxTerminal Test Window'),
                    h('main', { class: 'workspace' }, `Status: ${this.appStore.status}`),
                    h('footer', { class: 'footer' }, `Mounted: ${this.mounted}`)
                ]);
            }
        });

        // 1. Criação do app
        const app = createApp(RootComponentFixture);

        // 2. Instalação de manipuladores de erro globais
        uninstallLogger = installGlobalErrorHandlers(app, mockLogger);

        // 3. Registro dos plugins da aplicação (deve instalar o Pinia antes de montar)
        registerAppPlugins(app);

        // 4. Montagem no DOM real
        container = document.createElement('div');
        container.id = 'app';
        document.body.appendChild(container);

        expect(() => {
            app.mount(container!);
        }).not.toThrow();

        // 5. Verifica que o Pinia está ativo
        expect(getActivePinia()).toBeDefined();

        // 6. Verifica que nenhum erro de Vue ou bootstrap foi relatado ao logger
        expect(reportedEvents).toEqual([]);
        expect(mockLogger.report).not.toHaveBeenCalled();

        // 7. Verifica a renderização do conteúdo observável na tela
        const rootElement = container.querySelector('#app-root');
        expect(rootElement).not.toBeNull();
        expect(container.querySelector('.titlebar')?.textContent).toBe('MaxTerminal Test Window');
        expect(container.querySelector('.workspace')?.textContent).toBe('Status: idle');

        // 8. Desmontagem e limpeza de instâncias
        app.unmount();
        expect(container.innerHTML).toBe('');
    });

    it('respeita a ordem estrita e registra erro no logger caso stores sejam acessadas antes do Pinia ser instalado', () => {
        const reportedEvents: FrontendLogEvent[] = [];
        const mockLogger = {
            report: vi.fn((event: FrontendLogEvent) => {
                reportedEvents.push(event);
            })
        };

        const InvalidOrderComponent = defineComponent({
            name: 'InvalidOrderComponent',
            setup() {
                // Tenta acessar a store antes do Pinia estar registrado
                useAppStore();
                return {};
            },
            render() {
                return h('div', 'invalido');
            }
        });

        const app = createApp(InvalidOrderComponent);
        uninstallLogger = installGlobalErrorHandlers(app, mockLogger);

        // NÃO chama registerAppPlugins propositalmente
        container = document.createElement('div');
        document.body.appendChild(container);

        app.mount(container);

        // Deve registrar erro no manipulador de erros do Vue
        expect(reportedEvents).toContain('vue_error');

        app.unmount();
    });
});
