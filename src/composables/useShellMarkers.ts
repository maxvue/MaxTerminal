import type { CommandBlockId, PaneId } from '@/types/bindings';
import { getBlockMarkers, useCommandBlocksStore } from '@/stores/commandBlocks';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';

/** Qualidade da posição visual de um marcador de bloco no buffer do Xterm. */
export type MarkerPositionQuality = 'approximate' | 'unavailable';

/** Resultado do cálculo best-effort de linha de ancoragem no Xterm. */
export interface ResolvedShellMarker {
    line: number | undefined;
    positionQuality: MarkerPositionQuality;
}

/**
 * Composable utilitário para resolução best-effort de posições visuais de blocos de comando
 * após o flush do renderer Xterm, sem afetar o estado autoritativo do backend.
 */
export function useShellMarkers() {
    const terminalStore = useTerminalStore();
    const commandBlocksStore = useCommandBlocksStore();

    /**
     * Inspeciona se o painel possui âncora visual válida no buffer normal (não está em `altScreen`
     * nem sofreu corte de replay `partialView`).
     */
    function resolveAnchor(paneId: PaneId): ResolvedShellMarker {
        const instState = terminalStore.instance(paneId);
        if (instState?.modes.altScreen || instState?.flow.partialView) {
            return {
                line: undefined,
                positionQuality: 'unavailable'
            };
        }

        const instance = terminalInstanceRegistry.get(paneId);
        const normal = instance?.xterm?.buffer?.normal;
        if (!instance || !normal || instance.xterm.buffer?.active?.type !== 'normal') {
            return {
                line: undefined,
                positionQuality: 'unavailable'
            };
        }

        return {
            line: normal.baseY + normal.cursorY,
            positionQuality: 'approximate'
        };
    }

    /**
     * Executa `callback` após o próximo flush de escrita do Xterm naquele painel
     * (ou imediatamente caso a instância não tenha lote em voo).
     */
    function runAfterFlush(paneId: PaneId, callback: (anchor: ResolvedShellMarker) => void): () => void {
        const instance = terminalInstanceRegistry.get(paneId);
        if (!instance?.onAfterFlush) {
            callback(resolveAnchor(paneId));
            return () => {};
        }

        let cancelled = false;
        let unsubscribe: (() => void) | null = null;

        const timer = setTimeout(() => {
            if (cancelled) {
                return;
            }
            unsubscribe?.();
            unsubscribe = null;
            callback(resolveAnchor(paneId));
        }, 24);

        unsubscribe = instance.onAfterFlush(() => {
            if (cancelled) {
                return;
            }
            clearTimeout(timer);
            unsubscribe?.();
            unsubscribe = null;
            callback(resolveAnchor(paneId));
        });

        return () => {
            cancelled = true;
            clearTimeout(timer);
            unsubscribe?.();
            unsubscribe = null;
        };
    }

    /**
     * Marca blocos de um painel como `positionQuality: 'unavailable'` quando ocorre gap de stream
     * ou descarte do marcador no buffer do Xterm.
     */
    function markPaneMarkersUnavailable(paneId: PaneId): void {
        const blocks = commandBlocksStore.blocksOf(paneId);
        for (const block of blocks) {
            const rec = getBlockMarkers(paneId, block.id);
            if (!rec?.promptMarker || rec.promptMarker.isDisposed || rec.partialTrimmed) {
                commandBlocksStore.feed.patchMeta(paneId, block.id, {
                    positionQuality: 'unavailable'
                });
            }
        }
    }

    /** Consulta a qualidade de posição atual de um bloco específico. */
    function getPositionQuality(paneId: PaneId, blockId: CommandBlockId): MarkerPositionQuality {
        const block = commandBlocksStore.byPane[paneId]?.blocks[blockId];
        const metaQuality = block?.meta?.positionQuality;
        if (metaQuality === 'unavailable') {
            return 'unavailable';
        }
        const rec = getBlockMarkers(paneId, blockId);
        if (rec?.promptMarker && rec.promptMarker.isDisposed) {
            return 'unavailable';
        }
        return metaQuality === 'approximate' ? 'approximate' : 'unavailable';
    }

    return {
        resolveAnchor,
        runAfterFlush,
        markPaneMarkersUnavailable,
        getPositionQuality
    };
}
