import { defineStore } from 'pinia';
import type { IMarker } from '@xterm/xterm';
import type { CommandBlockId, PaneId } from '@/types/bindings';
import type {
    CommandBlockFeed,
    CommandBlockModel,
    CommandBlockStatus
} from '@/types/terminal';
import {
    bindBlockMarkersLookup,
    type BlockMarkersRecord
} from '@/terminal/blocks/blockActionRegistry';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';

/** Bucket serializável de blocos de um painel mantido na store Pinia. */
export interface PaneBlocksBucket {
    order: CommandBlockId[];
    blocks: Record<CommandBlockId, CommandBlockModel>;
}

/** Estado da store Pinia `useCommandBlocksStore`. */
export interface CommandBlocksStoreState {
    byPane: Record<PaneId, PaneBlocksBucket>;
    maxBlocksPerPane: number;
}

const markersByPane = new Map<PaneId, Map<CommandBlockId, BlockMarkersRecord>>();

/** Consulta os `IMarker`s reais associados a um bloco de comando fora da reatividade do Pinia. */
export function getBlockMarkers(
    paneId: PaneId,
    blockId: CommandBlockId
): BlockMarkersRecord | undefined {
    return markersByPane.get(paneId)?.get(blockId);
}

bindBlockMarkersLookup(getBlockMarkers);

function ensureMarkersMap(paneId: PaneId): Map<CommandBlockId, BlockMarkersRecord> {
    let map = markersByPane.get(paneId);
    if (!map) {
        map = new Map<CommandBlockId, BlockMarkersRecord>();
        markersByPane.set(paneId, map);
    }
    return map;
}

function disposeMarkerRecord(record: BlockMarkersRecord | undefined): void {
    if (!record) {
        return;
    }
    try {
        if (record.promptMarker && !record.promptMarker.isDisposed) {
            record.promptMarker.dispose();
        }
    } catch {
        // Ignora erro de dispose.
    }
    try {
        if (record.outputStartMarker && !record.outputStartMarker.isDisposed) {
            record.outputStartMarker.dispose();
        }
    } catch {
        // Ignora erro de dispose.
    }
    try {
        if (record.outputEndMarker && !record.outputEndMarker.isDisposed) {
            record.outputEndMarker.dispose();
        }
    } catch {
        // Ignora erro de dispose.
    }
}

function resolveCurrentNormalLine(paneId: PaneId, explicitLine?: number): {
    line: number;
    marker: IMarker | null;
} {
    const instance = terminalInstanceRegistry.get(paneId);
    if (!instance?.xterm?.buffer?.normal) {
        return {
            line: explicitLine ?? 0,
            marker: null
        };
    }

    const normal = instance.xterm.buffer.normal;
    const currentCursorAbs = normal.baseY + normal.cursorY;
    const targetLine = explicitLine !== undefined ? explicitLine : currentCursorAbs;

    if (instance.xterm.buffer.active?.type !== 'normal' || !instance.xterm.registerMarker) {
        return {
            line: targetLine,
            marker: null
        };
    }

    const offset = targetLine - currentCursorAbs;
    try {
        const marker = instance.xterm.registerMarker(offset) ?? null;
        return {
            line: marker && marker.line >= 0 ? marker.line : targetLine,
            marker
        };
    } catch {
        return {
            line: targetLine,
            marker: null
        };
    }
}

