import { beforeEach, describe, expect, it, vi } from 'vitest';
import { effectScope, ref } from 'vue';
import { createPinia, setActivePinia } from 'pinia';
import { useCommandBlocks } from '@/composables/terminal/useCommandBlocks';
import { useCommandBlocksStore } from '@/stores/commandBlocks';
import * as vwtModule from '@/terminal/blocks/vueWebTerminalAdapter';

const dummyCtx = {
    paneId: 'pane-1',
    ptyId: 'pty-1',
    projectId: null,
    cwd: '/app',
    extra: Object.freeze({})
};

describe('useCommandBlocks', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        vi.restoreAllMocks();
    });

    it('instancia exatamente 1 VueWebTerminalAdapter por painel, projeta entradas do CommandBlockFeed e descarta ao sair do escopo', () => {
        const createSpy = vi.spyOn(vwtModule, 'createVueWebTerminalAdapter');
        const store = useCommandBlocksStore();
        const paneIdRef = ref('pane-1');

        const scope = effectScope();
        const composable = scope.run(() =>
            useCommandBlocks(paneIdRef, {
                getContext: () => dummyCtx
            })
        )!;

        expect(createSpy).toHaveBeenCalledTimes(1);
        const firstAdapter = composable.adapter;

        store.feed.begin('pane-1', {
            id: 'blk-1',
            command: 'echo "<b>oi</b>"',
            cwd: '/app',
            startedAt: 100
        });
        store.feed.end('pane-1', 'blk-1', {
            exitCode: 0,
            finishedAt: 220
        });

        expect(composable.projectedEntries.value).toHaveLength(1);
        expect(composable.projectedEntries.value[0]?.blockId).toBe('blk-1');
        expect(composable.projectedEntries.value[0]?.status).toBe('success');
        expect(composable.projectedEntries.value[0]?.content.includes('<b>')).toBe(false);

        // Trocar paneId descarta o adaptador anterior e cria um novo.
        paneIdRef.value = 'pane-2';
        // Força flush síncrono do watch ou leitura.
        scope.stop();
        expect(firstAdapter.isDisposed()).toBe(true);
    });
});
