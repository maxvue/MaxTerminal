import type { App } from 'vue';
import type { PiniaPlugin } from 'pinia';
import { createPinia } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
import { useIconStore } from '@maxvue/max-components-ui/stores';
import '@maxvue/max-components-ui/style.css';
import { BUNDLED_ICONS } from '@/constants/bundledIcons';

/**
 * Registra os plugins fundamentais da aplicação Vue:
 * - Pinia com plugin MaxPinia
 * - Catálogo embutido de ícones essenciais em useIconStore para renderização imediata e offline
 * - Componentes globais MaxIcon e MaxIcons de @maxvue/max-components-ui
 */
export function registerAppPlugins(app: App): void {
    const pinia = createPinia();
    pinia.use(createMaxPinia() as unknown as PiniaPlugin);
    app.use(pinia);

    const iconStore = useIconStore(pinia as never);
    iconStore.icons_data = {
        ...BUNDLED_ICONS,
        ...(iconStore.icons_data || {})
    };

    app.component('MaxIcon', MaxIcon);
    app.component('MaxIcons', MaxIcon);
}
