import { beforeEach, describe, expect, it } from 'vitest';
import { createPinia, setActivePinia } from 'pinia';
import type { Terminal } from '@xterm/xterm';
import { getBlockMarkers, useCommandBlocksStore } from '@/stores/commandBlocks';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { createMaxNavyTealTheme } from '@/terminal/theme/defaultTheme';
import { DEFAULT_FLOW_CONFIG, DEFAULT_TERMINAL_PREFERENCES } from '@/types/terminal';
import { MockXtermTerminal } from './mocks/xtermMock';

describe('useCommandBlocksStore (CommandBlockFeed)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        terminalInstanceRegistry.disposeAll();
    });

    it('processa begin, markOutputStart, end (0 -> success, !=0 -> error, null -> aborted), patchMeta e descarta via IMarker.onDispose', () => {
        const mockTerm = new MockXtermTerminal();
        terminalInstanceRegistry.getOrCreate({
            paneId: 'pane-blk',
            ptyId: 'pty-blk',
            prefs: DEFAULT_TERMINAL_PREFERENCES,
            theme: createMaxNavyTealTheme(),
            flow: DEFAULT_FLOW_CONFIG,
            createTerminal: () => mockTerm as unknown as Terminal,
            createWebglAddon: () => ({
                onContextLoss: () => ({ dispose: () => {} }),
                dispose: () => {}
            })
        });

        const store = useCommandBlocksStore();

        // 1. Bloco com sucesso (exitCode = 0) + idempotência de begin.
        store.feed.begin('pane-blk', {
            id: 'b1',
            command: 'npm test',
            cwd: '/app',
            startedAt: 1000,
            line: 2
        });
        store.feed.begin('pane-blk', {
            id: 'b1',
            command: 'npm test',
            cwd: '/app',
            startedAt: 1000,
            line: 2
        });
        expect(store.blocksOf('pane-blk')).toHaveLength(1);

        store.feed.markOutputStart('pane-blk', 'b1', 3);
        store.feed.end('pane-blk', 'b1', { exitCode: 0, finishedAt: 1500, line: 10 });
        expect(store.blocksOf('pane-blk')[0]?.status).toBe('success');

        // 2. Bloco com erro (exitCode = 2).
        store.feed.begin('pane-blk', {
            id: 'b2',
            command: 'false',
            cwd: '/app',
            startedAt: 2000,
            line: 11
        });
        store.feed.end('pane-blk', 'b2', { exitCode: 2, finishedAt: 2100, line: 12 });
        expect(store.lastBlock('pane-blk')?.status).toBe('error');

        // 3. Bloco interrompido (exitCode = null).
        store.feed.begin('pane-blk', {
            id: 'b3',
            command: 'sleep 60',
            cwd: '/app',
            startedAt: 3000,
            line: 13
        });
        store.feed.patchMeta('pane-blk', 'b3', { signal: 'SIGINT' });
        store.feed.end('pane-blk', 'b3', { exitCode: null, finishedAt: 3200, line: 14 });
        expect(store.lastBlock('pane-blk')?.status).toBe('aborted');
        expect(store.lastBlock('pane-blk')?.meta).toEqual({ signal: 'SIGINT' });

        // 4. Quando o IMarker de b1 sofre dispose (trim do scrollback), b1 é removido automaticamente.
        const markersB1 = getBlockMarkers('pane-blk', 'b1');
        expect(markersB1?.promptMarker).not.toBeNull();
        markersB1!.promptMarker!.dispose();
        expect(store.blocksOf('pane-blk').map((b) => b.id)).toEqual(['b2', 'b3']);

        // 5. Limite FIFO de maxBlocksPerPane.
        store.maxBlocksPerPane = 2;
        store.beginBlock('pane-blk', {
            id: 'b4',
            command: 'pwd',
            cwd: '/app',
            startedAt: 4000,
            line: 15
        });
        expect(store.runningBlock('pane-blk')?.id).toBe('b4');
        store.markBlockOutputStart('pane-blk', 'b4', 15);
        store.patchBlockMeta('pane-blk', 'b4', { tag: 'ok' });
        store.endBlock('pane-blk', 'b4', { exitCode: 0, finishedAt: 4100, line: 16 });
        expect(store.blocksOf('pane-blk').map((b) => b.id)).toEqual(['b3', 'b4']);

        // 6. pruneEvicted e clearPaneBlocks.
        store.pruneEvicted('pane-blk', 14);
        expect(store.blocksOf('pane-blk').map((b) => b.id)).toEqual(['b4']);
        store.clearPaneBlocks('pane-blk');
        expect(store.blocksOf('pane-blk')).toEqual([]);
    });
});
