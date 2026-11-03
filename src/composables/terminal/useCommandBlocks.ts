import {
    computed,
    isRef,
    onScopeDispose,
    unref,
    watch,
    type ComputedRef,
    type Ref
} from 'vue';
import type { CommandBlockId, PaneId } from '@/types/bindings';
import type { BlockAction, CommandBlockModel, TerminalContext } from '@/types/terminal';
import { getBlockMarkers, useCommandBlocksStore } from '@/stores/commandBlocks';
import {
    executeBlockAction,
    getBlockActionsFor
} from '@/terminal/blocks/blockActionRegistry';
import {
    createVueWebTerminalAdapter,
    type VueWebTerminalAdapter,
    type VueWebTerminalProjectedEntry
} from '@/terminal/blocks/vueWebTerminalAdapter';

/** Opções de inicialização do composable `useCommandBlocks`. */
export interface UseCommandBlocksOptions {
    getContext: () => TerminalContext;
    onInputIntent?: ((commandText: string) => Promise<void>) | undefined;
}

/** Retorno do composable `useCommandBlocks`. */
export interface UseCommandBlocksReturn {
    readonly adapter: VueWebTerminalAdapter;
    blocks: ComputedRef<CommandBlockModel[]>;
    /** Projeção de `blocks` (mesma ordem) feita por `adapter.projectBlocks(...)`. */
    projectedEntries: ComputedRef<VueWebTerminalProjectedEntry[]>;
    runningBlock: ComputedRef<CommandBlockModel | null>;
    lastBlock: ComputedRef<CommandBlockModel | null>;
    actionsFor: (block: CommandBlockModel) => BlockAction[];
    runAction: (actionId: string, block: CommandBlockModel) => Promise<void>;
    clearBlocks: () => void;
    getMarkerLine: (blockId: CommandBlockId) => number;
}

/**
 * Composable único que instancia `VueWebTerminalAdapter` para um painel,
 * projetando os blocos do `useCommandBlocksStore` em `projectedEntries`.
 */
export function useCommandBlocks(
    paneId: Ref<PaneId> | PaneId,
    options: UseCommandBlocksOptions
): UseCommandBlocksReturn {
    const store = useCommandBlocksStore();

    let currentAdapter = createVueWebTerminalAdapter({
        paneId: unref(paneId),
        getContext: options.getContext,
        onInputIntent: options.onInputIntent
    });

    if (isRef(paneId)) {
        watch(paneId, (nextPaneId, prevPaneId) => {
            if (nextPaneId !== prevPaneId) {
                currentAdapter.dispose();
                currentAdapter = createVueWebTerminalAdapter({
                    paneId: nextPaneId,
                    getContext: options.getContext,
                    onInputIntent: options.onInputIntent
                });
            }
        });
    }

    onScopeDispose(() => {
        currentAdapter.dispose();
    });

    const blocks = computed<CommandBlockModel[]>(() => store.blocksOf(unref(paneId)));
    const projectedEntries = computed<VueWebTerminalProjectedEntry[]>(() => {
        return currentAdapter.projectBlocks(blocks.value);
    });
    const runningBlock = computed<CommandBlockModel | null>(() => store.runningBlock(unref(paneId)));
    const lastBlock = computed<CommandBlockModel | null>(() => store.lastBlock(unref(paneId)));

    function actionsFor(block: CommandBlockModel): BlockAction[] {
        return getBlockActionsFor(block);
    }

    async function runAction(actionId: string, block: CommandBlockModel): Promise<void> {
        await executeBlockAction(actionId, block, options.getContext());
    }

    function clearBlocks(): void {
        store.feed.clear(unref(paneId));
    }

    function getMarkerLine(blockId: CommandBlockId): number {
        const currentPaneId = unref(paneId);
        const rec = getBlockMarkers(currentPaneId, blockId);
        if (rec?.promptMarker && !rec.promptMarker.isDisposed && rec.promptMarker.line >= 0) {
            return rec.promptMarker.line;
        }
        const block = store.byPane[currentPaneId]?.blocks[blockId];
        return block?.promptLine ?? 0;
    }

    return {
        get adapter(): VueWebTerminalAdapter {
            return currentAdapter;
        },
        blocks,
        projectedEntries,
        runningBlock,
        lastBlock,
        actionsFor,
        runAction,
        clearBlocks,
        getMarkerLine
    };
}
