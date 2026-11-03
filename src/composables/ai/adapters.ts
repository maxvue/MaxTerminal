import { ptyWrite } from '@/ipc/pty';
import { useProjectsStore } from '@/stores/projects';
import { useTabsStore } from '@/stores/tabs';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import type { PaneId, ProjectId } from '@/types/bindings';

/** Callback registrado por painel para inserir texto diretamente na barra `RichInput`. */
export type RichInputInsertHandler = (command: string) => void;

const richInputHandlers = new Map<PaneId, RichInputInsertHandler>();

/**
 * Registra o manipulador de inserção de texto do `RichInput` para o painel `paneId`.
 * Retorna uma função de remoção.
 */
export function registerRichInputInsertHandler(
    paneId: PaneId,
    handler: RichInputInsertHandler
): () => void {
    richInputHandlers.set(paneId, handler);
    return () => {
        if (richInputHandlers.get(paneId) === handler) {
            richInputHandlers.delete(paneId);
        }
    };
}

/**
 * Insere um comando na barra `RichInput` do painel (ou cola no terminal caso não haja `RichInput` ativo).
 */
export function insertCommandIntoRichInput(paneId: PaneId, command: string): boolean {
    const clean = command.replace(/[\r\n]+/g, ' ').trim();
    if (!clean) {
        return false;
    }

    const handler = richInputHandlers.get(paneId);
    if (handler) {
        handler(clean);
        return true;
    }

    const instance = terminalInstanceRegistry.get(paneId);
    if (instance) {
        instance.inputPipeline.paste(clean);
        return true;
    }

    return false;
}

/**
 * Executa um comando diretamente no PTY associado ao painel `paneId`.
 */
export async function executeCommandInPane(paneId: PaneId, command: string): Promise<void> {
    const clean = command.replace(/[\r\n]+/g, ' ').trim();
    if (!clean) {
        return;
    }

    const instance = terminalInstanceRegistry.get(paneId);
    if (instance) {
        await instance.inputPipeline.sendLine(clean);
        return;
    }

    const terminalStore = useTerminalStore();
    const instState = terminalStore.instance(paneId);
    if (instState?.ptyId) {
        await ptyWrite(instState.ptyId, { kind: 'text', text: `${clean}\n` });
        return;
    }

    const tabsStore = useTabsStore();
    for (const tab of Object.values(tabsStore.byId)) {
        const pane = tab.panes.find((p) => p.id === paneId);
        if (pane?.ptyId) {
            await ptyWrite(pane.ptyId, { kind: 'text', text: `${clean}\n` });
            return;
        }
    }
}

/**
 * Resolve o `ProjectId` e a stack detectada do projeto ao qual pertence o painel `paneId`.
 */
export function resolvePaneProjectInfo(paneId: PaneId): {
    projectId: ProjectId | null;
    stack: string | null;
} {
    const tabsStore = useTabsStore();
    const projectsStore = useProjectsStore();

    for (const tab of Object.values(tabsStore.byId)) {
        if (tab.panes.some((p) => p.id === paneId)) {
            const proj = projectsStore.projectById(tab.projectId);
            return {
                projectId: tab.projectId,
                stack: proj?.stack ?? null
            };
        }
    }

    const selected = projectsStore.selectedProject;
    return {
        projectId: selected?.id ?? null,
        stack: selected?.stack ?? null
    };
}

/** Gera um identificador único para requisições de IA (`AiRequestId`). */
export function createAiRequestId(prefix = 'ai'): string {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
        return crypto.randomUUID();
    }
    return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}
