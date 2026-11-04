import axios from 'axios';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { createPinia, setActivePinia, type PiniaPlugin } from 'pinia';
import { createMaxPinia } from '@maxvue/max-pinia';
import {
    THEME_CATALOG_VIRTUAL_ROUTE,
    installThemeMaxPiniaAdapter
} from '@/ipc/themeMaxPiniaAdapter';
import { useThemeCatalogStore } from '@/stores/themeCatalog';
import type { ThemeSummary } from '@/types/theme';

const ipcMocks = vi.hoisted(() => ({
    themeList: vi.fn<() => Promise<ThemeSummary[]>>()
}));

vi.mock('@/ipc/themes', () => ({
    themeList: ipcMocks.themeList
}));

const mockThemes: ThemeSummary[] = [
    {
        id: 'max-navy-teal',
        name: 'Max Navy/Teal',
        origin: 'builtin',
        polarity: 'dark',
        background: '#0b1420',
        foreground: '#e2e8f0',
        cursor: '#14b8a6',
        accentSwatches: ['#0b1420', '#f43f5e', '#10b981', '#f59e0b', '#3b82f6', '#a855f7', '#06b6d4', '#e2e8f0']
    },
    {
        id: 'custom-solar',
        name: 'Custom Solar',
        origin: 'custom',
        polarity: 'light',
        background: '#fdf6e3',
        foreground: '#657b83',
        cursor: '#268bd2',
        accentSwatches: ['#eee8d5', '#dc322f', '#859900', '#b58900', '#268bd2', '#d33682', '#2aa198', '#073642']
    }
];

type MaxPiniaAxios = NonNullable<NonNullable<Parameters<typeof createMaxPinia>[0]>['axios']>;

describe('useThemeCatalogStore', () => {
    beforeEach(() => {
        vi.clearAllMocks();
        window.localStorage.clear();
    });

    it('inicializa com estado padrão e opções de rota virtual corretas', () => {
        const pinia = createPinia();
        setActivePinia(pinia);

        const store = useThemeCatalogStore();
        expect(store.id).toBe('theme-catalog');
        expect(store.isCached).toBe(true);
        expect(store.block_save).toBe(true);
        expect(store.data).toEqual([]);
        expect(store.options.get.route).toBe(THEME_CATALOG_VIRTUAL_ROUTE);
    });

    it('substitui o catálogo em memória via replaceCatalog', () => {
        const pinia = createPinia();
        setActivePinia(pinia);

        const store = useThemeCatalogStore();
        store.replaceCatalog(mockThemes);
        expect(store.data).toEqual(mockThemes);

        store.replaceCatalog(null as unknown as ThemeSummary[]);
        expect(store.data).toEqual([]);
    });

    it('recarrega o catálogo via refreshCatalog com plugin MaxPinia instalado', async () => {
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);
        ipcMocks.themeList.mockResolvedValueOnce(mockThemes);

        const pinia = createPinia();
        pinia.use(
            createMaxPinia({
                axios: client as unknown as MaxPiniaAxios
            }) as unknown as PiniaPlugin
        );
        setActivePinia(pinia);

        const store = useThemeCatalogStore();
        const reloaded = await store.refreshCatalog();

        expect(reloaded).toEqual(mockThemes);
        expect(store.data).toEqual(mockThemes);
        expect(ipcMocks.themeList).toHaveBeenCalledTimes(1);
    });

    it('propaga erro de status do servidor quando o recarregamento falha', async () => {
        const client = axios.create();
        installThemeMaxPiniaAdapter(client);
        ipcMocks.themeList.mockRejectedValueOnce(new Error('Erro de leitura do SQLite de temas'));

        const pinia = createPinia();
        pinia.use(
            createMaxPinia({
                axios: client as unknown as MaxPiniaAxios
            }) as unknown as PiniaPlugin
        );
        setActivePinia(pinia);

        const store = useThemeCatalogStore();
        await expect(store.refreshCatalog()).rejects.toThrow('Erro de leitura do SQLite de temas');
    });

    it('executa fallback defensivo quando store é chamado sem plugin MaxPinia ativo', async () => {
        const pinia = createPinia();
        setActivePinia(pinia);
        ipcMocks.themeList.mockResolvedValueOnce(mockThemes);

        const store = useThemeCatalogStore();
        const reloaded = await store.refreshCatalog();

        expect(reloaded).toEqual(mockThemes);
        expect(store.data).toEqual(mockThemes);
        expect(ipcMocks.themeList).toHaveBeenCalledTimes(1);
    });
});
