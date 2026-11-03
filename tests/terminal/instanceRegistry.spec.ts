import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Channel } from '@tauri-apps/api/core';
import type { Terminal } from '@xterm/xterm';
import {
    DEFAULT_FLOW_CONFIG,
    DEFAULT_TERMINAL_PREFERENCES,
    MtTerminalError
} from '@/types/terminal';
import { createTerminalInstanceRegistry } from '@/terminal/engine/instanceRegistry';
import {
    clearContextProvidersForTests,
    registerContextProvider,
    resolveTerminalContext
} from '@/terminal/extensions/terminalContext';
import {
    clearTerminalExtensionsForTests,
    registerTerminalExtension
} from '@/terminal/extensions/terminalExtensions';
import { isCopyShortcut, isPasteShortcut } from '@/terminal/keybindings';
import {
    PtyFrameKind,
    ptyClient,
    type PtyFrame
} from '@/terminal/stream/ptyClient';
import { ptyStreamHub } from '@/terminal/stream/ptyStreamHub';
import { createMaxNavyTealTheme } from '@/terminal/theme/defaultTheme';
import { themeService } from '@/terminal/theme/themeService';
import { MockXtermTerminal } from './mocks/xtermMock';

describe('TerminalInstanceRegistry, Extensions, Context e ThemeService', () => {
    beforeEach(() => {
        ptyStreamHub.resetForTests();
        clearContextProvidersForTests();
        clearTerminalExtensionsForTests();
        vi.restoreAllMocks();
        vi.spyOn(ptyClient, 'onExit').mockResolvedValue(() => {});
        vi.spyOn(ptyClient, 'onCleanupFailed').mockResolvedValue(() => {});
        vi.spyOn(ptyClient, 'attach').mockResolvedValue({
            info: {
                attachmentId: 'att-reg',
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

    it('preserva a instância entre detach e attach sem chamar xterm.open novamente e rejeita ptyId conflitante', async () => {
        const registry = createTerminalInstanceRegistry();
        let mockXterm: MockXtermTerminal | null = null;

        const inst = registry.getOrCreate({
            paneId: 'pane-1',
            ptyId: 'pty-1',
            prefs: DEFAULT_TERMINAL_PREFERENCES,
            theme: createMaxNavyTealTheme(),
            flow: DEFAULT_FLOW_CONFIG,
            createTerminal: () => {
                mockXterm = new MockXtermTerminal();
                return mockXterm as unknown as Terminal;
            },
            createWebglAddon: () => ({
                onContextLoss: () => ({ dispose: () => {} }),
                dispose: () => {}
            })
        });

        const host1 = document.createElement('div');
        document.body.appendChild(host1);
        await inst.attach(host1);
        expect(mockXterm!.openCount).toBe(1);

        inst.detach();
        const host2 = document.createElement('div');
        document.body.appendChild(host2);
        await inst.attach(host2);
        expect(mockXterm!.openCount).toBe(1);

        // Conflito de ptyId no mesmo paneId sem dispose prévio.
        expect(() => {
            registry.getOrCreate({
                paneId: 'pane-1',
                ptyId: 'pty-2-different',
                prefs: DEFAULT_TERMINAL_PREFERENCES,
                theme: createMaxNavyTealTheme(),
                flow: DEFAULT_FLOW_CONFIG
            });
        }).toThrowError(MtTerminalError);

        registry.disposeAll();
        host1.remove();
        host2.remove();
    });

    it('executa 100 montagens e desmontagens consecutivas sem vazar assinaturas no PtyStreamHub', async () => {
        const registry = createTerminalInstanceRegistry();
        const host = document.createElement('div');
        document.body.appendChild(host);

        for (let i = 0; i < 100; i++) {
            const paneId = `pane-loop-${i}`;
            const ptyId = `pty-loop-${i}`;
            const inst = registry.getOrCreate({
                paneId,
                ptyId,
                prefs: DEFAULT_TERMINAL_PREFERENCES,
                theme: createMaxNavyTealTheme(),
                flow: DEFAULT_FLOW_CONFIG,
                createTerminal: () => new MockXtermTerminal() as unknown as Terminal,
                createWebglAddon: () => ({
                    onContextLoss: () => ({ dispose: () => {} }),
                    dispose: () => {}
                })
            });

            await inst.attach(host);
            expect(ptyStreamHub.getActiveSubscriptionCount()).toBe(1);
            registry.dispose(paneId);
            expect(ptyStreamHub.getActiveSubscriptionCount()).toBe(0);
            expect(registry.size()).toBe(0);
        }

        host.remove();
    });

    it('exercita extensões globais, provedores de contexto, ThemeService, preferências, atalhos de cópia/colagem e envio de dados na TerminalInstance', async () => {
        const activatedOn: string[] = [];
        const disposedOn: string[] = [];

        const extSub = registerTerminalExtension({
            id: 'ext-test',
            activate: (inst) => {
                activatedOn.push(inst.paneId);
                return {
                    dispose: () => {
                        disposedOn.push(inst.paneId);
                    }
                };
            }
        });

        const ctxSub1 = registerContextProvider(({ paneId }) => ({
            projectId: `proj-${paneId}`,
            cwd: '/home/dev',
            extra: { branch: 'main' }
        }));
        const ctxSub2 = registerContextProvider(() => {
            throw new Error('Provedor falho ignorado');
        });

        const ctx = resolveTerminalContext({ paneId: 'p1', ptyId: 'pty1' });
        expect(ctx.projectId).toBe('proj-p1');
        expect(ctx.cwd).toBe('/home/dev');
        expect(ctx.extra).toEqual({ branch: 'main' });

        const registry = createTerminalInstanceRegistry();
        const mockXterm = new MockXtermTerminal();

        const inst = registry.getOrCreate({
            paneId: 'pane-ext',
            ptyId: 'pty-ext',
            prefs: DEFAULT_TERMINAL_PREFERENCES,
            theme: createMaxNavyTealTheme(),
            flow: DEFAULT_FLOW_CONFIG,
            createTerminal: () => mockXterm as unknown as Terminal,
            createWebglAddon: () => ({
                onContextLoss: () => ({ dispose: () => {} }),
                dispose: () => {}
            })
        });

        expect(activatedOn).toEqual(['pane-ext']);
        expect(registry.has('pane-ext')).toBe(true);
        const collected: string[] = [];
        registry.forEach((_inst, id) => {
            collected.push(id);
        });
        expect(collected).toEqual(['pane-ext']);

        // Testa provedor externo de tema no ThemeService.
        const customTheme = {
            ...createMaxNavyTealTheme(),
            id: 'ext-theme'
        };
        const providerSub = themeService.setThemeProvider({
            getActiveTheme: () => customTheme,
            onChange: () => ({ dispose: () => {} })
        });
        expect(themeService.getActiveTheme().id).toBe('ext-theme');
        expect(inst.getTheme().id).toBe('ext-theme');
        providerSub.dispose();
        themeService.notifyThemeChanged();

        // Aplica preferências em tempo real.
        inst.applyPreferences({ fontSize: 16, webglEnabled: false });
        expect(mockXterm.options.fontSize).toBe(16);

        // Envio de input, paste e linha.
        await inst.sendInput('ls');
        await inst.sendLine('pwd');
        inst.paste('colado');
        inst.focus();
        inst.forceWebglContextLossForDebug();

        // Testa atalhos Ctrl+Shift+C e Ctrl+Shift+V no customKeyEventHandler.
        mockXterm.selectionText = 'selecionado';
        const copyEv = new KeyboardEvent('keydown', { key: 'C', ctrlKey: true, shiftKey: true });
        const pasteEv = new KeyboardEvent('keydown', { key: 'V', ctrlKey: true, shiftKey: true });
        expect(isCopyShortcut(copyEv)).toBe(true);
        expect(isPasteShortcut(pasteEv)).toBe(true);
        expect(mockXterm.triggerCustomKeyEvent(copyEv)).toBe(false);
        expect(mockXterm.triggerCustomKeyEvent(pasteEv)).toBe(false);

        // Emite dados e binário pelo xterm.
        mockXterm.emitData('a');
        mockXterm.emitBinary('\x80');

        // Testa fluxo de frames (Data, Gap e End) recebidos pela TerminalInstance anexada.
        let capturedFrameCb: ((f: PtyFrame) => void) = () => {};
        vi.spyOn(ptyClient, 'attach').mockImplementation(async (_id, cb) => {
            capturedFrameCb = cb;
            return {
                info: {
                    attachmentId: 'att-ext',
                    replayFrom: { nextSeq: '1', byteOffset: '0' },
                    liveFrom: { nextSeq: '1', byteOffset: '0' },
                    truncated: true
                },
                channel: {} as Channel<ArrayBuffer>
            };
        });
        const host = document.createElement('div');
        document.body.appendChild(host);
        await inst.attach(host);

        const gapBuf = new ArrayBuffer(8);
        new DataView(gapBuf).setBigUint64(0, 512n, true);
        capturedFrameCb({
            kind: PtyFrameKind.Gap,
            seq: 1n,
            byteOffset: 0n,
            payload: new Uint8Array(gapBuf)
        });
        capturedFrameCb({
            kind: PtyFrameKind.Data,
            seq: 2n,
            byteOffset: 512n,
            payload: new Uint8Array([65, 66, 67])
        });
        capturedFrameCb({
            kind: PtyFrameKind.End,
            seq: 3n,
            byteOffset: 515n,
            payload: new TextEncoder().encode(
                JSON.stringify({
                    code: 0,
                    signal: null,
                    signalName: null,
                    reason: 'exited',
                    atMs: '1000'
                })
            )
        });
        await new Promise((r) => setTimeout(r, 20));
        host.remove();

        extSub.dispose();
        expect(disposedOn).toEqual(['pane-ext']);
        ctxSub1.dispose();
        ctxSub2.dispose();
        registry.disposeAll();
        expect(inst.isDisposed()).toBe(true);
    });
});
