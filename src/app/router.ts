import {
    defineAsyncComponent,
    defineComponent,
    h,
    type App,
    type Component
} from 'vue';
import {
    createMemoryHistory,
    createRouter,
    routerKey,
    type RouteLocationNormalizedLoaded,
    type RouteRecordRaw,
    type Router,
    type RouterHistory
} from 'vue-router';

/** Verifica se a rota atual de desenvolvimento solicita o Playground do Terminal (`/dev/terminal`). */
export function isDevTerminalPlaygroundRoute(pathname?: string): boolean {
    if (!import.meta.env.DEV) {
        return false;
    }
    const targetPath =
        pathname ?? (typeof window !== 'undefined' ? window.location.pathname : '/');
    return targetPath === '/dev/terminal';
}

/** Componente carregado sob demanda exclusivamente em `import.meta.env.DEV`. */
export const DevTerminalPlaygroundComponent: Component | null = import.meta.env.DEV
    ? defineAsyncComponent(() => import('@/components/terminal/__dev__/TerminalPlayground.vue'))
    : null;

const EmptyRouteView = defineComponent({
    name: 'MaxTerminalRoutePlaceholder',
    setup() {
        return () => h('div', { 'data-route-placeholder': 'true' });
    }
});

export const APP_ROUTES: RouteRecordRaw[] = [
    {
        path: '/',
        name: 'workspace-shell',
        component: EmptyRouteView
    },
    {
        path: '/dev/terminal',
        name: 'dev-terminal-playground',
        component: DevTerminalPlaygroundComponent ?? EmptyRouteView
    },
    {
        path: '/:pathMatch(.*)*',
        name: 'workspace-fallback',
        component: EmptyRouteView
    }
];

export function resolveInitialRoutePath(pathname?: string): string {
    if (isDevTerminalPlaygroundRoute(pathname)) {
        return '/dev/terminal';
    }
    const raw =
        pathname ?? (typeof window !== 'undefined' ? window.location.pathname : '/');
    return raw && raw.startsWith('/') ? raw : '/';
}

/**
 * Cria a instância do Vue Router do MaxTerminal garantindo que `route.name` esteja
 * definido sincronamente desde o primeiro ciclo de renderização de `<MaxApp>`.
 */
export function createAppRouter(
    history: RouterHistory = createMemoryHistory(),
    initialPath?: string
): Router {
    const router = createRouter({
        history,
        routes: APP_ROUTES
    });
    const targetPath = resolveInitialRoutePath(initialPath);
    router.currentRoute.value = router.resolve(
        targetPath
    ) as unknown as RouteLocationNormalizedLoaded;
    void router.replace(targetPath).catch(() => undefined);
    return router;
}

/**
 * Garante que a aplicação Vue possua um `Router` instalado no `appContext`
 * (inclusive quando componentes raiz são montados isoladamente em testes unitários).
 */
export function ensureAppRouterInstalled(app: App): Router {
    const provides = (app._context?.provides ?? {}) as Record<PropertyKey, unknown>;
    const existing = provides[routerKey as symbol] as Router | undefined;
    if (existing) {
        if (!existing.currentRoute.value.name) {
            const targetPath = resolveInitialRoutePath();
            existing.currentRoute.value = existing.resolve(
                targetPath
            ) as unknown as RouteLocationNormalizedLoaded;
        }
        return existing;
    }
    const router = createAppRouter();
    app.use(router);
    return router;
}
