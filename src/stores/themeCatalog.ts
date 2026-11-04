import { ref, type Ref } from 'vue';
import { defineStore } from 'pinia';
import { apiGetRoute } from '@maxvue/max-use';
import {
    THEME_CATALOG_VIRTUAL_ROUTE,
    installThemeMaxPiniaAdapter
} from '@/ipc/themeMaxPiniaAdapter';
import { themeList } from '@/ipc/themes';
import type { ThemeSummary } from '@/types/theme';

interface MaxPiniaCatalogStoreInstance {
    reload?: () => Promise<void>;
    status?: Ref<{
        server: {
            get: {
                is_error: boolean;
                error: unknown;
            };
        };
    }>;
}

export const useThemeCatalogStore = defineStore('theme-catalog', () => {
    installThemeMaxPiniaAdapter();

    const isCached = ref(true);
    const id = ref('theme-catalog');
    const data = ref<ThemeSummary[]>([]);
    const default_value = ref<ThemeSummary[]>([]);
    const block_save = ref(true);

    const options = {
        get: {
            route: THEME_CATALOG_VIRTUAL_ROUTE
        }
    };

    function replaceCatalog(themes: ThemeSummary[]): void {
        data.value = Array.isArray(themes) ? themes : [];
    }

    async function refreshCatalog(): Promise<ThemeSummary[]> {
        const self = useThemeCatalogStore() as unknown as MaxPiniaCatalogStoreInstance;
        if (typeof self.reload === 'function') {
            await self.reload();
            if (self.status?.value?.server?.get?.is_error) {
                const error = self.status.value.server.get.error;
                throw error instanceof Error
                    ? error
                    : new Error('Falha ao recarregar catálogo de temas via MaxPinia.');
            }
            return data.value;
        }

        // Fallback defensivo quando invocado fora do plugin MaxPinia
        try {
            const response: unknown = await apiGetRoute<ThemeSummary[]>(
                THEME_CATALOG_VIRTUAL_ROUTE
            );
            if (Array.isArray(response)) {
                data.value = response;
                return data.value;
            }
            if (
                response &&
                typeof response === 'object' &&
                'data' in response &&
                Array.isArray((response as { data?: unknown }).data)
            ) {
                data.value = (response as { data: ThemeSummary[] }).data;
                return data.value;
            }
        } catch {
            // Em caso de falha no adapter de rota virtual, recorre ao IPC direto
        }

        const fallbackList = await themeList();
        data.value = fallbackList;
        return data.value;
    }

    return {
        isCached,
        id,
        data,
        default_value,
        block_save,
        options,
        replaceCatalog,
        refreshCatalog
    };
});
