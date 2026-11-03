<template>
    <div class="pane-layout">
        <template v-if="props.node.kind === 'leaf'">
            <PaneView v-if="leafPane" :tab-id="props.tabId" :pane="leafPane" :focused="leafPane.id === props.activePaneId" :visible="props.visible" :can-split="canSplitLeaf" @focus="emit('focus-pane', $event)" @split="(pid, dir) => emit('split-pane', pid, dir)" @close="emit('close-pane', $event)" @restart="emit('restart-pane', $event)" @ready-for-initial-command="(tid, pid, gen) => emit('ready-for-initial-command', tid, pid, gen)" />
        </template>
        <Splitpanes v-else class="pane-layout__splitpanes default-theme" :horizontal="props.node.direction === 'horizontal'" :maximize-panes="false" @resized="onSplitResized">
            <Pane v-for="(child, index) in props.node.children" :key="childKey(child, index)" :size="normalizedSizes[index] ?? 50" :min-size="5">
                <PaneLayout :tab-id="props.tabId" :root-layout="props.rootLayout" :node="child" :panes-by-id="props.panesById" :active-pane-id="props.activePaneId" :visible="props.visible" @focus-pane="emit('focus-pane', $event)" @split-pane="(pid, dir) => emit('split-pane', pid, dir)" @close-pane="emit('close-pane', $event)" @restart-pane="emit('restart-pane', $event)" @resize-node="(nid, sz) => emit('resize-node', nid, sz)" @ready-for-initial-command="(tid, pid, gen) => emit('ready-for-initial-command', tid, pid, gen)" />
            </Pane>
        </Splitpanes>
    </div>
</template>

<script setup lang="ts">
    import { Pane, Splitpanes } from 'splitpanes';
    import { computed } from 'vue';
    import { canSplitPane, normalizeSplitSizes } from '@/composables/workspace/layoutTree';
    import type { PaneId, PaneLayoutNode, PaneView_Serialize, SplitDirection, TabId } from '@/types/bindings';
    import PaneView from './PaneView.vue';

    defineOptions({
        name: 'PaneLayout'
    });

    export interface PaneLayoutProps {
        tabId: TabId;
        rootLayout: PaneLayoutNode;
        node: PaneLayoutNode;
        panesById: Record<PaneId, PaneView_Serialize>;
        activePaneId: PaneId;
        visible?: boolean;
    }

    const props = withDefaults(defineProps<PaneLayoutProps>(), {
        visible: true
    });

    const emit = defineEmits<{
        'focus-pane': [paneId: PaneId];
        'split-pane': [paneId: PaneId, direction: SplitDirection];
        'close-pane': [paneId: PaneId];
        'restart-pane': [paneId: PaneId];
        'resize-node': [nodeId: string, sizes: [number, number]];
        'ready-for-initial-command': [tabId: TabId, paneId: PaneId, generation: number];
    }>();

    const leafPane = computed<PaneView_Serialize | null>(() => {
        if (props.node.kind !== 'leaf') {
            return null;
        }
        return props.panesById[props.node.paneId] ?? null;
    });

    const canSplitLeaf = computed<boolean>(() => {
        if (props.node.kind !== 'leaf') {
            return false;
        }
        return canSplitPane(props.rootLayout, props.node.paneId);
    });

    const normalizedSizes = computed<[number, number]>(() => {
        if (props.node.kind !== 'split') {
            return [50, 50];
        }
        return normalizeSplitSizes(props.node.sizes);
    });

    function childKey(child: PaneLayoutNode, index: number): string {
        if (child.kind === 'leaf') {
            return `leaf:${child.paneId}`;
        }
        return `split:${child.nodeId}:${index}`;
    }

    function onSplitResized(payload: unknown): void {
        if (props.node.kind !== 'split' || !payload || typeof payload !== 'object') {
            return;
        }
        const panes = Array.isArray(payload)
            ? payload
            : 'panes' in payload && Array.isArray((payload as { panes?: unknown }).panes)
                ? (payload as { panes: Array<{ size?: number }> }).panes
                : [];
        if (panes.length < 2) {
            return;
        }
        const first = typeof panes[0]?.size === 'number' ? panes[0].size : 50;
        const second = typeof panes[1]?.size === 'number' ? panes[1].size : 50;
        emit('resize-node', props.node.nodeId, [first, second]);
    }
</script>

<style lang="scss" scoped>
    .pane-layout {
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
        display: flex;
        overflow: hidden;
    }

    :deep(.pane-layout__splitpanes.splitpanes) {
        width: 100%;
        height: 100%;
        background: var(--background-950);
    }

    :deep(.pane-layout__splitpanes .splitpanes__pane) {
        min-width: 0;
        min-height: 0;
        overflow: hidden;
        background: transparent;
    }

    :deep(.pane-layout__splitpanes > .splitpanes__splitter) {
        background: var(--background-700);
        border: none;
        transition: background-color 120ms ease;

        &:hover {
            background: var(--max-primary-500);
        }
    }

    :deep(.pane-layout__splitpanes.splitpanes--vertical > .splitpanes__splitter) {
        width: 4px;
    }

    :deep(.pane-layout__splitpanes.splitpanes--horizontal > .splitpanes__splitter) {
        height: 4px;
    }
</style>