export const useCommandBlocksStore = defineStore('commandBlocks', {
    state: (): CommandBlocksStoreState => ({
        byPane: {},
        maxBlocksPerPane: 500
    }),

    getters: {
        blocksOf: (state) => {
            return (paneId: PaneId): CommandBlockModel[] => {
                const bucket = state.byPane[paneId];
                if (!bucket) {
                    return [];
                }
                const list: CommandBlockModel[] = [];
                for (const id of bucket.order) {
                    const b = bucket.blocks[id];
                    if (b) {
                        list.push(b);
                    }
                }
                return list;
            };
        },
        lastBlock: (state) => {
            return (paneId: PaneId): CommandBlockModel | null => {
                const bucket = state.byPane[paneId];
                if (!bucket || bucket.order.length === 0) {
                    return null;
                }
                const lastId = bucket.order[bucket.order.length - 1]!;
                return bucket.blocks[lastId] ?? null;
            };
        },
        runningBlock: (state) => {
            return (paneId: PaneId): CommandBlockModel | null => {
                const bucket = state.byPane[paneId];
                if (!bucket) {
                    return null;
                }
                for (let i = bucket.order.length - 1; i >= 0; i--) {
                    const b = bucket.blocks[bucket.order[i]!];
                    if (b && b.status === 'running') {
                        return b;
                    }
                }
                return null;
            };
        },
        feed(state): CommandBlockFeed {
            return {
                begin: (paneId, init) => {
                    beginBlockOnState(state, paneId, init);
                },
                markOutputStart: (paneId, id, line) => {
                    markBlockOutputStartOnState(state, paneId, id, line);
                },
                end: (paneId, id, result) => {
                    endBlockOnState(state, paneId, id, result);
                },
                patchMeta: (paneId, id, meta) => {
                    patchBlockMetaOnState(state, paneId, id, meta);
                },
                clear: (paneId) => {
                    clearPaneBlocksOnState(state, paneId);
                }
            };
        }
    },

    actions: {
        ensureBucket(paneId: PaneId): PaneBlocksBucket {
            return ensureBucketOnState(this, paneId);
        },

        beginBlock(
            paneId: PaneId,
            init: {
                id: CommandBlockId;
                command: string;
                cwd: string | null;
                startedAt: number;
                line?: number | undefined;
            }
        ): void {
            beginBlockOnState(this, paneId, init);
        },

        markBlockOutputStart(paneId: PaneId, id: CommandBlockId, line?: number): void {
            markBlockOutputStartOnState(this, paneId, id, line);
        },

        endBlock(
            paneId: PaneId,
            id: CommandBlockId,
            result: { exitCode: number | null; finishedAt: number; line?: number | undefined }
        ): void {
            endBlockOnState(this, paneId, id, result);
        },

        patchBlockMeta(paneId: PaneId, id: CommandBlockId, meta: Record<string, unknown>): void {
            patchBlockMetaOnState(this, paneId, id, meta);
        },

        clearPaneBlocks(paneId: PaneId): void {
            clearPaneBlocksOnState(this, paneId);
        },

        pruneEvictedBlock(paneId: PaneId, blockId: CommandBlockId): void {
            pruneEvictedBlockOnState(this, paneId, blockId);
        },

        pruneEvicted(paneId: PaneId, firstValidLine = 0): void {
            const bucket = this.byPane[paneId];
            if (!bucket) {
                return;
            }
            const paneMarkers = markersByPane.get(paneId);
            const toRemove: CommandBlockId[] = [];

            for (const id of bucket.order) {
                const block = bucket.blocks[id];
                const rec = paneMarkers?.get(id);
                if (!block) {
                    toRemove.push(id);
                    continue;
                }
                if (rec?.promptMarker) {
                    if (rec.promptMarker.isDisposed || rec.promptMarker.line < firstValidLine) {
                        toRemove.push(id);
                    }
                } else if (block.promptLine < firstValidLine) {
                    toRemove.push(id);
                }
            }

            for (const id of toRemove) {
                pruneEvictedBlockOnState(this, paneId, id);
            }
        }
    }
});

function ensureBucketOnState(state: CommandBlocksStoreState, paneId: PaneId): PaneBlocksBucket {
    let bucket = state.byPane[paneId];
    if (!bucket) {
        bucket = {
            order: [],
            blocks: {}
        };
        state.byPane[paneId] = bucket;
    }
    return bucket;
}

function pruneEvictedBlockOnState(
    state: CommandBlocksStoreState,
    paneId: PaneId,
    blockId: CommandBlockId
): void {
    const bucket = state.byPane[paneId];
    if (bucket && bucket.blocks[blockId]) {
        delete bucket.blocks[blockId];
        const idx = bucket.order.indexOf(blockId);
        if (idx !== -1) {
            bucket.order.splice(idx, 1);
        }
    }
    const paneMarkers = markersByPane.get(paneId);
    if (paneMarkers) {
        const rec = paneMarkers.get(blockId);
        paneMarkers.delete(blockId);
        disposeMarkerRecord(rec);
    }
}

