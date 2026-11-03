import { defineAsyncComponent, type Component } from 'vue';

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
