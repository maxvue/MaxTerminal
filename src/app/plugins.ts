import type { App, Plugin } from 'vue';
import type { PiniaPlugin } from 'pinia';
import { createPinia } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import MaxComponentsUi from '@maxvue/max-components-ui';
import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
import { useIconStore } from '@maxvue/max-components-ui/stores';
import '@maxvue/max-components-ui/style.css';
import { BUNDLED_ICONS } from '@/constants/bundledIcons';
import { createHorizonAxiosInstance } from '@/ipc/horizonMaxPiniaAdapter';

/**
 * Registra os plugins fundamentais da aplicação Vue:
 * - Pinia com plugin MaxPinia (com adapter Axios customizado para snapshot do Horizon)
 * - Plugin MaxComponentsUi (diretiva v-tooltip necessária para MaxIcon e opções de componentes)
 * - Catálogo embutido de ícones essenciais em useIconStore para renderização imediata e offline
 * - Componentes globais MaxIcon e MaxIcons de @maxvue/max-components-ui
 */
export function registerAppPlugins(app: App): void {
    const pinia = createPinia();
    const horizonAxios = createHorizonAxiosInstance();
    type MaxPiniaOptions = NonNullable<Parameters<typeof createMaxPinia>[0]>;
    pinia.use(
        createMaxPinia({
            axios: horizonAxios as unknown as NonNullable<MaxPiniaOptions['axios']>
        }) as unknown as PiniaPlugin
    );
    app.use(pinia);
    app.use(MaxComponentsUi as unknown as Plugin);

    const iconStore = useIconStore(pinia as never);
    iconStore.icons_data = {
        ...BUNDLED_ICONS,
        ...(iconStore.icons_data || {})
    };

    app.component('MaxIcon', MaxIcon);
    app.component('MaxIcons', MaxIcon);
}
