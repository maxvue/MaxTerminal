import type { PaneId, PaneLayoutNode } from '@/types/bindings';

export const MAX_PANES_PER_TAB = 8;
export const MAX_SPLIT_DEPTH = 4;

/** Gera um identificador único para `request_id` de mutações do workspace. */
export function buildRequestId(prefix = 'req'): string {
    if (typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function') {
        return `${prefix}-${crypto.randomUUID()}`;
    }
    return `${prefix}-${Date.now()}-${Math.random().toString(36).slice(2, 10)}`;
}

/** Conta o total de folhas (`leaf`) na árvore de layout. */
export function countLeafPanes(node: PaneLayoutNode): number {
    if (node.kind === 'leaf') {
        return 1;
    }
    return node.children.reduce((acc, child) => acc + countLeafPanes(child), 0);
}

/** Coleta todos os `PaneId` presentes nas folhas em ordem da esquerda para a direita. */
export function collectLeafPaneIds(node: PaneLayoutNode): PaneId[] {
    if (node.kind === 'leaf') {
        return [node.paneId];
    }
    const result: PaneId[] = [];
    for (const child of node.children) {
        result.push(...collectLeafPaneIds(child));
    }
    return result;
}

/** Retorna a profundidade máxima de splits da árvore (`0` para folha única). */
export function computeTreeDepth(node: PaneLayoutNode): number {
    if (node.kind === 'leaf') {
        return 0;
    }
    let maxChild = 0;
    for (const child of node.children) {
        const d = computeTreeDepth(child);
        if (d > maxChild) {
            maxChild = d;
        }
    }
    return 1 + maxChild;
}

/** Calcula a profundidade de uma folha específica (`paneId`) dentro da árvore (`-1` se não encontrada). */
export function findLeafDepth(node: PaneLayoutNode, paneId: PaneId, currentDepth = 0): number {
    if (node.kind === 'leaf') {
        return node.paneId === paneId ? currentDepth : -1;
    }
    for (const child of node.children) {
        const found = findLeafDepth(child, paneId, currentDepth + 1);
        if (found !== -1) {
            return found;
        }
    }
    return -1;
}

/** Verifica se o painel pode ser dividido respeitando `MAX_PANES_PER_TAB` (8) e `MAX_SPLIT_DEPTH` (4). */
export function canSplitPane(root: PaneLayoutNode, paneId: PaneId): boolean {
    if (countLeafPanes(root) >= MAX_PANES_PER_TAB) {
        return false;
    }
    const leafDepth = findLeafDepth(root, paneId);
    if (leafDepth === -1) {
        return false;
    }
    return leafDepth + 1 <= MAX_SPLIT_DEPTH;
}

/** Normaliza o par de tamanhos de um nó `split` garantindo valores válidos que somam 100. */
export function normalizeSplitSizes(sizes: (number | null)[] | undefined): [number, number] {
    const first = sizes?.[0];
    const second = sizes?.[1];
    if (
        typeof first === 'number' &&
        typeof second === 'number' &&
        Number.isFinite(first) &&
        Number.isFinite(second) &&
        first >= 5 &&
        second >= 5
    ) {
        const sum = first + second;
        if (Math.abs(sum - 100) <= 0.5) {
            return [first, 100 - first];
        }
    }
    return [50, 50];
}
