<template>
    <div v-if="!isAltScreen" class="command-block-layer" aria-label="Camada de blocos de comando">
        <template v-for="pair in visibleBlockPairs" :key="pair.block.id">
            <Teleport v-if="pair.decorationHost" :to="pair.decorationHost">
                <CommandBlock :block="pair.block" :entry="pair.entry" :actions="actionsFor(pair.block)" @action="onBlockAction($event, pair.block)">
                    <template #actions="{ block }">
                        <slot name="block-actions" :block="block" />
                    </template>
                </CommandBlock>
            </Teleport>
            <div v-else class="fallback-block-anchor" :data-line="pair.line">
                <CommandBlock :block="pair.block" :entry="pair.entry" :actions="actionsFor(pair.block)" @action="onBlockAction($event, pair.block)">
                    <template #actions="{ block }">
                        <slot name="block-actions" :block="block" />
                    </template>
                </CommandBlock>
            </div>
        </template>
    </div>
</template>

<script setup lang="ts">
    import { computed, onBeforeUnmount, onMounted, ref, toRef, watch } from 'vue';
    import type { IDecoration, IDisposable } from '@xterm/xterm';
    import type { CommandBlockId, PaneId } from '@/types/bindings';
    import type { CommandBlockModel, TerminalContext } from '@/types/terminal';
    import { useCommandBlocks } from '@/composables/terminal/useCommandBlocks';
    import { getBlockMarkers } from '@/stores/commandBlocks';
    import { useTerminalStore } from '@/stores/terminal';
    import type { VueWebTerminalProjectedEntry } from '@/terminal/blocks/vueWebTerminalAdapter';
    import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
    import CommandBlock from './CommandBlock.vue';

    export interface CommandBlockLayerProps {
        paneId: PaneId;
        getContext: () => TerminalContext;
        onInputIntent: (commandText: string) => Promise<void>;
    }

    const props = defineProps<CommandBlockLayerProps>();
    const paneIdRef = toRef(props, 'paneId');
    const terminalStore = useTerminalStore();

    const { blocks, projectedEntries, actionsFor, runAction, getMarkerLine } = useCommandBlocks(
        paneIdRef,
        {
            getContext: props.getContext,
            onInputIntent: props.onInputIntent
        }
    );

    const isAltScreen = computed<boolean>(() => {
        return Boolean(terminalStore.instance(props.paneId)?.modes.altScreen);
    });

    const viewportVersion = ref<number>(0);
    const activeDecorations = new Map<CommandBlockId, { decoration: IDecoration; element: HTMLElement | null }>();
    let renderSub: IDisposable | null = null;

    function syncDecorationsWithViewport(): void {
        const instance = terminalInstanceRegistry.get(props.paneId);
        const xterm = instance?.xterm;
        if (!xterm || isAltScreen.value) {
            disposeAllDecorations();
            viewportVersion.value += 1;
            return;
        }

        const normal = xterm.buffer?.normal;
        const viewportY = normal?.viewportY ?? 0;
        const rows = xterm.rows || 24;
        const minLine = Math.max(0, viewportY - rows);
        const maxLine = viewportY + rows * 2;

        const visibleIds = new Set<CommandBlockId>();
        for (const block of blocks.value) {
            const line = getMarkerLine(block.id);
            if (line >= minLine && line <= maxLine) {
                visibleIds.add(block.id);
                if (!activeDecorations.has(block.id) && xterm.registerDecoration) {
                    const markerRec = getBlockMarkers(props.paneId, block.id);
                    const marker = markerRec?.promptMarker;
                    if (marker && !marker.isDisposed) {
                        const dec = xterm.registerDecoration({
                            marker,
                            width: xterm.cols
                        });
                        if (dec) {
                            const entryRecord = {
                                decoration: dec,
                                element: dec.element ?? null
                            };
                            activeDecorations.set(block.id, entryRecord);
                            dec.onRender((el) => {
                                el.style.pointerEvents = 'none';
                                entryRecord.element = el;
                                viewportVersion.value += 1;
                            });
                            dec.onDispose(() => {
                                activeDecorations.delete(block.id);
                                viewportVersion.value += 1;
                            });
                        }
                    }
                }
            }
        }

        // Remove decorações fora da janela visível (viewport ± 1 tela).
        for (const [id, rec] of activeDecorations.entries()) {
            if (!visibleIds.has(id)) {
                activeDecorations.delete(id);
                try {
                    rec.decoration.dispose();
                } catch {
                    // Ignora erro de dispose.
                }
            }
        }

        viewportVersion.value += 1;
    }

    function disposeAllDecorations(): void {
        for (const rec of activeDecorations.values()) {
            try {
                rec.decoration.dispose();
            } catch {
                // Ignora erro de dispose.
            }
        }
        activeDecorations.clear();
    }

    const visibleBlockPairs = computed(() => {
        // Dependência reativa de viewportVersion para atualizar em xterm.onRender.
        void viewportVersion.value;
        if (isAltScreen.value) {
            return [];
        }

        const instance = terminalInstanceRegistry.get(props.paneId);
        const xterm = instance?.xterm;
        const normal = xterm?.buffer?.normal;
        const viewportY = normal?.viewportY ?? 0;
        const rows = xterm?.rows ?? 24;
        const minLine = Math.max(0, viewportY - rows);
        const maxLine = viewportY + rows * 2;

        const entryById = new Map<CommandBlockId, VueWebTerminalProjectedEntry>();
        for (const entry of projectedEntries.value) {
            entryById.set(entry.blockId, entry);
        }

        const pairs: Array<{
            block: CommandBlockModel;
            entry: VueWebTerminalProjectedEntry;
            line: number;
            decorationHost: HTMLElement | null;
        }> = [];

        for (const block of blocks.value) {
            const entry = entryById.get(block.id);
            if (!entry) {
                continue;
            }
            const line = getMarkerLine(block.id);
            if (line < minLine || line > maxLine) {
                continue;
            }
            const decRec = activeDecorations.get(block.id);
            pairs.push({
                block,
                entry,
                line,
                decorationHost: decRec?.element ?? null
            });
        }

        return pairs;
    });

    function onBlockAction(actionId: string, block: CommandBlockModel): void {
        void runAction(actionId, block).catch(() => {});
    }

    watch(
        [blocks, isAltScreen],
        () => {
            syncDecorationsWithViewport();
        },
        { deep: true }
    );

    onMounted(() => {
        const instance = terminalInstanceRegistry.get(props.paneId);
        if (instance?.xterm?.onRender) {
            renderSub = instance.xterm.onRender(() => {
                syncDecorationsWithViewport();
            });
        }
        syncDecorationsWithViewport();
    });

    onBeforeUnmount(() => {
        renderSub?.dispose();
        renderSub = null;
        disposeAllDecorations();
    });
</script>

<style lang="scss" scoped>
    .command-block-layer {
        position: absolute;
        inset: 0;
        pointer-events: none;
        z-index: 10;
        display: flex;
        flex-direction: column;
        gap: 6px;
        padding: 6px 8px;
        overflow: hidden;
    }

    .fallback-block-anchor {
        pointer-events: none;
        width: 100%;
    }
</style>
