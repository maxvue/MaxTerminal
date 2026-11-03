import type { App } from 'vue';
import type { PiniaPlugin } from 'pinia';
import { createPinia } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
import '@maxvue/max-components-ui/style.css';

/**
 * Registra os plugins fundamentais da aplicação Vue:
 * - Pinia com plugin MaxPinia
 * - Estilos base e componente global MaxIcon de @maxvue/max-components-ui
 */
export function registerAppPlugins(app: App): void {
    const pinia = createPinia();
    pinia.use(createMaxPinia() as unknown as PiniaPlugin);
    app.use(pinia);
    app.component('MaxIcon', MaxIcon);
}
