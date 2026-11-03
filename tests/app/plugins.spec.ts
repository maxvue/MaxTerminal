/* eslint-disable vue/one-component-per-file */
import { createApp, defineComponent, h } from 'vue';
import { defineStore, getActivePinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { registerAppPlugins } from '@/app/plugins';
import { useAppStore } from '@/stores/app';

describe('Registro de plugins da aplicação - registerAppPlugins', () => {
    let container: HTMLDivElement | null = null;

    beforeEach(() => {
        // Assegura que nenhum Pinia residual de testes anteriores esteja ativo
        setActivePinia(undefined);
    });

    afterEach(() => {
        if (container && container.parentNode) {
            container.parentNode.removeChild(container);
            container = null;
        }
        // Restaura o estado global ao final do teste
        setActivePinia(undefined);
    });

    it('instala o Pinia no app permitindo o uso de useAppStore e registra o componente global MaxIcon', () => {
        // Valida precondição: nenhum Pinia ativo antes da instalação dos plugins
        expect(getActivePinia()).toBeUndefined();

        const TestComponent = defineComponent({
            name: 'TestPluginConsumer',
            setup() {
                const appStore = useAppStore();
                return { appStore };
            },
            render() {
                // Renderiza o componente global MaxIcon registrado pelo plugin e exibe o status da store
                return h('div', { class: 'test-container' }, [
                    h('span', { class: 'store-status' }, this.appStore.status),
                    h('max-icon', { icon: 'lucide:terminal' })
                ]);
            }
        });

        const app = createApp(TestComponent);
        registerAppPlugins(app);

        container = document.createElement('div');
        document.body.appendChild(container);

        expect(() => {
            app.mount(container!);
        }).not.toThrow();

        // Pinia deve estar ativo após a montagem do app
        const activePinia = getActivePinia();
        expect(activePinia).toBeDefined();

        const statusElement = container.querySelector('.store-status');
        expect(statusElement).not.toBeNull();
        expect(statusElement?.textContent).toBe('idle');

        app.unmount();
    });

    it('aplica o plugin MaxPinia a stores elegíveis e preserva o estado da store de runtime', () => {
        const useTestCachedStore = defineStore('test-cached-store', {
            state: () => ({
                isCached: true,
                cache_name: 'test-cache-spec',
                data: { sample: 'ok' }
            })
        });

        const TestComponent = defineComponent({
            name: 'TestMaxPiniaConsumer',
            setup() {
                const appStore = useAppStore();
                const cachedStore = useTestCachedStore();
                return { appStore, cachedStore };
            },
            render() {
                return h('div', { class: 'test-max-pinia' });
            }
        });

        const app = createApp(TestComponent);
        registerAppPlugins(app);

        container = document.createElement('div');
        document.body.appendChild(container);

        app.mount(container);

        const appStore = useAppStore();
        const cachedStore = useTestCachedStore();

        // A store de runtime permanece com status idle intacto e sem poluição de métodos de cache
        expect(appStore.status).toBe('idle');
        expect((appStore as unknown as Record<string, unknown>).saveInCache).toBeUndefined();

        // A store elegível recebe a extensão do plugin MaxPinia (métodos e propriedades de cache expostos)
        const extendedStore = cachedStore as unknown as Record<string, unknown>;
        expect(typeof extendedStore.saveInCache).toBe('function');
        expect(typeof extendedStore.reload).toBe('function');
        expect(typeof extendedStore.clearAll).toBe('function');

        app.unmount();
    });
});
