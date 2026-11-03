import type { ProjectId } from '@/types/bindings';

/**
 * Calcula a permutação completa de todos os IDs de projetos ao mover um projeto visível.
 *
 * Preserva estritamente as posições (slots) dos projetos ocultos no catálogo completo,
 * alocando a nova sequência de projetos visíveis exclusivamente nos índices ocupados
 * por projetos visíveis.
 *
 * @param allProjects Lista completa de projetos no catálogo (ordenados por sortOrder).
 * @param hiddenIds Coleção de identificadores dos projetos atualmente ocultos.
 * @param movedId Identificador do projeto visível sendo movido.
 * @param targetId Identificador do projeto visível de destino.
 * @returns Novo array de ProjectId contendo todos os IDs na nova ordem, ou `null` caso inválido.
 */
export function computeReorderedProjectIds(
    allProjects: ReadonlyArray<{ id: ProjectId }>,
    hiddenIds: ReadonlySet<ProjectId> | ReadonlyArray<ProjectId>,
    movedId: ProjectId,
    targetId: ProjectId
): ProjectId[] | null {
    if (movedId === targetId) {
        return null;
    }

    const hiddenSet = hiddenIds instanceof Set ? hiddenIds : new Set(hiddenIds);

    if (hiddenSet.has(movedId) || hiddenSet.has(targetId)) {
        return null;
    }

    const visibleIds = allProjects
        .filter((p) => !hiddenSet.has(p.id))
        .map((p) => p.id);

    const fromIdx = visibleIds.indexOf(movedId);
    const toIdx = visibleIds.indexOf(targetId);

    if (fromIdx === -1 || toIdx === -1) {
        return null;
    }

    const reorderedVisible = [...visibleIds];
    const [moved] = reorderedVisible.splice(fromIdx, 1);
    if (!moved) {
        return null;
    }
    reorderedVisible.splice(toIdx, 0, moved);

    let visibleCursor = 0;
    const result: ProjectId[] = [];

    for (const p of allProjects) {
        if (hiddenSet.has(p.id)) {
            result.push(p.id);
        } else {
            const nextVisibleId = reorderedVisible[visibleCursor++];
            if (nextVisibleId === undefined) {
                return null;
            }
            result.push(nextVisibleId);
        }
    }

    return result;
}
