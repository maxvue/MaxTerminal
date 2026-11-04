import axios, { type AxiosAdapter, type AxiosInstance, type InternalAxiosRequestConfig } from 'axios';
import type { App, Plugin } from 'vue';
import type { PiniaPlugin } from 'pinia';
import { createPinia } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import MaxComponentsUi from '@maxvue/max-components-ui';
import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
import { useIconStore } from '@maxvue/max-components-ui/stores';
import '@maxvue/max-components-ui/style.css';
import { BUNDLED_ICONS } from '@/constants/bundledIcons';
import { HORIZON_SNAPSHOT_ROUTE, horizonMaxPiniaAdapter } from '@/ipc/horizonMaxPiniaAdapter';
import { workspaceSelectionMaxPiniaAdapter } from '@/ipc/workspaceSelectionMaxPiniaAdapter';
import { WORKSPACE_SELECTION_ROUTE } from '@/types/workspaceSelection';

/**
 * Adapter combinado do Axios para MaxPinia, delegando rotas específicas para seus
 * respectivos adapters de IPC nativos (Horizon e Persistência de Seleção do Workspace).
 */
export const appMaxPiniaAdapter: AxiosAdapter = async (config: InternalAxiosRequestConfig) => {
    if (
        config.url?.startsWith(WORKSPACE_SELECTION_ROUTE) ||
        config.url?.includes(WORKSPACE_SELECTION_ROUTE)
    ) {
        return workspaceSelectionMaxPiniaAdapter(config);
    }
    if (
        config.url?.startsWith(HORIZON_SNAPSHOT_ROUTE) ||
        config.url?.includes(HORIZON_SNAPSHOT_ROUTE)
    ) {
        return horizonMaxPiniaAdapter(config);
    }
    const defaultAdapter = axios.getAdapter(axios.defaults.adapter);
    return defaultAdapter(config);
};

/**
 * Cria a instância customizada de Axios com os adapters integrados para o MaxPinia.
 */
export function createAppMaxPiniaAxiosInstance(): AxiosInstance {
    return axios.create({
        adapter: appMaxPiniaAdapter
    });
}

/**
 * Registra os plugins fundamentais da aplicação Vue:
 * - Pinia com plugin MaxPinia (com adapters customizados para Horizon e Workspace Selection)
 * - Plugin MaxComponentsUi (diretiva v-tooltip necessária para MaxIcon e opções de componentes)
 * - Catálogo embutido de ícones essenciais em useIconStore para renderização imediata e offline
 * - Componentes globais MaxIcon e MaxIcons de @maxvue/max-components-ui
 */
export function registerAppPlugins(app: App): void {
    const pinia = createPinia();
    const appAxios = createAppMaxPiniaAxiosInstance();
    type MaxPiniaOptions = NonNullable<Parameters<typeof createMaxPinia>[0]>;
    pinia.use(
        createMaxPinia({
            axios: appAxios as unknown as NonNullable<MaxPiniaOptions['axios']>
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