function beginBlockOnState(
    state: CommandBlocksStoreState,
    paneId: PaneId,
    init: {
        id: CommandBlockId;
        command: string;
        cwd: string | null;
        startedAt: number;
        line?: number | undefined;
    }
): void {
    const bucket = ensureBucketOnState(state, paneId);
    const existing = bucket.blocks[init.id];
    if (existing) {
        if (!existing.command && init.command) {
            existing.command = init.command;
        }
        if (existing.cwd === null && init.cwd !== null) {
            existing.cwd = init.cwd;
        }
        return;
    }

    const { line: promptLine, marker: promptMarker } = resolveCurrentNormalLine(
        paneId,
        init.line
    );

    const block: CommandBlockModel = {
        id: init.id,
        paneId,
        command: init.command,
        cwd: init.cwd,
        startedAt: init.startedAt,
        finishedAt: null,
        exitCode: null,
        status: 'running',
        promptLine,
        outputStartLine: null,
        outputEndLine: null,
        meta: Object.freeze({})
    };

    bucket.blocks[init.id] = block;
    bucket.order.push(init.id);

    const paneMarkers = ensureMarkersMap(paneId);
    const markerRecord: BlockMarkersRecord = {
        promptMarker,
        outputStartMarker: null,
        outputEndMarker: null,
        partialTrimmed: false
    };
    paneMarkers.set(init.id, markerRecord);

    if (promptMarker) {
        promptMarker.onDispose(() => {
            pruneEvictedBlockOnState(state, paneId, init.id);
        });
    }

    while (bucket.order.length > state.maxBlocksPerPane) {
        const oldestId = bucket.order.shift();
        if (oldestId !== undefined) {
            delete bucket.blocks[oldestId];
            const oldRec = paneMarkers.get(oldestId);
            paneMarkers.delete(oldestId);
            disposeMarkerRecord(oldRec);
        }
    }
}

function markBlockOutputStartOnState(
    state: CommandBlocksStoreState,
    paneId: PaneId,
    id: CommandBlockId,
    line?: number
): void {
    const bucket = state.byPane[paneId];
    const block = bucket?.blocks[id];
    if (!block) {
        return;
    }

    const { line: outStartLine, marker: outStartMarker } = resolveCurrentNormalLine(
        paneId,
        line
    );
    block.outputStartLine = outStartLine;

    const rec = ensureMarkersMap(paneId).get(id);
    if (rec) {
        rec.outputStartMarker = outStartMarker;
        if (outStartMarker) {
            outStartMarker.onDispose(() => {
                rec.partialTrimmed = true;
            });
        }
    }
}

function endBlockOnState(
    state: CommandBlocksStoreState,
    paneId: PaneId,
    id: CommandBlockId,
    result: { exitCode: number | null; finishedAt: number; line?: number | undefined }
): void {
    const bucket = state.byPane[paneId];
    const block = bucket?.blocks[id];
    if (!block) {
        console.warn(`[commandBlocks] end ignorado para bloco inexistente "${id}" no painel "${paneId}".`);
        return;
    }
    if (block.status !== 'running') {
        return;
    }

    let status: CommandBlockStatus;
    if (result.exitCode === 0) {
        status = 'success';
    } else if (result.exitCode !== null && result.exitCode !== 0) {
        status = 'error';
    } else {
        status = 'aborted';
    }

    const { line: outEndLine, marker: outEndMarker } = resolveCurrentNormalLine(
        paneId,
        result.line
    );

    block.status = status;
    block.exitCode = result.exitCode;
    block.finishedAt = result.finishedAt;
    block.outputEndLine = outEndLine;
    if (block.outputStartLine === null) {
        block.outputStartLine = block.promptLine;
    }

    const rec = ensureMarkersMap(paneId).get(id);
    if (rec) {
        rec.outputEndMarker = outEndMarker;
    }
}

function patchBlockMetaOnState(
    state: CommandBlocksStoreState,
    paneId: PaneId,
    id: CommandBlockId,
    meta: Record<string, unknown>
): void {
    const bucket = state.byPane[paneId];
    const block = bucket?.blocks[id];
    if (!block) {
        return;
    }
    block.meta = Object.freeze({
        ...block.meta,
        ...meta
    });
}

function clearPaneBlocksOnState(state: CommandBlocksStoreState, paneId: PaneId): void {
    const paneMarkers = markersByPane.get(paneId);
    if (paneMarkers) {
        for (const rec of paneMarkers.values()) {
            disposeMarkerRecord(rec);
        }
        paneMarkers.clear();
        markersByPane.delete(paneId);
    }
    state.byPane[paneId] = {
        order: [],
        blocks: {}
    };
}
