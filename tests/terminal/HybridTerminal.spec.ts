import { beforeEach, describe, expect, it, vi } from 'vitest';
import { flushPromises, mount } from '@vue/test-utils';
import { createPinia, setActivePinia } from 'pinia';
import type { Channel } from '@tauri-apps/api/core';
import type { Terminal } from '@xterm/xterm';
import HybridTerminal from '@/components/terminal/HybridTerminal.vue';
import { useCommandBlocksStore } from '@/stores/commandBlocks';
import { useTerminalStore } from '@/stores/terminal';
import { terminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import { ptyClient } from '@/terminal/stream/ptyClient';
import { ptyStreamHub } from '@/terminal/stream/ptyStreamHub';
import { createMaxNavyTealTheme } from '@/terminal/theme/defaultTheme';
import { DEFAULT_FLOW_CONFIG, DEFAULT_TERMINAL_PREFERENCES, type TerminalTheme } from '@/types/terminal';
import { MockXtermTerminal } from './mocks/xtermMock';

describe('HybridTerminal.vue (Integração Completa Fase 04)', () => {
    beforeEach(() => {
        setActivePinia(createPinia());
        terminalInstanceRegistry.disposeAll();
        ptyStreamHub.resetForTests();
        vi.restoreAllMocks();

        vi.spyOn(ptyClient, 'onExit').mockResolvedValue(() => {});
        vi.spyOn(ptyClient, 'onCleanupFailed').mockResolvedValue(() => {});
        vi.spyOn(ptyClient, 'attach').mockResolvedValue({
            info: {
                attachmentId: 'att-hybrid',
                replayFrom: { nextSeq: '1', byteOffset: '0' },
                liveFrom: { nextSeq: '1', byteOffset: '0' },
                truncated: false
            },
            channel: {} as Channel<ArrayBuffer>
        });
        vi.spyOn(ptyClient, 'detach').mockResolvedValue();
        vi.spyOn(ptyClient, 'resize').mockResolvedValue();
        vi.spyOn(ptyClient, 'write').mockResolvedValue();
    });

    it('integra TerminalView, CommandBlockLayer (com VueWebTerminalAdapter), RichInput, SearchBar, GapBanner, ExitBanner e variáveis CSS de tema por painel', async () => {
        const terminalStore = useTerminalStore();
        const blocksStore = useCommandBlocksStore();

        const mockXterm1 = new MockXtermTerminal();
        const mockXterm2 = new MockXtermTerminal();

        const inst1 = terminalInstanceRegistry.getOrCreate({
            paneId: 'pane-h1',
            ptyId: 'pty-h1',
            prefs: DEFAULT_TERMINAL_PREFERENCES,
            theme: createMaxNavyTealTheme(),
            flow: DEFAULT_FLOW_CONFIG,
            createTerminal: () => mockXterm1 as unknown as Terminal,
            createWebglAddon: () => ({
                onContextLoss: () => ({ dispose: () => {} }),
                dispose: () => {}
            })
        });

        const customTheme2: TerminalTheme = {
            ...createMaxNavyTealTheme(),
            id: 'custom-2',
            ui: {
                blockBackground: '#112233',
                blockBorder: '#223344',
                blockHeaderBackground: '#334455',
                richInputBackground: '#445566',
                richInputBorder: '#556677'
            }
        };

        terminalInstanceRegistry.getOrCreate({
            paneId: 'pane-h2',
            ptyId: 'pty-h2',
            prefs: DEFAULT_TERMINAL_PREFERENCES,
            theme: customTheme2,
            flow: DEFAULT_FLOW_CONFIG,
            createTerminal: () => mockXterm2 as unknown as Terminal,
            createWebglAddon: () => ({
                onContextLoss: () => ({ dispose: () => {} }),
                dispose: () => {}
            })
        });

        const w1 = mount(HybridTerminal, {
            props: {
                paneId: 'pane-h1',
                ptyId: 'pty-h1',
                visible: true,
                richInput: true
            }
        });

        const w2 = mount(HybridTerminal, {
            props: {
                paneId: 'pane-h2',
                ptyId: 'pty-h2',
                visible: false,
                richInput: true
            }
        });

        await flushPromises();

        // Verifica que apenas 1 instância visível mantém WebGL ativo.
        expect(terminalStore.webglCount).toBe(1);

        // Verifica isolamento de variáveis CSS de tema por painel (.hybrid-terminal).
        expect(w2.attributes('style')).toContain('--mt-block-background: #112233');
        expect(w1.attributes('style')).not.toContain('#112233');

        // Aplica novo tema apenas no painel 1 e verifica reatividade local.
        inst1.applyTheme({
            ...createMaxNavyTealTheme(),
            ui: {
                blockBackground: '#abcdef',
                blockBorder: '#123456',
                blockHeaderBackground: '#654321',
                richInputBackground: '#fedcba',
                richInputBorder: '#010203'
            }
        });
        await w1.vm.$nextTick();
        expect(w1.attributes('style')).toContain('--mt-block-background: #abcdef');

        // Injeta um bloco no CommandBlockFeed e verifica renderização via CommandBlockLayer -> VueWebTerminalAdapter.
        blocksStore.feed.begin('pane-h1', {
            id: 'blk-live',
            command: 'cargo test',
            cwd: '/home/user/MaxTerminal',
            startedAt: 1000,
            line: 0
        });
        blocksStore.feed.end('pane-h1', 'blk-live', {
            exitCode: 0,
            finishedAt: 1350,
            line: 2
        });
        await w1.vm.$nextTick();
        expect(w1.find('.command-block').exists()).toBe(true);
        expect(w1.find('.command-block').text()).toContain('cargo test');

        // Atalho Ctrl+Shift+F abre a barra de busca.
        await w1.trigger('keydown', { key: 'F', ctrlKey: true, shiftKey: true });
        expect(terminalStore.instance('pane-h1')?.search.open).toBe(true);
        expect(w1.find('.terminal-search-bar').exists()).toBe(true);

        // Atalho Ctrl+Shift+I alterna o modo de input entre 'rich' e 'passthrough'.
        expect(w1.find('.rich-input-container').exists()).toBe(true);
        await w1.trigger('keydown', { key: 'I', ctrlKey: true, shiftKey: true });
        expect(terminalStore.instance('pane-h1')?.inputMode).toBe('passthrough');
        expect(w1.find('.rich-input-container').exists()).toBe(false);
        await w1.trigger('keydown', { key: 'I', ctrlKey: true, shiftKey: true });
        expect(terminalStore.instance('pane-h1')?.inputMode).toBe('rich');

        // Emite mudança de título e transição de altScreen pelo mockXterm1.
        mockXterm1.emitTitleChange('zsh ~/MaxTerminal');
        expect(w1.emitted('title-change')?.[0]).toEqual(['zsh ~/MaxTerminal']);

        mockXterm1.switchBuffer('alternate');
        await w1.vm.$nextTick();
        expect(terminalStore.instance('pane-h1')?.modes.altScreen).toBe(true);

        mockXterm1.switchBuffer('normal');
        await w1.vm.$nextTick();
        expect(terminalStore.instance('pane-h1')?.modes.altScreen).toBe(false);

        // Clique na viewport do terminal foca a instância.
        await w1.find('.terminal-view').trigger('mousedown');
        await w1.trigger('focusin');
        expect(w1.emitted('focus')).toBeTruthy();

        // Banner de Gap quando droppedBytes > 0.
        terminalStore.patch('pane-h1', {
            flow: {
                paused: false,
                pendingBytes: 0,
                droppedBytes: '4096',
                cursor: null
            }
        });
        await w1.vm.$nextTick();
        expect(w1.find('.hybrid-gap-banner').text()).toContain('4096 bytes descartados');

        // Banner de Exit e emissão de restart-requested.
        terminalStore.patch('pane-h1', {
            exit: {
                code: 0,
                signal: null,
                signalName: null,
                reason: 'exited',
                atMs: '1700000000000'
            }
        });
        await w1.vm.$nextTick();
        expect(w1.find('.terminal-exit-banner').exists()).toBe(true);
        await w1.find('.restart-btn').trigger('click');
        expect(w1.emitted('restart-requested')).toHaveLength(1);

        w1.unmount();
        w2.unmount();
    });
});
