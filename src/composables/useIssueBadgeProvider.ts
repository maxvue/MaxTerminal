import {
    useProjectBadgeRegistry,
    type ProjectBadgeRegistry
} from '@/composables/useProjectBadgeRegistry';
import { useGithubIssuesStore } from '@/stores/githubIssues';

/**
 * Registra o provedor do badge 🟣 Issues Abertas (`id: 'github-issues'`, `order: 60`)
 * no `ProjectBadgeRegistry` da coluna esquerda de projetos (Fase 06 + Fase 16).
 */
export function useIssueBadgeProvider(explicitRegistry?: ProjectBadgeRegistry): () => void {
    const registry = explicitRegistry ?? useProjectBadgeRegistry();
    const store = useGithubIssuesStore();

    try {
        return registry.register({
            id: 'github-issues',
            order: 60,
            icon: 'lucide:circle-dot',
            colorVar: '--max-purple-500, #a855f7',
            count: (projectId) => store.openCountFor(projectId),
            tooltip: (count) =>
                count === 1 ? '1 Issue aberta no GitHub' : `${count} Issues abertas no GitHub`
        });
    } catch {
        // Caso já esteja registrado no mesmo escopo, retorna no-op.
        return () => {};
    }
}
