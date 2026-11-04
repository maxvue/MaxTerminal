import type { ProjectId, TabId } from '@/types/bindings';

export type WorkspaceSelectionViewKind = 'terminal' | 'issues';

/** Representação da visão ativa (terminal ou issues) de um projeto selecionado. */
export interface ProjectActiveView {
    kind: WorkspaceSelectionViewKind;
    terminalTabId: TabId | null;
}

/**
 * Contrato canônico versionado para restauração da seleção do workspace (§3 da spec).
 * Persiste o projeto atualmente selecionado e a aba visível por projeto.
 */
export interface WorkspaceSelection {
    version: 1;
    mutationId: string;
    selectedProjectId: ProjectId | null;
    activeViewByProject: Record<ProjectId, ProjectActiveView>;
}

export const WORKSPACE_SELECTION_VERSION = 1 as const;
export const WORKSPACE_SELECTION_ROUTE = '/app/workspace/selection';
export const WORKSPACE_SELECTION_PENDING_STORAGE_KEY = 'maxterminal.workspace-selection.pending.v1';

/**
 * Cria o estado inicial padrão imutável da seleção do workspace.
 */
export function createDefaultWorkspaceSelection(): WorkspaceSelection {
    return {
        version: 1,
        mutationId: 'initial',
        selectedProjectId: null,
        activeViewByProject: {}
    };
}

/**
 * Validação rigorosa em runtime do contrato de WorkspaceSelection.
 */
export function isWorkspaceSelection(value: unknown): value is WorkspaceSelection {
    if (typeof value !== 'object' || value === null || Array.isArray(value)) {
        return false;
    }
    const candidate = value as Record<string, unknown>;
    if (candidate.version !== 1) {
        return false;
    }
    if (typeof candidate.mutationId !== 'string' || candidate.mutationId.trim().length === 0) {
        return false;
    }
    if (candidate.selectedProjectId !== null && typeof candidate.selectedProjectId !== 'string') {
        return false;
    }
    if (
        typeof candidate.activeViewByProject !== 'object' ||
        candidate.activeViewByProject === null ||
        Array.isArray(candidate.activeViewByProject)
    ) {
        return false;
    }

    const views = candidate.activeViewByProject as Record<string, unknown>;
    for (const [projId, view] of Object.entries(views)) {
        if (typeof projId !== 'string') {
            return false;
        }
        if (typeof view !== 'object' || view === null || Array.isArray(view)) {
            return false;
        }
        const v = view as Record<string, unknown>;
        if (v.kind !== 'terminal' && v.kind !== 'issues') {
            return false;
        }
        if (v.terminalTabId !== null && typeof v.terminalTabId !== 'string') {
            return false;
        }
    }

    return true;
}

/**
 * Normaliza e valida um valor qualquer, retornando um WorkspaceSelection garantido.
 */
export function normalizeWorkspaceSelection(raw: unknown): WorkspaceSelection {
    if (isWorkspaceSelection(raw)) {
        return raw;
    }
    return createDefaultWorkspaceSelection();
}
