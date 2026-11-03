import { createApp, defineComponent, h, nextTick } from 'vue';
import { getActivePinia, setActivePinia } from 'pinia';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { MaxToast, Toast } from '@maxvue/max-components-ui';
import { registerAppPlugins } from '@/app/plugins';

describe('Integração de Toast com MaxComponentsUi', () => {
    let container: HTMLDivElement | null = null;

    beforeEach(() => {
        setActivePinia(undefined);
    });

    afterEach(() => {
        if (container && container.parentNode) {
            container.parentNode.removeChild(container);
            container = null;
        }
        setActivePinia(undefined);
    });

    it('exibe notificação montada via MaxToast após Toast.show com severidade warning', async () => {
        const HostComponent = defineComponent({
            name: 'ToastHostComponent',
            render() {
                return h('div', { class: 'test-toast-host' }, [h(MaxToast)]);
            }
        });

        const app = createApp(HostComponent);
        registerAppPlugins(app);

        container = document.createElement('div');
        document.body.appendChild(container);
        app.mount(container);

        expect(getActivePinia()).toBeDefined();

        Toast.show({
            title: 'Este resultado não está mais disponível',
            severity: 'warning'
        });

        await nextTick();

        const toastTitle = container.querySelector('.max-toast-title');
        expect(toastTitle).not.toBeNull();
        expect(toastTitle?.textContent).toBe('Este resultado não está mais disponível');

        const toastItem = container.querySelector('.max-toast-item');
        expect(toastItem?.classList.contains('severity-warning')).toBe(true);

        app.unmount();
    });
});
