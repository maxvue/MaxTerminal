import { getCurrentScope, inject, onScopeDispose, provide, readonly, shallowRef, type InjectionKey, type Ref } from 'vue';
import type { ProjectId } from '@/types/bindings';

/**
 * Provedor de badge adicional para itens de projeto na coluna lateral (ex.: Fase 16 registra `'github-issues'`).
 */
export interface ProjectBadgeProvider {
    /** Identificador único do provedor (ex.: `'github-issues'`). */
    id: string;
    /** Ordem de exibição (>= 60; badges nativos ocupam 10..50). */
    order: number;
    /** Identificador de ícone Iconify. */
    icon: string;
    /** Token CSS de cor (ex.: `'--max-primary-500'`). */
    colorVar: string;
    /** Getter síncrono reativo; retorna `null` ou `0` quando oculto. */
    count: (projectId: ProjectId) => number | null;
    /** Formata o tooltip em pt-BR para a contagem positiva. */
    tooltip: (count: number) => string;
}

/** Registro de provedores de badges por instância da aplicação (provide/inject). */
export interface ProjectBadgeRegistry {
    register(provider: ProjectBadgeProvider): () => void;
    providers: Readonly<Ref<readonly ProjectBadgeProvider[]>>;
}

export const PROJECT_BADGE_REGISTRY_KEY: InjectionKey<ProjectBadgeRegistry> = Symbol('MaxTerminalProjectBadgeRegistry');

/** Cria uma instância isolada de `ProjectBadgeRegistry`. */
export function createProjectBadgeRegistry(): ProjectBadgeRegistry {
    const list = shallowRef<readonly ProjectBadgeProvider[]>([]);

    function register(provider: ProjectBadgeProvider): () => void {
        const id = provider.id.trim();
        if (!id) {
            throw new Error('O identificador do ProjectBadgeProvider não pode ser vazio.');
        }
        if (provider.order < 60 || !Number.isFinite(provider.order)) {
            throw new Error(`A ordem do ProjectBadgeProvider '${id}' deve ser um número finito >= 60.`);
        }
        if (list.value.some((p) => p.id === id)) {
            throw new Error(`ProjectBadgeProvider duplicado: '${id}'.`);
        }

        const next = [...list.value, { ...provider, id }].sort((a, b) => {
            if (a.order !== b.order) {
                return a.order - b.order;
            }
            return a.id.localeCompare(b.id);
        });
        list.value = Object.freeze(next);

        let disposed = false;
        const unregister = (): void => {
            if (disposed) {
                return;
            }
            disposed = true;
            const filtered = list.value.filter((p) => p.id !== id);
            list.value = Object.freeze(filtered);
        };

        if (getCurrentScope()) {
            onScopeDispose(unregister);
        }

        return unregister;
    }

    return {
        register,
        providers: readonly(list)
    };
}

/** Registra o `ProjectBadgeRegistry` no contexto de injeção atual da aplicação. */
export function provideProjectBadgeRegistry(existing?: ProjectBadgeRegistry): ProjectBadgeRegistry {
    const registry = existing ?? createProjectBadgeRegistry();
    provide(PROJECT_BADGE_REGISTRY_KEY, registry);
    return registry;
}

/** Obtém o `ProjectBadgeRegistry` da instância atual (ou cria um local isolado se chamado fora de um provider). */
export function useProjectBadgeRegistry(): ProjectBadgeRegistry {
    const injected = inject(PROJECT_BADGE_REGISTRY_KEY, null);
    if (injected) {
        return injected;
    }
    return createProjectBadgeRegistry();
}

/** Avalia de forma segura a contagem de um `ProjectBadgeProvider` (aceita apenas inteiros finitos positivos). */
export function evaluateBadgeProviderCount(
    provider: ProjectBadgeProvider,
    projectId: ProjectId
): number | null {
    try {
        const raw = provider.count(projectId);
        if (raw === null || raw === undefined) {
            return null;
        }
        if (typeof raw !== 'number' || !Number.isFinite(raw) || !Number.isInteger(raw)) {
            return null;
        }
        if (raw <= 0) {
            return null;
        }
        return raw;
    } catch {
        return null;
    }
}
