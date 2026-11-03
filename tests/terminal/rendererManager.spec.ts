import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Terminal } from '@xterm/xterm';
import type { RendererKind } from '@/types/terminal';
import { RendererManager, type WebglAddonLike } from '@/terminal/engine/rendererManager';
import { MockXtermTerminal } from './mocks/xtermMock';

function createFakeWebglAddon() {
    let lossListener: (() => void) | null = null;
    let disposed = false;

    const addon: WebglAddonLike & {
        activate: () => void;
        triggerLoss: () => void;
        isDisposed: () => boolean;
    } = {
        activate() {},
        onContextLoss(cb) {
            lossListener = cb;
            return {
                dispose() {
                    lossListener = null;
                }
            };
        },
        dispose() {
            disposed = true;
            lossListener = null;
        },
        triggerLoss() {
            lossListener?.();
        },
        isDisposed() {
            return disposed;
        }
    };
    return addon;
}

describe('RendererManager', () => {
    beforeEach(() => {
        vi.useFakeTimers();
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it('ativa webgl quando visível, suspende ao ficar oculto e trava em dom após 3 perdas de contexto em 60s', () => {
        const mockTerm = new MockXtermTerminal();
        const host = document.createElement('div');
        mockTerm.open(host);

        let nowMs = 1_000_000;
        const rendererEvents: RendererKind[] = [];
        const createdAddons: Array<ReturnType<typeof createFakeWebglAddon>> = [];

        const manager = new RendererManager({
            xterm: mockTerm as unknown as Terminal,
            webglEnabled: true,
            visible: true,
            now: () => nowMs,
            onRendererChange: (kind) => rendererEvents.push(kind),
            createWebglAddon: () => {
                const a = createFakeWebglAddon();
                createdAddons.push(a);
                return a;
            }
        });

        expect(manager.activate()).toBe('webgl');
        expect(manager.getKind()).toBe('webgl');

        // Suspende ao ficar oculto.
        manager.setVisible(false);
        expect(manager.getKind()).toBe('dom');
        expect(createdAddons[0]?.isDisposed()).toBe(true);

        // Retoma ao voltar a ficar visível.
        manager.setVisible(true);
        expect(manager.getKind()).toBe('webgl');

        // 1ª perda de contexto -> cai para dom e tenta recuperar após 30 s.
        createdAddons[1]!.triggerLoss();
        expect(manager.getKind()).toBe('dom');
        expect(manager.isLockedToDom()).toBe(false);

        nowMs += 30_000;
        vi.advanceTimersByTime(30_000);
        expect(manager.getKind()).toBe('webgl');

        // 2ª perda dentro de 60 s.
        nowMs += 5_000;
        createdAddons[2]!.triggerLoss();
        expect(manager.isLockedToDom()).toBe(false);

        // 3ª perda dentro da mesma janela de 60 s -> trava definitivamente em dom.
        nowMs += 5_000;
        manager.forceContextLossForDebug();
        expect(manager.isLockedToDom()).toBe(true);
        expect(manager.getKind()).toBe('dom');

        // Mesmo tentando reativar, permanece travado em dom.
        expect(manager.activate()).toBe('dom');
        manager.dispose();
    });
});
