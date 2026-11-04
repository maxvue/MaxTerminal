import axios, { type AxiosAdapter, type AxiosInstance, type InternalAxiosRequestConfig } from 'axios';
import type { App, Plugin } from 'vue';
import type { PiniaPlugin } from 'pinia';
import { createPinia } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import MaxComponentsUi, { configureMaxApp } from '@maxvue/max-components-ui';
import MaxIcon from '@maxvue/max-components-ui/components/MaxIcon';
import { useIconStore } from '@maxvue/max-components-ui/stores';
import '@maxvue/max-components-ui/style.css';
import { ensureAppRouterInstalled } from '@/app/router';
import { BUNDLED_ICONS } from '@/constants/bundledIcons';
import { HORIZON_SNAPSHOT_ROUTE, horizonMaxPiniaAdapter } from '@/ipc/horizonMaxPiniaAdapter';
import {
    THEME_CATALOG_VIRTUAL_ROUTE,
    THEME_SCHEME_VIRTUAL_ROUTE,
    installThemeMaxPiniaAdapter
} from '@/ipc/themeMaxPiniaAdapter';
import { workspaceSelectionMaxPiniaAdapter } from '@/ipc/workspaceSelectionMaxPiniaAdapter';
import { WORKSPACE_SELECTION_ROUTE } from '@/types/workspaceSelection';

/**
 * Configuração offline do `MaxApp` para ambiente desktop Tauri (`blank-mode` + `no-login`),
 * desativando chamadas HTTP para rotas web de usuário, menus e autenticação.
 */
export const MAX_APP_OFFLINE_CONFIG = {
    routeUser: '',
    routeUserSave: '',
    routeMenus: '',
    routeLogin: '',
    routeProviders: '',
    routeImpersonateStatus: ''
} as const;

/**
 * Aplica a configuração offline do `MaxApp` antes de montar componentes que instanciam stores do `MaxComponentsUi`.
 */
export function applyMaxAppOfflineConfig(): void {
    configureMaxApp({ ...MAX_APP_OFFLINE_CONFIG });
}

/**
 * Adapter combinado do Axios para MaxPinia, delegando rotas específicas para seus
 * respectivos adapters de IPC nativos (Temas, Horizon e Persistência de Seleção do Workspace).
 */
export const appMaxPiniaAdapter: AxiosAdapter = async (config: InternalAxiosRequestConfig) => {
    const rawUrl = config.url as unknown;
    if (
        !rawUrl ||
        typeof rawUrl !== 'string' ||
        rawUrl.trim().length === 0
    ) {
        return {
            data: null,
            status: 200,
            statusText: 'OK',
            headers: {},
            config
        };
    }
    if (
        rawUrl.startsWith(WORKSPACE_SELECTION_ROUTE) ||
        rawUrl.includes(WORKSPACE_SELECTION_ROUTE)
    ) {
        return workspaceSelectionMaxPiniaAdapter(config);
    }
    if (
        rawUrl.startsWith(HORIZON_SNAPSHOT_ROUTE) ||
        rawUrl.includes(HORIZON_SNAPSHOT_ROUTE)
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
    const instance = axios.create({
        adapter: appMaxPiniaAdapter
    });
    installThemeMaxPiniaAdapter(instance);
    return instance;
}

export { THEME_CATALOG_VIRTUAL_ROUTE, THEME_SCHEME_VIRTUAL_ROUTE };

/**
 * Registra os plugins fundamentais da aplicação Vue:
 * - Configuração offline de `MaxApp` (`configureMaxApp` com rotas vazias)
 * - Vue Router com rotas nomeadas (`/`, `/dev/terminal` e fallback)
 * - Pinia com plugin MaxPinia (com adapters customizados para Temas, Horizon e Workspace Selection)
 * - Plugin MaxComponentsUi (diretiva v-tooltip necessária para MaxIcon e opções de componentes)
 * - Catálogo embutido de ícones essenciais em useIconStore para renderização imediata e offline
 * - Componentes globais MaxIcon e MaxIcons de @maxvue/max-components-ui
 */
export function registerAppPlugins(app: App): void {
    applyMaxAppOfflineConfig();
    installThemeMaxPiniaAdapter();

    const pinia = createPinia();
    const appAxios = createAppMaxPiniaAxiosInstance();
    type MaxPiniaOptions = NonNullable<Parameters<typeof createMaxPinia>[0]>;
    pinia.use(
        createMaxPinia({
            axios: appAxios as unknown as NonNullable<MaxPiniaOptions['axios']>
        }) as unknown as PiniaPlugin
    );
    app.use(pinia);
    ensureAppRouterInstalled(app);
    app.use(MaxComponentsUi as unknown as Plugin);

    const iconStore = useIconStore(pinia as never);
    iconStore.icons_data = {
        ...BUNDLED_ICONS,
        ...(iconStore.icons_data || {})
    };

    app.component('MaxIcon', MaxIcon);
    app.component('MaxIcons', MaxIcon);
}
