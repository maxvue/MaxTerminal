import { describe, expect, it } from 'vitest';
import {
    canSplitPane,
    collectLeafPaneIds,
    computeTreeDepth,
    countLeafPanes,
    findLeafDepth,
    MAX_PANES_PER_TAB,
    MAX_SPLIT_DEPTH,
    normalizeSplitSizes
} from '@/composables/workspace/layoutTree';
import type { PaneLayoutNode } from '@/types/bindings';

describe('layoutTree (Fase 07)', () => {
    it('conta folhas, coleta IDs e calcula profundidade de uma folha simples', () => {
        const leaf: PaneLayoutNode = {
            kind: 'leaf',
            paneId: 'p1'
        };
        expect(countLeafPanes(leaf)).toBe(1);
        expect(collectLeafPaneIds(leaf)).toEqual(['p1']);
        expect(computeTreeDepth(leaf)).toBe(0);
        expect(findLeafDepth(leaf, 'p1')).toBe(0);
        expect(canSplitPane(leaf, 'p1')).toBe(true);
    });

    it('navega árvore binária dividida em múltiplos níveis', () => {
        const tree: PaneLayoutNode = {
            kind: 'split',
            nodeId: 's1',
            direction: 'vertical',
            sizes: [60, 40],
            children: [
                { kind: 'leaf', paneId: 'p1' },
                {
                    kind: 'split',
                    nodeId: 's2',
                    direction: 'horizontal',
                    sizes: [50, 50],
                    children: [
                        { kind: 'leaf', paneId: 'p2' },
                        { kind: 'leaf', paneId: 'p3' }
                    ]
                }
            ]
        };

        expect(countLeafPanes(tree)).toBe(3);
        expect(collectLeafPaneIds(tree)).toEqual(['p1', 'p2', 'p3']);
        expect(computeTreeDepth(tree)).toBe(2);
        expect(findLeafDepth(tree, 'p1')).toBe(1);
        expect(findLeafDepth(tree, 'p3')).toBe(2);
        expect(canSplitPane(tree, 'p3')).toBe(true);
    });

    it('bloqueia divisão quando profundidade atinge MAX_SPLIT_DEPTH (4) ou MAX_PANES_PER_TAB (8)', () => {
        let deep: PaneLayoutNode = { kind: 'leaf', paneId: 'p-deep' };
        for (let i = 1; i <= MAX_SPLIT_DEPTH; i += 1) {
            deep = {
                kind: 'split',
                nodeId: `node-${i}`,
                direction: 'vertical',
                sizes: [50, 50],
                children: [deep, { kind: 'leaf', paneId: `p-sibling-${i}` }]
            };
        }

        expect(findLeafDepth(deep, 'p-deep')).toBe(MAX_SPLIT_DEPTH);
        expect(canSplitPane(deep, 'p-deep')).toBe(false);

        const eightLeaves: PaneLayoutNode = {
            kind: 'split',
            nodeId: 'root',
            direction: 'vertical',
            sizes: [50, 50],
            children: [
                {
                    kind: 'split',
                    nodeId: 'l',
                    direction: 'horizontal',
                    sizes: [50, 50],
                    children: [
                        {
                            kind: 'split',
                            nodeId: 'll',
                            direction: 'vertical',
                            sizes: [50, 50],
                            children: [
                                { kind: 'leaf', paneId: 'p1' },
                                { kind: 'leaf', paneId: 'p2' }
                            ]
                        },
                        {
                            kind: 'split',
                            nodeId: 'lr',
                            direction: 'vertical',
                            sizes: [50, 50],
                            children: [
                                { kind: 'leaf', paneId: 'p3' },
                                { kind: 'leaf', paneId: 'p4' }
                            ]
                        }
                    ]
                },
                {
                    kind: 'split',
                    nodeId: 'r',
                    direction: 'horizontal',
                    sizes: [50, 50],
                    children: [
                        {
                            kind: 'split',
                            nodeId: 'rl',
                            direction: 'vertical',
                            sizes: [50, 50],
                            children: [
                                { kind: 'leaf', paneId: 'p5' },
                                { kind: 'leaf', paneId: 'p6' }
                            ]
                        },
                        {
                            kind: 'split',
                            nodeId: 'rr',
                            direction: 'vertical',
                            sizes: [50, 50],
                            children: [
                                { kind: 'leaf', paneId: 'p7' },
                                { kind: 'leaf', paneId: 'p8' }
                            ]
                        }
                    ]
                }
            ]
        };

        expect(countLeafPanes(eightLeaves)).toBe(MAX_PANES_PER_TAB);
        expect(canSplitPane(eightLeaves, 'p1')).toBe(false);
    });

    it('normaliza proporções de splitters garantindo mínimo de 5% e soma 100%', () => {
        expect(normalizeSplitSizes([70, 30])).toEqual([70, 30]);
        expect(normalizeSplitSizes([2, 98])).toEqual([50, 50]);
        expect(normalizeSplitSizes([null, null])).toEqual([50, 50]);
    });
});
